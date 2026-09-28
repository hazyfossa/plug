use eyre::Result;
use facet::Facet;
pub use plug_macro::plug;

mod future;

// TODO: replace Box<dyn T> and eyre::Error
// with Stored Objects (blocked on paradigm)

// TODO(err): it is logical for the error enum to be associated with an interface, not
// individual objects

pub trait InterfaceDescriptor {
    type Tag: PartialEq + 'static;
    const ALL: &[Self::Tag];
}

pub trait Reflected: for<'a> Facet<'a> {}
impl<T: for<'a> Facet<'a>> Reflected for T {}

pub trait Object {
    type Config: Reflected + Default;
}

// TODO: properly split initialization (memory gather) and construction (memory map: cfg -> state)
#[allow(async_fn_in_trait)]
pub trait Init: Object + Sized {
    // TODO: async init via AsyncMethod
    async fn init(config: &Self::Config) -> Result<Self>;
}

pub trait Tagged<I: InterfaceDescriptor> {
    const TAG: I::Tag;
}

#[cfg(feature = "dyn")]
#[doc(hidden)]
pub mod __dyn_codegen {
    use super::*;
    use future::*;

    use facet_value::Value;

    pub trait DynamicImpl<I: InterfaceDescriptor> {
        fn tag() -> I::Tag;
        fn init(config: Value) -> DynAsyncMethod<Result<Box<Self>>>;
    }

    impl<I, T> DynamicImpl<I> for T
    where
        I: InterfaceDescriptor,
        T: Init + Tagged<I> + Object,
    {
        fn tag() -> I::Tag {
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
