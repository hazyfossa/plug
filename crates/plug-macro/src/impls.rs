use proc_macro2::TokenStream;
use syn::{ItemImpl, Result, Type, parse::Nothing, spanned::Spanned};

use crate::{bail, ensure_empty_tokens};

pub fn register_impl(attrs: Nothing, input: ItemImpl) -> Result<TokenStream> {
    let interface = match input.trait_ {
        Some((path, _)) => path,
        None => bail!(=> "This macro only makes sense for interface implementations"),
    };

    // NOTE: the following code does not actually check if the path resolves to a thing
    // that implements "plug::Object". It only saves downstream code from working with
    // obviously wrong inputs (since, for example, plug::Object will surely never be
    // implemented for a slice or tuple)
    let object = match *input.self_ty {
        Type::Path(x) => x.path,
        other => bail!(other => "Interfaces can only be implemented on objects"),
    };

    ensure_empty_tokens!(
        input.generics.params,
        "Generic interfaces are not supported (yet)"
    );

    todo!("get interface meta, codegen")
}
