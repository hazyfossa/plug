pub use plug_macro::plug;

// TODO
type Routine<Output> = Box<dyn Future<Output = Output>>;

pub trait Object {
    type Config;
    const TAG: &str;
}

#[allow(async_fn_in_trait)]
pub trait Init: Object + Sized {
    type Error; // TODO: impl-associated errors, fallback to dynamic

    fn init(config: &Self::Config) -> Routine<Result<Self, Self::Error>>;
}

#[allow(type_alias_bounds)]
pub type Constructor<T: Init> = fn(&T::Config) -> Routine<Result<T, T::Error>>;
