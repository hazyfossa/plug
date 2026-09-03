pub use plug_macro::plug;

// TODO: this level of granularity is inapplicable since we cannot change fn signature at link time
// continue to dispatch via two-way meta on direct, separate thing (micro-opt) for dyn
// TODO: have a flag for one-way-meta (dispatch over trait even for direct) - faster compile times,
// hope compiler can optimize hot paths as Dyn Meta is all `const`
#[doc(hidden)]
pub mod codegen {
    pub enum DispatchTarget {
        /// The function will construct and manage it's own `Routine`
        /// In some sense, this is equivalent to -> impl Future, except
        /// all plumbing is done by plug, not rustc
        Routine,

        /// The function behaves like a normal method,
        /// call plumbing is handled by rustc
        Value,
    }

    pub enum ReflectedDispatchKind {
        // Callable without any additional work
        Direct,
        Future,
        Routine,
    }
}

pub trait Object {
    type Config;
    const TAG: &str;
}

#[allow(async_fn_in_trait)]
pub trait Init: Object + Sized {
    type Error;

    async fn init(config: &Self::Config) -> Result<Self, Self::Error>;
}
