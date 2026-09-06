use plug::plug;

#[plug]
// TODO (cfg derives) #[derive(Default)]
struct TestObject {
    a: String,
    b: String,

    state: !,

    c: u32,
}

#[plug]
trait Test {
    fn test(&self) -> bool;
}

#[plug]
impl Test for TestObject {
    fn test(&self) -> bool {
        true
    }
}

// impl Init for TestObject::State {
//     type Error = std::convert::Infallible;
//     async fn init(config: &Self::Config) -> Result<Self, Self::Error> {
//         Ok(Self {
//             c: config.a.len() + config.b.len(),
//         })
//     }
// }

fn main() {}
