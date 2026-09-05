use std::error::Error;

use base64::{Engine, engine::general_purpose::STANDARD as base64};
use proc_macro2::{Span, TokenStream};
use quote::ToTokens;
use serde::{Serialize, de::DeserializeOwned};
use syn::{LitStr, Result, parse::Parse};

use crate::amyhow;

pub struct ViaSerde<T>(pub T);

impl<T> Parse for ViaSerde<T>
where
    T: DeserializeOwned,
{
    fn parse(input: syn::parse::ParseStream) -> Result<Self> {
        let str: LitStr = input.parse()?;

        let ret: std::result::Result<T, Box<dyn Error>> = (|| {
            let data = base64.decode(str.value())?;
            let data = minicbor_serde::from_slice(&data)?;
            Ok(data)
        })();

        ret.map(Self)
            .map_err(|e| amyhow!(=> "failed to decode: {e}"))
    }
}

impl<T> ToTokens for ViaSerde<T>
where
    T: Serialize,
{
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let data = minicbor_serde::to_vec(&self.0).expect("failed to encode");
        let data = base64.encode(data);
        data.to_tokens(tokens);
    }
}

// TODO: serialize spans

struct Cell {
    line: u32,
    column: u32,
}

struct SerdeSpan {
    start: Cell,
    end: Cell,
}

// impl From<Span> for SerdeSpan {
//     fn from(value: proc_macro::Span) -> Self {
//         proc_macro::Span::
//     }
// }
