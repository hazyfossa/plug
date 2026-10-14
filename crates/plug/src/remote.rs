// You may already know remote interfaces as "services"

use eyre::Result;

pub trait RemoteDescriptor: crate::InterfaceDescriptor {
    type Request;
    type Response;

    type Client<C: Caller<Self>>;
}

#[allow(async_fn_in_trait)]
pub trait Caller<S: RemoteDescriptor>: crate::Init {
    async fn call(&mut self, request: S::Request) -> Result<S::Response>;
}
