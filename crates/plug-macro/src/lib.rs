#![allow(dead_code)]

// TODO: syn is both heavy and too restrictive (disallows CFIT, finals)
// TODO: remove dep on syn_derive, replace with type-less impl (fixes `.inner` noise)

use proc_macro2::TokenStream;
use syn::{ItemImpl, ItemStruct, ItemTrait, Result, Token};
use syn_derive::Parse;

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

fn plug_impl(attrs: TokenStream, input: Code) -> Result<TokenStream> {
    match input {
        Code::Trait(x) => interface::trait_to_interface(syn::parse2(attrs)?, x),
        Code::Impl(x) => interface::register_impl(syn::parse2(attrs)?, x),
        Code::Struct(x) => object::struct_to_object(syn::parse2(attrs)?, x),
    }
}
