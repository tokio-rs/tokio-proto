use bytes::{alloc, Buf, MutBuf, Bytes, BlockBuf, BlockBufCursor};

/// Buffer abstraction
pub trait Buffer<'a> : MutBuf {
    /// Type of Buf returned
    type Cursor: Buf;

    /// Returns a `Buf` over buffered bytes.
    fn buf(&'a self) -> Self::Cursor;

    /// Return the number of buffered bytes
    fn len(&self) -> usize;

    /// Drain the first `n` buffered bytes, returning them as a `Bytes` value.
    fn shift(&mut self, n: usize) -> Bytes;

    /// Move all buffered bytes into a sequential memory
    ///
    /// # Panics
    ///
    /// Panics if the `Buffer` is unable to allocate sequential memory large
    /// enough to contain the buffered bytes.
    fn compact(&mut self);

    /// Returns a byte slice if all bytes are located in sequential memory
    fn bytes(&self) -> Option<&[u8]>;
}

impl<'a, A: alloc::FixedSize> Buffer<'a> for BlockBuf<A> {
    type Cursor = BlockBufCursor<'a>;

    fn buf(&'a self) -> BlockBufCursor<'a> {
        BlockBuf::buf(self)
    }

    fn len(&self) -> usize {
        BlockBuf::len(self)
    }

    fn shift(&mut self, n: usize) -> Bytes {
        BlockBuf::shift(self, n)
    }

    fn compact(&mut self) {
        BlockBuf::compact(self)
    }

    fn bytes(&self) -> Option<&[u8]> {
        BlockBuf::bytes(self)
    }
}
