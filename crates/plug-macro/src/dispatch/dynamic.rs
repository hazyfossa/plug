use proc_macro2::TokenStream;
use syn::{Path, Result, Type};

pub struct Impl;

impl super::Dispatch for Impl {
    fn dispatch(attrs: TokenStream, shape: super::InterfaceShape) -> Result<TokenStream> {
        todo!()
    }

    fn register_impl(target_interface: Path, object: Box<Type>) -> Result<TokenStream> {
        todo!()
    }
}
