use proc_macro2::TokenStream;
use syn::{Path, Result};

use crate::InterfaceShape;

#[cfg(feature = "direct")]
pub mod direct;

pub mod common;
pub mod dynamic;

pub trait Dispatch {
    fn dispatch(attrs: TokenStream, shape: InterfaceShape) -> Result<TokenStream>;
    fn register_impl(target_interface: Path, object: Path) -> Result<TokenStream>;
}

#[cfg(feature = "direct")]
pub type Impl = direct::Impl;

#[cfg(not(feature = "direct"))]
pub type Impl = dynamic::Impl;
