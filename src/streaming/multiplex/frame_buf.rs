//! Frame buffer

use smallvec::SmallVec;
use std::{cmp, ptr};
use std::cell::{Cell, UnsafeCell};
use std::rc::Rc;

/// Buffers frames
pub struct FrameBuf<T> {
    inner: Rc<UnsafeCell<Inner<T>>>,
}

/// Tracks a single stream of frames
///
/// The frames are stored in a shared `FrameBuf`
pub struct FrameDeque<T> {
    inner: Rc<UnsafeCell<Inner<T>>>,
    head: Cell<*mut Slot<T>>,
    tail: Cell<*mut Slot<T>>,
    len: Cell<usize>,
}

const MAX_BLOCKS: usize = 16;
const INITIAL_BLOCK_SIZE: usize = 32;
const MAX_CAPACITY: usize = 1_048_576;

struct Inner<T> {
    // Max number of elements that can be stored
    max_capacity: usize,
    // Number of allocated elements
    allocated: usize,
    // Free slot stack
    free: *mut Slot<T>,
    // All blocks
    blocks: SmallVec<[Vec<Slot<T>>; MAX_BLOCKS]>,
}

struct Slot<T> {
    next: *mut Slot<T>,
    val: Option<T>,
}

impl<T> FrameBuf<T> {
    /// Return a new `FrameBuf` with the given capacity
    pub fn with_capacity(capacity: usize) -> FrameBuf<T> {
        assert!(capacity < MAX_CAPACITY,
                "requested frame buffer capacity too large; max={}; requested={}",
                MAX_CAPACITY, capacity);

        let inner = UnsafeCell::new(Inner::with_capacity(capacity));
        FrameBuf { inner: Rc::new(inner) }
    }

    #[cfg(test)]
    pub fn capacity(&self) -> usize {
        unsafe { &*self.inner.get() }.max_capacity
    }

    #[cfg(test)]
    pub fn allocated(&self) -> usize {
        unsafe { &*self.inner.get() }.allocated
    }

    pub fn deque(&self) -> FrameDeque<T> {
        FrameDeque {
            inner: self.inner.clone(),
            head: Cell::new(ptr::null_mut()),
            tail: Cell::new(ptr::null_mut()),
            len: Cell::new(0),
        }
    }
}

impl<T> FrameDeque<T> {
    pub fn push(&self, val: T) {
        unsafe {
            let ptr;
            let slot = (*self.inner.get())
                .reserve_slot()
                .expect("FrameBuf out of capacity");

            debug_assert!(slot.next.is_null());

            slot.val = Some(val);
            ptr = slot as *mut Slot<T>;

            if let Some(tail) = self.tail.get().as_mut() {
                tail.next = ptr;
            } else {
                self.head.set(ptr);
            }

            self.len.set(self.len.get() + 1);

            self.tail.set(ptr);
        }
    }

    pub fn push_front(&self, val: T) {
        unsafe {
            let ptr;
            let slot = (*self.inner.get())
                .reserve_slot()
                .expect("FrameBuf out of capacity");

            debug_assert!(slot.next.is_null());

            slot.val = Some(val);
            slot.next = self.head.get();

            ptr = slot as *mut Slot<T>;
            self.head.set(ptr);

            if self.tail.get().as_ref().is_none() {
                self.tail.set(ptr);
            }

            self.len.set(self.len.get() + 1);
        }
    }

    pub fn pop(&self) -> Option<T> {
        unsafe {
            let ptr = self.head.get();

            if let Some(head) = ptr.as_mut() {
                self.head.set(head.next);

                if head.next.is_null() {
                    self.tail.set(ptr::null_mut());
                }

                self.len.set(self.len.get() - 1);

                let val = head.val.take();

                let inner = &mut *self.inner.get();

                head.next = inner.free;
                inner.free = ptr;

                if val.is_none() {
                    assert!(self.len() == 0);
                }

                val
            } else {
                None
            }
        }
    }

    pub fn clear(&mut self) {
        while let Some(_) = self.pop() {
        }
    }

