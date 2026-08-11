use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, Visibility};

macro_rules! bail {
    ($tokens:expr, $($err:tt)*) => {
        return Err(syn::Error::new($tokens.span(), format!($($err)*)))
    };
}
pub(crate) use bail;

macro_rules! ensure_empty {
    ($tokens:expr, $($err:tt)*) => {
        if !$tokens.is_empty() {
            bail!($tokens, $($err)*);
        }
    };
}
pub(crate) use ensure_empty;

pub fn purescope(vis: Visibility, name: Ident, content: TokenStream) -> TokenStream {
    quote! {
        #vis mod #name {
            pub use super::*;
            #content
        }
    }
}
