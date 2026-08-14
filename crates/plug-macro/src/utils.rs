use std::marker::PhantomData;

use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, TokenStreamExt, quote};
use serde::{Deserialize, Serialize};
use syn::{
    Attribute, Ident, Meta, Result, Token, Visibility,
    parse::{Parse, ParseStream},
    parse_quote,
    punctuated::Punctuated,
    spanned::Spanned,
};

pub trait MacroReturn {
    fn into_syn_result(self) -> Result<TokenStream>;
}

impl MacroReturn for Result<TokenStream> {
    fn into_syn_result(self) -> Result<TokenStream> {
        self
    }
}

impl MacroReturn for TokenStream {
    fn into_syn_result(self) -> Result<TokenStream> {
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

pub struct Many<T, C = Vec<T>> {
    pub inner: C,
    _phantom: PhantomData<T>,
}

impl<T, C: FromIterator<T>> FromIterator<T> for Many<T, C> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Self {
            inner: iter.into_iter().collect(),
            _phantom: PhantomData,
        }
    }
}

impl<T: Parse, C: FromIterator<T>> Parse for Many<T, C> {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Punctuated::<T, Token![,]>::parse_terminated(input).map(|x| x.into_iter().collect())
    }
}

impl<T, C> ToTokens for Many<T, C>
where
    T: ToTokens,
    for<'a> &'a C: IntoIterator<Item = &'a T>,
{
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.append_terminated(&self.inner, syn::token::Comma::default());
    }
}

// Generic span monad
#[cfg_attr(feature = "direct", derive(Serialize, Deserialize))]
pub struct WithSpan<T> {
    inner: T,

    #[cfg_attr(feature = "direct", serde(skip))]
    span: Option<Span>,
}

impl<T> WithSpan<T> {
    fn new(value: T, span: Span) -> Self {
        Self {
            inner: value,
            span: Some(span),
        }
    }

    fn span(&self) -> Span {
        self.span
            .expect("Cannot rely on spans of values that have been passed thorugh metadata")
    }

    pub fn map<U, F>(self, f: F) -> WithSpan<U>
    where
        F: FnOnce(T) -> U,
    {
        WithSpan {
            inner: f(self.inner),
            span: self.span,
        }
    }
}

impl<T> std::ops::Deref for WithSpan<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

pub struct Attrs(Vec<Attribute>);

impl Attrs {
    pub fn extract(input: &mut Vec<Attribute>) -> Result<Self> {
        input
            .extract_if(.., |attr| attr.path().is_ident(crate::NAME))
            .map(|attr| {
                let meta = &attr.meta.require_list()?.tokens;
                Ok(parse_quote!(#[#meta]))
            })
            .collect::<Result<_>>()
            .map(Self)
    }

    // This is very far from performant, but it works
    pub fn pull<'a, 'b>(
        &'a mut self,
        flags: impl IntoIterator<Item = &'b str>,
    ) -> Result<Option<Attribute>> {
        let flags: Vec<_> = flags.into_iter().collect();

        let mut matches = self.0.extract_if(.., |attr| {
            flags.iter().any(|flag| attr.path().is_ident(flag))
                // Ensure a bare flag // TODO: or not
                && matches!(attr.meta, Meta::Path(_))
        });

        let ret = matches.next();

        if let Some(second_match) = matches.next() {
            // TODO: is it possible to point out the first one here?
            bail!(second_match => "duplicate attribute");
        }

        Ok(ret)
    }

    pub fn pull_tokenum<T: TokEnum>(&mut self) -> Result<Option<WithSpan<T>>> {
        self.pull(T::all_states())?
            .map(|arg| {
                arg.parse_args()
                    .map(|value| WithSpan::new(value, arg.span()))
            })
            .transpose()
    }
}

pub trait TokEnum: Parse {
    fn all_states() -> impl IntoIterator<Item = &'static str>;
}

macro_rules! tokenum {
    (
        $(#[$($attr:meta)*])*
        $vis:vis enum $name:ident {
            $($field:ident $(= $str:literal)?),*
            $(,)?
        }
) => {
        $(#[$($attr)*])*
        $vis enum $name {
            $($field),*
        }

        impl syn::parse::Parse for $name {
            fn parse(input: ParseStream) -> syn::Result<Self> {
                let ident: syn::Ident = input.parse()?;

                let ret = match &*ident.to_string() {
                    $($crate::tokenum!(@str $field $($str)?) => Self::$field,)*
                    // TODO: expected one of
                    other => bail!(other => "unexpected value: {other}")
                };

                Ok(ret)
            }
        }

        impl TokEnum for $name {
            fn all_states() -> impl IntoIterator<Item = &'static str> {
                [$( $crate::tokenum!(@str $field $($str)?) ),*]
            }
        }
    };

    (@str $field:ident $str:literal) => { $str };
    (@str $field:ident) => { stringify!($field) };
}
pub(crate) use tokenum;

pub fn retain_by_mask<T>(mask: &[bool], values: &mut Vec<T>) {
    assert_eq!(mask.len(), values.len());

    let mut iter = mask.iter();

    // Compiler should eliminate unwraps with the assert above
    values.retain(|_| *iter.next().unwrap());
}
