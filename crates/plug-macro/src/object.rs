use proc_macro2::TokenStream;
use quote::quote;
use syn::{
    Field, ItemStruct, Result, Type, Visibility, parse::Nothing, parse_quote, spanned::Spanned,
};

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
    let _: Nothing = syn::parse2(attrs)?; // TODO: tag

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

        impl ::plug::State for State {
            type Config = Config;
        }
    };

    Ok(purescope(input.vis, input.ident, content))
}
