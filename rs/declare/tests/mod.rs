#[declare::extra(newtype_variants, common_accessors)]
#[derive(Debug)]
enum A<'a, T>
where
    T: std::fmt::Debug,
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

        let a: A<'_, String> = w.clone().into();

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

        let a: A<'_, String> = y.into();

        let back: Y<'_, String> = a.try_into().unwrap();

        assert_eq!(back.a, 10);
        assert_eq!(back.b, 20);
        assert_eq!(*back.d, "abc");
    }

    #[test]
    fn try_from_wrong_variant_returns_err() {
        let a: A<'_, String> = A::Z { a: 5, c: 9 };

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

        let w = A::from(W {
            a: 1,
            b: None,
            d: value.clone(),
        });

        let y = A::from(Y {
            a: 2,
            b: 3,
            d: &value,
        });

        let z: A<'static, String> = A::Z { a: 4, c: 5 };

        assert_eq!(*w.a_ref(), 1);
        assert_eq!(*y.a_ref(), 2);
        assert_eq!(*z.a_ref(), 4);
    }

    #[test]
    fn a_mut_updates_all_variants() {
        let value = String::from("abc");

        let mut w = A::from(W {
            a: 1,
            b: None,
            d: value.clone(),
        });

        *w.a_mut() = 100;

        assert_eq!(*w.a_ref(), 100);

        let mut z: A<'_, String> = A::Z { a: 3, c: 7 };

        *z.a_mut() = 200;

        assert_eq!(*z.a_ref(), 200);
    }

    #[test]
    fn into_a_returns_value() {
        let value = String::from("abc");

        let w = A::from(W {
            a: 42,
            b: None,
            d: value,
        });

        assert_eq!(w.into_a(), 42);
    }

    #[test]
    fn b_accessors_unify_option_and_non_option() {
        let value = String::from("abc");

        let w = A::from(W {
            a: 1,
            b: Some(5),
            d: value.clone(),
        });

        let y = A::from(Y {
            a: 1,
            b: 9,
            d: &value,
        });

        let z: A<'_, String> = A::Z { a: 1, c: 2 };

        assert_eq!(w.b_ref(), Some(&5));
        assert_eq!(y.b_ref(), Some(&9));
        assert_eq!(z.b_ref(), None);
    }

    #[test]
    fn b_mut_updates_inner_value() {
        let value = String::from("abc");

        let mut w = A::from(W {
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

        let w = A::from(W {
            a: 0,
            b: None,
            d: value.clone(),
        });

        let y = A::from(Y {
            a: 0,
            b: 0,
            d: &value,
        });

        let z: A<'_, String> = A::Z { a: 0, c: 0 };

        assert_eq!(w.d_ref().unwrap(), "hello");
        assert_eq!(y.d_ref().unwrap(), "hello");
        assert!(z.d_ref().is_none());
    }

    #[test]
    fn c_accessors_only_exist_for_z() {
        let z: A<'_, String> = A::Z { a: 1, c: 42 };

        assert_eq!(z.c_ref(), Some(&42));

        let value = String::from("abc");

        let w = A::from(W {
            a: 0,
            b: None,
            d: value,
        });

        assert_eq!(w.c_ref(), None);
    }

    #[test]
    fn c_mut_updates_value() {
        let mut z: A<'_, String> = A::Z { a: 1, c: 5 };

        *z.c_mut().unwrap() = 123;

        assert_eq!(z.into_c(), Some(123));
    }

    #[test]
    fn into_b_normalizes_option() {
        let value = String::from("abc");

        let w = A::from(W {
            a: 0,
            b: Some(7),
            d: value.clone(),
        });

        let y = A::from(Y {
            a: 0,
            b: 8,
            d: &value,
        });

        let z: A<'_, String> = A::Z { a: 0, c: 1 };

        assert_eq!(w.into_b(), Some(7));
        assert_eq!(y.into_b(), Some(8));
        assert_eq!(z.into_b(), None);
    }
}