    pub fn len(&self) -> usize {
        self.len.get()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<T> Inner<T> {
    fn with_capacity(mut capacity: usize) -> Inner<T> {
        capacity = cmp::max(INITIAL_BLOCK_SIZE, capacity.next_power_of_two());


        Inner {
            max_capacity: capacity,
            allocated: 0,
            free: ptr::null_mut(),
            blocks: SmallVec::new(),
        }
    }

    fn reserve_slot(&mut self) -> Option<&mut Slot<T>> {
        unsafe {
            // If there is a pre-allocated available slot, use that
            if let Some(slot) = self.free.as_mut() {
                self.free = slot.next;
                slot.next = ptr::null_mut();
                return Some(slot);
            }

            let grow = {
                if self.blocks.is_empty() {
                    true
                } else {
                    let block = self.blocks.last().unwrap();
                    block.len() == block.capacity()
                }
            };

            if grow && !self.grow() {
                return None;
            }

            let block = self.blocks.last_mut().unwrap();
            let idx = block.len();

            block.push(Slot {
                next: ptr::null_mut(),
                val: None,
            });

            Some(&mut block[idx])
        }
    }

    fn grow(&mut self) -> bool {
        if self.blocks.is_empty() {
            // Push the initial block
            self.blocks.push(Vec::with_capacity(INITIAL_BLOCK_SIZE));
            self.allocated = INITIAL_BLOCK_SIZE;
            return true;
        }

        // Only grow if there is room
        if self.allocated == self.max_capacity {
            return false;
        }

        // Check that the number of allocated elements is a power of two
        debug_assert!(self.allocated & self.allocated - 1 == 0);

        let new_block_cap = self.allocated;

        // Push the new block
        self.blocks.push(Vec::with_capacity(new_block_cap));
        self.allocated += new_block_cap;

        true
    }
}

#[cfg(test)]
mod test {
    use super::{FrameBuf};

    #[test]
    fn test_capacity() {
        assert_eq!(32, FrameBuf::<()>::with_capacity(0).capacity());
        assert_eq!(32, FrameBuf::<()>::with_capacity(10).capacity());
        assert_eq!(32, FrameBuf::<()>::with_capacity(32).capacity());
        assert_eq!(64, FrameBuf::<()>::with_capacity(33).capacity());
        assert_eq!(64, FrameBuf::<()>::with_capacity(64).capacity());
    }

    #[test]
    fn test_reusing() {
        let fb = FrameBuf::with_capacity(32);
        let d = fb.deque();

        d.push(123);
        assert_eq!(Some(123), d.pop());

        for i in 0..32 {
            d.push(i);
        }

        assert_eq!(32, fb.allocated());

        for i in 0..32 {
            assert_eq!(Some(i), d.pop());
        }

        assert!(d.pop().is_none());
    }

    #[test]
    fn test_growing_buffer() {
        let fb = FrameBuf::with_capacity(128);
        let d = fb.deque();

        for i in 0..32 {
            d.push(i);
        }

        assert_eq!(32, fb.allocated());

        d.push(32);
        assert_eq!(64, fb.allocated());

        for i in 0..33 {
            assert_eq!(Some(i), d.pop());
        }

        for i in 0..64 {
            d.push(i);
        }

        assert_eq!(64, fb.allocated());
        d.push(64);
        assert_eq!(128, fb.allocated());

        for i in 0..65 {
            assert_eq!(Some(i), d.pop());
        }
    }

    #[test]
    #[should_panic]
    fn test_attempting_allocation_past_capacity() {
        let fb = FrameBuf::with_capacity(32);
        let d = fb.deque();

        for i in 0..33 {
            d.push(i);
        }
    }

    #[test]
    fn test_multiple_deque() {
        let fb = FrameBuf::with_capacity(64);
        let d1 = fb.deque();
        let d2 = fb.deque();

        for i in 0..32 {
            d1.push(i);
            d2.push(i);
        }

        for i in 0..32 {
            assert_eq!(Some(i), d1.pop());
        }

        for i in 33..64 {
            d1.push(i);
        }

        for i in 0..32 {
            assert_eq!(Some(i), d2.pop());
        }

        for i in 33..64 {
            assert_eq!(Some(i), d1.pop());
        }
    }
}
