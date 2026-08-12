// TODO: syn is both heavy and too restrictive (disallows CFIT)

#![allow(dead_code)]

use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, quote};
use syn::{
    Field, Ident, ItemImpl, ItemStruct, ItemTrait, Path, Result, Token, punctuated::Punctuated,
    spanned::Spanned,
};

mod utils;
use utils::*;

define!(plug = plug_impl);

fn export(marker: &str, input: impl ToTokens) -> TokenStream {
    let ident = Ident::new(marker, Span::call_site());

    quote! {
        #[doc(hidden)]
        #[macro_export(local_inner_macros)]
        macro_rules! #ident {
            () => { #input };
        }

        pub(crate) use #ident;
    }
}

// TODO: figure out if this is useful or insane

// struct WithPart<Mark, Other> {
//     imported: TokenStream,
//     other: Other,
//     _mark: PhantomData<Mark>,
// }

// impl<Mark, Other> Deref for WithPart<Mark, Other> {
//     type Target = Other;
//     fn deref(&self) -> &Self::Target {
//         &self.other
//     }
// }

// impl<Mark, Other> Parse for WithPart<Mark, Other>
// where
//     Mark: Token + Parse,
//     Other: Parse,
// {
//     fn parse(input: ParseStream) -> Result<Self> {
//         input.parse::<Mark>()?; // TODO: context about parts here

//         let imported;
//         syn::bracketed!(imported in input);

//         Ok(Self {
//             imported: imported.parse()?,
//             other: input.parse()?,
//             _mark: PhantomData,
//         })
//     }
// }

#[derive(syn_derive::Parse)]
enum Code {
    #[parse(peek = Token![struct])]
    Struct(ItemStruct),
    #[parse(peek = Token![trait])]
    Trait(ItemTrait),
    #[parse(peek = Token![impl])]
    Impl(ItemImpl),
}

// impl Parse for Code {
//     fn parse(input: ParseStream) -> Result<Self> {
//         let lookahead = input.lookahead1();

//         if lookahead.peek(syn::Token![struct]) {
//             input.parse().map(Self::Struct)
//         } else if lookahead.peek(syn::Token![trait]) {
//             input.parse().map(Self::Trait)
//         } else if lookahead.peek(syn::Token![impl]) {
//             input.parse().map(Self::Impl)
//         } else {
//             Err(lookahead.error())
//         }
//     }
// }

fn plug_impl(attrs: TokenStream, input: Code) -> Result<TokenStream> {
    match input {
        Code::Trait(x) => trait_to_interface(attrs, x),
        Code::Struct(x) => struct_to_object(x),
        Code::Impl(x) => register_impl(x),
    }
}

#[derive(syn_derive::Parse, syn_derive::ToTokens)]
enum Mode {
    Direct,
    FromMod,
}

#[derive(syn_derive::Parse, syn_derive::ToTokens)]
struct ParsedAttrs {
    pub mode: Mode,

    #[parse(Punctuated::parse_terminated)]
    pub paths: Punctuated<Path, Token![,]>,
}

impl ParsedAttrs {
    fn resolve_impls(self) -> Vec<Path> {
        match self.mode {
            Mode::Direct => self.paths.into_iter().collect(),
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

    let content = quote! {};

    Ok(purescope(input.vis, input.ident, content))
}

fn struct_to_object(input: ItemStruct) -> Result<TokenStream> {
    let config: Vec<Field> = Vec::new();
    let state: Vec<Field> = Vec::new();

    let content = quote! {};

    Ok(purescope(input.vis, input.ident, content))
}

const REGISTRATION_MARKER: &str = "__primary_object_for_this_module";

fn register_impl(input: ItemImpl) -> Result<TokenStream> {
    let interface = match input.trait_ {
        Some((path, _)) => path,
        None => bail!(=> "This macro only makes sense for interface implementations"),
    };

    let content = quote! {};

    Ok(content)
}

#[derive(syn_derive::Parse, syn_derive::ToTokens)]
enum FnKind {
    Regular,
    Async,
    Const,
}

#[derive(syn_derive::Parse, syn_derive::ToTokens)]
struct FnShape {
    name: Ident,
    kind: FnKind,
}

#[derive(syn_derive::Parse, syn_derive::ToTokens)]
struct MetaForInterface {
    mode: Mode,

    #[parse(Punctuated::parse_terminated)]
    shape: Punctuated<FnShape, Token![,]>,
}

fn register_impl_inner(object: Ident, resolution_mode: Mode) -> TokenStream {
    match resolution_mode {
        Mode::Direct => TokenStream::new(),
        Mode::FromMod => export(REGISTRATION_MARKER, object),
    }
}
