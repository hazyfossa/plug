// You may already know remote interfaces as "services"

use std::marker::PhantomData;

use eyre::Result;

pub trait RemoteDescriptor: crate::InterfaceDescriptor {
    type Request;
    type Response;
}

#[allow(async_fn_in_trait)]
pub trait Caller<S: RemoteDescriptor>: crate::Init {
    async fn call(&mut self, request: S::Request) -> Result<S::Response>;
}

pub struct Client<S: RemoteDescriptor, C: Caller<S>> {
    caller: C,
    _remote: PhantomData<S>,
}

impl<S, C> Client<S, C>
where
    S: RemoteDescriptor,
    C: Caller<S>,
{
    pub async fn new(config: &C::Config) -> Result<Self> {
        let caller = C::init(config).await?;

        Ok(Self {
            caller,
            _remote: PhantomData,
        })
    }

    #[inline(always)]
    pub fn raw(&mut self) -> &mut C {
        &mut self.caller
    }
}
