use plug::plug;

#[plug(implement::TestImpl)]
trait Test {
    fn foo(&self) -> bool;
    const fn bar(&self) -> u8;
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
        fn foo(&self) -> bool {
            true
        }

        const fn bar(&self) -> u8 {
            0
        }
    }
}

const fn do_something(input: TestObject) {
    input.bar();
}

fn main() {}
