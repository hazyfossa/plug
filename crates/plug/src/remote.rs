// You may already know remote interfaces as "services"

use eyre::Result;

use crate::Data;

pub trait RemoteDescriptor: crate::InterfaceDescriptor {
    type Request: Data;
    type Response: Data;

    type Client<C: Caller<Self>>: From<C>;
}

#[allow(async_fn_in_trait)]
pub trait Caller<S: RemoteDescriptor> {
    type Error;
    async fn call(&mut self, request: S::Request) -> Result<S::Response, Self::Error>;
}
