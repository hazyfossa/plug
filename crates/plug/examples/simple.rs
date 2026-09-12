use plug::plug;

#[plug(implement::TestImpl)]
trait Test {
    async fn foo(&self) -> bool;
    const fn bar(&self) -> u8;
}

mod implement {
    use super::Test; // we need purescope after all -_-
    use plug::plug;

    #[plug]
    // TODO (cfg derives) #[derive(Default)]
    struct TestImpl {
        a: String,
        b: String,

        state: !,

        c: u8,
    }

    #[plug]
    impl Test::Interface for TestImpl {
        async fn foo(&self) -> bool {
            true
        }

        const fn bar(&self) -> u8 {
            self.c
        }
    }
}

fn main() {}
