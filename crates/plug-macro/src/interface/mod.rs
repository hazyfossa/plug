use std::collections::HashMap;

use proc_macro2::TokenStream;
use quote::{ToTokens, format_ident, quote};
use serde::{Deserialize, Serialize};
use syn::parse::{Nothing, Parse, ParseStream};
use syn::spanned::Spanned;
use syn::{FnArg, ImplItem, ItemImpl, Pat, PathSegment, Signature, Token, Type, parse_quote};
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

    fn maybe_await(&self) -> Option<syn::token::Await> {
        matches!(self, Self::Async).then_some(syn::token::Await::default())
    }
}

enum MethodKind {
    Accessor,
    Constructor,
}

impl MethodKind {
    fn parse(sig: &syn::Signature) -> Self {
        match sig.receiver() {
            Some(_) => Self::Accessor,
            None => Self::Constructor,
        }
    }
}

struct Method {
    signature: syn::Signature,
    fn_kind: FnKind,
}

impl Method {
    // This function will modify the signature to be appropriate
    // as part of a trait (hint), while storing a copy for dispatch
    fn from_signature(sig: &mut syn::Signature) -> Result<Self> {
        let this = Self {
            signature: sig.clone(),
            fn_kind: FnKind::parse(sig)?,
        };

        sig.constness = None;

        // TODO: box async

        Ok(this)
    }

    fn kind(&self) -> MethodKind {
        MethodKind::parse(&self.signature)
    }

    fn name(&self) -> &Ident {
        &self.signature.ident
    }

    fn args(&self) -> Result<Vec<&Ident>> {
        let mut args: Vec<&Ident> = Vec::new();

        for arg in &self.signature.inputs {
            match arg {
                FnArg::Receiver(_) => { /* handled by disciminant parse */ }

                // TODO: this will become more complex when we add versioning
                FnArg::Typed(p) if let Pat::Ident(ref arg_p) = *p.pat => {
                    args.push(&arg_p.ident);
                }

                other => bail!(other => "unsupported syntax"),
            }
        }

        Ok(args)
    }

    // TODO: factor in the sync path here
    // (or in another place)
    fn call_via_context(&self) -> Result<TokenStream> {
        let name = self.name();
        let args = self.args()?;
        let maybe_await = self.fn_kind.maybe_await();

        Ok(quote! { #name( #(#args)* ) #maybe_await  })
    }
}

struct StaticDispatch {
    name: Ident,
    variants: Vec<Ident>,
}

impl StaticDispatch {
    // TODO: visibility as setting
    fn new(object_name: Ident, impls: Vec<Path>) -> Result<(Self, TokenStream)> {
        let variants: Vec<_> = impls
            .iter()
            .map(|x| path_ident(x).cloned())
            .collect::<Result<_>>()?;

        let content = quote! {
            pub enum #object_name { #(
                #variants(#impls))*
            }
        };

        let this = Self {
            name: object_name,
            variants,
        };
        Ok((this, content))
    }

    fn dispatch_method(&self, method: &Method) -> Result<TokenStream> {
        let sig = &method.signature;
        let call = method.call_via_context()?;

        let content = match method.kind() {
            MethodKind::Constructor => todo!("constructors are TBD"),
            MethodKind::Accessor => {
                let branches: Vec<_> = self
                    .variants
                    .iter()
                    .map(|v| {
                        quote! { Self::#v(obj)
                        => obj.#call }
                    })
                    .collect();

                quote! { #sig {
                    match self { #(#branches),* }
                } }
            }
        };

        Ok(content)
    }
}

struct InterfaceShape {
    methods: Vec<Method>,
    final_methods: Vec<TraitItemFn>,
    // TODO: assoc const, types
}

impl InterfaceShape {
    fn new() -> Self {
        Self {
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

        let method = Method::from_signature(&mut input.sig)?;
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
            Mode::Dynamic => {
                let _marker = input.parse::<Token![dyn]>()?;
                None
            }
        };

        let paths = paths.map(|x| x.inner);

        Ok(InterfaceAttrs { mode, paths })
    }
}

impl InterfaceAttrs {
    fn resolve_static_impls(self, interface: &Ident) -> Vec<Path> {
        match self.mode {
            Mode::Dynamic => panic!("cannot resolve static impls in dynamic mode"),
            Mode::Direct => self.paths.unwrap(),
            Mode::FromMod => {
                let mut paths = self.paths.unwrap();

                for p in &mut paths {
                    p.segments.push_value(PathSegment {
                        ident: registered_module_object_marker(interface),
                        arguments: syn::PathArguments::None,
                    });
                }
                paths
            }
        }
    }
}

const EXPORTED_META: &str = "meta";

fn parse_shape(input: &mut ItemTrait) -> Result<InterfaceShape> {
    let mut shape = InterfaceShape::new();
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

    Ok(shape)
}

pub fn trait_to_interface(attrs: InterfaceAttrs, mut input: ItemTrait) -> Result<TokenStream> {
    ensure_empty_tokens!(
        input.generics.params,
        "Generic interfaces are not supported (yet)"
    );

    let name = input.ident.clone();
    let InterfaceShape {
        methods,
        final_methods,
    } = parse_shape(&mut input)?;

    // Meta

    let codegen_marker = interface_codegen_marker(&name);

    let meta = InterfaceMeta {
        mode: attrs.mode.clone(),
        methods: methods
            .iter()
            .map(|x| (x.name().to_string(), x.fn_kind.clone()))
            .collect(),
    };

    let exported_meta = export(format_ident!("{EXPORTED_META}"), ViaSerde(meta))?;

    // Dispatch

    let object_name = format_ident!("{name}Object");

    // TODO: kill it with fire
    let (dispatched_methods, object) = match attrs.mode.dispatch_kind() {
        Dispatch::Dynamic => todo!(),
        Dispatch::Static => {
            let impls = attrs.resolve_static_impls(&name);
            if impls.is_empty() {
                return Ok(input.into_token_stream());
            }
            let (dispatcher, object) = StaticDispatch::new(object_name.clone(), impls)?;
            let a: Vec<TokenStream> = methods
                .iter()
                .map(|x| dispatcher.dispatch_method(x))
                .collect::<Result<_>>()?;

            (a, object)
        }
    };

    //

    let content = quote! {
        #input
        #object

        impl #object_name {
            #(#dispatched_methods)*
            #(#final_methods)*
        }

        #[doc(hidden)]
        #[allow(non_camel_case)]
        pub(crate) mod #codegen_marker {
            #exported_meta
        }
    };

    Ok(content)
}

// impls

#[derive(Serialize, Deserialize)]
struct InterfaceMeta {
    pub mode: Mode,
    pub methods: HashMap<String, FnKind>,
}

fn interface_codegen_marker(interface: &Ident) -> Ident {
    format_ident!("__codegen_{interface}")
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

    let mut meta = path_sibling(&interface, interface_codegen_marker)?;
    meta.segments.push(parse_quote!(#EXPORTED_META));

    let ctx = Context {
        object,
        interface,
        impl_methods: impl_functions,
    };

    let continuation = with_import!(#simple meta => register_impl_inner(ctx))?;

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

    for func in ctx.impl_methods.inner {
        let name = func.sig.ident.to_string();
        // TODO: rustc already checks whether the method exists as part of trait resolution
        // we should instead return Ok("") here instead of unwrap
        let as_defined = meta.methods.get(&name).unwrap();
        let as_implemented = FnKind::parse(&func.sig)?;
    }

    let content = quote! {
        #signal
        // TODO: syn-path trait, tag, associated error
    };

    Ok(content)
}
