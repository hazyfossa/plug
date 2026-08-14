// TODO: syn is both heavy and too restrictive (disallows CFIT)

#![allow(dead_code)]

// TODO: it is most definitely possible to avoid the ImplMetadata passing
// at an unclear performance cost
// note that such mode of operation will even be required for dyn impls
// if the cost is small enough, we may get rid of compile-time costly
// metadata passing and resolve all FnKind mismatches via dyn path

use std::collections::HashMap;

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use serde::{Deserialize, Serialize};
use syn::{
    parse::{Nothing, Parse, ParseStream},
    spanned::Spanned,
    *,
};
use syn_derive::{Parse, ToTokens};

mod meta;
use meta::*;

mod utils;
use utils::*;

mod dispatch;

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
        Code::Struct(x) => struct_to_object(attrs, x),
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

struct ParsedAttrs {
    mode: Mode,
    paths: Option<Vec<Path>>,
}

// TODO: derive this
impl Parse for ParsedAttrs {
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

        Ok(ParsedAttrs { mode, paths })
    }
}

struct StateMatcher {
    passed_marker: bool,
}

impl StateMatcher {
    fn new() -> Self {
        Self {
            passed_marker: false,
        }
    }

    fn is_state(&mut self, field: &Field) -> Result<Option<bool>> {
        // TODO: consider other approaches
        // (but not attrs, they do not work)
        if matches!(field.ty, Type::Never(_)) {
            if self.passed_marker {
                bail!(field.ty => "An object cannot have >1 state separator");
            } else {
                self.passed_marker = true;
                return Ok(None);
            }
        }

        Ok(Some(self.passed_marker))
    }
}

// TODO: rewrite this as quote!
fn outscope_field(vis: Visibility) -> Visibility {
    if matches!(vis, Visibility::Inherited) {
        Visibility::Restricted(VisRestricted {
            pub_token: token::Pub::default(),
            paren_token: token::Paren::default(),
            in_token: None,
            path: Box::new(token::Super::default().into()),
        })
    } else {
        vis
    }
}

fn struct_to_object(attrs: TokenStream, input: ItemStruct) -> Result<TokenStream> {
    let _: Nothing = syn::parse2(attrs)?;

    let mut config: Vec<Field> = Vec::new();
    let mut state: Vec<Field> = Vec::new();

    let mut state_matcher = StateMatcher::new();

    for mut field in input.fields {
        let target = match state_matcher.is_state(&field)? {
            Some(true) => &mut state,
            Some(false) => &mut config,
            None => continue,
        };

        field.vis = outscope_field(field.vis);
        target.push(field);
    }

    let attrs = input.attrs;

    let content = quote! {
        #(#attrs)*
        pub struct Config {
            #(#config,)*
        }

        // TODO: allow borrows from self
        //
        // TODO: make Borrow<Config> part of self
        // (combined with self-ref allows borrow from Config)
        pub struct State {
            #(#state,)*
        }

        impl ::plug::State for State {
            type Config = Config;
        }
    };

    Ok(purescope(input.vis, input.ident, content))
}

type ObjectPath = Path;

#[cfg_attr(feature = "direct", derive(Serialize, Deserialize))]
#[derive(Clone, Copy, Default)]
enum AsyncDispatchKind {
    #[default]
    Inline,

    Outline,
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
    methods_attr_map: HashMap<FunctionIdent, Span>,
    // TODO: const, types
    final_method_impls: Vec<ItemFn>,
}

impl InterfaceShape {
    fn new(ident: Ident) -> Self {
        Self {
            ident,
            methods: Vec::new(),
            methods_attr_map: HashMap::new(),
            final_method_impls: Vec::new(),
        }
    }

    fn register_method(&mut self, input: &TraitItemFn) -> Result<()> {
        let function = &input.sig;
        let name = function.ident.to_string();

        let attrs = &input.attrs;
        

        // TODO: modifers that we want but syn doesn't parse: final

        let is_async = function.asyncness.is_some();
        let is_const = function.constness.is_some();

        if is_async && is_const {
            bail!(function.constness => "constant async methods are impossible")
        }

        let kind = if is_async {
            let modifier = query_attr_flag(attrs, "outline")
                .is_some()
                .then_some(AsyncDispatchModifier::Outline)
                .unwrap_or_default();

            // TODO: make this a modifier of modifier instead ( #[final(outline)] )
            let is_final = query_attr_flag(&input.attrs., "dispatch_final");
            FnKind::Async { dispatch }
        } else if is_const {
            FnKind::Const
        } else {
            FnKind::Regular
        };

        self.methods_attr_map.insert(name, attrs.)
        self.methods.push((name, kind));

        Ok(())
    }

    fn parse(input: &ItemTrait) -> Result<Self> {
        let mut this = Self::new(input.ident.clone());

        for item in &input.items {
            match item {
                TraitItem::Fn(function) => {
                    this.register_method(function)?;
                }

                // TODO: const support
                // a polyfill to const fn is required for dispatch
                // we could also treat these differently in remote codegen
                // (autogenerated get_property method)
                TraitItem::Const(x) => bail!(x => "Constant property support is TBD"),

                // TODO: do we object-ify recursively or require to pass concrete (like dyn does)
                TraitItem::Type(x) => bail!(x => "Associated type support is TBD"),

                TraitItem::Macro(x) => bail!(x => "Cannot define part of an interface via macros"),
                x => bail!(x => "This syntax is not supported inside interfaces"),
            }
        }

        Ok(this)
    }
}

fn trait_to_interface(attrs: TokenStream, input: ItemTrait) -> Result<TokenStream> {
    ensure_empty!(
        input.generics.params,
        "Generic interfaces are not supported (yet)"
    );

    let attrs: ParsedAttrs = syn::parse2(attrs)?;
    let shape = InterfaceShape::parse(&input)?;

    let meta = export(
        "meta",
        MetaForInterface {
            mode: attrs.mode,
            methods: shape.methods,
        },
    )?;

    let content = quote! {
        pub trait Trait {}

        #meta
    };

    Ok(purescope(input.vis, input.ident, content))
}

fn registered_module_object() -> Ident {
    format_ident!("__primary_object_for_this_module")
}

fn register_impl(attrs: TokenStream, input: ItemImpl) -> Result<TokenStream> {
    let interface = match input.trait_ {
        Some((path, _)) => path,
        None => bail!(=> "This macro only makes sense for interface implementations"),
    };

    ensure_empty!(
        input.generics.params,
        "Generic interfaces are not supported (yet)"
    );

    let content = todo!("call with tokens");

    Ok(content)
}

fn register_impl_inner(input: ItemImpl, meta: MetaForInterface) -> Result<TokenStream> {
    let (vis, ident) = match meta.mode {
        Mode::Direct => (Visibility::Inherited, todo!("random ident")),
        Mode::FromMod => (
            Visibility::Public(token::Pub::default()),
            registered_module_object(),
        ),
        Mode::Dynamic => todo!("whole different path here"),
    };

    let meta = export("meta", ())?; // TODO

    let content = quote! { #meta #input };
    Ok(purescope(vis, ident, content))
}
