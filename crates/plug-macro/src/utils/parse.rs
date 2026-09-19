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

// Macro

macro_rules! parse {
    (
        $(#[$($attr:meta)*])*
        $vis:vis enum $name:ident {
            $( $field:ident = $tok:tt ),* $(,)?
            $(@default $default:ident)?
        }
) => {
        $(#[$($attr)*])*
        $vis enum $name {
            $($field,)*
            $($default)?
        }

        impl syn::parse::Parse for $name {
            fn parse(input: syn::parse::ParseStream) -> Result<Self> {
                let next = input.lookahead1();

                $(
                    if next.peek(syn::Token![$tok]) {
                        // TODO: consider not discarding the span here
                        let _ = input.parse::<syn::Token![$tok]>()?;
                        return Ok(Self::$field);
                    }
                )*

                $(
                    return Ok(Self::$default);
                )?

                #[allow(unused)]
                Err(next.error())
            }
        }

        impl quote::ToTokens for $name {
            fn to_tokens(&self, tokens: &mut TokenStream) {
                match self {
                    $(Self::$field => Token![$tok](proc_macro2::Span::call_site()).to_tokens(tokens),)*
                    $(Self::$default => (),)?
                }
            }
        }
    };


    ($vis:vis struct $name:ident {
        $( $field:ident: $ty:ty ),* $(,)?
    }) => {
        $vis struct $name {
            $($field: $ty),*
        }

        impl syn::parse::Parse for $name {
            fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
                Ok(Self {
                    $($field: input.parse()?),*
                })
            }
        }

        impl quote::ToTokens for $name {
            fn to_tokens(&self, tokens: &mut TokenStream) {
                $(self.$field.to_tokens(tokens);)*
            }
        }
    };
}
pub(crate) use parse;

// Path

pub trait PathExt {
    fn last_ident(&self) -> Result<&Ident>;
    fn sibling(&self, f: impl Fn(&Ident) -> Ident) -> Result<Path>;
    fn extend(&mut self, ident: Ident);
}

impl PathExt for syn::Path {
    fn last_ident(&self) -> Result<&Ident> {
        match self.segments.last() {
            Some(x) => Ok(&x.ident),
            None => bail!(self => "expected non-empty path"),
        }
    }

    fn sibling(&self, f: impl Fn(&Ident) -> Ident) -> Result<Path> {
        let mut path = self.clone();

        let ident = match path.segments.last_mut() {
            Some(x) => &mut x.ident,
            None => bail!(path => "expected non-empty path"),
        };

        *ident = f(ident);

        Ok(path)
    }

    fn extend(&mut self, ident: Ident) {
        self.segments.push(syn::PathSegment {
            ident,
            arguments: syn::PathArguments::None,
        })
    }
}

// Attrs

// TODO: consider also using syn-native parse_attrs instead of darling's
pub fn parse_attrs<T: darling::FromAttributes>(x: &mut Vec<Attribute>) -> Result<T> {
    let attrs: Vec<_> = x
        .extract_if(.., |attr| attr.path().is_ident(crate::NAME))
        .collect();

    Ok(T::from_attributes(&attrs)?)
}
