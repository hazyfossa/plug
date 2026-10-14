use std::marker::PhantomData;

use proc_macro2::TokenStream;
use quote::quote;
use syn::{Result, ReturnType, parse_quote};

use crate::{
    bail,
    interface::{CallTarget, InterfaceShape, Method},
};

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
        let name = &method.sig.ident;
        let inputs = method.input_shape(CallTarget::Interface);

        quote! { #name { #inputs } }
    }

    fn codegen(body: Vec<TokenStream>) -> TokenStream {
        quote! { pub enum Request { #(#body),* } }
    }
}

// TODO: when we (inevitably) reach for our own wire format
// consider making responses *untagged* unions, as it should be possible
// to prove safety of variant deref based on protocol state
struct ResponseBuilder;

impl Codegen for ResponseBuilder {
    fn handle_method(method: &Method) -> TokenStream {
        let name = &method.sig.ident;

        // TODO: consider Result as special case here
        let output = match &method.sig.output {
            ReturnType::Type(_, x) => x,
            ReturnType::Default => {
                bail!(@immediately name => "Remote methods without a return value are TBD. 
                Consider specifying `-> ()` in the meantime.")
            }
        };

        quote! { #name( #output ) }
    }

    fn codegen(body: Vec<TokenStream>) -> TokenStream {
        quote! { pub enum Response { #(#body),* } }
    }
}

struct ClientBuilder;

impl Codegen for ClientBuilder {
    fn handle_method(method: &Method) -> TokenStream {
        let name = &method.sig.ident;

        let args = &method.args(CallTarget::Interface);
        let inputs = method.input_shape(CallTarget::Interface);

        let mut client_sig = method.sig.clone();
        client_sig.constness = None;
        client_sig.asyncness = Some(parse_quote!(async));

        // TODO: looks.. not great
        let previous_ret = match &method.sig.output {
            ReturnType::Type(_, x) => x,
            _ => unreachable!(), // guarded by ResponseBuilder
        };

        quote! {
            pub async fn #name(&mut self, #inputs) -> eyre::Result<#previous_ret> {
                // TODO: add context here that error comes from caller
                let ret = self.raw().call(Request::#name { #args }).await?;
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
                where C: Caller<Descriptor>
            {
                #[inline(always)]
                pub fn raw(&mut self) -> &mut C {
                    &mut self.0
                }

                #(#body)*
            }
        }
    }
}

struct CallerBuilder;

impl Codegen for CallerBuilder {
    fn handle_method(method: &Method) -> TokenStream {
        let name = &method.sig.ident;
        let args = method.args(CallTarget::Interface);

        let call = method.call_via_self(CallTarget::Interface);

        quote! { Request::#name { #args } => {
            let ret = #call;
            Ok(Response::#name(ret))
        } }
    }

    fn codegen(body: Vec<TokenStream>) -> TokenStream {
        quote! {
            impl Caller<Descriptor> for Object {
                async fn call(&mut self, request: Request) -> eyre::Result<Response> {
                    match request { #(#body)* }
                }
            }
        }
    }
}

impl InterfaceShape {
    pub fn remote_codegen(&self) -> Result<TokenStream> {
        let request = Code::<RequestBuilder>::with_methods(&self.methods);
        let response = Code::<ResponseBuilder>::with_methods(&self.methods);
        let client_impl = Code::<ClientBuilder>::with_methods(&self.methods);
        let caller_impl = Code::<CallerBuilder>::with_methods(&self.methods);

        let content = quote! {
            mod remote {
                use super::*;
                use ::plug::remote::*;

                pub struct Client<C>(C);

                impl RemoteDescriptor for Descriptor {
                    type Request = Request;
                    type Response = Response;
                    type Client<C: Caller<Descriptor>> = Client<C>;
                }

                #request #response #client_impl #caller_impl
            }
        };

        Ok(content)
    }
}
