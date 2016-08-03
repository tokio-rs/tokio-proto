//! The non-blocking event driven core of Tokio
//!
use io::Ready;
use reactor::task::{Task, IntoTick, Tick};
use reactor::source::{self, Source};
use mio::{Evented, Events, EventSet, Poll, PollOpt, Token};
use mio::channel::{self, Sender, Receiver};
use slab::Slab;
use std::{io, usize};
use std::cell::{Cell, RefCell, UnsafeCell};
use std::sync::atomic::{AtomicUsize, ATOMIC_USIZE_INIT, Ordering};

/// Reactor configuration options
#[derive(Debug)]
pub struct Config {
    max_sources: usize,
}

/// Schedules tasks based on I/O events.
///
/// For more details, read the module level documentation.
pub struct Reactor {
    tx: Sender<Op>,
    rx: Receiver<Op>,
    config: Config,
}

pub struct EventLoop {
    // Receiver on which the EventLoop receives commands from other threads
    rx: Receiver<Op>,
    // Events received on last call to Poll::poll
    events: Events,
    // Storage of currently active tasks on the EventLoop
    //
    // TODO: In order to avoid allocating for the handler here, it should be
    // possible to create slabs per service category, in which case the
    // concrete type would be known.
    tasks: Slab<TaskCell, Token>,
    // Data that is shared at runtime to tasks via a thread-local. This is
    // splilt out to make the borrow checker happy
    rt: Rt,
}

struct Rt {
    // The Reactor runs as long as this is true
    run: Cell<bool>,
    // Unique identifier representing the EventLoop
    id: EventLoopId,
    // Handle to `Poll` associated with the `EventLoop`
    poll: Poll,
    // Monotonically increasing iteration identifier. This number is increased
    // each time `Poll::poll` is invoked and used to track progress among
    // tasks.
    poll_num: u64,
    // Sources of events. These are usually sockets, but they could also be
    // timers, channels, etc... Anything that integrates with the reactor to
    // provide readiness notifications.
    sources: RefCell<Slab<source::Weak, Token>>,
    // Currently running task
    current_task: Option<TaskRef>,
    // True if the current task did advance
    current_task_did_advance: Cell<bool>,
    // New tasks that have been created during a task invocation and are
    // pending being registered with the EventLoop.
    staged_tasks: Stack<Box<Task>>,
}

/// Info associated with the currently running task
#[derive(Debug, Copy, Clone)]
pub struct TaskRef {
    token: Token,
    tick_num: u64,
}

/// Internal task storage
struct TaskCell {
    tick_num: u64,
    poll_num: u64,
    task: Box<Task>,
}

/// Handle to a `Reactor` instance. Used for communication.
#[derive(Clone)]
pub struct ReactorHandle {
    tx: Sender<Op>,
}

enum Op {
    Schedule(Box<Task + Send + 'static>),
}

// TODO: clean this up
struct Stack<T> {
    inner: UnsafeCell<Vec<T>>,
}

pub type EventLoopId = usize;

static NEXT_EVENT_LOOP_ID: AtomicUsize = ATOMIC_USIZE_INIT;
scoped_thread_local!(static CURRENT_RT: Rt);

const OP_RECEIVER: Token = Token(usize::MAX - 2);

impl Default for Config {
    fn default() -> Config {
        Config {
            max_sources: 1024,
        }
    }
}

impl Config {
    /// Create a `Config` with default values
    pub fn new() -> Config {
        Config::default()
    }

    /// Set the max number of sources the `Reactor` may concurrently handle.
    pub fn max_sources(mut self, val: usize) -> Self {
        self.max_sources = val;
        self
    }
}

impl Reactor {
    /// Create a `Reactor` instance with default configuration values.
    pub fn default() -> io::Result<Reactor> {
        Reactor::new(Config::default())
    }

    /// Create a `Reactor` with the given configuration values.
    pub fn new(config: Config) -> io::Result<Reactor> {
        let (tx, rx) = channel::channel();

        Ok(Reactor {
            tx: tx,
            rx: rx,
            config: config,
        })
    }

    /// Return a handle for the `Reactor`
    pub fn handle(&self) -> ReactorHandle {
        ReactorHandle { tx: self.tx.clone() }
    }

    /// Runs the `Reactor` on the current thread, blocking the thread until the
    /// reactor shuts down.
    pub fn run(self) -> io::Result<()> {
        let Reactor { config, rx, .. } = self;

        let id = NEXT_EVENT_LOOP_ID.fetch_add(1, Ordering::Relaxed);

        let event_loop = EventLoop {
            rx: rx,
            events: Events::with_capacity(256),
            tasks: Slab::new(100),
            rt: Rt {
                run: Cell::new(true),
                id: id,
                poll: try!(Poll::new()),
                poll_num: 0,
                sources: RefCell::new(Slab::new(config.max_sources)),
                current_task: None,
                current_task_did_advance: Cell::new(false),
                staged_tasks: Stack::with_capacity(128),
            },
        };

        try!(event_loop.run());
        Ok(())
    }

