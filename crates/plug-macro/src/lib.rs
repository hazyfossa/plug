// TODO: syn is both heavy and too restrictive (disallows CFIT)

#![allow(dead_code)]

use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, format_ident, quote, quote_spanned};
use syn::{
    parse::{Parse, ParseStream},
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
        Code::Struct(x) => struct_to_object(x),
        Code::Impl(x) => register_impl(x),
    }
}

#[derive(Clone, Parse, ToTokens)]
enum Mode {
    #[parse(peek = Token![@])]
    FromMod,
    Direct,
}

struct ParsedAttrs {
    mode: Mode,
    paths: TokenVec<Path>,
}

// TODO: derive this
impl Parse for ParsedAttrs {
    fn parse(input: ParseStream) -> Result<Self> {
        let mode: Mode = input.parse()?;

        let paths = match mode {
            Mode::Direct => input.parse(),
            Mode::FromMod => {
                let _marker = input.parse::<Token![@]>()?;
                let content;
                syn::parenthesized!(content in input);
                content.parse()
            }
        }?;

        Ok(ParsedAttrs { mode, paths })
    }
}

impl ParsedAttrs {
    fn resolve_impls(self) -> Vec<Path> {
        match self.mode {
            Mode::Direct => self.paths.0,
            Mode::FromMod => todo!("resolve mod"),
        }
    }
}

fn match_attr_flag<'a>(field: &'a Field, flag: &str) -> Option<&'a Attribute> {
    field.attrs.iter().find(|attr| {
        let exact_match = attr.path().is_ident(flag);
        let is_standalone = matches!(attr.meta, Meta::Path(_));
        exact_match && is_standalone
    })
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

fn struct_to_object(input: ItemStruct) -> Result<TokenStream> {
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

const REGISTRATION_MARKER: &str = "__primary_object_for_this_module";

fn register_impl(input: ItemImpl) -> Result<TokenStream> {
    let interface = match input.trait_ {
        Some((path, _)) => path,
        None => bail!(=> "This macro only makes sense for interface implementations"),
    };

    let content = quote! {};

    Ok(content)
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
    shape: TokenVec<FnShape>,
}

struct Interface {
    mode: Mode,
    impls: Vec<Path>,
    methods: Vec<FnShape>,
    direct_methods: Vec<ItemFn>,
}

impl Interface {
    fn new(attrs: ParsedAttrs) -> Self {
        Self {
            mode: attrs.mode.clone(),
            impls: attrs.resolve_impls(),
            methods: Vec::new(),
            direct_methods: Vec::new(),
        }
    }

    fn register_method(&mut self, function: TraitItemFn) {
        let function = function.sig; // TODO: should we forward anything here?

        // TODO: modifers that we want but syn doesn't parse: final
        // TODO: error on const async

        let kind = if function.asyncness.is_some() {
            FnKind::Async
        } else if function.constness.is_some() {
            FnKind::Const
        } else {
            FnKind::Regular
        };

        self.methods.push(FnShape {
            name: function.ident,
            kind,
        });
    }

    fn meta(self) -> MetaForInterface {
        MetaForInterface {
            mode: self.mode,
            shape: TokenVec(self.methods),
        }
    }

    fn construct(self, span: Span) -> TokenStream {
        let variant_idents: Vec<_> = (0..self.impls.len())
            .map(|x| format_ident!("V{x}", span = span))
            .collect();

        let variants: Vec<_> = variant_idents
            .iter()
            .zip(self.impls)
            .map(|(v, imp)| quote! { #v(#imp) })
            .collect();

        let meta = export(
            "meta",
            MetaForInterface {
                mode: self.mode,
                shape: TokenVec(self.methods),
            },
        );

        quote_spanned! { span=>
            pub trait Trait {}

            // This alternatively would be newtype of Stored<S, dyn T>
            pub enum Object {
                #(#variants,)*
            }

            impl Object {}

            #meta
        }
    }
}

fn trait_to_interface(attrs: TokenStream, input: ItemTrait) -> Result<TokenStream> {
    ensure_empty!(
        input.generics.params,
        "Generic objects are not supported (yet)"
    );

    let attrs: ParsedAttrs = syn::parse2(attrs)?;
    let mut interface = Interface::new(attrs);

    for item in input.items {
        match item {
            TraitItem::Fn(function) => {
                interface.register_method(function);
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

    let content = interface.construct(Span::call_site());
    Ok(purescope(input.vis, input.ident, content))
}

fn register_impl_inner(object: Ident, resolution_mode: Mode) -> TokenStream {
    match resolution_mode {
        Mode::Direct => TokenStream::new(),
        Mode::FromMod => export(REGISTRATION_MARKER, object),
    }
}
