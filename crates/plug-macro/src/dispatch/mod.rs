use proc_macro2::TokenStream;
use syn::{Path, Result};

use crate::InterfaceShape;

#[cfg(feature = "direct")]
pub mod direct;

pub mod dynamic;

// TODO: common "clean args" function

pub trait Dispatch {
    fn dispatch(shape: InterfaceShape) -> Result<TokenStream>;
    fn register_impl(target_interface: Path, object: Path) -> Result<TokenStream>;
}
