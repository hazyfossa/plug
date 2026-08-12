use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, Visibility};

pub trait MacroReturn {
    fn into_syn_result(self) -> syn::Result<TokenStream>;
}

impl MacroReturn for syn::Result<TokenStream> {
    fn into_syn_result(self) -> syn::Result<TokenStream> {
        self
    }
}

impl MacroReturn for TokenStream {
    fn into_syn_result(self) -> syn::Result<TokenStream> {
        Ok(self)
    }
}

// TODO: auto-define version for tests (syn::parse2)
// TODO: infallible macros (no flatten)
macro_rules! define {
    ($(#[$($attr:tt)+])* $name:ident = $impl:ident) => {
        $(#[$($attr)+])*
        #[proc_macro_attribute]
        pub fn $name(
            attrs: proc_macro::TokenStream,
            input: proc_macro::TokenStream,
        ) -> proc_macro::TokenStream {
            let predicate = |a,b| {
                let a = syn::parse(a)?;
                let b = syn::parse(b)?;
                Ok((a, b))
            };

            let ret = predicate(attrs.into(), input.into())
                .map(|(a,b)| $impl(a, b))
                .map($crate::MacroReturn::into_syn_result)
                .flatten();

            match ret {
                Ok(tokens) => tokens.into(),
                Err(e) => e.to_compile_error().into(),
            }
        }
    };
}
pub(crate) use define;

macro_rules! bail {
    (@span $tokens:expr) => { $tokens.span() };
    (@span) => { proc_macro2::Span::call_site() };

    ($($tokens:expr)? => $err:literal $($fmt:tt)*) => {
        return Err(syn::Error::new(
            $crate::bail!(@span $($tokens)?),
            format!($err $($fmt)*)
        ))
    };
}
pub(crate) use bail;

macro_rules! ensure_empty {
    ($tokens:expr, $($err:tt)*) => {
        if !$tokens.is_empty() {
            bail!($tokens => $($err)*);
        }
    };
}
pub(crate) use ensure_empty;

pub fn purescope(vis: Visibility, ident: Ident, content: TokenStream) -> TokenStream {
    quote! {
        #[allow(non_snake_case)]
        #vis mod #ident {
            pub use super::*;
            #content
        }
    }
}

macro_rules! tkpass {
    (struct $name:ident {
        $($field:ident: $type:ty),*
        $(,)?
    }) => {
        struct $name {
            $($field: $type),*
        }

        impl syn::parse::Parse for $name {
            fn parse(input: ParseStream) -> Result<Self> {
                $(
                    let $field;
                    syn::bracketed!($field in input);
                    let $field = $field.parse()?;
                )*

                Ok(Self { $(
                    $field,
                )* })

            }
        }

        impl quote::ToTokens for $name {
            fn to_tokens(&self, tokens: &mut TokenStream) {
                $(
                    syn::token::Bracket::default()
                    .surround(tokens, |cx|
                        self.$field.to_tokens(cx)
                    );
                )*

            }
        }
    };

    (enum $name:ident {
        $($field:ident)*
    }) => {
        enum $name { $($field)* }

        impl syn::parse::Parse for $name {
            fn parse(input: ParseStream) -> Result<Self> {
                $(
                    let $field;
                    syn::bracketed!($field in input);
                    let $field = $field.parse()?;
                )*

                Ok(Self { $(
                    $field,
                )* })

            }
        }

        impl quote::ToTokens for $name {
            fn to_tokens(&self, tokens: &mut TokenStream) {
                $(
                    syn::token::Bracket::default()
                    .surround(tokens, |cx|
                        self.$field.to_tokens(cx)
                    );
                )*

            }
        }
    }
}
pub(crate) use tkpass;
