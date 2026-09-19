use plug::plug;

#[plug(implement::TestImpl)]
trait Test {
    fn associated() -> String;
    async fn foo(&self) -> bool;
    const fn bar(&self) -> u8;

    #[plug(final)]
    fn fas() -> u16 {
        0
    }
}

mod implement {
    use super::Test;
    use plug::plug;

    #[plug]
    // TODO (cfg derives) #[derive(Default)]
    struct TestImpl {
        a: String,
        b: String,
    }

    #[plug]
    impl Test::Interface for TestImpl {
        fn associated() -> String {
            "Hello, world".to_string()
        }

        fn fas() -> u16 {
            1
        }

        async fn foo(&self) -> bool {
            true
        }

        const fn bar(&self) -> u8 {
            0
        }
    }
}

fn do_something(x: Test::Object) {
    let a = x.bar();
    let b = Test::Object::associated(Test::Tag::TestImpl);
}

fn main() {}
