use std::collections::HashSet;

use darling::FromAttributes;
use proc_macro2::TokenStream;
use quote::{ToTokens, format_ident, quote};
use syn::{
    FnArg, Ident, ImplItem, ImplItemFn, ItemImpl, ItemTrait, Pat, Path, Result, Signature, Token,
    TraitItem, TraitItemFn, Type,
    parse::{Nothing, Parse, ParseStream},
    parse_quote,
    spanned::Spanned,
};

use crate::{
    amyhow, bail, ensure_empty_tokens,
    meta_passing::{export, with_import},
    parse::{Many, PathExt, parse, parse_attrs},
    retain_by_mask,
};

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
            _ => Dispatch::Static,
        }
    }
}

#[derive(Default)]
enum Dispatch {
    /// similar to enum-dispatch
    #[default]
    Static,

    /// similar to rustc's trait objects
    /// (uses them under the hood, in fact)
    Dynamic,
}

impl Dispatch {
    fn supports_const(&self) -> bool {
        matches!(self, Self::Static)
    }
}

pub struct InterfaceAttrs {
    mode: Mode,
    paths: Option<Vec<Path>>,
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

impl FnKind {
    fn parse(sig: &syn::Signature) -> Result<Self> {
        let is_async = sig.asyncness.is_some();
        let is_const = sig.constness.is_some();

        if is_async && is_const {
            bail!(sig.constness.unwrap() => "constant async methods are impossible")
        }

        let kind = if is_async {
            // TODO: sync path optimization
            Self::Async { sync_path: false }
        } else if is_const {
            Self::Const
        } else {
            Self::Regular
        };

        Ok(kind)
    }
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

struct Method {
    dispatch_signature: syn::Signature,
    fn_kind: FnKind,
}

impl Method {
    // This function will leave the signature as appropriate for a trait item
    // while storing an internal copy for dispatch
    fn parse(sig: &mut Signature) -> Result<Self> {
        let fn_kind = FnKind::parse(sig)?;

        // Rust does not support const fn in trait natively
        // Const-ness will be restored by dispatcher
        let dispatch_signature = sig.clone();
        sig.constness = None;

        Ok(Self {
            dispatch_signature,
            fn_kind,
        })
    }

    fn kind(&self) -> MethodKind {
        MethodKind::parse(&self.dispatch_signature)
    }

    fn name(&self) -> Ident {
        let ident = &self.dispatch_signature.ident;

        match self.fn_kind {
            FnKind::Const => format_ident!("__const_{ident}"),
            _ => ident.clone(),
        }
    }

