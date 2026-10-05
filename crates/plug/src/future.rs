use core::{
    mem,
    pin::Pin,
    task::{Context, Poll},
};

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

pub type DynAsyncMethod<T, const LIKELY_SYNC: bool = false> =
    AsyncMethod<Pin<Box<dyn Future<Output = T>>>, LIKELY_SYNC>;

impl<F: Future + 'static, const LIKELY_SYNC: bool> AsyncMethod<F, LIKELY_SYNC>
where
    F::Output: Unpin,
{
    pub fn box_dynamic(self) -> DynAsyncMethod<F::Output, LIKELY_SYNC> {
        match self {
            Self::Deferred(fut) => DynAsyncMethod::Deferred(Box::pin(fut)),
            Self::Direct(output) => DynAsyncMethod::Direct(output),
            Self::Finished => DynAsyncMethod::Finished,
        }
    }
}
