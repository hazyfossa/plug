use proc_macro2::{Literal, TokenStream};
use quote::{format_ident, quote};
use syn::{Field, ItemStruct, Result, Type, spanned::Spanned};

use crate::bail;

struct StateMatcher {
    passed_marker: bool,
}

impl StateMatcher {
    fn new() -> Self {
        Self {
            passed_marker: false,
        }
    }

    fn is_state(&mut self, field: &Field) -> Result<Option<bool>> {
        // TODO: consider other approaches
        // (but not attrs, they do not work)
        if matches!(field.ty, Type::Never(_)) {
            if self.passed_marker {
                bail!(field.ty => "An object cannot have >1 state separator");
            } else {
                self.passed_marker = true;
                return Ok(None);
            }
        }

        Ok(Some(self.passed_marker))
    }
}

pub fn struct_to_object(attrs: TokenStream, input: ItemStruct) -> Result<TokenStream> {
    let tag: Option<Literal> = syn::parse2(attrs)?; // TODO

    let tag = tag
        .map(|x| x.to_string())
        .unwrap_or(input.ident.to_string());

    let mut config: Vec<Field> = Vec::new();
    let mut state: Vec<Field> = Vec::new();

    let mut state_matcher = StateMatcher::new();

    for field in input.fields {
        let target = match state_matcher.is_state(&field)? {
            Some(true) => &mut state,
            Some(false) => &mut config,
            None => continue,
        };

        target.push(field);
    }

    let attrs = input.attrs;
    let name = input.ident;
    let config_ident = format_ident!("{name}Config");

    let maybe_empty_init = state.is_empty().then_some(quote! {
        // #[automatically_derived]
        impl ::plug::Init for #name {
            fn init(_: &Self::Config) -> ::plug::Construct<Self> {
                let instance = #name {};
                ::plug::Routine::define_direct(Ok(instance))
            }
        }
    });

    let content = quote! {
        #(#attrs)*
        pub struct #config_ident {
            #(#config,)*
        }

        // TODO: allow borrows from self
        //
        // TODO: make Borrow<Config> part of self
        // (combined with self-ref allows borrow from Config)
        pub struct #name {
            #(#state,)*
        }

        impl ::plug::Object for #name {
            type Config = #config_ident;
            const TAG: &str = #tag;
        }

        #maybe_empty_init
    };

    Ok(content)
}
