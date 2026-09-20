use std::{
    mem,
    pin::{Pin, pin},
    task::{Context, Poll, Waker},
};

use facet::Facet;
pub use plug_macro::plug;

pub trait Reflected: for<'a> Facet<'a> {}
impl<T: for<'a> Facet<'a>> Reflected for T {}

// TODO: replace Box<dyn T> and eyre::Error
// with Stored Objects (blocked on paradigm)

pub enum Routine<T> {
    Direct(T),
    Resumable(Pin<Box<dyn Future<Output = T>>>),
    Finished,
}

impl<T> Routine<T> {
    #[inline]
    pub fn define_direct(output: T) -> Self {
        Self::Direct(output)
    }

    #[inline]
    pub fn define_deferred(f: impl Future<Output = T> + 'static) -> Self {
        Self::Resumable(Box::pin(f))
    }

    pub fn start(mut f: impl Future<Output = T> + Unpin + 'static) -> Self {
        let future = pin!(&mut f);

        // TODO: are there even reasonable futures which wake on first poll?
        let ret = future.poll(&mut Context::from_waker(Waker::noop()));

        match ret {
            Poll::Ready(x) => Self::define_direct(x),
            Poll::Pending => Self::define_deferred(f),
        }
    }
}

impl<T: Unpin> Future for Routine<T> {
    type Output = T;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match &mut *self {
            this @ Self::Direct(_) => {
                let ret = match mem::replace(this, Self::Finished) {
                    Self::Direct(value) => value,
                    _ => unreachable!(), // guarded by match above
                };

                Poll::Ready(ret)
            }
            // SAFETY: `fut` is pinned as `self` is (structual projection)
            Self::Resumable(fut) => unsafe { Pin::new_unchecked(fut) }.poll(cx),
            Self::Finished => panic!("future polled after completion"),
        }
    }
}

// TODO(err): it is logical for the error enum to be associated with an interface, not
// individual objects

#[allow(type_alias_bounds)]
pub type Construct<T: Object> = Routine<eyre::Result<T>>;

pub trait Object {
    type Config: Reflected;
    const TAG: &str;
}

// TODO: properly split initialization (memory gather) and construction (memory map: cfg -> state)
pub trait Init: Object + Sized {
    fn init(config: &Self::Config) -> Construct<Self>;
}

pub trait DynamicObject {
    fn tag() -> &'static str;
    fn init(config: ()) -> Construct<Box<Self>>;
}

impl<T> DynamicObject for T
where
    T: Object + Init,
{
    fn tag() -> &'static str {
        T::TAG
    }

    fn init(config: ()) -> Construct<Box<Self>> {
        todo!()
    }
}
