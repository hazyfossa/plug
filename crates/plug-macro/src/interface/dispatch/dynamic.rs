use quote::quote;
use syn::Result;

use crate::interface::{InterfaceShape, dispatch::DispatchCode};

pub fn dispatch(shape: InterfaceShape) -> Result<DispatchCode> {
    let codegen = quote! {
        pub struct Object {
            storage: Box<dyn Interface>,
        }
    };

    Ok(DispatchCode { codegen })
}
