use std::collections::HashSet;

use darling::FromAttributes;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{
    Attribute, FnArg, Ident, ItemTrait, Pat, Path, Result, Signature, Token, TraitItem,
    TraitItemFn,
    parse::{Parse, ParseStream},
    parse_quote,
    spanned::Spanned,
};

use crate::{
    bail, ensure_empty_tokens,
    meta_passing::export,
    parse::{Many, parse, parse_attrs},
    retain_by_mask,
};

mod dispatch;
pub mod implement;
mod remote;

//

parse!(
#[derive(Clone)]
pub enum Mode {
    FromMod = mod,
    Dynamic = dyn,
    @default Direct
});

impl Mode {
    fn dispatch_kind(&self) -> Dispatch {
        match self {
            Self::Dynamic => Dispatch::Dynamic,
            _ => Dispatch::Direct,
        }
    }
}

#[derive(Default)]
enum Dispatch {
    /// similar to enum-dispatch
    #[default]
    Direct,

    /// similar to rustc's trait objects
    Dynamic,
}

impl Dispatch {
    fn supports_const(&self) -> bool {
        matches!(self, Self::Direct)
    }
}

struct DispatchCode {
    all_tags: TokenStream,
    object: TokenStream,
    dispatched_methods: Vec<TokenStream>,
    other_codegen: Option<TokenStream>,
}

//

pub struct InterfaceAttrs {
    mode: Mode,
    paths: Option<Vec<Path>>,
    // tag_repr: Type
}

