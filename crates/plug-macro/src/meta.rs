use std::error::Error;

use base64::{Engine, engine::general_purpose::STANDARD as base64};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use serde::{Serialize, de::DeserializeOwned};

use crate::utils::bail;

fn encode<T: Serialize>(input: T) -> Result<String, Box<dyn Error>> {
    let data = minicbor_serde::to_vec(input)?;
    let data = base64.encode(data);

    Ok(data)
}

fn decode<T: DeserializeOwned>(input: String) -> Result<T, Box<dyn Error>> {
    todo!()
}

pub fn export<T: Serialize>(marker: &str, input: T) -> syn::Result<TokenStream> {
    let ident = format_ident!("{marker}");

    let data = match encode(input) {
        Ok(x) => x,
        Err(e) => bail!(=> "Failed to encode metadata: {e:?}"),
    };

    let content = quote! {
        #[doc(hidden)]
        macro_rules! #ident {
            ($($m:tt)*) => { $($m)*!(#data); };
        }

        pub(crate) use #ident;
    };

    Ok(content)
}
