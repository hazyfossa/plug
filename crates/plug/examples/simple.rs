use plug::plug;

mod implement {
    use super::{__codegen_Test, Test}; // we need purescope after all -_-
    use plug::plug;

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
        fn foo(&self) -> bool {
            true
        }

        const fn bar(&self) -> u8 {
            0
        }
    }
}

#[plug(implement::TestImpl)]
trait Test {
    fn foo(&self) -> bool;
    const fn bar(&self) -> u8;
}

const fn do_something(input: TestObject) {
    input.bar();
}

fn main() {}
