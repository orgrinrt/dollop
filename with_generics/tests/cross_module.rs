//------------------------------------------------------------------------------
// Copyright (c) 2025 orgrinrt (orgrinrt@ikiuni.dev)
//                    Hiisi Digital Oy (contact@hiisi.digital)
// SPDX-License-Identifier: MPL-2.0
//------------------------------------------------------------------------------

use with_generics::{impl_, with_generics};

trait SomeTrait {
    fn compute(&self) -> bool;
}

// simple wrapper type for generics
#[derive(Copy, Clone, PartialEq, Debug)]
struct Wrapper<T: Copy>(T);

#[with_generics(
    T: Copy + PartialEq,
    U: Copy,
)]
pub struct SomeStruct<const MAGIC: usize = 42> {
    first: T,
    second: U,
}

mod separate_module {
    use super::*;
    use with_generics::impl_;

    pub(crate) trait SomeTraitWithGenerics<T> {
        fn get_value(&self) -> T;
    }

    impl_!(
        SomeTrait for SomeStruct {
            fn compute(&self) -> bool {
                // use the const magic from generics
                MAGIC == 42
            }
        }

        SomeTraitWithGenerics<T> for SomeStruct {
            fn get_value(&self) -> T {
                self.first
            }
        }

        // inherent methods
        SomeStruct {
            pub(crate) fn is_first_equal_to(&self, other: T) -> bool {
                self.first == other
            }

            pub(crate) fn get_second(&self) -> U {
                self.second
            }
        }
    );

    #[test]
    fn test_separate_module() {
        let instance: SomeStruct<Wrapper<i32>, bool> = SomeStruct {
            first: Wrapper(10),
            second: true,
        };

        // default magic value (42)
        assert!(instance.compute());
        assert!(instance.is_first_equal_to(Wrapper(10)));
        assert!(instance.get_second());
    }
}

// for visibility test, implement another trait in the root module
trait AnotherTrait {
    fn is_magical(&self) -> bool;
}

impl_!(
    AnotherTrait for SomeStruct {
        fn is_magical(&self) -> bool {
            MAGIC > 0
        }
    }
);

#[test]
fn test_cross_module_visibility() {
    let instance: SomeStruct<Wrapper<i32>, bool> = SomeStruct {
        first: Wrapper(10),
        second: true,
    };

    // methods from separate_module should be accessible here
    assert!(instance.compute());
    assert!(instance.is_first_equal_to(Wrapper(10)));
    assert!(instance.get_second());

    // method from root module
    assert!(instance.is_magical());
}
