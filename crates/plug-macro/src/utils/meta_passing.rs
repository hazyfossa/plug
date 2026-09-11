use std::{
    collections::HashMap,
    str::FromStr,
    sync::{LazyLock, RwLock},
};

use proc_macro2::TokenStream;
use quote::{ToTokens, quote};
use syn::{
    Ident, LitStr, Path, Result, Token,
    parse::{Parse, ParseStream},
    parse_quote,
};
use syn_derive::{Parse, ToTokens};

use crate::{
    bail,
    parse::{Bracketed, Many, Tokens},
};

fn random_string() -> Result<String> {
    let rand = match getrandom::u32() {
        Ok(x) => x,
        _ => bail!(=> "getrandom failed"),
    };

    Ok(format!("{rand:x}"))
}

pub fn export<T: ToTokens>(marker: Ident, input: T) -> Result<TokenStream> {
    let content = quote! {
        macro_rules! #marker {
            ([$($other_imports:tt)*] $($data:tt)*) => {
                ::plug_macro::__import_advance!([[#input], $($other_imports)*] $($data)* );
            };
        }

        pub(crate) use #marker;
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
struct ImportContinuation {
    callback_token: LitStr,
    passed: Tokens,
}

// #[derive(Parse, ToTokens)]
pub struct ImportChain {
    got: Bracketed<Many<Tokens>>,
    remaining_sources: Bracketed<Many<Path>>,
    cons: Bracketed<ImportContinuation>,
}

impl ::syn::parse::Parse for ImportChain {
    fn parse(__input: ::syn::parse::ParseStream) -> ::syn::Result<Self> {
        let got = __input.parse()?;
        let remaining_sources = __input.parse()?;

        let cons = __input.parse()?;

        ::syn::Result::Ok(Self {
            got,
            remaining_sources,
            cons,
        })
    }
}
impl ::quote::ToTokens for ImportChain {
    fn to_tokens(&self, tokens: &mut ::proc_macro2::TokenStream) {
        let Self {
            got,
            remaining_sources,
            cons,
        } = self;
        {
            got.to_tokens(tokens);
            remaining_sources.to_tokens(tokens);
            cons.to_tokens(tokens);
        }
    }
}

impl ImportChain {
    fn new(sources: Vec<Path>, callback_token: CallbackToken, pass: TokenStream) -> Self {
        Self {
            got: Many::from(Vec::new()).into(),
            remaining_sources: Many::from(sources).into(),
            cons: ImportContinuation {
                callback_token: parse_quote!(#callback_token),
                passed: pass.into(),
            }
            .into(),
        }
    }
}

// TODO: non-empty case can be outlined as declarative macro
pub fn import_advance(mut chain: ImportChain) -> Result<TokenStream> {
    let sources = &mut chain.remaining_sources.inner.inner;
    match &mut sources.is_empty() {
        true => {
            let cons = chain.cons.inner;

            let aux = cons.passed.inner;
            let callback_token = cons.callback_token;
            let imported = chain.got.inner.inner.into_iter().map(|x| x.inner).collect();

            let callback = CB.get_callback(callback_token.value())?;
            callback(RawImport { imported, aux })
        }
        false => {
            // NOTE: this unwrap is guarded by is_empty check above
            let source = sources.pop().unwrap();

            Ok(quote! { #source!(#chain); })
        }
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

// TODO: import once as a common special case
macro_rules! with_import {
    (#simple $source:ident => $fn:ident($aux:ident)) => {
        $crate::meta_passing::with_import!([$source] => |$aux, imported_raw| {
            let mut raw = imported_raw;
            let imp = raw.pop().unwrap();
            $fn($aux, imp)
        })
    };

    ($sources:expr => |$aux:ident, $imp:ident| { $($body:tt)* } ) => {
        $crate::meta_passing::import($sources, $aux, move |raw| {
            let ($aux, $imp) = raw.decode()?;
            let ret = { $($body)* };
            $crate::utils::MacroReturn::into_syn_result(ret)
        })
    };
}

pub(crate) use with_import;
