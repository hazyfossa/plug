use std::marker::PhantomData;

use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, TokenStreamExt, quote};
use serde::{Deserialize, Serialize};
use syn::{
    Attribute, Ident, Meta, MetaList, Path, Result, Token,
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
    spanned::Spanned,
};
use syn_derive::ToTokens;

// Many

// TODO: nicer signature for custom collections (GAT)
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
        tokens.append_terminated(&self.inner, Token![,](Span::call_site()));
    }
}

// Bracketed

pub struct Bracketed<T> {
    pub inner: T,
}

impl<T> From<T> for Bracketed<T> {
    fn from(value: T) -> Self {
        Self { inner: value }
    }
}

impl<T> Parse for Bracketed<T>
where
    T: Parse,
{
    fn parse(input: syn::parse::ParseStream) -> Result<Self> {
        let content;
        syn::bracketed!(content in input);
        let inner = content.parse()?;
        Ok(Self { inner })
    }
}

impl<T> ToTokens for Bracketed<T>
where
    T: ToTokens,
{
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let this = &self.inner;
        tokens.append_all(quote! { [#this] });
    }

    fn into_token_stream(self) -> TokenStream
    where
        Self: Sized,
    {
        let this = &self.inner;
        quote! { [#this] }
    }
}

// Tokens
pub type Tokens = Bracketed<TokenStream>;

// Generic span monad

#[derive(Clone, Serialize, Deserialize)]
pub struct WithSpan<T> {
    inner: T,

    #[cfg_attr(feature = "meta-passing", serde(skip))]
    span: Option<Span>,
}

impl<T> WithSpan<T> {
    pub fn new(value: T, span: Span) -> Self {
        Self {
            inner: value,
            span: Some(span),
        }
    }

    pub fn span(&self) -> Span {
        self.span
            .expect("Cannot rely on spans of values that have been passed through metadata")
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

    pub fn forget_span(self) -> T {
        self.inner
    }
}

impl<T> std::ops::Deref for WithSpan<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

// Attribute parsing

#[derive(ToTokens)]
pub struct Attrs(Many<MetaList>);

impl Parse for Attrs {
    fn parse(input: ParseStream) -> Result<Self> {
        // TODO
        // input.parse().map(Self)
        Ok(Self(Many::from(Vec::new())))
    }
}

impl Attrs {
    pub fn extract(input: &mut Vec<Attribute>) -> Self {
        // TODO: simplify?
        let inner = input
            .extract_if(.., |attr| {
                attr.meta
                    .require_list()
                    .is_ok_and(|x| x.path.is_ident(crate::NAME))
            })
            .map(|attr| match attr.meta {
                Meta::List(x) => x,
                _ => unreachable!(),
            })
            .collect::<Vec<_>>();

        Self(inner.into())
    }

    // This is very far from performant, but it works
    // TODO: rewrite without extract
    pub fn select_one<'a, 'b>(
        &'a mut self,
        flags: impl IntoIterator<Item = &'b str>,
    ) -> Result<Option<Ident>> {
        let flags: Vec<_> = flags.into_iter().collect();

        let mut matches = self
            .0
            .inner
            .extract_if(.., |attr| flags.iter().any(|flag| attr.path.is_ident(flag)));

        let ret = match matches.next() {
            Some(x) => {
                ensure_empty_tokens!(x.tokens, "this is a flag-style attribute");

                // Unwrap is guarded by extraction case above
                x.path.get_ident().unwrap().clone()
            }
            None => return Ok(None),
        };

        if let Some(second_match) = matches.next() {
            // TODO: is it possible to point out the first one here?
            bail!(second_match => "duplicate attribute");
        } else {
            Ok(Some(ret))
        }
    }

    pub fn select_tokenum<T: TokEnum>(&mut self) -> Result<Option<WithSpan<T>>> {
        self.select_one(T::all_states())?
            .map(|flag| T::from_ident(&flag).map(|value| WithSpan::new(value, flag.span())))
            .transpose()
    }
}

// token enum
// TODO: non-ident reprs

pub trait TokEnum: Parse {
    fn from_ident(ident: &syn::Ident) -> syn::Result<Self>;
    fn all_states() -> impl IntoIterator<Item = &'static str>;
}

macro_rules! tokenum {
    (
        $(#[$($attr:meta)*])*
        $vis:vis enum $name:ident {
            $(
                $(#[$($fattr:meta)*])*
                $field:ident $(= $str:literal)?
            ),*
            $(,)?
        }
) => {
        $(#[$($attr)*])*
        $vis enum $name {
            $( $(#[$($fattr)*])* $field /* { span: proc_macro2::Span } */ ),*
        }

        impl $crate::utils::parse::TokEnum for $name {
            fn all_states() -> impl IntoIterator<Item = &'static str> {
                [$( $crate::parse::tokenum!(@str $field $($str)?) ),*]
            }

            fn from_ident(ident: &syn::Ident) -> syn::Result<Self> {
                // let span = syn::spanned::Spanned::span(&ident);

                let ret = match &*ident.to_string() {
                    $($crate::parse::tokenum!(@str $field $($str)?) => Self::$field /* { span } */ ,)*
                    // TODO: expected one of
                    other => bail!(other => "unexpected value: {other}")
                };

                Ok(ret)
            }
        }
    };

    (@str $field:ident $str:literal) => { $str };
    (@str $field:ident) => { stringify!($field) };
}

pub(crate) use tokenum;

use crate::{bail, ensure_empty_tokens};

pub fn path_ident(path: &Path) -> Result<&Ident> {
    match path.segments.last() {
        Some(x) => Ok(&x.ident),
        None => bail!(path => "expected non-empty path"),
    }
}

pub fn path_sibling(source: &Path, f: impl Fn(&Ident) -> Ident) -> Result<Path> {
    let mut path = source.clone();

    let ident = match path.segments.last_mut() {
        Some(x) => &mut x.ident,
        None => bail!(path => "expected non-empty path"),
    };

    *ident = f(ident);

    Ok(path)
}

pub fn path_extend(path: &mut Path, ident: Ident) {
    path.segments.push(syn::PathSegment {
        ident,
        arguments: syn::PathArguments::None,
    })
}
