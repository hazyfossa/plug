// TODO: syn is both heavy and too restrictive (disallows CFIT)

#![allow(dead_code)]

use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, format_ident, quote, quote_spanned};
use syn::{
    parse::{Nothing, Parse, ParseStream},
    spanned::Spanned,
    *,
};

mod utils;
use syn_derive::{Parse, ToTokens};
use utils::*;

define!(plug = plug_impl);

fn export(marker: &str, input: impl ToTokens) -> TokenStream {
    // TODO: reconsider (pass marker directly to quote)
    let ident = Ident::new(marker, Span::call_site());

    quote! {
        #[doc(hidden)]
        macro_rules! #ident {
            ($($m:tt)*) => { $($m)*!(#input); };
        }

        pub(crate) use #ident;
    }
}

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

#[derive(Clone, Parse, ToTokens)]
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

    // fn is_state(&mut self, field: &Field) -> Result<bool> {
    //     self.check_for_marker(field)?;
    //     Ok(self.passed_marker) // || match_attr_flag(field, "state").is_some()
    // }
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

#[derive(Parse, ToTokens)]
enum FnKind {
    Regular,
    Async,
    Const,
}

#[derive(Parse, ToTokens)]
struct FnShape {
    name: Ident,
    kind: FnKind,
}

#[derive(Parse, ToTokens)]
struct MetaForInterface {
    mode: Mode,
    const_methods: Many<Ident>,
}

struct EnumDispatch {
    impls: Vec<Path>,
    shape: Vec<FnShape>,
}

impl EnumDispatch {
    fn construct(self) -> TokenStream {
        todo!()
    }
}

struct DynDispatch {
    shape: Vec<FnShape>,
}

impl DynDispatch {
    fn construct(self) -> TokenStream {
        todo!()
    }
}

struct Interface {
    mode: Mode,
    paths: Option<Vec<Path>>,
    methods: Vec<FnShape>,
    direct_methods: Vec<ItemFn>,
}

enum AsyncDispatch {
    Direct,
    Outlined,
}

// TODO: We could technically borrow idents from syn here
// but this will propagate lifetimes everywhere
struct InterfaceShape {
    ident: Ident,
    methods: Vec<FnShape>,
    // TODO: const, types
}

impl InterfaceShape {
    fn new(ident: Ident) -> Self {
        Self {
            ident,
            methods: Vec::new(),
        }
    }

    fn register_method(&mut self, input: &TraitItemFn) -> Result<()> {
        let function = &input.sig;

        // TODO: modifers that we want but syn doesn't parse: final

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

        self.methods.push(FnShape {
            name: function.ident.clone(),
            kind,
        });

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

    let const_methods = shape
        .methods
        .iter()
        .filter_map(|x| matches!(x.kind, FnKind::Const).then_some(x.name.clone()))
        .collect();

    let meta = export(
        "meta",
        MetaForInterface {
            mode: attrs.mode,
            const_methods,
        },
    );

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

fn register_impl_inner(input: ItemImpl, resolution_mode: Mode) -> TokenStream {
    let markers = match resolution_mode {
        Mode::Direct => TokenStream::default(),
        Mode::FromMod => {
            let marker = registered_module_object();
            let target = input.self_ty.clone();
            quote! { pub type #marker = #target; }
        }
        Mode::Dynamic => todo!("linkage"),
    };

    quote! { #markers #input }
}