    fn args(&self) -> Result<Vec<&Ident>> {
        let mut args: Vec<&Ident> = Vec::new();

        for arg in &self.dispatch_signature.inputs {
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

    // Requires prepending `provider`. or `Provider`:: as appropriate
    fn call_via_context(&self) -> Result<TokenStream> {
        let name = self.name();
        let args = self.args()?;

        let content = match &self.fn_kind {
            FnKind::Async { sync_path: true } => todo!("sync path optimization"),
            FnKind::Async { sync_path: false } => quote! { #name( #(#args)* ).await },
            _direct_call => quote! { #name( #(#args)* ) },
        };

        Ok(content)
    }

    fn call_via_self(&self) -> Result<TokenStream> {
        let provider = match self.kind() {
            MethodKind::Associated => quote! { Self:: },
            MethodKind::Stateful => quote! { self. },
        };

        let call = self.call_via_context()?;

        let content = quote! { #provider #call };
        Ok(content)
    }
}

#[derive(FromAttributes)]
#[darling(attributes(plug))]
struct MethodAttrs {
    #[darling(default, rename = "final")]
    is_final: bool,
}

struct InterfaceShape {
    name: Ident,
    attrs: InterfaceAttrs,

    dispatchable_methods: Vec<Method>,
    final_methods: Vec<TraitItemFn>,
    // TODO: assoc const, types
}

impl InterfaceShape {
    fn new(name: Ident, attrs: InterfaceAttrs) -> Self {
        Self {
            name,
            attrs,
            dispatchable_methods: Vec::new(),
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
                TraitItem::Fn(f) => mask.push(this.register_method(f)?),
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

    // Returns whether the method is implementable
    fn register_method(&mut self, input: &mut TraitItemFn) -> Result<bool> {
        // TODO: modifers that we want but syn doesn't parse: final
        // for now, we substitute via custom attr
        let attrs: MethodAttrs = parse_attrs(&mut input.attrs)?;

        if attrs.is_final {
            self.final_methods.push(input.clone());
            return Ok(false);
        }

        let method = Method::parse(&mut input.sig)?;

        if matches!(method.fn_kind, FnKind::Const) && !self.get_dispatch_kind().supports_const() {
            bail!(
                method.dispatch_signature.constness =>
                "Const fn is not supported when using dynamic dispatch"
            );
        }

        self.dispatchable_methods.push(method);

        Ok(true)
    }

    fn export_meta(&self) -> Result<TokenStream> {
        let mut variable_async_methods = HashSet::new();
        let mut const_methods = HashSet::new();

        for method in &self.dispatchable_methods {
            let ident = &method.dispatch_signature.ident;
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

    fn resolve_impls(&self) -> Option<Vec<Path>> {
        let mut paths = self.attrs.paths.clone()?;

        if paths.is_empty() {
            return None;
        }

        if matches!(self.attrs.mode, Mode::FromMod) {
            for path in &mut paths {
                path.extend(registered_module_object_marker(&self.name));
            }
        }

        Some(paths)
    }

    fn dispatch(self) -> Result<TokenStream> {
        let DispatchCode {
            codegen,
            dispatched_methods,
        } = match self.attrs.mode.dispatch_kind() {
            Dispatch::Dynamic => todo!("dyn path"),
            Dispatch::Static => {
                // TODO: consider making "dyn" the default, switch to static when first impl seen
                // Pros: no annoying error on first write
                // Cons: goes against "make perf cost explicit"
                let impls = self.resolve_impls().ok_or(
                    amyhow!(self.name => "At least one impl is required for interface dispatch"),
                )?;

                static_dispatch(impls, self.dispatchable_methods)?
            }
        };

        let final_methods = self.final_methods;

        let content = quote! {
            #codegen

            impl Object {
                #(#final_methods)*
                #(#dispatched_methods)*
            }
        };

        Ok(content)
    }
}

struct DispatchCode {
    codegen: TokenStream,
    dispatched_methods: Vec<TokenStream>,
}

// TODO: consider instead caching variant (needs impl to be a newtype with self-ref field)
fn provide_call_context(kind: &MethodKind, impl_: &Path) -> Result<TokenStream> {
    let variant = impl_.last_ident()?;
    let content = match kind {
        MethodKind::Stateful => quote! { Self::#variant(obj) => obj. },
        MethodKind::Associated => quote! { Tag::#variant => #impl_:: },
    };

    Ok(content)
}

fn static_dispatch_method(impls: &[Path], method: Method) -> Result<TokenStream> {
    let kind = method.kind();
    let call_via_ctx = method.call_via_context()?;
    let mut sig = method.dispatch_signature;

    let determinant = match kind {
        MethodKind::Stateful => quote! { self },
        MethodKind::Associated => {
            sig.inputs.insert(0, parse_quote!(tag: Tag));
            quote! { tag }
        }
    };

    let branches: Vec<TokenStream> = impls
        .iter()
        .map(|impl_| {
            let ctx = provide_call_context(&kind, impl_)?;
            Ok(quote! { #ctx #call_via_ctx })
        })
        .collect::<Result<_>>()?;

    let content = quote! { pub #sig {
        match #determinant { #(#branches)* }
    }};

    Ok(content)
}

fn static_dispatch(impls: Vec<Path>, methods: Vec<Method>) -> Result<DispatchCode> {
    let variants: Vec<_> = impls
        .iter()
        .map(|x| x.last_ident())
        .collect::<Result<_>>()?;

    let tags: Vec<_> = impls
        .iter()
        .map(|x| quote! { <#x as ::plug::Object>::TAG })
        .collect();

    let codegen = quote! {
        pub enum Object { #(
            #variants(#impls)
        )*}

        #[derive(Debug, ::facet::Facet)]
        #[repr(C)]
        pub enum Tag { #(#variants)* }

        impl ::core::str::FromStr for Tag {
            type Err = String; // TODO
            fn from_str(value: &str) -> Result<Self, Self::Err> {
                match value {
                    #(#tags => Ok(Self::#variants),)*
                    // TODO: proper error
                    other => Err(format!("Unrecognized value: {other}. Possible states are: {ALL:?}")),
                }
            }
        }

        pub const ALL: &[&'static str] = &[#(#tags)*];


    };

    // Product: (Methods x Impls)
    let dispatched_methods = methods
        .into_iter()
        .map(|m| static_dispatch_method(&impls, m))
        .collect::<Result<_>>()?;

    Ok(DispatchCode {
        codegen,
        dispatched_methods,
    })
}

pub fn trait_to_interface(attrs: InterfaceAttrs, mut trait_: ItemTrait) -> Result<TokenStream> {
    ensure_empty_tokens!(trait_.generics.params, "Generic interfaces are TBD");
    ensure_empty_tokens!(trait_.supertraits, "Nested interfaces are TBD");

    let vis = trait_.vis.clone();

    let shape = InterfaceShape::parse(&mut trait_, attrs)?;

    let name = shape.name.clone(); // TODO
    let meta = shape.export_meta()?;

    let dispatched = shape.dispatch()?;

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
    format_ident!("registered {interface} impl for this module")
}

struct Impl {
    interface: Path,
    object: Path,
    inherents: Vec<TokenStream>,
    methods: Vec<ImplementedMethod>,
}

impl Impl {
    fn new(interface: Path, object: Path) -> Self {
        Self {
            interface,
            object,
            inherents: Vec::new(),
            methods: Vec::new(),
        }
    }

    // fn parse(input: &mut ImplItem) -> Result<Self> {}

    fn register_const_fn(&mut self, mut func: ImplItemFn) {
        func.vis = parse_quote!(pub(crate)); // TODO: consider the implications of this
        func.attrs.push(parse_quote!(#[doc(hidden)]));
        func.sig.ident = format_ident!("__const_{}", &func.sig.ident);

        self.inherents.push(func.to_token_stream());
    }

    fn register_method(&mut self, func: &mut ImplItemFn) -> Result<()> {
        let method = Method::parse(&mut func.sig)?;

        if matches!(method.fn_kind, FnKind::Const) {
            // TODO: here we undo what Method::parse did. Reconsider Method::parse.
            let const_fn = {
                let mut x = func.clone();
                x.sig.constness = Some(parse_quote!(const));
                x
            };

            self.register_const_fn(const_fn);

            let call_inherent = method.call_via_self()?;
            func.block = parse_quote!({ #call_inherent });
        };

        Ok(())
    }

    fn codegen(self) -> Result<TokenStream> {
        let object = self.object.clone();
        let inherents = self.inherents.clone();

        let cons = continue_with_interface_meta(Context {
            object: self.object,
            interface: self.interface,
            impl_methods: self.methods.into(),
        })?;

        let content = quote! {
            #cons

            impl #object {
                #(#inherents)*
            }
        };

        Ok(content)
    }
}

parse!(
    struct Context {
        object: Path,
        interface: Path,
        impl_methods: Many<ImplementedMethod>,
    }
);

fn continue_with_interface_meta(ctx: Context) -> Result<TokenStream> {
    let meta = ctx.interface.sibling(|_| format_ident!("__meta"))?;
    with_import!(#simple meta => register_impl_inner(ctx))
}

// TODO: this only exists to provide spans, otherwise could be just `Method`
parse!(
    struct ImplementedMethod {
        sig: Signature,
    }
);

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

    let mut impl_ = Impl::new(interface, object);

    for item in &mut input.items {
        match item {
            ImplItem::Fn(func) => impl_.register_method(func)?,
            _ => (),
        }
    }

    let codegen = impl_.codegen()?;
    let content = quote! {
        #input
        #codegen
    };
    Ok(content)
}

fn register_impl_inner(ctx: Context, meta: InterfaceMeta) -> Result<TokenStream> {
    let object = ctx.object;

    // Signal is the thing that allows us to gather implementations
    let signal = match meta.mode {
        Mode::Direct => None,
        Mode::FromMod => {
            let interface = &ctx.interface.last_ident()?;
            let marker = registered_module_object_marker(interface);

            Some(quote! {
                #[doc(hidden)]
                type #marker = #object;
            })
        }
        Mode::Dynamic => todo!("dyn registration"),
    };

    for method in ctx.impl_methods.inner {
        let ImplementedMethod { sig, .. } = method;

        if meta.const_methods.inner.contains(&sig.ident) {
            // TODO: the following currently never fires,
            // because rustc aborts parent mod prior to macro exec
            // with reason: (inherent) method not found
            if sig.constness.is_none() {
                bail!(sig.ident => "function must be const")
            }
        }

        if meta.variable_async_methods.inner.contains(&sig.ident) {
            todo!("variable async path");
        };
    }

    // TODO: sync-path trait, associated error
    let content = quote! {
        #signal
    };

    Ok(content)
}
