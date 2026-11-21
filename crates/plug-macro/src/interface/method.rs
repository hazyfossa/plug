use darling::FromAttributes;
use proc_macro2::TokenStream;
use quote::{ToTokens, quote};
use syn::{
    Attribute, FnArg, Ident, Pat, PatIdent, PatType, Path, Receiver, Result, ReturnType, Signature,
    TraitItemFn, Type, parse_quote, spanned::Spanned,
};

use crate::{TokenDef, bail, interface::implement::const_redirect, parse::parse_attrs};

pub enum FnKind {
    Regular,
    Async { sync_path: bool },
    Const,
}

pub enum MethodKind {
    Stateful,
    Associated,
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

pub struct MethodArg {
    binding: PatIdent,
    pub ty: Type,
}

impl MethodArg {
    fn from_pattern(p: &PatType) -> Result<Self> {
        let binding = match &*p.pat {
            Pat::Ident(x) => x.clone(),
            other => bail!(other => "Unsupported syntax"),
        };

        let ty = *p.ty.clone();

        Ok(Self { binding, ty })
    }

    pub fn ident(&self) -> &Ident {
        &self.binding.ident
    }
}

impl ToTokens for MethodArg {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        self.binding.to_tokens(tokens);
        TokenDef![:].to_tokens(tokens);
        self.ty.to_tokens(tokens);
    }
}

// TODO: consider splitting into ImplMethod and InterfaceMethod
pub enum CallTarget {
    Impl,
    Interface,
}

pub struct Method {
    pub name: Ident,
    receiver: Option<Receiver>,
    args: Vec<MethodArg>,
    ret: Type,

    pub fn_kind: FnKind,
    pub is_final: bool,
}

impl Method {
    pub fn from_trait(item: &mut TraitItemFn) -> Result<Self> {
        let attrs = &mut item.attrs;
        let sig = &mut item.sig;

        let method = Self::parse(attrs, sig)?;

        // Const methods are not supported as trait items by rustc
        // const-ness is restored by redirecting the call chain through an inherent impl
        sig.constness = None;

        Ok(method)
    }

    pub fn parse(attrs: &mut Vec<Attribute>, sig: &Signature) -> Result<Self> {
        let attrs: MethodAttrs = parse_attrs(attrs)?;
        let name = sig.ident.clone();

        let is_const = sig.constness.is_some() || attrs.is_const;
        // TODO: support `impl Future` syntax
        let is_async = sig.asyncness.is_some();

        if is_async && is_const {
            bail!(sig.asyncness.unwrap() => "constant async methods are impossible")
        }

        let fn_kind = if is_async {
            FnKind::Async {
                sync_path: attrs.is_sync_path,
            }
        } else if is_const {
            FnKind::Const
        } else {
            FnKind::Regular
        };

        // let mut object_signature = sig.clone();

        // if matches!(fn_kind, FnKind::Const) {
        //     object_signature.constness = parse_quote!(const);
        // }

        // if matches!(kind, MethodKind::Associated) {
        //     object_signature.inputs.insert(0, parse_quote!(tag: Tag));
        // }

        let mut receiver = None;
        let mut args = Vec::new();

        for arg in &sig.inputs {
            match arg {
                FnArg::Receiver(r) => {
                    receiver.replace(r.clone());
                }
                // TODO: this will become more complex when we add versioning
                FnArg::Typed(p) => args.push(MethodArg::from_pattern(p)?),
            }
        }

        let ret = match &sig.output {
            ReturnType::Default => parse_quote!(()),
            ReturnType::Type(_, x) => *x.clone(),
        };

        Ok(Self {
            name,
            receiver,
            args,
            ret,

            fn_kind,
            is_final: attrs.is_final,
        })
    }

    pub fn kind(&self) -> MethodKind {
        match self.receiver.is_some() {
            true => MethodKind::Stateful,
            false => MethodKind::Associated,
        }
    }

    pub fn requires_dispatch(&self) -> bool {
        !self.is_final
    }

    fn requires_tag(&self, target: &CallTarget) -> bool {
        // When calling an associated method of an interface,
        // the interface has no knowledge of what impl to use.
        //
        // Thus, `tag` is required as a quasi-receiver.
        //
        // These `receiver-like` variables are called determinants
        // in dispatch code.
        matches!(target, CallTarget::Interface) && matches!(self.kind(), MethodKind::Associated)
    }

    // TODO: better clarify in code that receiver / tag are mutually exclusive

    fn arguments(&self, target: &CallTarget) -> TokenStream {
        let receiver = &self.receiver;
        let tag = self.requires_tag(target).then_some(quote! {tag: Tag});
        let args = &self.args;
        quote! { #receiver #tag #(#args)* }
    }

    fn inputs(&self, target: &CallTarget) -> TokenStream {
        let tag = self.requires_tag(target).then_some(quote! { tag });
        let args = self.args.iter().map(|x| x.ident());
        quote! {#tag #(#args)* }
    }

    pub fn call(&self, target: CallTarget, via_type: &Path) -> TokenStream {
        let name = if matches!(&self.fn_kind, FnKind::Const) && matches!(&target, CallTarget::Impl)
        {
            &const_redirect(&self.name)
        } else {
            &self.name
        };

        let inputs = self.inputs(&target);

        let access = match self.kind() {
            MethodKind::Associated => quote! { :: },
            MethodKind::Stateful => quote! { . },
        };

        let maybe_await =
            matches!(&self.fn_kind, FnKind::Async { .. }).then_some(quote! { .await });

        quote! { #via_type #access #name( #inputs ) #maybe_await }
    }

    pub fn call_via_self(&self, target: CallTarget) -> TokenStream {
        let target_path = match &self.kind() {
            MethodKind::Associated => parse_quote!(Self),
            MethodKind::Stateful => parse_quote!(self),
        };

        self.call(target, &target_path)
    }

    // Emitted signature does not include `vis`
    pub fn signature(&self, target: &CallTarget) -> TokenStream {
        let modifier = match self.fn_kind {
            FnKind::Async { .. } => Some(quote! { async }),
            FnKind::Const => Some(quote! { const }),
            FnKind::Regular => None,
        };

        let name = &self.name;
        let args = self.arguments(target);
        let ret = &self.ret;

        quote! { #modifier fn #name( #args ) -> #ret }
    }
}
