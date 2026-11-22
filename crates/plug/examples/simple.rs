use plug::plug;

#[plug(implement::TestImpl)]
trait Test {
    fn constructor() -> Self;
    async fn foo(&self) -> bool;
    const fn bar(&self) -> u8;
}

mod implement {
    use super::Test;
    use plug::plug;

    #[plug]
    struct TestImpl {
        a: String,
        b: String,
    }

    #[plug]
    impl Test::Interface for TestImpl {
        fn constructor() -> Test::Object {
            Self {}.into()
        }

        async fn foo(&self) -> bool {
            true
        }

        #[plug(const)]
        fn bar(&self) -> u8 {
            0
        }
    }
}

#[allow(unused)]
async fn do_something() {
    let user_input = "TestImpl";
    let tag = Test::Tag::parse(user_input).expect("Provided invalid tag");

    let x = Test::Object::constructor(tag);

    let a = x.bar();
    let b = x.foo().await;
}

fn main() {}