    /// Run the reactor until shutdown on a background thread.
    pub fn spawn(self) {
        use std::thread;

        thread::spawn(move || {
            if let Err(e) = self.run() {
                error!("Reactor failed to run; error={:?}", e);
            }
        });
    }
}

impl ReactorHandle {
    /// Schedule the given `Task` on the reactor
    pub fn schedule<T: Task + Send + 'static>(&self, task: T) {
        // TODO: Figure out how to reduce boxing
        self.tx.send(Op::Schedule(Box::new(task))).ok().unwrap();
    }

    /// Run the given function on the reactor
    pub fn oneshot<F: FnOnce() -> T + Send + 'static, T: IntoTick>(&self, f: F) {
        use take::Take;
        self.schedule(Take::new(f))
    }

    /// Shutdown the reactor
    pub fn shutdown(&self) {
        self.oneshot(|| shutdown());
    }
}

/*
 *
 * ===== Thread-local API =====
 *
 */

/// Schedule the given task for execution on the currently running `Reactor`.
///
/// # Panics
///
/// If not currently on a reactor thread, this function panics.
pub fn schedule<T: Task + 'static>(task: T) {
    CURRENT_RT.with(|rt| rt.staged_tasks.push(Box::new(task)))
}

/// Run the given function on the reactor.
///
/// # Panics
///
/// If not currently on a reactor thread, this function panics.
pub fn oneshot<F: FnOnce() -> T + Send + 'static, T: IntoTick>(f: F) {
    use take::Take;
    schedule(Take::new(f))
}

/// Register the given `evented` value with the currently running `Reactor` and
/// return a `Source` representing the registration.
///
/// # Panics
///
/// If not currently on a reactor thread, this function panics.
pub fn register_source<E: Evented>(evented: &E, interest: Ready) -> io::Result<Source> {
    CURRENT_RT.with(|rt| {
        // Create the new source
        let source = {
                let mut sources = rt.sources.borrow_mut();

            let token = sources
                .insert_with(|token| source::new(token, rt.id))
                .unwrap();

            sources[token].strong()
        };

        try!(rt.poll.register(evented, source::token(&source), interest.into(), PollOpt::edge()));
        Ok(source)
    })
}


/// Shutdown the currently running `Reactor`.
///
/// # Panics
///
/// If not currently on a reactor thread, this function panics.
pub fn shutdown() {
    CURRENT_RT.with(|rt| rt.shutdown());
}

impl EventLoop {
    pub fn current_task(event_loop_id: EventLoopId) -> Option<TaskRef> {
        CURRENT_RT.with(|rt| {
            if rt.id != event_loop_id {
                None
            } else {
                rt.current_task
            }
        })
    }

    pub fn current_task_did_advance() {
        CURRENT_RT.with(|rt| rt.current_task_did_advance.set(true))
    }

    pub fn drop_source(token: Token) {
        CURRENT_RT.with(|rt| rt.sources.borrow_mut().remove(token));
    }

    /*
     *
     * ===== Execution =====
     *
     */

    /// Run the EventLoop
    pub fn run(mut self) -> io::Result<()> {
        debug!("starting Reactor loop");

        // Register the op channel
        try!(self.rt.poll.register(&self.rx, OP_RECEIVER, EventSet::readable(), PollOpt::edge()));

        while self.rt.run() {
            try!(self.rt.poll.poll(&mut self.events, None));
            self.rt.poll_num += 1;

            trace!("event loop iteration; num={:?}", self.rt.poll_num);

            // First, loop over all the events and update readiness of
            // associated sources. All sources must be updated before
            // dispatching events in order for the task FSM to have a
            // full view of its state.
            self.update_source_readiness();

            // Next dispatch events to tasks
            self.dispatch_events();

        }

        Ok(())
    }

    fn update_source_readiness(&mut self) {
        for i in 0..self.events.len() {
            let event = self.events.get(i).unwrap();

            if event.token() != OP_RECEIVER {
                let source = &self.rt.sources.borrow()[event.token()];
                let readiness = source.peek_readiness();
                source.set_readiness(readiness | event.kind().into());
            }
        }
    }

    fn dispatch_events(&mut self) {
        for i in 0..self.events.len() {
            let event = self.events.get(i).unwrap();

            trace!("Worker::ready; token={:?}; kind={:?}", event.token(), event.kind());

            if event.token() == OP_RECEIVER {
                if let Err(e) = self.process_ops() {
                    panic!("unexpected error; e={:?}", e);
                }
            } else {
                self.process_source(event.token());
            }

            self.process_queued();

            if !self.rt.run() {
                return;
            }
        }
    }

    fn process_source(&mut self, token: Token) {
        let task = match self.rt.sources.borrow()[token].task() {
            Some(t) => t,
            None => {
                debug!("source has no associated task; token={:?}", token);
                return;
            }
        };

        self.execute_task(task);
    }

