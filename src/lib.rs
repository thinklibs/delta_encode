
#[cfg(feature="cgmath")]
mod cgmath_support;

pub use delta_encode_derive::*;
pub use think_bitio as bitio;

use bitio::*;
use std::io::{self, Read, Write};
use std::sync::Arc;

pub trait DeltaEncodable: Sized {

    fn encode<W>(&self, base: Option<&Self>, w: &mut Writer<W>) -> io::Result<()>
        where W: Write;

    fn decode<R>(base: Option<&Self>, r: &mut Reader<R>) -> io::Result<Self>
        where R: Read;
}

impl <T> DeltaEncodable for Arc<T>
    where T: DeltaEncodable
{
    #[inline]
    fn encode<W>(&self, base: Option<&Self>, w: &mut Writer<W>) -> io::Result<()>
        where W: Write
    {
        T::encode(self, base.map(|v| &**v), w)
    }

    #[inline]
    fn decode<R>(base: Option<&Self>, r: &mut Reader<R>) -> io::Result<Self>
        where R: Read
    {
        Ok(Arc::new(T::decode(base.map(|v| &**v), r)?))
    }
}

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


impl DeltaEncodable for String {
    #[inline]
    fn encode<W>(&self, base: Option<&Self>, w: &mut Writer<W>) -> io::Result<()>
        where W: Write
    {
        encode_str(self, base.map(|v| v.as_str()), w)
    }

    #[inline]
    fn decode<R>(base: Option<&Self>, r: &mut Reader<R>) -> io::Result<Self>
        where R: Read
    {
        decode_string(base.map(|v| v.as_str()), r)
    }
}

impl DeltaEncodable for Arc<str> {
    #[inline]
    fn encode<W>(&self, base: Option<&Self>, w: &mut Writer<W>) -> io::Result<()>
        where W: Write
    {
        encode_str(self, base.map(|v| &**v), w)
    }

    #[inline]
    fn decode<R>(base: Option<&Self>, r: &mut Reader<R>) -> io::Result<Self>
        where R: Read
    {
        decode_string(base.map(|v| &**v), r).map(|v| v.into())
    }
}

#[derive(Debug, PartialOrd, PartialEq, Clone)]
pub struct AlwaysVec<T>(pub Vec<T>);

impl <T> DeltaEncodable for AlwaysVec<T>
    where T: DeltaEncodable
{
    #[inline]
    fn encode<W>(&self, base: Option<&Self>, w: &mut Writer<W>) -> io::Result<()>
        where W: Write
    {
        write_len_bits(w, self.0.len())?;
        for (idx, val) in self.0.iter().enumerate() {
            T::encode(val, base.and_then(|v | v.0.get(idx)), w)?;
        }
        Ok(())
    }

    #[inline]
    fn decode<R>(base: Option<&Self>, r: &mut Reader<R>) -> io::Result<Self>
        where R: Read
    {
        let len = read_len_bits(r)?;
        let mut buf = Vec::with_capacity(len);
        for idx in 0 .. len {
            buf.push(T::decode(base.and_then(|v| v.0.get(idx)), r)?);
        }
        Ok(AlwaysVec(buf))
    }
}

impl <T> DeltaEncodable for Vec<T>
    where T: DeltaEncodable,
          Vec<T>: PartialEq + Clone
{
    #[inline]
    fn encode<W>(&self, base: Option<&Self>, w: &mut Writer<W>) -> io::Result<()>
        where W: Write
    {
        if let Some(base) = base {
            if base == self {
                w.write_bool(false)?;
                return Ok(())
            }
        }
        w.write_bool(true)?;

        write_len_bits(w, self.len())?;
        for (idx, val) in self.iter().enumerate() {
            T::encode(val, base.and_then(|v | v.get(idx)), w)?;
        }
        Ok(())
    }

    #[inline]
    fn decode<R>(base: Option<&Self>, r: &mut Reader<R>) -> io::Result<Self>
        where R: Read
    {
        if r.read_bool()? {
            let len = read_len_bits(r)?;
            let mut buf = Vec::with_capacity(len);
            for idx in 0 .. len {
                buf.push(T::decode(base.and_then(|v| v.get(idx)), r)?);
            }
            Ok(buf)
        } else if let Some(base) = base {
            Ok(base.to_owned())
        } else {
            Err(io::Error::new(io::ErrorKind::InvalidInput, "Missing previous vec state"))
        }
    }
}

impl <T> DeltaEncodable for Option<T>
    where T: DeltaEncodable
{
    #[inline]
    fn encode<W>(&self, base: Option<&Self>, w: &mut Writer<W>) -> io::Result<()>
        where W: Write
    {
        if let Some(ref s) = *self {
            w.write_bool(true)?;
            T::encode(s, base.and_then(|v| v.as_ref()), w)?;
        } else {
            w.write_bool(false)?;
        }
        Ok(())
    }

    #[inline]
    fn decode<R>(base: Option<&Self>, r: &mut Reader<R>) -> io::Result<Self>
        where R: Read
    {
        if r.read_bool()? {
            Ok(Some(
                T::decode(base.and_then(|v| v.as_ref()), r)?
            ))
        } else {
            Ok(None)
        }
    }
}


impl DeltaEncodable for f32 {
    #[inline]
    fn encode<W>(&self, _base: Option<&Self>, w: &mut Writer<W>) -> io::Result<()>
        where W: Write
    {
        w.write_f32(*self)
    }

    #[inline]
    fn decode<R>(_base: Option<&Self>, r: &mut Reader<R>) -> io::Result<Self>
        where R: Read
    {
        r.read_f32()
    }
}

impl_create_array!([32] a, b, c, d, e, f, g, h, j, k, l, m, n, o, p, q, r, s, t, u, v, w, x, y, z, za, zb, zc, zd, ze, zf, zg,);