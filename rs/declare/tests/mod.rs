

// #[declare::extra(newtype_variants, common_accessors)]
// enum A {
//     #[newtype]
//     // #[derive(Debug, Clone)]
//     W {
//         a: usize,
//         b: Option<usize>,
//     },
//     T {
//         a: usize,
//         b: usize
//     },
//     Z {
//         a: usize,
//         c: usize,
//     }
// }

#[declare::extra(newtype_variants, common_accessors)]
enum A<'a, T> where T: std::fmt::Debug {
    #[newtype]
    #[derive(Debug, Clone)]
    W {
        a: usize,
        b: Option<usize>,
        d: T,
    },
    #[newtype]
    Y {
        a: usize,
        b: usize,
        d: &'a T,
    },
    Z {
        a: usize,
        c: usize,
    }
}