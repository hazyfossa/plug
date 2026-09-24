use eyre::Result;
use facet::Facet;
pub use plug_macro::plug;
use std::{
    mem,
    pin::Pin,
    task::{Context, Poll},
};

pub trait Reflected: for<'a> Facet<'a> {}
impl<T: for<'a> Facet<'a>> Reflected for T {}

// TODO: replace Box<dyn T> and eyre::Error
// with Stored Objects (blocked on paradigm)

pub enum AsyncMethod<F: Future, const LIKELY_SYNC: bool = false> {
    Direct(F::Output),
    Deferred(F),
    Finished,
}

impl<F, const LIKELY_SYNC: bool> Future for AsyncMethod<F, LIKELY_SYNC>
where
    F: Future,
    F::Output: Unpin,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // Safety: we only ever move F::Output out of Self::Direct
        // so pinned data (F) is not moved
        let this = unsafe { self.get_unchecked_mut() };

        match this {
            Self::Direct(_) => {
                let ret = match mem::replace(this, Self::Finished) {
                    Self::Direct(value) => value,
                    _ => unreachable!(), // guarded by match above
                };

                Poll::Ready(ret)
            }
            // SAFETY: `fut` is pinned as `self` is (structual projection)
            Self::Deferred(fut) => {
                LIKELY_SYNC.then_some(core::hint::cold_path());
                unsafe { Pin::new_unchecked(fut) }.poll(cx)
            }
            Self::Finished => panic!("future polled after completion"),
        }
    }
}

// TODO(err): it is logical for the error enum to be associated with an interface, not
// individual objects

pub trait Object {
    type Config: Reflected + Default;
    const TAG: &str;
}

// TODO: properly split initialization (memory gather) and construction (memory map: cfg -> state)
#[allow(async_fn_in_trait)]
pub trait Init: Object + Sized {
    // TODO: async init via AsyncMethod
    async fn init(config: &Self::Config) -> Result<Self>;
}

#[cfg(feature = "dyn")]
#[doc(hidden)]
pub mod __dyn_codegen {
    use super::*;

    use facet_value::Value;

    type DynAsyncMethod<T, const LIKELY_SYNC: bool = false> =
        AsyncMethod<Pin<Box<dyn Future<Output = T>>>, LIKELY_SYNC>;

    impl<F: Future + 'static, const LIKELY_SYNC: bool> AsyncMethod<F, LIKELY_SYNC>
    where
        F::Output: Unpin,
    {
        fn store_dynamic(self) -> DynAsyncMethod<F::Output, LIKELY_SYNC> {
            match self {
                Self::Deferred(fut) => DynAsyncMethod::Deferred(Box::pin(fut)),
                Self::Direct(output) => DynAsyncMethod::Direct(output),
                Self::Finished => DynAsyncMethod::Finished,
            }
        }
    }

    pub trait DynamicObject {
        fn tag() -> &'static str;
        fn init(config: Value) -> DynAsyncMethod<Result<Box<Self>>>;
    }

    impl<T> DynamicObject for T
    where
        T: Object + Init,
    {
        fn tag() -> &'static str {
            T::TAG
        }

        fn init(config: Value) -> DynAsyncMethod<Result<Box<Self>>> {
            AsyncMethod::Deferred(async {
                let config: T::Config = facet_value::from_value(config)?;
                let ret = <T as Init>::init(&config).await?;
                let stored_self = Box::new(ret);
                Ok(stored_self)
            })
            .store_dynamic()
        }
    }
}
