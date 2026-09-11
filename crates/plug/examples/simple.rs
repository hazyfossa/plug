use plug::plug;

#[plug(implement::TestImpl)]
trait Test {
    fn test(&self) -> bool;
}

mod implement {
    use super::*;

    #[plug]
    // TODO (cfg derives) #[derive(Default)]
    struct TestImpl {
        a: String,
        b: String,

        state: !,

        c: u32,
    }

    #[plug]
    impl Test for TestImpl {
        fn test(&self) -> bool {
            true
        }
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

fn do_something(input: TestObject) {
    input.test();
}

fn main() {}
