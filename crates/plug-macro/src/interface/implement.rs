use proc_macro2::TokenStream;
use quote::{ToTokens, format_ident, quote};
use syn::{
    Ident, ImplItem, ImplItemFn, ItemImpl, Path, Result, Signature, Type, parse::Nothing,
    parse_quote, spanned::Spanned,
};

use crate::{
    bail,
    interface::{
        InterfaceMeta, Mode,
        method::{CallTarget, FnKind, Method},
    },
    meta_passing::with_import,
    parse::{Many, PathExt, parse},
};

pub fn const_redirect(name: &Ident) -> Ident {
    format_ident!("__const_{}", name)
}

pub fn registered_module_object_marker(interface: &Ident) -> Ident {
    format_ident!("registered_{interface}_impl_for_this_module")
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

    fn copy_to_inherent(&mut self, func: &ImplItemFn) {
        let mut inherent = func.clone();

        inherent.vis = parse_quote!(pub(crate)); // TODO: consider the implications of this
        inherent.attrs.push(parse_quote!(#[doc(hidden)]));
        inherent.sig.ident = const_redirect(&func.sig.ident);
        inherent.sig.constness = parse_quote!(const);

        self.inherents.push(inherent.to_token_stream());
    }

    fn register_method(&mut self, func: &mut ImplItemFn) -> Result<()> {
        let method = Method::parse(&mut func.attrs, &mut func.sig)?;

        if matches!(method.fn_kind, FnKind::Const) {
            self.copy_to_inherent(func);

            let call_inherent = method.call_via_self(CallTarget::Impl);
            func.block = parse_quote!({ #call_inherent });
        };

        Ok(())
    }

    fn codegen(self) -> Result<TokenStream> {
        let object = self.object.clone();
        let inherents = self.inherents.clone();

        let ctx = Context {
            object: self.object,
            interface: self.interface,
            impl_methods: self.methods.into(),
        };

        let meta = ctx.interface.join(format_ident!("__meta"));
        let cons = with_import!(#simple meta => register_impl_inner(ctx))?;

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

// TODO: this only exists to provide spans, otherwise could be just `Method`
parse!(
    struct ImplementedMethod {
        sig: Signature,
    }
);

fn interface_from_impl(input: &ItemImpl) -> Option<Path> {
    let mut path = input.trait_.clone()?.0;
    let last_ident = path.segments.pop()?.ident;
    let _ = path.segments.pop_punct()?;

    if last_ident != "Interface" {
        return None;
    };

    Some(path)
}

pub fn register_impl(_: Nothing, mut input: ItemImpl) -> Result<TokenStream> {
    let interface = match interface_from_impl(&input) {
        Some(x) => x,
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

    let tag_type = quote! { <#interface::Descriptor as ::plug::InterfaceDescriptor>::Tag };

    // TODO: tag overrides
    let tag = quote! {
        impl ::plug::Tagged<#interface::Descriptor> for #object {
            const TAG: #tag_type = #tag_type::define(#object::inherent_tag());
        }
    };

    let mut impl_ = Impl::new(interface, object);

    for item in &mut input.items {
        if let ImplItem::Fn(func) = item {
            impl_.register_method(func)?
        }
    }

    let codegen = impl_.codegen()?;
    let content = quote! {
        #input #tag #codegen
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
                pub(crate) type #marker = #object;
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
