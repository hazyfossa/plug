pub use plug_macro::plug;

pub trait Object {
    type Config;
    const TAG: &str;
}

#[allow(async_fn_in_trait)]
pub trait Init: Object + Sized {
    type Error;

    async fn init(config: &Self::Config) -> Result<Self, Self::Error>;
}
