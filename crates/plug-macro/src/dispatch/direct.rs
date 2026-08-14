use proc_macro2::TokenStream;
use quote::quote;
use serde::{Deserialize, Serialize};
use syn::{FnArg, Signature};

use crate::{AsyncDispatchKind, AsyncDispatchModifier, FnKind, InterfaceShape, Methods, Mode};

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
        (FnKind::Const { .. }, _) => DispatchKind::Direct,
        (FnKind::Async { dispatch: a }, FnKind::Async { dispatch: b }) => {
            async_dispatch_resolve(a.as_deref(), b.as_deref())
        }
        (FnKind::Async { .. }, _) => DispatchKind::Direct,
        _ => todo!(),
    }
}

struct Resolved {
    shape: InterfaceShape,
    impls: Vec<MetaForImpl>,
}

impl Resolved {
    fn dispatch_method(&self, sig: Signature) -> TokenStream {
        // let dispatch_kind_map: Vec<_> =
        let can_be_const = impls.iter().all(|(_, kind)| matches!(kind, FnKind::Const));

        let must_be_async = impls.iter().any(|(_, kind)| match kind {
            FnKind::Async { dispatch } => !dispatch.is_outline(outline_by_default),
            _ => false,
        });

        let modifier = if can_be_const {
            quote! { const }
        } else if must_be_async {
            quote! { async }
        } else {
            quote! { /* */ }
        };

        let Signature {
            ident,
            generics,
            inputs,
            output,
            ..
        } = sig;

        let mut args = Vec::new();
        let mut receiver = None;
        for arg in &inputs {
            match arg {
                FnArg::Receiver(x) => {
                    receiver.replace(x);
                }
                FnArg::Typed(pattern) => {
                    let ident = match pattern.pat.as_ref() {
                        Pat::Ident(x) => &x.ident,
                        _ => panic!("x"),
                    };

                    args.push(ident.clone());
                }
            }
        }

        quote! {
            #modifier fn #generics #ident(#inputs) -> #output {

            }
        }
    }
}
