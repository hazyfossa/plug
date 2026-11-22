use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, Result};

use crate::{
    derive_data,
    method::{CallTarget, Method},
};

use std::marker::PhantomData;

struct Code<T> {
    body: Vec<TokenStream>,
    _rules: PhantomData<T>,
}

trait Codegen {
    fn handle_method(method: &Method) -> TokenStream;
    fn codegen(body: Vec<TokenStream>) -> TokenStream;
}

impl<T: Codegen> Code<T> {
    fn new() -> Self {
        Self {
            body: Vec::new(),
            _rules: PhantomData,
        }
    }

    fn register_method(&mut self, method: &Method) {
        self.body.push(T::handle_method(method));
    }

    fn with_methods(methods: &[Method]) -> TokenStream {
        let mut this = Self::new();

        for method in methods {
            this.register_method(method);
        }

        T::codegen(this.body)
    }
}

struct RequestBuilder;

impl Codegen for RequestBuilder {
    fn handle_method(method: &Method) -> TokenStream {
        let name = &method.name;
        let arguments = method.arguments(CallTarget::Interface);

        quote! { #name { #arguments } }
    }

    fn codegen(body: Vec<TokenStream>) -> TokenStream {
        let derive_data = derive_data();

        quote! {
            #derive_data
            pub enum Request { #(#body),* }
        }
    }
}

// TODO: when we (inevitably) reach for our own wire format
// consider making responses *untagged* unions, as it should be possible
// to prove safety of variant deref based on protocol state
struct ResponseBuilder;

impl Codegen for ResponseBuilder {
    fn handle_method(method: &Method) -> TokenStream {
        let name = &method.name;
        let ret = &method.ret;

        // TODO: consider Result as special case of ret here

        quote! { #name( #ret ) }
    }

    fn codegen(body: Vec<TokenStream>) -> TokenStream {
        let derive_data = derive_data();

        quote! {
            #derive_data
            pub enum Response { #(#body),* }
        }
    }
}

struct ClientBuilder;

impl Codegen for ClientBuilder {
    fn handle_method(method: &Method) -> TokenStream {
        let name = &method.name;

        let args = &method.arguments(CallTarget::Interface);
        let inputs = method.inputs(CallTarget::Interface);

        let ret = &method.ret;

        quote! {
            pub async fn #name(&mut self, #args) -> eyre::Result<#ret> {
                // TODO: add context here that error comes from caller
                let ret = self.raw().call(Request::#name { #inputs }).await?;
                match ret {
                    Response::#name(output) => Ok(output),
                    other => todo!("Invalid variant error"),
                }
            }
        }
    }

    fn codegen(body: Vec<TokenStream>) -> TokenStream {
        quote! {
            impl<C> Client<C>
            where
                C: Caller<Descriptor>,
                eyre::Error: From<C::Error>,

            {
                #[inline(always)]
                pub(crate) fn raw(&mut self) -> &mut C {
                    &mut self.0
                }

                #(#body)*
            }

            impl<C> From<C> for Client<C>
                where C: Caller<Descriptor>
            {
                fn from(caller: C) -> Self {
                    Self(caller)
                }
            }
        }
    }
}

struct CallerBuilder;

impl Codegen for CallerBuilder {
    fn handle_method(method: &Method) -> TokenStream {
        let name = &method.name;
        let inputs = method.inputs(CallTarget::Interface);

        let call = method.call_via_self(CallTarget::Interface);

        quote! { Request::#name { #inputs } => {
            let ret = #call;
            Ok(Response::#name(ret))
        } }
    }

    fn codegen(body: Vec<TokenStream>) -> TokenStream {
        quote! {
            impl Caller<Descriptor> for Object {
                type Error = std::convert::Infallible;
                async fn call(&mut self, request: Request) -> Result<Response, std::convert::Infallible> {
                    match request { #(#body)* }
                }
            }
        }
    }
}

pub fn remote_codegen(methods: &[Method], scope_name: Ident, object: Ident) -> Result<TokenStream> {
    let request = Code::<RequestBuilder>::with_methods(methods);
    let response = Code::<ResponseBuilder>::with_methods(methods);
    let client_impl = Code::<ClientBuilder>::with_methods(methods);
    let caller_impl = Code::<CallerBuilder>::with_methods(methods);

    let content = quote! {
        mod #scope_name {
            use super::*;
            use ::plug::remote::*;
            use ::plug::__codegen::*;

            pub struct Client<C>(C);

            impl RemoteDescriptor for #object {
                type Request = Request;
                type Response = Response;
                type Client<C: Caller<Self>> = Client<C>;
            }

            type Descriptor = #object;

            #request #response #client_impl #caller_impl
        }
    };

    Ok(content)
}
