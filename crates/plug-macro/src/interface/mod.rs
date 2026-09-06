use std::collections::HashMap;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use serde::{Deserialize, Serialize};
use syn::parse::{Nothing, Parse, ParseStream};
use syn::spanned::Spanned;
use syn::{FnArg, ImplItem, ItemImpl, Pat, Signature, Token, Type};
use syn::{Ident, ItemTrait, Path, Result, TraitItem, TraitItemFn};
use syn_derive::{Parse, ToTokens};

use crate::parse::path_ident;
use crate::{
    bail, ensure_empty_tokens,
    meta_passing::{export, with_import},
    parse::{Attrs, Many, path_sibling},
    retain_by_mask,
    syn_serde::ViaSerde,
};

#[derive(Clone, Parse, ToTokens, Serialize, Deserialize)]
pub enum Mode {
    #[parse(peek = Token![mod])]
    FromMod,
    #[parse(peek = Token![dyn])]
    Dynamic,

    Direct,
}

#[derive(Serialize, Deserialize, Default)]
enum Dispatch {
    /// similar to enum-dispatch
    #[default]
    Static,

    /// similar to rustc's trait objects
    /// (uses them under the hood, in fact)
    Dynamic,
}

impl Mode {
    fn dispatch_kind(&self) -> Dispatch {
        match self {
            Self::Dynamic => Dispatch::Dynamic,
            _ => Dispatch::Static,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
enum FnKind {
    Regular,
    Async,
    Const,
}

impl FnKind {
    fn parse(sig: &syn::Signature) -> Result<Self> {
        let is_async = sig.asyncness.is_some();
        let is_const = sig.constness.is_some();

        if is_async && is_const {
            bail!(sig.constness.unwrap() => "constant async methods are impossible")
        }

        let kind = if is_async {
            Self::Async
        } else if is_const {
            Self::Const
        } else {
            Self::Regular
        };

        Ok(kind)
    }
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
        let mut attrs = Attrs::extract(&mut input.attrs);

        // TODO: modifers that we want but syn doesn't parse: final
        // for now, we substitute via custom attr
        let is_final = attrs.select_one(["final"])?.is_some();

        if is_final {
            self.final_methods.push(input.clone());
            return Ok(false);
        }

        let method = Method::parse(&input.sig)?;
        self.methods.push(method);

        // Rust does not support const fn in traits natively
        // so we erase this after parsing
        // const-ness will be restored as appropriate by dispatch impl
        input.sig.constness = None;

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

    let exported_meta = export(interface_meta_marker(&shape.name), ViaSerde(meta))?;

    let content = quote! {
        #input
        #exported_meta
    };

    Ok(content)
}

// impls

type Methods = HashMap<String, FnKind>;

#[derive(Serialize, Deserialize)]
struct InterfaceMeta {
    pub mode: Mode,
    pub methods: Methods,
}

fn interface_meta_marker(interface: &Ident) -> Ident {
    format_ident!("__codegen_{interface}_meta")
}

fn registered_module_object_marker(interface: &Ident) -> Ident {
    format_ident!("registered {interface} impl for this module")
}

pub fn register_impl(_: Nothing, mut input: ItemImpl) -> Result<TokenStream> {
    let interface = match input.trait_ {
        Some((ref path, _)) => path.clone(),
        None => bail!(=> "This macro only makes sense for interface implementations"),
    };

    // NOTE: the following code does not actually check if the path resolves to a thing
    // that implements "plug::Object". It only saves downstream code from working with
    // obviously wrong inputs (since, for example, plug::Object will surely never be
    // implemented for a slice or tuple)
    let object = match *input.self_ty {
        Type::Path(ref x) => x.path.clone(),
        other => bail!(other => "Interfaces can only be implemented on objects"),
    };

    ensure_empty_tokens!(
        input.generics.params,
        "Generic interfaces are not supported (yet)"
    );

    let impl_functions = input
        .items
        .iter_mut()
        .filter_map(|x| match x {
            ImplItem::Fn(func) => Some(ImplementedMethod {
                attrs: Attrs::extract(&mut func.attrs),
                sig: func.sig.clone(),
            }),
            _ => None,
        })
        .collect();

    let meta_source = path_sibling(&interface, interface_meta_marker)?;

    let ctx = Context {
        object,
        interface,
        impl_methods: impl_functions,
    };

    let continuation = with_import!(#simple meta_source => register_impl_inner(ctx))?;

    Ok(quote! {
        #input
        #continuation
    })
}

#[derive(Parse, ToTokens)]
struct Context {
    object: Path,
    interface: Path,
    impl_methods: Many<ImplementedMethod>,
}

#[derive(Parse, ToTokens)]
struct ImplementedMethod {
    attrs: Attrs,
    sig: Signature,
}

fn register_impl_inner(ctx: Context, meta: ViaSerde<InterfaceMeta>) -> Result<TokenStream> {
    let meta = meta.0;

    // Signal is the thing that allows us to gather implementations
    let signal = match meta.mode {
        Mode::Direct => None,
        Mode::FromMod => {
            let interface = path_ident(&ctx.interface)?;
            let marker = registered_module_object_marker(interface);

            let object = ctx.object;

            Some(quote! {
                #[doc(hidden)]
                type #marker = #object;
            })
        }
        Mode::Dynamic => todo!(),
    };

    let is_static = matches!(meta.mode.dispatch_kind(), Dispatch::Static);
    let supports_const = is_static;
    let supports_inline_async = is_static;

    for func in ctx.impl_methods.inner {
        let name = func.sig.ident.to_string();
        // TODO: rustc already checks whether the method exists as part of trait resolution
        // we should instead return Ok("") here instead of unwrap
        let as_defined = meta.methods.get(&name).unwrap();
        let as_implemented = FnKind::parse(&func.sig)?;
    }

    todo!()
}
