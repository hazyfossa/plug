use proc_macro2::TokenStream;
use quote::{ToTokens, TokenStreamExt, quote};
use syn::{
    Ident, Token, Visibility,
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
};

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

    ($($tokens:expr)? => $($fmt:tt)*) => {
        return Err(syn::Error::new(
            $crate::bail!(@span $($tokens)?),
            format!($($fmt)*)
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

pub struct TokenVec<T>(pub Vec<T>);

impl<T: Parse> Parse for TokenVec<T> {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Punctuated::<T, Token![,]>::parse_terminated(input).map(|x| Self(x.into_iter().collect()))
    }
}

impl<T: ToTokens> ToTokens for TokenVec<T> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.append_terminated(&self.0, syn::token::Comma::default());
    }
}