// TODO: derive this
impl Parse for InterfaceAttrs {
    fn parse(input: ParseStream) -> Result<Self> {
        let mode_parse = input.lookahead1();

        let mode = if mode_parse.peek(Token![mod]) {
            let _ = input.parse::<Token![mod]>()?;
            Mode::FromMod
        } else if mode_parse.peek(Token![dyn]) {
            let _ = input.parse::<Token![dyn]>()?;
            Mode::Dynamic
        } else {
            Mode::Direct
        };

        let paths: Option<Many<Path>> = match mode {
            Mode::Direct => Some(input.parse()?),
            Mode::FromMod => {
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

//

#[derive(Clone)]
enum FnKind {
    Regular,
    Async { sync_path: bool },
    Const,
}

enum MethodKind {
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

struct Method {
    sig: syn::Signature,
    args: Vec<Ident>,

    kind: MethodKind,
    fn_kind: FnKind,
    is_final: bool,
}

impl Method {
    // This function will leave the signature as appropriate for a trait item
    // while storing an internal copy for dispatch
    fn parse(attrs: &mut Vec<Attribute>, sig: &mut Signature) -> Result<Self> {
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

    fn requires_dispatch(&self) -> bool {
        !self.is_final
    }

    fn call_name(&self) -> Ident {
        let ident = &self.sig.ident;

        // TODO: this breaks remote const methods
        match self.fn_kind {
            FnKind::Const => format_ident!("__const_{ident}"),
            _ => ident.clone(),
        }
    }

    // TODO: simplify
    fn call(&self, target: Path) -> TokenStream {
        let name = self.call_name();
        let args = &self.args;

        let access = match self.kind {
            MethodKind::Associated => quote! { :: },
            MethodKind::Stateful => quote! { . },
        };

        let call_body = match &self.fn_kind {
            FnKind::Async { sync_path: true } => todo!("sync path optimization"),
            FnKind::Async { sync_path: false } => quote! { #name( #(#args)* ).await },
            _direct_call => quote! { #name( #(#args)* ) },
        };

        quote! { #target #access #call_body }
    }

    fn call_via_self(&self) -> TokenStream {
        let target = match self.kind {
            MethodKind::Associated => parse_quote!(Self),
            MethodKind::Stateful => parse_quote!(self),
        };

        self.call(target)
    }
}

struct InterfaceShape {
    name: Ident,
    attrs: InterfaceAttrs,

    methods: Vec<Method>,
    final_methods: Vec<TraitItemFn>,
    // TODO: assoc const, types
}

impl InterfaceShape {
    fn new(name: Ident, attrs: InterfaceAttrs) -> Self {
        Self {
            name,
            attrs,
            methods: Vec::new(),
            final_methods: Vec::new(),
        }
    }

    fn parse(input: &mut ItemTrait, attrs: InterfaceAttrs) -> Result<Self> {
        let mut this = Self::new(input.ident.clone(), attrs);

        input.vis = parse_quote!(pub);
        input.ident = format_ident!("Interface");
        input.supertraits.push(parse_quote!(::plug::Init));

        let mut mask = Vec::new();

        for item in input.items.iter_mut() {
            match item {
                TraitItem::Fn(f) => {
                    let method = this.register_method(f)?;
                    mask.push(method.requires_dispatch())
                }
                TraitItem::Const(x) => bail!(x => "Constant property support is TBD"),
                TraitItem::Type(x) => bail!(x => "Associated type support is TBD"),
                TraitItem::Macro(x) => bail!(x => "Cannot define part of an interface via macros"),
                x => bail!(x => "This syntax is not supported inside interfaces"),
            }
        }

        // This removes all methods that are not implementable
        retain_by_mask(&mask, &mut input.items);

        Ok(this)
    }

    fn get_dispatch_kind(&self) -> Dispatch {
        self.attrs.mode.dispatch_kind()
    }

    fn register_method(&mut self, input: &mut TraitItemFn) -> Result<&Method> {
        let sig = &mut input.sig;
        let method = Method::parse(&mut input.attrs, sig)?;

        if matches!(method.fn_kind, FnKind::Const) && !self.get_dispatch_kind().supports_const() {
            bail!(
                sig.ident =>
                "Const fn is not supported when using dynamic dispatch"
            );
        }

        let ret = self.methods.push_mut(method);

        Ok(ret)
    }

    fn export_meta(&self) -> Result<TokenStream> {
        let mut variable_async_methods = HashSet::new();
        let mut const_methods = HashSet::new();

        for method in &self.methods {
            let ident = &method.sig.ident;
            match method.fn_kind {
                FnKind::Const => {
                    const_methods.insert(ident.clone());
                }
                FnKind::Async { sync_path: true } => {
                    variable_async_methods.insert(ident.clone());
                }
                _ => (),
            };
        }

        let data = InterfaceMeta {
            mode: self.attrs.mode.clone(),
            const_methods: const_methods.into(),
            variable_async_methods: variable_async_methods.into(),
        };

        let exported_meta = export(format_ident!("__meta"), data)?;

        Ok(exported_meta)
    }

    fn codegen(self) -> Result<TokenStream> {
        let final_methods = self.final_methods.clone(); // TODO

        let DispatchCode {
            all_tags,
            object,
            other_codegen,
            dispatched_methods,
        } = match self.attrs.mode.dispatch_kind() {
            Dispatch::Dynamic => todo!("dyn path"),
            Dispatch::Direct => dispatch::direct::dispatch(self),
        }?;

        let content = quote! {
            #object
            #other_codegen

            // TODO: arbitrarily represented tags
            #[derive(PartialEq)]
            pub struct Tag(&'static str);

            // TODO: tag parsing with exhaustive hints
            impl Tag {
                pub(crate) const fn define(repr: &'static str) -> Self { Self(repr) }
            }

            pub struct Descriptor;

            impl ::plug::InterfaceDescriptor for Descriptor {
                type Tag = Tag;
                const ALL: &[Self::Tag] = #all_tags;
            }

            impl Object {
                #(#final_methods)*
                #(#dispatched_methods)*
            }
        };

        Ok(content)
    }
}

pub fn trait_to_interface(attrs: InterfaceAttrs, mut trait_: ItemTrait) -> Result<TokenStream> {
    ensure_empty_tokens!(trait_.generics.params, "Generic interfaces are TBD");
    ensure_empty_tokens!(trait_.supertraits, "Nested interfaces are TBD");

    let vis = trait_.vis.clone();

    let shape = InterfaceShape::parse(&mut trait_, attrs)?;

    let name = shape.name.clone(); // TODO
    let meta = shape.export_meta()?;

    let dispatched = shape.codegen()?;

    let content = quote! {
        #[allow(non_snake_case)]
        #vis mod #name {
            use super::*;

            #trait_ #meta #dispatched
        }
    };

    Ok(content)
}

// Interface meta is imported by impls

parse!(
    struct InterfaceMeta {
        mode: Mode,
        const_methods: Many<Ident, HashSet<Ident>>,
        variable_async_methods: Many<Ident, HashSet<Ident>>,
    }
);

fn registered_module_object_marker(interface: &Ident) -> Ident {
    format_ident!("registered_{interface}_impl_for_this_module")
}
