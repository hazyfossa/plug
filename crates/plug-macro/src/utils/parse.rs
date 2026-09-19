#![allow(dead_code)]
use std::marker::PhantomData;

use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, TokenStreamExt, quote};
use syn::{
    Attribute, Ident, Path, Result, Token,
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
    spanned::Spanned,
};

use crate::bail;

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

pub type Tokens = Bracketed<TokenStream>;

// Path. TODO: as Ext

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

pub fn parse_attrs<T: darling::FromAttributes>(x: &mut Vec<Attribute>) -> Result<T> {
    let attrs: Vec<_> = x
        .extract_if(.., |attr| attr.path().is_ident(crate::NAME))
        .collect();

    Ok(T::from_attributes(&attrs)?)
}
