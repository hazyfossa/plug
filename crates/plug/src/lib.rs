pub use plug_macro::plug;

pub trait State {
    type Config;
}

#[allow(async_fn_in_trait)]
pub trait Init: State + Sized {
    type Error;

    async fn init(config: &Self::Config) -> Result<Self, Self::Error>;
}
