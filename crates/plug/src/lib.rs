use eyre::Result;
use facet::Facet;
pub use plug_macro::plug;
use std::{
    mem,
    pin::Pin,
    task::{Context, Poll},
};

mod future;

pub trait Reflected: for<'a> Facet<'a> {}
impl<T: for<'a> Facet<'a>> Reflected for T {}

// TODO: replace Box<dyn T> and eyre::Error
// with Stored Objects (blocked on paradigm)

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
    use future::*;

    use facet_value::Value;

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
            .box_dynamic()
        }
    }
}
