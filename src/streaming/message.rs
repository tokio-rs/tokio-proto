use std::{cmp, fmt, ops};

/// Message sent and received from a multiplexed service
pub enum Message<T, B> {
    /// Has no associated streaming body
    WithoutBody(T),
    /// Has associated streaming body
    WithBody(T, B),
}

impl<T, B> Message<T, B> {
    /// Returns a reference to the inner value
    pub fn get_ref(&self) -> &T {
        match *self {
            Message::WithoutBody(ref v) => v,
            Message::WithBody(ref v, _) => v,
        }
    }

    /// Returns a mutable reference to the inner value
    pub fn get_mut(&mut self) -> &mut T {
        match *self {
            Message::WithoutBody(ref mut v) => v,
            Message::WithBody(ref mut v, _) => v,
        }
    }

    /// Consumes the value and returns the inner value
    pub fn into_inner(self) -> T {
        match self {
            Message::WithoutBody(v) => v,
            Message::WithBody(v, _) => v,
        }
    }

    /// If the `Message` value has an associated body stream, return it. The
    /// original `Message` value will then become a `WithoutBody` variant.
    pub fn take_body(&mut self) -> Option<B> {
        use std::ptr;

        // unfortunate that this is unsafe, but I think it is preferable to add
        // a little bit of unsafe code instead of adding a useless variant to
        // Message.
        unsafe {
            match ptr::read(self as *const Message<T, B>) {
                m @ Message::WithoutBody(..) => {
                    ptr::write(self as *mut Message<T, B>, m);
                    None
                }
                Message::WithBody(m, b) => {
                    ptr::write(self as *mut Message<T, B>, Message::WithoutBody(m));
                    Some(b)
                }
            }
        }
    }
}

impl<T, B> cmp::PartialEq<T> for Message<T, B>
    where T: cmp::PartialEq
{
    fn eq(&self, other: &T) -> bool {
        (**self).eq(other)
    }
}

impl<T, B> ops::Deref for Message<T, B> {
    type Target = T;

    fn deref(&self) -> &T {
        match *self {
            Message::WithoutBody(ref v) => v,
            Message::WithBody(ref v, _) => v,
        }
    }
}

impl<T, B> ops::DerefMut for Message<T, B> {
    fn deref_mut(&mut self) -> &mut T {
        match *self {
            Message::WithoutBody(ref mut v) => v,
            Message::WithBody(ref mut v, _) => v,
        }
    }
}

impl<T, B> fmt::Debug for Message<T, B>
    where T: fmt::Debug
{
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Message::WithoutBody(ref v) => write!(fmt, "Message::WithoutBody({:?})", v),
            Message::WithBody(ref v, _) => write!(fmt, "Message::WithBody({:?}, ...)", v),
        }
    }
}
