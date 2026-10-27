use serde::{Deserialize, Serialize};

pub use plug_macro::plug;

pub mod future;
pub mod remote;

pub trait BorrowedData<'a>: Serialize + Deserialize<'a> + 'a {}
impl<'a, T: 'a + Serialize + Deserialize<'a>> BorrowedData<'a> for T {}

pub trait Data: for<'a> BorrowedData<'a> + 'static {}
impl<T: for<'a> BorrowedData<'a> + 'static> Data for T {}

pub trait InterfaceDescriptor {
    // type Error;
    type Tag: BorrowedData<'static> + PartialEq;
    const ALL: &[Self::Tag];
}

pub trait Object {
    type Config: Data;
}

// TODO: properly split initialization (memory gather) and construction (memory map: cfg -> state)
#[allow(async_fn_in_trait)]
pub trait Init: Object + Sized {
    // TODO: optimize async init via AsyncMethod
    async fn init(config: &Self::Config) -> eyre::Result<Self>;
}

pub trait Tagged<I: InterfaceDescriptor> {
    const TAG: I::Tag;
}

#[doc(hidden)]
pub mod __codegen {
    // #[cfg(feature = "dyn")]
    // pub use super::dyn_codegen::*;

    pub use eyre; // TODO: better error handling
    pub use plug_macro;
    pub use serde;
}

#[cfg(feature = "dyn")]
mod dyn_codegen {
    // use super::*;
    // use future::*;

    // pub trait DynamicImpl<I: InterfaceDescriptor> {
    //     fn tag() -> I::Tag;
    //     fn init(config: Value) -> DynAsyncMethod<Result<Box<Self>>>;
    // }

    // impl<I, T> DynamicImpl<I> for T
    // where
    //     I: InterfaceDescriptor,
    //     T: Init + Tagged<I> + Object,
    // {
    //     fn tag() -> I::Tag {
    //         T::TAG
    //     }

    //     fn init(config: Value) -> DynAsyncMethod<Result<Box<Self>>> {
    //         AsyncMethod::Deferred(async {
    //             let config: T::Config = facet_value::from_value(config)?;
    //             let ret = <T as Init>::init(&config).await?;
    //             let stored_self = Box::new(ret);
    //             Ok(stored_self)
    //         })
    //         .box_dynamic()
    //     }
    // }
}
