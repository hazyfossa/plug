use proc_macro2::TokenStream;
use quote::quote;
use syn::Result;

use crate::interface::InterfaceShape;

impl InterfaceShape {
    pub fn remote_codegen(&self) -> Result<TokenStream> {
        let mut request = Vec::new();
        let mut response = Vec::new();

        for method in &self.methods {
            let sig = &method.object_signature;

            let name = &sig.ident;
            let inputs = &sig.inputs;
            let output = &sig.output;

            request.push(quote! { #name { #inputs } });
            response.push(quote! { #name( #output ) });
        }

        let content = quote! {
            mod remote {
                use super::*;
                pub enum Request { #(#request),* }
                pub enum Response { #(#response),* }
            }

            impl ::plug::remote::RemoteDescriptor for Descriptor {
                type Request = remote::Request;
                type Response = remote::Response;
            }
        };

        Ok(content)
    }
}
