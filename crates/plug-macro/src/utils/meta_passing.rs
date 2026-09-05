use std::{
    collections::HashMap,
    sync::{LazyLock, RwLock},
};

use proc_macro2::TokenStream;
use quote::{ToTokens, format_ident, quote};
use syn::{LitStr, Path, Result, parse::Parse, parse_quote};
use syn_derive::{Parse, ToTokens};

use crate::{
    bail,
    parse::{Many, Tokens},
};

fn random_string() -> Result<String> {
    let rand = match getrandom::u32() {
        Ok(x) => x,
        _ => bail!(=> "getrandom failed"),
    };

    Ok(format!("{rand:x}"))
}

pub fn export<T: ToTokens>(marker: &str, input: T) -> Result<TokenStream> {
    let ident = format_ident!("{marker}");

    let content = quote! {
        #[doc(hidden)]
        macro_rules! #ident {
            ($($m:tt)*) => {
                ::plug_macro::__import_advance!([#input], $($m)* );
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
    pub fn decode<Aux: Parse, Imp: Parse>(self) -> Result<(Aux, Vec<Imp>)> {
        let aux = syn::parse2(self.aux)?;

        let imported = self
            .imported
            .into_iter()
            .map(syn::parse2)
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

pub fn import<Aux: ToTokens>(
    sources: impl IntoIterator<Item = Path>,
    aux: Aux,
    callback: Callback,
) -> Result<TokenStream> {
    let token = CB.register_callback(callback)?;

    let sources = sources.into_iter().collect();
    let pass = aux.to_token_stream();
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
