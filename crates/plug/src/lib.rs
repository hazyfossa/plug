use std::ops::Deref;

use dyn_utils::{
    DynObject,
    object::DynTrait,
    storage::{DefaultStorage, Storage},
};

// hazymacros::setup!(extern _h);
// use hazymacros::trait_alias;

#[doc(hidden)]
pub mod __codegen {
    pub use dyn_utils;
    pub use paste;
    pub use typetag;
}

pub use dyn_utils::storage;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

#[derive(Serialize, Deserialize)]
pub struct ConfigCell<T: Pluggable> {
    #[serde(flatten)]
    pub common: T::CommonConfig,
    #[serde(flatten)]
    specific: T,
}

// Make specific configuration available by default
impl<T: Pluggable> Deref for ConfigCell<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.specific
    }
}

// trait_alias!(pub FromConfig: Serialize + DeserializeOwned);

pub trait Pluggable: Sized + DynTrait {
    type CommonConfig: FromConfig;
    fn plug<C>(config: &C) -> Option<ConfigCell<Self>>;
}

pub struct Plugged<T: Pluggable, S: Storage = DefaultStorage> {
    object: DynObject<T, S>,
}

// TODO: we can maybe get away without `dyn_object` on trait
// this will require enum_dispatch and listing all impls on define explicitly
// the latter may be made amenable with plug_mod
// note: this will still require `dyn_trait` for future dispatch

// TODO: we can cut `paste` dependency if we manually write DynTrait and run `dyn_object` directly on it
// which seems doable and even reasonable
#[macro_export]
macro_rules! define {
    ($vis:vis trait $name:ident { $($body:tt)* }) => {
        $crate::_h::hazymacros::purescope! { $vis $name {
            use $crate::__codegen::*;
            use facet::Shape;
            use scattered_collect::{gather, slice::ScatteredSlice};

            #[dyn_trait]
            #[dyn_trait(dyn_object)]
            pub trait Interface { $($body)* }

            paste::paste! { pub type Object = dyn_utils::object::DynObject<dyn [<Dyn $name>]> }
        }}
    };
}

// macro_rules! root {
//     () => {};
// }
