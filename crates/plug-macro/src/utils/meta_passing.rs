use std::{
    collections::HashMap,
    error::Error,
    sync::{LazyLock, RwLock},
};

use base64::{Engine, engine::general_purpose::STANDARD as base64};
use proc_macro2::TokenStream;
use quote::{ToTokens, format_ident, quote};
use serde::{Serialize, de::DeserializeOwned};
use syn::{LitStr, Path, Result, parse_quote};
use syn_derive::{Parse, ToTokens};

use crate::{
    amyhow, bail,
    parse::{Many, Tokens},
};

fn random_string() -> Result<String> {
    let rand = match getrandom::u32() {
        Ok(x) => x,
        _ => bail!(=> "getrandom failed"),
    };

    Ok(format!("{rand:x}"))
}

fn encode<T: Serialize>(input: T) -> Result<TokenStream> {
    let data = minicbor_serde::to_vec(input).map_err(|e| amyhow!(=> "failed to encode: {e}"))?;

    let data = base64.encode(data);
    let data = data.into_token_stream();

    Ok(data)
}

fn decode<T: DeserializeOwned>(input: TokenStream) -> Result<T> {
    let str = syn::parse2::<LitStr>(input)?;

    let ret: std::result::Result<T, Box<dyn Error>> = (|| {
        let data = base64.decode(str.value())?;
        let data = minicbor_serde::from_slice(&data)?;
        Ok(data)
    })();

    ret.map_err(|e| amyhow!(=> "failed to decode: {e}"))
}

pub fn export<T: Serialize>(marker: &str, input: T) -> Result<TokenStream> {
    let ident = format_ident!("{marker}");

    let data = encode(input)?;

    let content = quote! {
        #[doc(hidden)]
        macro_rules! #ident {
            ($($m:tt)*) => {
                ::plug_macro::__import_advance!([#data], $($m)* );
            };
        }

        pub(crate) use #ident;
    };

    Ok(content)
}

pub struct RawImport {
    imported: Vec<TokenStream>,
    aux: TokenStream,
}

impl RawImport {
    pub fn decode<Aux: DeserializeOwned, Imp: DeserializeOwned>(self) -> Result<(Aux, Vec<Imp>)> {
        let aux = decode(self.aux)?;

        let imported = self
            .imported
            .into_iter()
            .map(decode::<Imp>)
            .collect::<Result<_>>()?;

        Ok((aux, imported))
    }
}

// TODO: this could be linktime...
type Callback = fn(RawImport) -> Result<TokenStream>;
type CallbackToken = String;

struct CallbackRegistry {
    inner: RwLock<HashMap<CallbackToken, Callback>>,
}

impl CallbackRegistry {
    fn new() -> Self {
        Self {
            inner: RwLock::new(HashMap::new()),
        }
    }

    fn register_callback(&self, f: Callback) -> Result<CallbackToken> {
        let token = random_string()?;
        self.inner.write().unwrap().insert(token.clone(), f);
        Ok(token)
    }

    fn get_callback(&self, token: CallbackToken) -> Result<Callback> {
        match self.inner.read().unwrap().get(&token) {
            Some(x) => Ok(*x),
            None => bail!(=> "undefined callback token: {token}"),
        }
    }
}

#[allow(private_interfaces)]
pub static CB: LazyLock<CallbackRegistry> = LazyLock::new(|| CallbackRegistry::new());

#[derive(Parse, ToTokens)]
pub struct ImportChain {
    got: Many<Tokens>,
    remaining_sources: Many<Path>,
    callback_token: LitStr,
    passed: Tokens,
}

impl ImportChain {
    fn new(sources: Vec<Path>, callback_token: CallbackToken, pass: TokenStream) -> Self {
        Self {
            got: Vec::new().into(),
            remaining_sources: sources.into(),
            callback_token: parse_quote!(#callback_token),
            passed: Tokens(pass),
        }
    }
}

// TODO: non-empty case can be outlined as declarative macro
pub fn import_advance(mut chain: ImportChain) -> Result<TokenStream> {
    if chain.remaining_sources.is_empty() {
        let ImportChain {
            got,
            callback_token,
            passed,
            ..
        } = chain;

        let imported = got.inner.into_iter().map(|x| x.0).collect();
        let inner = passed.0;

        let callback = CB.get_callback(callback_token.value())?;

        callback(RawImport {
            imported,
            aux: inner,
        })
    } else {
        // unwrap is guarded by empty check above
        let source = chain.remaining_sources.inner.pop().unwrap();

        Ok(quote! { #source!(#chain); })
    }
}

pub fn import<Aux: Serialize>(
    sources: impl IntoIterator<Item = Path>,
    aux: Aux,
    callback: Callback,
) -> Result<TokenStream> {
    let token = CB.register_callback(callback)?;

    let sources = sources.into_iter().collect();
    let pass = encode(aux)?;
    let chain = ImportChain::new(sources, token, pass);

    import_advance(chain)
}

macro_rules! with_import {
    ($sources:ident => |$aux:ident, $imp:ident| { $($body:tt)* } ) => {
        $crate::meta_passing::import($sources, $aux, move |raw| {
            let ($aux, $imp) = raw.decode()?;
            let ret = { $($body)* };
            $crate::utils::MacroReturn::into_syn_result(ret)
        })
    };
}

pub(crate) use with_import;
