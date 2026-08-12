use plug_macro::plug;

#[plug]
trait Test {
    fn test(&self) -> bool;
}
