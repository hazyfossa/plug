use proc_macro2::TokenStream;
use quote::quote;
use serde::{Deserialize, Serialize};
use syn::spanned::Spanned;
use syn::{FnArg, Pat};
use syn::{
    Ident, ItemTrait, Path, Result, Token, TraitItem, TraitItemFn,
    parse::{Parse, ParseStream},
};
use syn_derive::{Parse, ToTokens};

use crate::syn_serde::ViaSerde;
use crate::{
    FnKind, bail, ensure_empty_tokens,
    parse::{Attrs, Many},
    retain_by_mask,
};
use crate::{InterfaceMeta, interface_meta_marker, meta_passing};

#[derive(Clone, Parse, ToTokens, Serialize, Deserialize)]
pub enum Mode {
    #[parse(peek = Token![mod])]
    FromMod,
    #[parse(peek = Token![dyn])]
    Dynamic,

    Direct,
}

#[derive(Serialize, Deserialize)]
enum Dispatch {
    /// similar to enum-dispatch
    Static,

    /// similar to rustc's trait objects
    /// (uses them under the hood, in fact)
    Dynamic,
}

enum CallConvention {
    /// fn() -> Value
    Direct,

    /// fn().await -> Value
    Await,
}

impl FnKind {
    fn call_convention(&self) -> CallConvention {
        match self {
            Self::Async => CallConvention::Await,
            _ => CallConvention::Direct,
        }
    }
}

enum Discriminant {
    // &self, &mut self, etc
    Object,

    // for methods that do not need
    // object state
    Tag,
}

impl Discriminant {
    fn parse(sig: &syn::Signature) -> Self {
        match sig.receiver() {
            Some(_) => Self::Object,
            None => Self::Tag,
        }
    }
}

struct Method {
    name: Ident,
    args: Many<Ident>,
    kind: FnKind,
    discriminant: Discriminant,
}

impl Method {
    fn parse(sig: &syn::Signature) -> Result<Self> {
        let mut args: Many<Ident> = Vec::new().into();

        for arg in &sig.inputs {
            match arg {
                FnArg::Receiver(_) => { /* handled by disciminant parse */ }

                // TODO: this will become more complex when we add versioning
                FnArg::Typed(p) if let Pat::Ident(ref arg_p) = *p.pat => {
                    args.push(arg_p.ident.clone());
                }

                other => bail!(other => "unsupported syntax"),
            }
        }

        let name = sig.ident.clone();
        let kind = FnKind::parse(sig)?;
        let discriminant = Discriminant::parse(sig);

        Ok(Self {
            name,
            args,
            kind,
            discriminant,
        })
    }
}

struct InterfaceShape {
    name: Ident,
    methods: Vec<Method>,
    final_methods: Vec<TraitItemFn>,
    // TODO: assoc const, types
}

impl InterfaceShape {
    fn new(name: Ident) -> Self {
        Self {
            name,
            methods: Vec::new(),
            final_methods: Vec::new(),
        }
    }

    // Returns whether the method is implementable
    fn register_method(&mut self, input: &mut TraitItemFn) -> Result<bool> {
        let mut attrs = Attrs::extract(&mut input.attrs)?;

        // TODO: modifers that we want but syn doesn't parse: final
        // for now, we substitute via custom attr
        let is_final = attrs.select_one(["final"])?.is_some();

        if is_final {
            self.final_methods.push(input.clone());
            return Ok(false);
        }

        let method = Method::parse(&input.sig)?;
        self.methods.push(method);

        Ok(true)
    }
}

pub struct InterfaceAttrs {
    mode: Mode,
    paths: Option<Vec<Path>>,
}

// TODO: derive this
impl Parse for InterfaceAttrs {
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

        Ok(InterfaceAttrs { mode, paths })
    }
}

pub fn trait_to_interface(attrs: InterfaceAttrs, mut input: ItemTrait) -> Result<TokenStream> {
    ensure_empty_tokens!(
        input.generics.params,
        "Generic interfaces are not supported (yet)"
    );

    let mut shape = InterfaceShape::new(input.ident.clone());

    let mut ret = Vec::new();

    for item in input.items.iter_mut() {
        match item {
            TraitItem::Fn(f) => ret.push(shape.register_method(f)?),
            TraitItem::Const(x) => bail!(x => "Constant property support is TBD"),
            TraitItem::Type(x) => bail!(x => "Associated type support is TBD"),
            TraitItem::Macro(x) => bail!(x => "Cannot define part of an interface via macros"),
            x => bail!(x => "This syntax is not supported inside interfaces"),
        }
    }

    // This removes all methods that are not implementable
    retain_by_mask(&ret, &mut input.items);

    // TODO: dispatch here

    let meta = InterfaceMeta {
        mode: attrs.mode,
        methods: shape
            .methods
            .into_iter()
            .map(|x| (x.name.to_string(), x.kind))
            .collect(),
    };

    let meta = meta_passing::export(interface_meta_marker(&shape.name), ViaSerde(meta))?;

    let content = quote! {
        #meta
    };

    Ok(content)
}
