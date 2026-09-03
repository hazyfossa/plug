use proc_macro2::TokenStream;
use syn::{ImplItem, Path, Result};

pub struct Impl;

impl super::Dispatch for Impl {
    fn dispatch(attrs: TokenStream, shape: super::InterfaceShape) -> Result<TokenStream> {
        todo!()
    }

    fn register_impl(
        target_interface: Path,
        object: Path,
        _body: Vec<ImplItem>,
    ) -> Result<TokenStream> {
        todo!()
    }
}
