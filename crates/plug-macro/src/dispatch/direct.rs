use proc_macro2::TokenStream;
use quote::quote;
use serde::{Deserialize, Serialize};
use syn::{FnArg, Ident, Pat, Result, Signature, parse_quote, spanned::Spanned};

use crate::{
    AsyncDispatchKind, AsyncDispatchModifier, FnKind, InterfaceShape, Methods, Mode, bail,
};

mod meta_passing {
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
                ($($m:tt)*) => { $($m)*!(#data); };
            }

            pub(crate) use #ident;
        };

        Ok(content)
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
    match (as_defined, as_implemented) {
        (FnKind::Const, _) => DispatchKind::Direct,
        (FnKind::Regular, FnKind::Const) => DispatchKind::Direct, // TODO: annotate here that we can raise to const

        (FnKind::Async { dispatch: a }, FnKind::Async { dispatch: b }) => {
            async_dispatch_resolve(a.as_deref(), b.as_deref())
        }
        (FnKind::Async { .. }, _) => DispatchKind::Direct,
        _ => todo!(),
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

    if !args.receives_self {
        sig.inputs.insert(0, parse_quote!(tag: Tag));
    }

    let content = quote! {
        #sig {

        }
    };

    Ok(content)
}

struct Resolved {
    shape: InterfaceShape,
    impls: Vec<MetaForImpl>,
}

impl Resolved {}
