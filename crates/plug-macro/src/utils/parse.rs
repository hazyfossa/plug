use std::{marker::PhantomData, ops::Deref};

use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, TokenStreamExt};
use serde::{Deserialize, Serialize};
use syn::{
    Attribute, Ident, Meta, Path, Result, Token,
    parse::{Parse, ParseStream},
    parse_quote,
    punctuated::Punctuated,
    spanned::Spanned,
};

// Many

pub struct Many<T, C = Vec<T>> {
    pub inner: C,
    _phantom: PhantomData<T>,
}

impl<T, C> From<C> for Many<T, C> {
    fn from(value: C) -> Self {
        Self {
            inner: value,
            _phantom: PhantomData,
        }
    }
}

impl<T, C: FromIterator<T>> FromIterator<T> for Many<T, C> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let collection = iter.into_iter().collect::<C>();
        Self::from(collection)
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

impl<T, C> Deref for Many<T, C> {
    type Target = C;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

// Tokens

pub struct Tokens(pub TokenStream);

impl Parse for Tokens {
    fn parse(input: ParseStream) -> Result<Self> {
        let content;
        syn::bracketed!(content in input);
        content.parse().map(Self)
    }
}

impl ToTokens for Tokens {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.append_all(self.0.clone());
    }

    fn into_token_stream(self) -> TokenStream
    where
        Self: Sized,
    {
        self.0
    }
}

// Generic span monad

#[derive(Clone, Serialize, Deserialize)]
pub struct WithSpan<T> {
    inner: T,

    #[cfg_attr(feature = "meta-passing", serde(skip))]
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

// Attribute parsing

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
    pub fn select_one<'a, 'b>(
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

    pub fn select_tokenum<T: TokEnum>(&mut self) -> Result<Option<WithSpan<T>>> {
        self.select_one(T::all_states())?
            .map(|arg| {
                arg.parse_args()
                    .map(|value| WithSpan::new(value, arg.span()))
            })
            .transpose()
    }
}

// token enum
// TODO: non-ident reprs

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
            $($field { span: proc_macro2::Span } ),*
        }

        impl syn::parse::Parse for $name {
            fn parse(input: ParseStream) -> syn::Result<Self> {
                let ident: syn::Ident = input.parse()?;
                let span = syn::spanned::Spanned::span(&ident);

                let ret = match &*ident.to_string() {
                    $($crate::parse::tokenum!(@str $field $($str)?) => Self::$field { span },)*
                    // TODO: expected one of
                    other => bail!(other => "unexpected value: {other}")
                };

                Ok(ret)
            }
        }

        impl $crate::utils::parse::TokEnum for $name {
            fn all_states() -> impl IntoIterator<Item = &'static str> {
                [$( $crate::parse::tokenum!(@str $field $($str)?) ),*]
            }
        }
    };

    (@str $field:ident $str:literal) => { $str };
    (@str $field:ident) => { stringify!($field) };
}
pub(crate) use tokenum;

use crate::bail;

pub fn path_ident(path: &Path) -> Result<&Ident> {
    match path.segments.last() {
        Some(x) => Ok(&x.ident),
        None => bail!(path => "expected non-empty path"),
    }
}

// TODO: reconsider this as a pattern
// better written as replace_path_ident which makes a clone
pub fn path_ident_mut(path: &mut Path) -> Result<&mut Ident> {
    let span = path.span(); // This is purely for for borrowck happiness

    match path.segments.last_mut() {
        Some(x) => Ok(&mut x.ident),
        None => bail!(span => "expected non-empty path"),
    }
}
