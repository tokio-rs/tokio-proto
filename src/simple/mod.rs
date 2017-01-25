pub mod pipeline;
pub mod multiplex;

// A utility struct to enable "lifting" from an RPC to a streaming proto, which
// is how RPC protos are implemented under the hood. Unfortunately:
//
// - Having a blanket impl that lifts *directly* from
//   e.g. simple::pipeline::ServerProto to streaming::pipeline::ServerProto causes
//   inference problems, so we need a newtype.
//
// - We can't do `LiftProto<'a, P: 'a>(&'a P)` because of the `'static` requirement,
//   but we need to work with references because binding a transport taked `&self`.
//
// Thus, we use a newtype over the actual protocol type, which requires a bit of
// transmute hackery to transform references. Since newtypes are guaranteed not
// to change layout, this is kosher.
struct LiftProto<P>(P);

impl<P> LiftProto<P> {
    fn from_ref(proto: &P) -> &LiftProto<P> {
        unsafe { ::std::mem::transmute(proto) }
    }

    fn lower(&self) -> &P {
        &self.0
    }
}
