pub mod parse;
mod syn_serde;

#[cfg(feature = "meta-passing")]
pub(crate) mod meta_passing;

use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, Result};

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

macro_rules! ensure_empty_tokens {
    ($tokens:expr, $($err:tt)*) => {
        if !$tokens.is_empty() {
            bail!($tokens => $($err)*);
        }
    };
}
pub(crate) use ensure_empty_tokens;

pub fn purescope(vis: syn::Visibility, ident: Ident, content: TokenStream) -> TokenStream {
    quote! {
        #[allow(non_snake_case)]
        #vis mod #ident {
            pub use super::*;
            #content
        }
    }
}

pub fn retain_by_mask<T>(mask: &[bool], values: &mut Vec<T>) {
    assert_eq!(mask.len(), values.len());

    let mut iter = mask.iter();

    // Compiler should eliminate unwraps with the assert above
    values.retain(|_| *iter.next().unwrap());
}
