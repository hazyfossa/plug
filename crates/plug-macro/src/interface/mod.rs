use std::collections::HashSet;

use darling::FromAttributes;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{Ident, ItemTrait, Result, TraitItem, TraitItemFn, Type, parse_quote, spanned::Spanned};

use crate::{
    bail, derive_data, ensure_empty_tokens,
    meta_passing::export,
    method::{FnKind, Method},
    parse::{Many, parse, parse_attrs},
    remote::remote_codegen,
    retain_by_mask,
};

mod dispatch;
mod implement;

use dispatch::{DispatchCode, DispatchTarget};

pub use implement::{const_redirect, register_impl};

parse!(
#[derive(Clone)]
pub enum Mode {
    FromMod = mod,
    Dynamic = dyn,
    @default Direct
});

#[derive(FromAttributes)]
#[darling(attributes(plug))]
struct InterfaceOptions {
    #[darling(default, rename = "remote")]
    is_remote: bool,

    // NOTE: the default, string repr is performant enough for most cases
    // it is stored as a pointer to a 'static str
    // the only application for other reprs are tags with width less than that of a pointer
    //
    // TODO: support for tag_type (const define will need to be out-of-trait)
    tag_repr: Option<Type>,
}

// TODO: Trimmed version (ShapeForDispatch)
struct InterfaceShape {
    name: Ident,
    options: InterfaceOptions,
    dispatch_target: DispatchTarget,

    methods: Vec<Method>,
    final_methods: Vec<TraitItemFn>,
    // TODO: assoc const, types
}

impl InterfaceShape {
    fn new(name: Ident, options: InterfaceOptions, dispatch_target: DispatchTarget) -> Self {
        Self {
            name,
            options,
            dispatch_target,
            methods: Vec::new(),
            final_methods: Vec::new(),
        }
    }

    fn parse(input: &mut ItemTrait, dispatch_target: DispatchTarget) -> Result<Self> {
        let options = parse_attrs(&mut input.attrs)?;

        let mut this = Self::new(input.ident.clone(), options, dispatch_target);

        input.vis = parse_quote!(pub);
        input.ident = format_ident!("Interface");

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

    fn register_method(&mut self, input: &mut TraitItemFn) -> Result<&Method> {
        let method = Method::from_trait(input)?;

        if matches!(method.fn_kind, FnKind::Const) && !self.dispatch_target.supports_const() {
            bail!(
                input.sig.ident =>
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
            let ident = &method.name;
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
            mode: self.dispatch_target.clone(),
            const_methods: const_methods.into(),
            variable_async_methods: variable_async_methods.into(),
        };

        let exported_meta = export(format_ident!("__meta"), data)?;

        Ok(exported_meta)
    }

    fn codegen(self) -> Result<TokenStream> {
        let tag_repr = &self
            .options
            .tag_repr
            .clone()
            .unwrap_or(parse_quote! { &'static str });

        let final_methods = self.final_methods.clone();

        let maybe_remote = self.options.is_remote.then_some(remote_codegen(
            &self.methods,
            format_ident!("remote"),
            format_ident!("Object"),
        )?);

        let derive_data = derive_data();

        let DispatchCode { codegen } = dispatch::dispatch(self)?;

        let content = quote! {
            #codegen // ALL, Config, Object
            #maybe_remote

            #derive_data // TODO: custom derive
            #[derive(PartialEq, Clone, Copy)]
            pub struct Tag(#tag_repr);

            impl Tag {
                pub(crate) const fn define(repr: #tag_repr) -> Self { Self(repr) }

                // TODO: parse error with exhaustive hints
                pub fn parse(value: impl PartialEq<#tag_repr>) -> Option<Self> {
                    ALL.iter().find(|tag| &value == &tag.0).copied()
                }
            }

            impl Object {
                pub(crate) fn current_tag(&self) -> Tag {
                    todo!();
                }
            }

            pub struct Descriptor;

            impl ::plug::InterfaceDescriptor for Descriptor {
                type Config = Config;
                type Object = Object;
                type Tag = Tag;
                const ALL: &[Self::Tag] = ALL;
            }

            impl Object {
                #(#final_methods)*
            }
        };

        Ok(content)
    }
}

pub fn trait_to_interface(attrs: DispatchTarget, mut trait_: ItemTrait) -> Result<TokenStream> {
    ensure_empty_tokens!(trait_.generics.params, "Generic interfaces are TBD");
    ensure_empty_tokens!(trait_.supertraits, "Nested interfaces are TBD");

    let vis = trait_.vis.clone();

    let shape = InterfaceShape::parse(&mut trait_, attrs)?;

    let name = shape.name.clone();
    let meta = shape.export_meta()?;

    let dispatched = shape.codegen()?;

    let content = quote! {
        #[allow(non_snake_case, non_camel_case_types)]
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
