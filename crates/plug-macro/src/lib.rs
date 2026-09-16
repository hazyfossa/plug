// TODOs:
// [ ] remove dep on syn_derive, replace with type-less impl (fixes `.inner` noise)
// [ ] hide __meta! better (consider submodule)
// [ ] allow #[plug(const)] as alternative to `const fn`
// [ ] support passing attrs (requires change of ret convention)
// [ ] Tag <-> String for enums
// [ ] consider moving `impl` into separate crate for ergonomics (rust analyzer excludes)
// [ ] loading interfaces from config
// [ ] storage model (blocked on paradigm)
// [ ] (try to) rewrite attr parsing
// [ ] dyn path

use proc_macro2::TokenStream;
use syn::{ItemImpl, ItemStruct, ItemTrait, Result, Token};
use syn_derive::Parse;

mod interface;
mod object;

mod utils;
use utils::*;

const NAME: &str = "plug";

define!(attribute plug = plug_impl);
define!(fn_like #[doc(hidden)] __import_advance = meta_passing::import_advance);

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
