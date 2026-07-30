use std::ops::Deref;

use dyn_utils::{
    DynObject,
    object::DynTrait,
    storage::{DefaultStorage, Storage},
};

use facet::Facet;

hazymacros::setup!(extern _h);
use hazymacros::trait_alias;

#[derive(Facet)]
pub struct ConfigCell<T: Pluggable> {
    #[facet(flatten)]
    pub common: T::CommonConfig,
    #[facet(flatten)]
    specific: T,
}

// Make specific configuration available by default
impl<T: Pluggable> Deref for ConfigCell<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.specific
    }
}

trait_alias!(pub FromConfig: for<'a> Facet<'a>);

pub trait Pluggable: Sized + DynTrait {
    type CommonConfig: FromConfig;
    fn plug<C>(config: &C) -> Option<ConfigCell<Self>>;
}

pub struct Plugged<T: Pluggable, S: Storage = DefaultStorage> {
    object: DynObject<T, S>,
}

macro_rules! define {
    ($vis:vis trait $name:ident { $($body:tt)* }) => {};
}

macro_rules! root {
    () => {};
}
