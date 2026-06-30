#[declare::newtype_variants]
#[declare::common_accessors]
#[declare::field_traits]
// #[declare::augment(newtype_variants, common_accessors, field_traits)]
#[derive(Debug)]
enum Enum<'a, T>
where
    T: core::fmt::Debug,
{
    #[newtype]
    #[derive(Debug, Clone)]
    W {
        a: usize,
        b: Option<usize>,
        d: T,
    },
    #[newtype]
    #[derive(Debug)]
    Y {
        a: usize,
        b: usize,
        d: &'a T,
    },
    Z {
        a: usize,
        c: usize,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::{TryFrom, TryInto};

    #[test]
    fn from_and_try_from_w() {
        let w = W {
            a: 1,
            b: Some(2),
            d: "hello".to_string(),
        };

        let a: Enum<'_, String> = w.clone().into();

        let back: W<String> = a.try_into().unwrap();

        assert_eq!(back.a, 1);
        assert_eq!(back.b, Some(2));
        assert_eq!(back.d, "hello");
    }

    #[test]
    fn from_and_try_from_y() {
        let value = String::from("abc");

        let y = Y {
            a: 10,
            b: 20,
            d: &value,
        };

        let a: Enum<'_, String> = y.into();

        let back: Y<'_, String> = a.try_into().unwrap();

        assert_eq!(back.a, 10);
        assert_eq!(back.b, 20);
        assert_eq!(*back.d, "abc");
    }

    #[test]
    fn try_from_wrong_variant_returns_err() {
        let a: Enum<'_, String> = Enum::Z { a: 5, c: 9 };

        assert!(W::<String>::try_from(a).is_err());
    }

    #[test]
    fn derive_clone_and_debug_forwarded() {
        let w = W {
            a: 1,
            b: None,
            d: "hello".to_string(),
        };

        let cloned = w.clone();

        assert_eq!(cloned.a, 1);
        assert_eq!(format!("{:?}", cloned), format!("{:?}", w));
    }

    #[test]
    fn a_accessors_work_for_all_variants() {
        let value = String::from("abc");

        let w = Enum::from(W {
            a: 1,
            b: None,
            d: value.clone(),
        });

        let y = Enum::from(Y {
            a: 2,
            b: 3,
            d: &value,
        });

        let z: Enum<'static, String> = Enum::Z { a: 4, c: 5 };

        assert_eq!(*w.a_ref(), 1);
        assert_eq!(*y.a_ref(), 2);
        assert_eq!(*z.a_ref(), 4);
    }

    #[test]
    fn a_mut_updates_all_variants() {
        let value = String::from("abc");

        let mut w = Enum::from(W {
            a: 1,
            b: None,
            d: value.clone(),
        });

        *w.a_mut() = 100;

        assert_eq!(*w.a_ref(), 100);

        let mut z: Enum<'_, String> = Enum::Z { a: 3, c: 7 };

        *z.a_mut() = 200;

        assert_eq!(*z.a_ref(), 200);
    }

    #[test]
    fn into_a_returns_value() {
        let value = String::from("abc");

        let w = Enum::from(W {
            a: 42,
            b: None,
            d: value,
        });

        assert_eq!(w.into_a(), 42);
    }

    #[test]
    fn b_accessors_unify_option_and_non_option() {
        let value = String::from("abc");

        let w = Enum::from(W {
            a: 1,
            b: Some(5),
            d: value.clone(),
        });

        let y = Enum::from(Y {
            a: 1,
            b: 9,
            d: &value,
        });

        let z: Enum<'_, String> = Enum::Z { a: 1, c: 2 };

        assert_eq!(w.b_ref(), Some(&5));
        assert_eq!(y.b_ref(), Some(&9));
        assert_eq!(z.b_ref(), None);
    }

    #[test]
    fn b_mut_updates_inner_value() {
        let value = String::from("abc");

        let mut w = Enum::from(W {
            a: 0,
            b: Some(1),
            d: value,
        });

        *w.b_mut().unwrap() = 99;

        assert_eq!(w.into_b(), Some(99));
    }

    #[test]
    fn d_ref_handles_owned_and_borrowed() {
        let value = String::from("hello");

        let w = Enum::from(W {
            a: 0,
            b: None,
            d: value.clone(),
        });

        let y = Enum::from(Y {
            a: 0,
            b: 0,
            d: &value,
        });

        let z: Enum<'_, String> = Enum::Z { a: 0, c: 0 };

        assert_eq!(w.d_ref().unwrap(), "hello");
        assert_eq!(y.d_ref().unwrap(), "hello");
        assert!(z.d_ref().is_none());
    }

    #[test]
    fn c_accessors_only_exist_for_z() {
        let z: Enum<'_, String> = Enum::Z { a: 1, c: 42 };

        assert_eq!(z.c_ref(), Some(&42));

        let value = String::from("abc");

        let w = Enum::from(W {
            a: 0,
            b: None,
            d: value,
        });

        assert_eq!(w.c_ref(), None);
    }

    #[test]
    fn c_mut_updates_value() {
        let mut z: Enum<'_, String> = Enum::Z { a: 1, c: 5 };

        *z.c_mut().unwrap() = 123;

        assert_eq!(z.into_c(), Some(123));
    }

    #[test]
    fn into_b_normalizes_option() {
        let value = String::from("abc");

        let w = Enum::from(W {
            a: 0,
            b: Some(7),
            d: value.clone(),
        });

        let y = Enum::from(Y {
            a: 0,
            b: 8,
            d: &value,
        });

        let z: Enum<'_, String> = Enum::Z { a: 0, c: 1 };

        assert_eq!(w.into_b(), Some(7));
        assert_eq!(y.into_b(), Some(8));
        assert_eq!(z.into_b(), None);
    }

    #[test]
    fn try_from_shared_ref_w() {
        let w = W {
            a: 1,
            b: Some(2),
            d: "hello".to_string(),
        };
        let a: Enum<'_, String> = w.into();

        let back_ref: &W<String> = <&W<String>>::try_from(&a).unwrap();
        assert_eq!(back_ref.a, 1);

        let z: Enum<'_, String> = Enum::Z { a: 5, c: 9 };
        let err_ref = <&W<String>>::try_from(&z);
        assert!(err_ref.is_err());
        if let Err(returned_a_ref) = err_ref {
            assert_eq!(*returned_a_ref.a_ref(), 5);
        }
    }

    #[test]
    fn try_from_mut_ref_w() {
        let w = W {
            a: 1,
            b: Some(2),
            d: "hello".to_string(),
        };
        let mut a: Enum<'_, String> = w.into();

        {
            let back_mut: &mut W<String> = <&mut W<String>>::try_from(&mut a).unwrap();
            back_mut.a = 42;
        }
        assert_eq!(*a.a_ref(), 42);

        let mut z: Enum<'_, String> = Enum::Z { a: 5, c: 9 };
        let err_mut = <&mut W<String>>::try_from(&mut z);
        assert!(err_mut.is_err());
    }

    #[test]
    fn try_from_shared_ref_y() {
        let value = String::from("abc");
        let y = Y {
            a: 10,
            b: 20,
            d: &value,
        };
        let a: Enum<'_, String> = y.into();

        let back_ref: &Y<'_, String> = <&Y<'_, String>>::try_from(&a).unwrap();
        assert_eq!(back_ref.a, 10);
        assert_eq!(*back_ref.d, "abc");
    }

    // FIELD TRAITS TESTS
    //************************************************************************//

    #[test]
    fn trait_a_polymorphism() {
        fn check_a_ref<T: ARef>(item: &T, expected: usize) {
            assert_eq!(*item.a_ref(), expected);
        }

        fn check_a_mut<T: AMut + ARef>(item: &mut T, new_val: usize) {
            *item.a_mut() = new_val;
            assert_eq!(*item.a_ref(), new_val);
        }

        fn check_into_a<T: IntoA>(item: T, expected: usize) {
            assert_eq!(item.into_a(), expected);
        }

        let value = String::from("generic");

        let mut w_struct = W {
            a: 10,
            b: None,
            d: value.clone(),
        };
        check_a_ref(&w_struct, 10);
        check_a_mut(&mut w_struct, 15);
        check_into_a(w_struct, 15);

        let mut y_struct = Y {
            a: 20,
            b: 5,
            d: &value,
        };
        check_a_ref(&y_struct, 20);
        check_a_mut(&mut y_struct, 25);
        check_into_a(y_struct, 25);

        let mut enum_w = Enum::from(W {
            a: 30,
            b: None,
            d: value,
        });
        check_a_ref(&enum_w, 30);
        check_a_mut(&mut enum_w, 35);
        check_into_a(enum_w, 35);
    }

    #[test]
    fn trait_b_implemented_for_y_struct() {
        fn check_b<T: BRef + BMut + IntoB>(mut item: T) {
            assert_eq!(*item.b_ref(), 42);
            *item.b_mut() = 100;
            assert_eq!(item.into_b(), 100);
        }

        let value = String::from("b_test");
        let y_struct = Y {
            a: 1,
            b: 42,
            d: &value,
        };

        check_b(y_struct);
    }

    #[test]
    fn trait_d_handles_owned_and_borrowed_via_generics() {
        fn check_d_ref<T: core::fmt::Debug, U: DRef<T>>(item: &U, expected: &T)
        where
            T: PartialEq,
        {
            assert_eq!(item.d_ref(), expected);
        }

        let value = String::from("d_test");

        let w_struct = W {
            a: 0,
            b: None,
            d: value.clone(),
        };
        check_d_ref(&w_struct, &value);

        let y_struct = Y {
            a: 0,
            b: 0,
            d: &value,
        };
        check_d_ref(&y_struct, &value);
    }

    #[test]
    fn trait_c_bounds_exist() {
        struct DummyZ;
        impl CRef for DummyZ {
            fn c_ref(&self) -> &usize {
                &0
            }
        }

        assert_eq!(*DummyZ.c_ref(), 0);
    }
}