    fn process_ops(&mut self) -> io::Result<()> {
        use std::sync::mpsc::TryRecvError::*;

        while self.rt.run() {
            match self.rx.try_recv() {
                Ok(op) => try!(self.process_op(op)),
                Err(Empty) => return Ok(()),
                Err(Disconnected) => {
                    // TODO: Error while reading off the queue, should probably
                    // shutdown here
                    unimplemented!();
                }
            }
        }

        Ok(())
    }

    fn process_op(&mut self, op: Op) -> io::Result<()> {
        match op {
            Op::Schedule(mut f) => {
                if f.oneshot() {
                    match self.rt.scope(None, || f.tick()) {
                        Ok(Tick::Final) => {} // Expected return value
                        Ok(Tick::WouldBlock) => warn!("oneshot `Task` returned `Tick::WouldBlock`"),
                        Ok(Tick::Yield) => warn!("oneshot `Task` returned `Tick::Yield`"),
                        Err(e) => warn!("oneshot `Task` returned error; err={:?}", e),
                    }
                } else {
                }
            }
        }

        self.process_queued();

        Ok(())
    }

    fn process_queued(&mut self) {
        while let Some(task) = self.rt.staged_tasks.pop() {
            self.add_task(task);
        }
    }

    fn add_task(&mut self, task: Box<Task>) {
        let task = TaskCell {
            tick_num: 0,
            poll_num: 0,
            task: task,
        };

        let token = match self.tasks.insert(task) {
            Ok(token) => token,
            Err(_) => unimplemented!(),
        };

        self.execute_task(token);
    }

    fn execute_task(&mut self, token: Token) {
        let mut task_shutdown = false;

        {
            let task = &mut self.tasks[token];

            if task.poll_num == self.rt.poll_num {
                // Task already has been executed this iteration of the event loop,
                // so skip it
                return;
            }

            task.poll_num = self.rt.poll_num;

            while self.rt.run() {
                // Increment the task's tick_num
                task.tick_num += 1;

                trace!("running task; task={:?}; tick={:?}", token, task.tick_num);

                let task_ref = TaskRef {
                    token: token,
                    tick_num: task.tick_num,
                };

                // Run the task while setting the current event loop variable
                let res = self.rt.scope(Some(task_ref), || task.task.tick());

                match res {
                    Ok(Tick::Final) => {
                        debug!("finalizing task; token={:?}", token);
                        task_shutdown = true;
                        break;
                    }
                    Ok(Tick::Yield) => {

                        // # Thoughts
                        //
                        // It is possible for a task to process sources without
                        // hitting a would block. The problem here is that
                        // there is no way for the reactor to detect that the
                        // task is still ready. Imagine that the task reads
                        // from a Transport that has data buffered in memory
                        // and the read does not deplete the buffer. The
                        // transport is still ready and the read did not hit
                        // the underlying socket. Now, imagine the task
                        // continues to work and reads from a source returning
                        // would-block. In this case, the reactor observed that
                        // 100% of the sources accessed by the task are not
                        // ready.
                        //
                        // Now, the current requirement for implementing a task
                        // is to read sources until a would-block his hit, but
                        // this may not be desirable in all cases.
                        //
                        // One solution would be to add a Tick:Yield return
                        // value.

                        unimplemented!();
                    }
                    Ok(Tick::WouldBlock) => {
                        // Task would have blocked. In this case, we must determine
                        // if the FSM made any progress at all. If progress was
                        // made, call the FSM until no progress was made.

                        if !self.rt.current_task_did_advance.get() {
                            trace!("current task made no progress");
                            return;
                        }

                        trace!("current task made progress");
                    }
                    Err(_) => {
                        unimplemented!();
                    }
                }
            }
        }

        if task_shutdown {
            let tasks = &mut self.tasks;

            self.rt.scope(None, || {
                // Drop the task within an RT scope
                // TODO: Should the current task info be set?
                let _ = tasks.remove(token);
            });
        }
    }
}

impl Rt {
    fn run(&self) -> bool {
        self.run.get()
    }

    fn shutdown(&self) {
        self.run.set(false);
    }

    fn scope<F: FnOnce() -> R, R>(&mut self, task: Option<TaskRef>, f: F) -> R {
        // Set `did_advance` to false
        self.current_task_did_advance.set(false);

        // Set the currently running task
        self.current_task = task;

        CURRENT_RT.set(self, move || f())
    }
}

impl TaskRef {
    pub fn token(&self) -> Token {
        self.token
    }

    pub fn tick_num(&self) -> u64 {
        self.tick_num
    }
}

impl<T> Stack<T> {
    fn with_capacity(capacity: usize) -> Stack<T> {
        Stack { inner: UnsafeCell::new(Vec::with_capacity(capacity)) }
    }

    fn push(&self, val: T) {
        unsafe { (*self.inner.get()).push(val) }
    }

    fn pop(&self) -> Option<T> {
        unsafe { (*self.inner.get()).pop() }
    }
}
