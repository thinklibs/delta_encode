
pub use delta_encode_derive::*;
pub use think_bitio as bitio;


pub trait CreateArray<T>: Sized {
    fn create<'a, F, E>(init_func: F) -> Result<Self, E>
        where F: FnMut(usize) -> Result<T, E> + 'a;
}

macro_rules! impl_create_array {
    (
        [$size:expr] $first:ident, $($var:ident,)*
    ) => {
        impl <T> CreateArray<T> for [T; $size] {
            #[allow(unused_variables, unused_mut, unused_assignments, clippy::eval_order_dependence)]
            fn create<'a, F, E>(mut init_func: F) -> Result<Self, E>
                where F: FnMut(usize) -> Result<T, E> + 'a
            {
                let mut offset = 1;
                Ok([
                    init_func(0)?,
                $({
                    let $var = init_func(offset)?;
                    offset += 1;
                    $var
                }),*
                ])
            }
        }
        impl_create_array!([$size - 1] $($var,)*);
    };
    ([$size:expr]) => {};
}

impl_create_array!([32] a, b, c, d, e, f, g, h, j, k, l, m, n, o, p, q, r, s, t, u, v, w, x, y, z, za, zb, zc, zd, ze, zf, zg,);