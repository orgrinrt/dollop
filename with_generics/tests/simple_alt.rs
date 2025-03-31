//------------------------------------------------------------------------------
// Copyright (c) 2025 orgrinrt (orgrinrt@ikiuni.dev)
//                    Hiisi Digital Oy (contact@hiisi.digital)
// SPDX-License-Identifier: MPL-2.0
//------------------------------------------------------------------------------

use with_generics::{impl_, with_generics};

trait SomeTrait {
    fn compute(&self) -> bool;
}

trait SomeTraitWithGenerics<T> {
    fn get_value(&self) -> T;
}

// simple wrapper type for generics
#[derive(Copy, Clone, PartialEq, Debug)]
struct Wrapper<T: Copy>(T);

#[with_generics]
pub struct SomeStruct<T: Copy + PartialEq, U: Copy, const MAGIC: usize = 42> {
    first: T,
    second: U,
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
        fn is_first_equal_to(&self, other: T) -> bool {
            self.first == other
        }

        fn get_second(&self) -> U {
            self.second
        }
    }
);

#[test]
fn test_trait_impls() {
    let instance: SomeStruct<Wrapper<i32>, bool, 100> = SomeStruct {
        first: Wrapper(10),
        second: true,
    };

    // magic is 100, not 42
    assert!(!instance.compute());

    assert_eq!(instance.get_value(), Wrapper(10));
}

#[test]
fn test_inherent_methods() {
    let instance: SomeStruct<Wrapper<i32>, bool> = SomeStruct {
        first: Wrapper(10),
        second: true,
    };

    // test with default const value (42)
    assert!(instance.compute());
    assert!(instance.is_first_equal_to(Wrapper(10)));
    assert!(instance.is_first_equal_to(Wrapper(20)));
    assert!(instance.get_second());
}
