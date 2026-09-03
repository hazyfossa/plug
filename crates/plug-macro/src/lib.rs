#![allow(dead_code)]

// TODO: syn is both heavy and too restrictive (disallows CFIT, finals)

// TODO: it is most definitely possible to avoid some metadata passing
// at an unclear performance cost
//
// note that such mode of operation will even be required for dyn impls
// if the cost is small enough, we may get rid of compile-time costly
// metadata passing and resolve all FnKind mismatches via dyn path
//
// After some pondering, the cost seems to be a branch on associated const
// (per call). Check if compiler optimizes.
//
// there is also the issue of checking if all impls of an async fn
// are outlined, without which full outline optimization (to sync fn)
// is impossible. Options:
// 1. Clever tricks over [<Variant as ThisInterface::ImplMeta>::Meta::X_METHOD_OUTLINED, ...]
// 2. Do not make it possible for impls to override (all async dispatch becomes `final`)
//
// Consider also: a hybrid approach, where Interface passes a lot more metadata, but one-way.
// Based on that, enforce invariants at impl time, which helps with dispatch somewhat

use proc_macro2::TokenStream;
use syn::{ItemImpl, ItemStruct, ItemTrait, Result, Token, parse::Parse};
use syn_derive::Parse;

mod impls;
mod interface;
mod object;

mod utils;
use utils::*;

const NAME: &str = "plug";

define!(attribute plug = plug_impl);

#[cfg(feature = "meta-passing")]
define!(fn_like #[doc(hidden)] __import_advance = meta_passing::import_advance);
#[cfg(not(feature = "meta-passing"))]
compile_error!(
    "Plug currently always requires full meta passing. This may be relaxed in the future"
);

#[derive(Parse)]
enum Code {
    #[parse(peek = Token![struct])]
    Struct(ItemStruct),
    #[parse(peek = Token![trait])]
    Trait(ItemTrait),
    #[parse(peek = Token![impl])]
    Impl(ItemImpl),
}

#[rustfmt::skip]
fn plug_impl(attrs: TokenStream, input: Code) -> Result<TokenStream> {
    match input {
        Code::Trait(x)  => interface::trait_to_interface (syn::parse2(attrs)?, x),
        Code::Struct(x) => object::struct_to_object      (syn::parse2(attrs)?, x),
        Code::Impl(x)   => impls::register_impl          (syn::parse2(attrs)?, x),
    }
}
