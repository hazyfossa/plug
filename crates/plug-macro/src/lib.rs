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
use syn::{Attribute, ItemImpl, ItemStruct, ItemTrait, Result, Token, parse::Parse};

mod interface;
mod object;

mod utils;
use utils::*;

const NAME: &str = "plug";

define!(attribute plug = plug_impl);
define!(fn_like #[doc(hidden)] __import_advance = meta_passing::import_advance);

enum Code {
    Struct(ItemStruct),
    Trait(ItemTrait),
    Impl(ItemImpl),
}

impl Parse for Code {
    fn parse(input: syn::parse::ParseStream) -> Result<Self> {
        let tmp = input.fork();
        let _ = tmp.call(Attribute::parse_outer)?;
        let target = tmp.lookahead1();

        if target.peek(Token![trait]) {
            input.parse().map(Self::Trait)
        } else if target.peek(Token![impl]) {
            input.parse().map(Self::Impl)
        } else if target.peek(Token![struct]) {
            input.parse().map(Self::Struct)
        } else {
            Err(target.error())
        }
    }
}

fn plug_impl(attrs: TokenStream, input: Code) -> Result<TokenStream> {
    match input {
        Code::Trait(x) => interface::trait_to_interface(syn::parse2(attrs)?, x),
        Code::Impl(x) => interface::register_impl(syn::parse2(attrs)?, x),
        Code::Struct(x) => object::struct_to_object(syn::parse2(attrs)?, x),
    }
}
