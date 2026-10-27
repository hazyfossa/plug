use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, Path, Result, parse_quote};

use crate::{
    amyhow,
    interface::{
        InterfaceShape,
        implement::registered_module_object_marker,
        method::{CallTarget, Method, MethodKind},
    },
    parse::PathExt,
};

use super::{DispatchCode, Mode};

// Technically, this is a self-referrential struct
struct ImplRef {
    path: Path,
    variant: Ident,
}

impl ImplRef {
    // Uses well-known items from purescope: Descriptor
    fn tag(&self) -> TokenStream {
        let path = &self.path;
        quote! { <#path as ::plug::Tagged<Descriptor>>::TAG }
    }
}

fn branch(method: &Method, impl_: &ImplRef) -> TokenStream {
    let ImplRef { path, variant } = impl_;

    let content = match method.kind {
        MethodKind::Stateful => {
            let call = method.call(CallTarget::Impl, &parse_quote!(obj));

            quote! { Self::#variant(obj) => #call }
        }
        MethodKind::Associated => {
            let tag = impl_.tag();
            let call = method.call(CallTarget::Impl, path);
            quote! { #tag => #call }
        }
    };

    content
}

fn dispatch_one_method(impls: &[ImplRef], method: &Method) -> TokenStream {
    let determinant = match &method.kind {
        MethodKind::Stateful => quote! { self },
        MethodKind::Associated => quote! { tag },
    };

    let branches = impls.iter().map(|impl_| branch(method, impl_));

    let sig = &method.sig;
    let content = quote! { pub #sig {
        match #determinant {
            #(#branches,)*

            // Completely omitting this branch requires Tag::define to be unsafe
            // TODO: measure perf
            // TODO: we can already omit this branch for stateful methods of static interfaces
            // TODO: for static traits, we can have tag_repr(enum), which the compiler can prove to be total
            // TODO: this may cause errors with const-dispatch
            _ => panic!("Caught an invalid tag. Most likely, an erroneous ::define exists somewhere."),
        }
    }};

    content
}

fn dispatch_all_methods(impls: Vec<ImplRef>, methods: Vec<Method>) -> Result<DispatchCode> {
    let paths: Vec<_> = impls.iter().map(|x| &x.path).collect();
    let variants: Vec<_> = impls.iter().map(|x| &x.variant).collect();

    let object = quote! {
        pub enum Object { #(
            #variants(#paths),
        )*}
    };

    // Product: (Methods x Impls)
    let dispatched_methods = methods
        .iter()
        .filter(|m| m.requires_dispatch())
        .map(|m| dispatch_one_method(&impls, m))
        .collect();

    let tags: Vec<_> = impls.iter().map(|x| x.tag()).collect();
    let all_tags = quote! { &[#(#tags,)*] };

    Ok(DispatchCode {
        all_tags,
        object,
        dispatched_methods,
        other_codegen: None,
    })
}

fn resolve_impls(shape: &InterfaceShape) -> Option<Vec<ImplRef>> {
    let paths = shape
        .dispatch_target
        .paths
        .as_ref()
        .filter(|p| !p.is_empty())?;

    let impls = paths
        .iter()
        .map(|path| {
            let variant = path.last_ident().unwrap().clone();

            let path = match shape.dispatch_target.mode {
                Mode::FromMod => {
                    let marker = registered_module_object_marker(&shape.name);
                    path.join(marker)
                }
                _ => path.clone(),
            };

            ImplRef { variant, path }
        })
        .collect();

    Some(impls)
}

pub fn dispatch(shape: InterfaceShape) -> Result<DispatchCode> {
    // TODO: consider making "dyn" the default, switch to static when first impl seen
    // Pros: no annoying error on first write
    // Cons: goes against "make perf cost explicit"
    let impls = resolve_impls(&shape).ok_or(
        amyhow!(shape.name => "At least one impl or `dyn` is required for interface dispatch"),
    )?;

    dispatch_all_methods(impls, shape.methods)
}
