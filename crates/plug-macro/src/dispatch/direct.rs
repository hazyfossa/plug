use proc_macro2::TokenStream;
use quote::{ToTokens, format_ident, quote};
use serde::{Deserialize, Serialize};
use syn::{
    FnArg, Ident, Pat, Path, Result, Signature, Token, Type, Visibility,
    parse::{Parse, ParseStream},
    parse_quote,
    spanned::Spanned,
};

use crate::{
    AsyncDispatchKind, AsyncDispatchModifier, FnKind, InterfaceShape, Methods, Mode, bail,
    many::Many, meta_passing::with_import, path_ident, path_ident_mut, purescope, token,
};

pub mod meta_passing {
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

    use crate::{Tokens, amyhow, dispatch::direct::random_string, many::Many, utils::bail};

    fn encode<T: Serialize>(input: T) -> Result<TokenStream> {
        let data =
            minicbor_serde::to_vec(input).map_err(|e| amyhow!(=> "failed to encode: {e}"))?;

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
                    plug::__import_advance!([#data], $($m)* );
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
        pub fn decode<Aux: DeserializeOwned, Imp: DeserializeOwned>(
            self,
        ) -> Result<(Aux, Vec<Imp>)> {
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

struct Method {
    name: Ident,
    receives_self: bool,
    args: Vec<Ident>,
}

impl Method {
    fn parse(sig: &Signature) -> Result<Self> {
        let name = sig.ident.clone();
        let mut receives_self = false;
        let mut args = Vec::new();

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

                    args.push(ident.clone());
                }
            }
        }

        Ok(Self {
            name,
            receives_self,
            args,
        })
    }

    fn call_variant_inner(&self, with_await: bool) -> TokenStream {
        let Method {
            name,
            receives_self,
            args,
        } = self;

        let r#await = with_await.then_some(token!(await));

        let receive_state = match receives_self {
            true => quote! { (x) => x. },
            false => quote! { => },
        };

        quote! { #(#receive_state)? #name( #(#args)* ) #(#r#await)? }
    }
}

fn tag_type() -> Type {
    parse_quote!(Tag)
}

type Variant = Ident;
fn dispatch_one(mut sig: Signature, impls: Vec<(Variant, DispatchKind)>) -> Result<TokenStream> {
    let can_be_const = impls
        .iter()
        .all(|(_, kind)| matches!(kind, DispatchKind::Const));

    let must_be_async = impls
        .iter()
        .any(|(_, kind)| matches!(kind, DispatchKind::InlineFuture));

    if can_be_const {
        sig.asyncness = Some(parse_quote!(async));
    } else if must_be_async {
        sig.constness = Some(parse_quote!(const))
    }

    let method = Method::parse(&sig)?;

    let determinant = match method.receives_self {
        true => quote! { self },
        false => {
            let t = tag_type();
            sig.inputs.insert(0, parse_quote!(tag: #t));
            quote! { tag }
        }
    };

    let branches = impls.iter().map(|(variant, dispatch_kind)| {
        let with_await = must_be_async || matches!(dispatch_kind, DispatchKind::InlineFuture);
        let inner = method.call_variant_inner(with_await);
        quote! { #variant #inner , }
    });

    let content = quote! {
        #sig {
            match #determinant {
                #(#branches)*
            }
        }
    };

    Ok(content)
}

struct Resolved {
    methods: Methods,
    impls: Vec<MetaForImpl>,
}

impl Resolved {
    fn codegen(self) -> Result<TokenStream> {
        todo!()
    }
}

fn resolve_and_dispatch(attrs: ParsedAttrs, shape: &InterfaceShape) -> Result<TokenStream> {
    let interface_name = &shape.ident;

    let mut paths = attrs.paths.expect("TODO");

    for path in &mut paths {
        match attrs.mode {
            Mode::Direct => {
                let last = path_ident_mut(path)?;
                *last = direct_impl_token(interface_name, &last).into();
            }
            Mode::FromMod => {
                let query = registered_object_token();
                path.segments.push(query.into());
            }
            Mode::Dynamic => unreachable!(),
        }
    }

    let methods = &shape.methods;
    with_import!(paths => |methods, impls| {
        let resolved = Resolved { methods, impls };
        resolved.codegen()
    })
}

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
                mode: attrs.mode.clone(),
                methods: shape.methods.clone(),
            },
        )?;

        let continuation = resolve_and_dispatch(attrs, &shape)?;

        let content = quote! {
            pub trait Trait {}

            #meta
            #continuation
        };

        Ok(content)
    }

    fn register_impl(mut target_interface: Path, object: Path) -> Result<TokenStream> {
        let interface_name = path_ident(&target_interface)?.to_string();

        // TODO: actually test remote impls (and maybe relax this)
        let object_name = object.require_ident()?.to_string();

        let meta = ();

        let args = Args {
            interface_name,
            object_name,
            as_implemented: meta,
        };

        *path_ident_mut(&mut target_interface)? = interface_meta_token(interface_name);
        let source = target_interface;
        let source = [source];

        with_import!(source => |args, as_defined| { register_impl_inner(args, as_defined) })
    }
}

fn interface_meta_token(interface_name: impl std::fmt::Display) -> Ident {
    format_ident!("__{interface_name}_meta")
}

fn registered_object_token() -> Ident {
    format_ident!("__primary_object_for_this_module")
}

fn direct_impl_token(
    interface_name: impl std::fmt::Display,
    object_name: impl std::fmt::Display,
) -> Ident {
    format_ident!("{object_name}_implements_{interface_name}")
}

fn random_string() -> Result<String> {
    let rand = match getrandom::u32() {
        Ok(x) => x,
        _ => bail!(=> "getrandom failed"),
    };

    Ok(format!("{rand:x}"))
}

#[derive(Serialize, Deserialize)]
struct Args {
    as_implemented: MetaForImpl,
    interface_name: String,
    object_name: String,
}

fn register_impl_inner(args: Args, as_defined: Vec<MetaForInterface>) -> Result<TokenStream> {
    let as_defined = &as_defined[0]; // TODO: common case: import of one

    let (vis, ident) = match as_defined.mode {
        Mode::Direct => (
            Visibility::Inherited,
            direct_impl_token(args.interface_name, args.object_name),
        ),
        Mode::FromMod => (parse_quote!(pub), registered_object_token()),
        Mode::Dynamic => todo!("whole different path here"),
    };

    let export_meta = meta_passing::export("meta", ())?; // TODO

    // TODO: consider running without purescope
    Ok(purescope(vis, ident, export_meta.into_token_stream()))
}
