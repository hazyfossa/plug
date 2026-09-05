use std::collections::HashMap;

use proc_macro2::TokenStream;
use quote::format_ident;
use serde::{Deserialize, Serialize};
use syn::{Ident, ItemImpl, Path, Result, Type, parse::Nothing, spanned::Spanned};

use crate::{
    FnKind, InterfaceMeta, bail, ensure_empty_tokens,
    interface::{self, Mode},
    interface_meta_marker,
    meta_passing::with_import,
    parse::path_sibling,
    syn_serde::ViaSerde,
};

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

    let meta_source = path_sibling(&interface, interface_meta_marker)?;

    with_import!(#simple meta_source => register_impl_inner(object))
}

fn register_impl_inner(object: Path, meta: ViaSerde<InterfaceMeta>) -> Result<TokenStream> {
    let meta = meta.0;
    let x = matches!(meta.mode, Mode::Dynamic);
    bail!(object => "{x}");
}
