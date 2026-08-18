use std::marker::PhantomData;

use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, TokenStreamExt, quote};
use serde::{Deserialize, Serialize};
use syn::{
    Attribute, Ident, Meta, Path, Result, Token, Visibility,
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

// TODO: expose proc_macro2 version
macro_rules! define {
    (@impl $kind:ident; $(#[$($attr:tt)+])* $name:ident = $impl:path; $($arg:ident),+) => {
        define!(@attr $kind $name
        $(#[$($attr)+])*
        pub fn $name($($arg: proc_macro::TokenStream),+) -> proc_macro::TokenStream {
            let predicate = |$($arg: proc_macro::TokenStream),+| {
                $(let $arg = syn::parse($arg)?;)+
                Ok(($($arg),+))
            };

            #[allow(unused_parens)]
            let ret = predicate($($arg.into()),+)
                .map(|($($arg),+)| $impl($($arg),+))
                .map($crate::MacroReturn::into_syn_result)
                .flatten();

            match ret {
                Ok(tokens) => tokens.into(),
                Err(e) => e.to_compile_error().into(),
            }
        });
    };

    (@attr attribute  $name:ident $($body:tt)*)  => { #[proc_macro_attribute]     $($body)*};
    (@attr derive     $name:ident $($body:tt)*)  => { #[proc_macro_derive($name)] $($body)*};
    (@attr fn_like    $name:ident $($body:tt)*)  => { #[proc_macro]               $($body)*};

    (attribute $($tt:tt)*) => {
        define!(@impl attribute; $($tt)*; attrs, input);
    };

    ($other_kind:tt $($tt:tt)*) => {
        define!(@impl $other_kind; $($tt)*; input);
    };
}
pub(crate) use define;

macro_rules! amyhow {
    (@span $tokens:expr) => { $tokens.span() };
    (@span) => { proc_macro2::Span::call_site() };

    ($($tokens:expr)? => $($fmt:tt)*) => {
        syn::Error::new(
            $crate::amyhow!(@span $($tokens)?),
            format!($($fmt)*)
        )
    };
}
pub(crate) use amyhow;

macro_rules! bail {
    ($($tt:tt)*) => {
        return Err($crate::amyhow!($($tt)*))
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

pub mod many {
    use std::ops::Deref;

    use super::*;

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
}

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
#[derive(Clone)]
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

macro_rules! token {
    ($($tt:tt)*) => {
        syn::Token![$($tt)*](proc_macro2::Span::call_site())
    };
}
pub(crate) use token;

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
