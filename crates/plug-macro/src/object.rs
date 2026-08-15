use proc_macro2::{Literal, TokenStream};
use quote::quote;
use syn::{Field, ItemStruct, Result, Type, Visibility, parse_quote, spanned::Spanned};

use crate::{bail, purescope};

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
    let tag: Option<Literal> = syn::parse2(attrs)?;

    let tag = tag
        .map(|x| x.to_string())
        .unwrap_or(input.ident.to_string());

    let mut config: Vec<Field> = Vec::new();
    let mut state: Vec<Field> = Vec::new();

    let mut state_matcher = StateMatcher::new();

    for mut field in input.fields {
        let target = match state_matcher.is_state(&field)? {
            Some(true) => &mut state,
            Some(false) => &mut config,
            None => continue,
        };

        if matches!(field.vis, Visibility::Inherited) {
            field.vis = parse_quote! { pub(super) }
        };

        target.push(field);
    }

    let attrs = input.attrs;

    let content = quote! {
        #(#attrs)*
        pub struct Config {
            #(#config,)*
        }

        // TODO: allow borrows from self
        //
        // TODO: make Borrow<Config> part of self
        // (combined with self-ref allows borrow from Config)
        pub struct State {
            #(#state,)*
        }

        #[doc(hidden)]
        pub struct T;

        impl ::plug::Object for T {
            type Config = Config;
            type State = State;

            const TAG: &str = #tag;
        }
    };

    Ok(purescope(input.vis, input.ident, content))
}
