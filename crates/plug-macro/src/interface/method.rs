use darling::FromAttributes;
use proc_macro2::TokenStream;
use quote::quote;
use syn::{Attribute, FnArg, Ident, Pat, Path, Result, Signature, parse_quote, spanned::Spanned};

use crate::{bail, interface::implement::const_redirect, parse::parse_attrs};

pub enum FnKind {
    Regular,
    Async { sync_path: bool },
    Const,
}

pub enum MethodKind {
    Stateful,
    Associated,
}

impl MethodKind {
    fn parse(sig: &syn::Signature) -> Self {
        match sig.receiver() {
            Some(_) => Self::Stateful,
            None => Self::Associated,
        }
    }
}

#[derive(FromAttributes)]
#[darling(attributes(plug))]
struct MethodAttrs {
    #[darling(default, rename = "final")]
    is_final: bool,
    #[darling(default, rename = "const")]
    is_const: bool,
    #[darling(default, rename = "sync")]
    is_sync_path: bool,
}

// TODO: consider splitting into ImplMethod and InterfaceMethod
pub enum CallTarget {
    Impl,
    Interface,
}

pub struct Method {
    // signature is CallTarget::Interface
    pub sig: syn::Signature,

    args: Vec<Ident>,

    pub kind: MethodKind,
    pub fn_kind: FnKind,
    pub is_final: bool,
}

impl Method {
    // This function will leave the signature as appropriate for a trait item
    // while storing an internal copy for dispatch
    pub fn parse(attrs: &mut Vec<Attribute>, sig: &mut Signature) -> Result<Self> {
        let kind = MethodKind::parse(sig);

        let attrs: MethodAttrs = parse_attrs(attrs)?;

        let is_const = sig.constness.is_some() || attrs.is_const;
        // TODO: support `impl Future` syntax
        let is_async = sig.asyncness.is_some();

        if is_async && is_const {
            bail!(sig.asyncness.unwrap() => "constant async methods are impossible")
        }

        // Const methods are not supported as trait items by rustc
        // const-ness is restored by redirecting the call chain through an inherent impl
        sig.constness = None;

        let fn_kind = if is_async {
            FnKind::Async {
                sync_path: attrs.is_sync_path,
            }
        } else if is_const {
            FnKind::Const
        } else {
            FnKind::Regular
        };

        //

        let mut object_signature = sig.clone();

        if matches!(fn_kind, FnKind::Const) {
            object_signature.constness = parse_quote!(const);
        }

        if matches!(kind, MethodKind::Associated) {
            object_signature.inputs.insert(0, parse_quote!(tag: Tag));
        }

        //

        let mut args = Vec::new();

        for arg in &sig.inputs {
            match arg {
                FnArg::Receiver(_) => {}

                // TODO: this will become more complex when we add versioning
                FnArg::Typed(p) if let Pat::Ident(ref arg_p) = *p.pat => {
                    args.push(arg_p.ident.clone());
                }

                other => bail!(other => "unsupported syntax"),
            }
        }

        //

        Ok(Self {
            sig: object_signature,
            args,
            kind,
            fn_kind,
            is_final: attrs.is_final,
        })
    }

    pub fn requires_dispatch(&self) -> bool {
        !self.is_final
    }

    fn requires_tag(&self, target: &CallTarget) -> bool {
        matches!(target, CallTarget::Interface) && matches!(&self.kind, MethodKind::Associated)
    }

    pub fn input_shape(&self, target: CallTarget) -> TokenStream {
        if !matches!(target, CallTarget::Interface) {
            panic!()
        }

        let shape = &self
            .sig
            .inputs
            .iter()
            .filter(|x| !matches!(x, FnArg::Receiver(_)))
            .collect::<Vec<_>>();

        quote! { #(#shape),* }
    }

    pub fn args(&self, target: CallTarget) -> TokenStream {
        let args = &self.args;

        let maybe_tag = self.requires_tag(&target).then_some(quote!(tag,));

        quote! { #maybe_tag #(#args),* }
    }

    pub fn call(&self, target: CallTarget, target_path: &Path) -> TokenStream {
        let name = if matches!(&self.fn_kind, FnKind::Const)
            && matches!(&target, CallTarget::Impl { .. })
        {
            const_redirect(&self.sig.ident)
        } else {
            self.sig.ident.clone()
        };

        let args = self.args(target);

        let access = match self.kind {
            MethodKind::Associated => quote! { :: },
            MethodKind::Stateful => quote! { . },
        };

        let maybe_await =
            matches!(&self.fn_kind, FnKind::Async { .. }).then_some(quote! { .await });

        quote! { #target_path #access #name( #args ) #maybe_await }
    }

    pub fn call_via_self(&self, target: CallTarget) -> TokenStream {
        let target_path = match &self.kind {
            MethodKind::Associated => parse_quote!(Self),
            MethodKind::Stateful => parse_quote!(self),
        };

        self.call(target, &target_path)
    }
}
