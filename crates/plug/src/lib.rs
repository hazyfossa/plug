pub use plug_macro::plug;

#[cfg(feature = "direct-dispatch")]
#[doc(hidden)]
pub use plug_macro::__import_advance;

pub trait Object {
    type Config;
    const TAG: &str;
}

#[allow(async_fn_in_trait)]
pub trait Init: Object + Sized {
    type Error;

    async fn init(config: &Self::Config) -> Result<Self, Self::Error>;
}
