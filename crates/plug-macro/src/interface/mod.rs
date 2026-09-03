use std::collections::HashMap;

use proc_macro2::TokenStream;
use serde::{Deserialize, Serialize};
use syn::spanned::Spanned;
use syn::{
    Ident, ItemTrait, Path, Result, Token, TraitItem, TraitItemFn,
    parse::{Parse, ParseStream},
};
use syn_derive::{Parse, ToTokens};

use crate::{
    bail, ensure_empty_tokens,
    parse::{Attrs, Many},
    retain_by_mask,
};

#[derive(Clone, Parse, ToTokens, Serialize, Deserialize)]
enum Mode {
    #[parse(peek = Token![mod])]
    FromMod,
    #[parse(peek = Token![dyn])]
    Dynamic,

    Direct,
}

enum CallConvention {
    Direct,
    Await,
}

#[derive(Clone, Serialize, Deserialize)]
enum FnKind {
    Regular,
    Async,
    Const,
}

type FunctionIdent = String;
type Methods = HashMap<FunctionIdent, FnKind>;

type InterfaceMeta = Methods;

struct InterfaceShape {
    ident: Ident,
    methods: Methods,
    // TODO: const, types
    final_method_impls: Vec<TraitItemFn>,
}

impl InterfaceShape {
    fn new(ident: Ident) -> Self {
        Self {
            ident,
            methods: HashMap::new(),
            final_method_impls: Vec::new(),
        }
    }

    // Returns whether the method is implementable
    fn register_method(&mut self, input: &mut TraitItemFn) -> Result<bool> {
        let mut attrs = Attrs::extract(&mut input.attrs)?;

        let is_final = attrs.select_one(["final"])?.is_some();
        if is_final {
            self.final_method_impls.push(input.clone());
            return Ok(false);
        }

        let function = &input.sig;
        let name = function.ident.to_string();

        // TODO: modifers that we want but syn doesn't parse: final
        // for now, we substitute via custom attr

        let is_async = function.asyncness.is_some();
        let is_const = function.constness.is_some();

        if is_async && is_const {
            bail!(function.constness => "constant async methods are impossible")
        }

        let kind = if is_async {
            FnKind::Async
        } else if is_const {
            FnKind::Const
        } else {
            FnKind::Regular
        };

        self.methods.insert(name, kind);

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

    todo!()
}
