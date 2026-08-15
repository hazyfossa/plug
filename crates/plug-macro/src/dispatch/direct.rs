use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use serde::{Deserialize, Serialize};
use syn::{
    FnArg, Ident, ItemImpl, Pat, Path, Result, Signature, Token, Type, Visibility,
    parse::{Parse, ParseStream},
    parse_quote,
    spanned::Spanned,
};

use crate::{
    AsyncDispatchKind, AsyncDispatchModifier, FnKind, InterfaceShape, Methods, Mode, bail,
    many::Many, purescope,
};

pub mod meta_passing {
    use std::{cell::RefCell, collections::HashMap, error::Error, sync::LazyLock};

    use base64::{Engine, engine::general_purpose::STANDARD as base64};
    use proc_macro2::TokenStream;
    use quote::{format_ident, quote};
    use serde::{Serialize, de::DeserializeOwned};
    use syn::{Ident, Path, parse::Parse};
    use syn_derive::{Parse, ToTokens};

    use crate::{Tokens, many::Many, utils::bail};

    fn encode<T: Serialize>(input: T) -> Result<String, Box<dyn Error>> {
        let data = minicbor_serde::to_vec(input)?;
        let data = base64.encode(data);

        Ok(data)
    }

    fn decode<T: DeserializeOwned>(input: String) -> Result<T, Box<dyn Error>> {
        let data = base64.decode(input)?;
        let data = minicbor_serde::from_slice(&data)?;

        Ok(data)
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
                ($($m:tt)*) => { plug::__import_advance!([#data], $($m)* ); };
            }

            pub(crate) use #ident;
        };

        Ok(content)
    }

    pub struct WithImported {
        imported: Vec<TokenStream>,
        inner: TokenStream,
    }

    impl WithImported {
        fn unwrap_signature<T: Parse>(self) -> syn::Result<(T, Vec<TokenStream>)> {
            let parsed = syn::parse2(self.inner)?;
            Ok((parsed, self.imported))
        }
    }

    type Callback = fn(WithImported) -> syn::Result<TokenStream>;
    pub const CALLBACK_LOOKUP: LazyLock<RefCell<HashMap<Ident, Callback>>> =
        LazyLock::new(|| RefCell::new(HashMap::new()));

    #[derive(Parse, ToTokens)]
    pub struct ImportChain {
        got: Many<Tokens>,
        remaining_sources: Many<Path>,
        callback_token: Ident,
        passed: Tokens,
    }

    impl ImportChain {
        fn new(sources: Vec<Path>, callback_token: Ident, pass: TokenStream) -> Self {
            Self {
                got: Vec::new().into(),
                remaining_sources: sources.into(),
                callback_token,
                passed: Tokens(pass),
            }
        }
    }

    pub fn import_advance(mut chain: ImportChain) -> syn::Result<TokenStream> {
        if chain.remaining_sources.is_empty() {
            let ImportChain {
                got,
                callback_token,
                passed,
                ..
            } = chain;

            let imported = got.inner.into_iter().map(|x| x.0).collect();
            let inner = passed.0;

            match CALLBACK_LOOKUP.borrow().get(&callback_token) {
                Some(callback) => callback(WithImported { imported, inner }),
                None => bail!(=> "undefined callback token"),
            }
        } else {
            // unwrap is guarded by empty check above
            let source = chain.remaining_sources.inner.pop().unwrap();

            Ok(quote! { #source!(#chain) })
        }
    }
}

// Read by implementations
#[derive(Serialize, Deserialize)]
struct MetaForInterface {
    mode: Mode,
    methods: Methods,
}

#[derive(Serialize, Deserialize)]
struct MetaForImpl {
    tag_ident: String,
    tag_repr: Option<String>,
    methods: Methods,
}

enum DispatchKind {
    Direct,
    Const,
    InlineFuture,
    OutlineFuture,
}

impl From<AsyncDispatchKind> for DispatchKind {
    fn from(value: AsyncDispatchKind) -> Self {
        match value {
            AsyncDispatchKind::Inline => Self::InlineFuture,
            AsyncDispatchKind::Outline => Self::OutlineFuture,
        }
    }
}

fn async_dispatch_resolve(
    as_defined: Option<&AsyncDispatchModifier>,
    as_implemented: Option<&AsyncDispatchModifier>,
) -> DispatchKind {
    let primary = match as_defined {
        Some(x) if x.is_final => as_defined,
        _ => as_implemented.or(as_defined),
    };

    primary.map(|x| x.kind).unwrap_or_default().into()
}

fn fn_dispatch_resolve(as_defined: FnKind, as_implemented: FnKind) -> DispatchKind {
    use FnKind::*;

    match (as_defined, as_implemented) {
        // no matter how they are defined, const functions
        // are always const-dispatchable
        (_, Const) => DispatchKind::Const,

        // Async dispatch is complex enough to have its own thing
        (Async { dispatch: a }, Async { dispatch: b }) => {
            async_dispatch_resolve(a.as_deref(), b.as_deref())
        }

        // If an impl of an "async" method does not require it,
        // dispatch it as a normal function
        (Async { .. }, Regular) => DispatchKind::Direct,

        (Regular, Regular) => DispatchKind::Direct,

        _ => unreachable!("Pathological impl. This should have been caught at definition time."),
    }
}

type Variant = Ident;

struct FnArgs {
    receives_self: bool,
    idents: Vec<Ident>,
}

impl FnArgs {
    fn parse(sig: &Signature) -> Result<Self> {
        let mut idents = Vec::new();
        let mut receives_self = false;

        for arg in &sig.inputs {
            match arg {
                FnArg::Receiver(_) => {
                    receives_self = true;
                }
                FnArg::Typed(pattern) => {
                    let ident = match pattern.pat.as_ref() {
                        Pat::Ident(x) => &x.ident,
                        other => bail!(other => "unsupported argument pattern"),
                    };

                    idents.push(ident.clone());
                }
            }
        }

        Ok(Self {
            receives_self,
            idents,
        })
    }
}

fn dispatch_method(mut sig: Signature, impls: Vec<(Variant, DispatchKind)>) -> Result<TokenStream> {
    let can_be_const = impls
        .iter()
        .all(|(_, kind)| matches!(kind, DispatchKind::Const));

    let must_be_async = impls
        .iter()
        .any(|(_, kind)| matches!(kind, DispatchKind::InlineFuture));

    let modifier = if can_be_const {
        quote! { const }
    } else if must_be_async {
        quote! { async }
    } else {
        quote! { /* */ }
    };

    let args = FnArgs::parse(&sig)?;

    let determinant = match args.receives_self {
        true => quote! { self },
        false => {
            sig.inputs.insert(0, parse_quote!(tag: Tag));
            quote! { tag }
        }
    };

    let content = quote! {
        #sig {
            match #determinant {

            }
        }
    };

    Ok(content)
}

struct Resolved {
    shape: InterfaceShape,
    impls: Vec<MetaForImpl>,
}

impl Resolved {}

struct ParsedAttrs {
    mode: Mode,
    paths: Option<Vec<Path>>,
}

// TODO: derive this
impl Parse for ParsedAttrs {
    fn parse(input: ParseStream) -> Result<Self> {
        let mode: Mode = input.parse()?;

        let paths: Option<Many<Path>> = match mode {
            Mode::Direct => Some(input.parse()?),
            Mode::FromMod => {
                let _marker = input.parse::<Token![mod]>()?;
                let content;
                syn::parenthesized!(content in input);
                Some(content.parse()?)
            }
            Mode::Dynamic => None,
        };

        let paths = paths.map(|x| x.inner);

        Ok(ParsedAttrs { mode, paths })
    }
}

pub struct Impl;

impl super::Dispatch for Impl {
    fn dispatch(attrs: TokenStream, shape: InterfaceShape) -> Result<TokenStream> {
        let attrs: ParsedAttrs = syn::parse2(attrs)?;

        let meta = meta_passing::export(
            "meta",
            MetaForInterface {
                mode: attrs.mode,
                methods: shape.methods,
            },
        )?;

        let content = quote! {
            pub trait Trait {}

            #meta
        };

        Ok(content)
    }

    fn register_impl(target_interface: Path, object: Box<Type>) -> Result<TokenStream> {
        todo!()
    }
}

fn registered_module_object() -> Ident {
    format_ident!("__primary_object_for_this_module")
}

fn random_ident(prefix: &str) -> Result<Ident> {
    let rand = match getrandom::u32() {
        Ok(x) => x,
        _ => bail!(=> "getrandom failed"),
    };

    Ok(format_ident!("{prefix}_{rand:x}"))
}

fn register_impl_inner(input: ItemImpl, meta: MetaForInterface) -> Result<TokenStream> {
    let (vis, ident) = match meta.mode {
        Mode::Direct => (Visibility::Inherited, random_ident("impl")?),
        Mode::FromMod => (parse_quote!(pub), registered_module_object()),
        Mode::Dynamic => todo!("whole different path here"),
    };

    let meta = meta_passing::export("meta", ())?; // TODO

    let content = quote! { #meta #input };
    Ok(purescope(vis, ident, content))
}
