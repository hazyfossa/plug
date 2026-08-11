// TODO: syn is both heavy and too restrictive (disallows CFIT)

#![allow(dead_code)]

use proc_macro as raw;
use proc_macro2::TokenStream;
use syn::{
    ItemImpl, ItemStruct, ItemTrait, Path, Result, Token,
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
    spanned::Spanned,
};

mod utils;
use utils::*;

#[proc_macro_attribute]
pub fn plug(attrs: raw::TokenStream, input: raw::TokenStream) -> raw::TokenStream {
    match plug_impl(attrs.into(), input.into()) {
        Ok(tokens) => tokens.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

enum Code {
    Struct(ItemStruct),
    Trait(ItemTrait),
    Impl(ItemImpl),
}

impl Parse for Code {
    fn parse(input: ParseStream) -> Result<Self> {
        let lookahead = input.lookahead1();

        if lookahead.peek(syn::Token![struct]) {
            input.parse().map(Self::Struct)
        } else if lookahead.peek(syn::Token![trait]) {
            input.parse().map(Self::Trait)
        } else if lookahead.peek(syn::Token![impl]) {
            input.parse().map(Self::Impl)
        } else {
            Err(lookahead.error())
        }
    }
}

fn plug_impl(attrs: TokenStream, input: TokenStream) -> Result<TokenStream> {
    let code: Code = syn::parse2(input)?;

    match code {
        Code::Trait(x) => trait_to_interface(attrs, x),
        Code::Struct(x) => struct_to_object(x),
        Code::Impl(x) => register_impl(x),
    }
}

#[derive(Debug)]
enum Mode {
    Direct,
    FromMod,
}

#[derive(Debug)]
struct ParsedAttrs {
    pub mode: Mode,
    pub paths: Vec<Path>,
}

impl Parse for ParsedAttrs {
    fn parse(input: ParseStream) -> Result<Self> {
        let ident: syn::Ident = input.parse()?;

        let content;
        syn::parenthesized!(content in input);

        let paths: Vec<Path> = Punctuated::<Path, Token![,]>::parse_terminated(&content)?
            .into_iter()
            .collect();

        ensure_empty!(input, "unexpected tokens after attribute arguments");

        let mode = match ident.to_string().as_str() {
            "direct" => Mode::Direct,
            "from_mod" => Mode::FromMod,
            other => {
                bail!(ident, "expected `direct` or `from_mod`, found `{other}`");
            }
        };

        Ok(ParsedAttrs { mode, paths })
    }
}

impl ParsedAttrs {
    fn resolve_impls(self) -> Vec<Path> {
        match self.mode {
            Mode::Direct => self.paths,
            Mode::FromMod => todo!(),
        }
    }
}

fn trait_to_interface(attrs: TokenStream, input: ItemTrait) -> Result<TokenStream> {
    ensure_empty!(
        input.generics.params,
        "Generic objects are not supported (yet)"
    );

    let attrs: ParsedAttrs = syn::parse2(attrs)?;
    let impls = attrs.resolve_impls();

    todo!()
}

fn struct_to_object(input: ItemStruct) -> Result<TokenStream> {
    todo!()
}

fn register_impl(input: ItemImpl) -> Result<TokenStream> {
    todo!()
}
