#![allow(dead_code)]

// TODO: syn is both heavy and too restrictive (disallows CFIT, finals)

// TODO: it is most definitely possible to avoid the ImplMetadata passing
// at an unclear performance cost
// note that such mode of operation will even be required for dyn impls
// if the cost is small enough, we may get rid of compile-time costly
// metadata passing and resolve all FnKind mismatches via dyn path
//
// After some pondering, the cost seems to be a branch on associated const
// (per call). Check if compiler optimizes.

use proc_macro2::TokenStream;
use serde::{Deserialize, Serialize};
use syn::{
    parse::{Nothing, ParseStream},
    spanned::Spanned,
    *,
};
use syn_derive::{Parse, ToTokens};

mod object;

mod utils;
use utils::*;

mod dispatch;
use dispatch::Dispatch;

const NAME: &str = "plug";

define!(plug = plug_impl);

#[derive(Parse)]
enum Code {
    #[parse(peek = Token![struct])]
    Struct(ItemStruct),
    #[parse(peek = Token![trait])]
    Trait(ItemTrait),
    #[parse(peek = Token![impl])]
    Impl(ItemImpl),
}

fn plug_impl(attrs: TokenStream, input: Code) -> Result<TokenStream> {
    match input {
        Code::Trait(x) => trait_to_interface(attrs, x),
        Code::Struct(x) => object::struct_to_object(attrs, x),
        Code::Impl(x) => register_impl(attrs, x),
    }
}

#[derive(Clone, Parse, ToTokens, Serialize, Deserialize)]
enum Mode {
    #[parse(peek = Token![mod])]
    FromMod,
    #[parse(peek = Token![dyn])]
    Dynamic,

    Direct,
}

tokenum! {
#[cfg_attr(feature = "direct", derive(Serialize, Deserialize))]
#[derive(Clone, Copy)]
enum AsyncDispatchKind {
    Inline,
    Outline,
}}

impl Default for AsyncDispatchKind {
    fn default() -> Self {
        Self::Inline
    }
}

#[cfg_attr(feature = "direct", derive(Serialize, Deserialize))]
struct AsyncDispatchModifier {
    kind: AsyncDispatchKind,
    is_final: bool,
}

#[cfg_attr(feature = "direct", derive(Serialize, Deserialize))]
enum FnKind {
    Regular,
    Async {
        dispatch: Option<WithSpan<AsyncDispatchModifier>>,
    },
    Const,
}

type FunctionIdent = String;
type Methods = Vec<(FunctionIdent, FnKind)>;

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
            methods: Vec::new(),
            final_method_impls: Vec::new(),
        }
    }

    fn register_method(&mut self, input: &mut TraitItemFn) -> Result<bool> {
        let mut attrs = Attrs::extract(&mut input.attrs)?;

        let is_final = attrs.pull(["final"])?.is_some();
        if is_final {
            self.final_method_impls.push(input.clone());
            return Ok(false);
        }

        let function = &input.sig;
        let name = function.ident.to_string();

        // TODO: modifers that we want but syn doesn't parse: final

        let is_async = function.asyncness.is_some();
        let is_const = function.constness.is_some();

        if is_async && is_const {
            bail!(function.constness => "constant async methods are impossible")
        }

        let kind = if is_async {
            let dispatch = attrs
                .pull_tokenum::<AsyncDispatchKind>()?
                // TODO parse finality of dispatch
                .map(|kind| {
                    kind.map(|x| AsyncDispatchModifier {
                        kind: x,
                        is_final: false,
                    })
                });

            FnKind::Async { dispatch }
        } else if is_const {
            FnKind::Const
        } else {
            FnKind::Regular
        };

        self.methods.push((name, kind));

        Ok(true)
    }

    // This will split the stuff `plug` manages internally
    // and leave the trait as suitable for IDE hints on impl
    fn from_trait(input: &mut ItemTrait) -> Result<Self> {
        let mut this = Self::new(input.ident.clone());

        let mut ret = Vec::new();

        for item in input.items.iter_mut() {
            match item {
                TraitItem::Fn(f) => ret.push(this.register_method(f)?),
                TraitItem::Const(x) => bail!(x => "Constant property support is TBD"),
                TraitItem::Type(x) => bail!(x => "Associated type support is TBD"),
                TraitItem::Macro(x) => bail!(x => "Cannot define part of an interface via macros"),
                x => bail!(x => "This syntax is not supported inside interfaces"),
            }
        }

        retain_by_mask(&ret, &mut input.items);

        Ok(this)
    }
}

fn trait_to_interface(attrs: TokenStream, mut input: ItemTrait) -> Result<TokenStream> {
    ensure_empty!(
        input.generics.params,
        "Generic interfaces are not supported (yet)"
    );

    let shape = InterfaceShape::from_trait(&mut input)?;
    let content = dispatch::Impl::dispatch(attrs, shape)?;

    Ok(purescope(input.vis, input.ident, content))
}

fn register_impl(attrs: TokenStream, input: ItemImpl) -> Result<TokenStream> {
    let _: Nothing = syn::parse2(attrs)?;

    let interface = match input.trait_ {
        Some((path, _)) => path,
        None => bail!(=> "This macro only makes sense for interface implementations"),
    };

    let object = input.self_ty;

    ensure_empty!(
        input.generics.params,
        "Generic interfaces are not supported (yet)"
    );

    dispatch::Impl::register_impl(interface, object)
}
