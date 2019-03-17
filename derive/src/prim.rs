use super::*;

#[derive(Debug)]
pub(crate) enum Prim {
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    Bool,
}

impl Prim {
    fn range(&self, bits: u32) -> (TokenStream, TokenStream) {
        macro_rules! gen_prim {
            ($(
                $key:ident => ($ty:ident, $nbits:expr),
            )*) => {
                match *self {
                    $(
                        Prim::$key => {
                            let min = $ty::min_value() >> ($nbits - bits);
                            let max = $ty::max_value() >> ($nbits - bits);
                            (quote!(#min), quote!(#max))
                        }
                    )*
                    _ => panic!("No range for bool"),
                }
            }
        }
        gen_prim! {
            I8 => (i8, 8),
            I16 => (i16, 16),
            I32 => (i32, 32),
            I64 => (i64, 64),
            U8 => (u8, 8),
            U16 => (u16, 16),
            U32 => (u32, 32),
            U64 => (u64, 64),
        }
    }
    pub fn from_ident(i: &syn::Ident) -> Option<Prim> {
        Some(match i.to_string().as_str() {
            "i8" => Prim::I8,
            "i16" => Prim::I16,
            "i32" => Prim::I32,
            "i64" => Prim::I64,
            "u8" => Prim::U8,
            "u16" => Prim::U16,
            "u32" => Prim::U32,
            "u64" => Prim::U64,
            "bool" => Prim::Bool,
            _ =>  return None,
        })
    }

    pub(crate) fn build(
        &self,
        mut flags: GenFlags,
        encode: &mut Vec<TokenStream>,
        encode_part: &mut Vec<TokenStream>,
        decode: &mut Vec<TokenStream>,
        decode_part: &mut Vec<TokenStream>,
        name_self: &TokenStream,
        name_base: &TokenStream,
        de_target: TokenStream,
        attrs: &[syn::Attribute]
    ) {
        if let Prim::Bool = *self {
            encode.push(quote!{
                w.write_bool(#name_self)?;
            });
            encode_part.push(quote!{
                w.write_bool(#name_self)?;
            });
            decode.push(quote!{
                #de_target r.read_bool()?
            });
            decode_part.push(quote!{
                #de_target r.read_bool()?
            });
            return;
        }
        let max_bit_size = match *self {
            Prim::I8 | Prim::U8 => 8,
            Prim::I16 | Prim::U16 => 16,
            Prim::I32 | Prim::U32 => 32,
            Prim::I64 | Prim::U64 => 64,
            _ => unreachable!(),
        };

        let mut bit_size = max_bit_size;
        let mut sub_bits = vec![];
        flags |= decode_flags(attrs);
        for attr in attrs {
            match attr.interpret_meta().unwrap() {
                syn::Meta::NameValue(syn::MetaNameValue{ref ident, lit: syn::Lit::Str(ref val), ..}) if ident == "delta_bits" => {
                    let val = val.value();
                    let val: i32 = val.parse().unwrap();
                    if val > max_bit_size {
                        panic!("Wanted {} bits but the max is {}", val, max_bit_size)
                    }
                    bit_size = val;
                },
                syn::Meta::NameValue(syn::MetaNameValue{ref ident, lit: syn::Lit::Str(ref val), ..}) if ident == "delta_subbits" => {
                    let val = val.value();
                    for val in val.split(",").map(|v| v.trim()) {
                        let val: u32 = val.parse().unwrap();
                        if val > max_bit_size as u32 {
                            panic!("Wanted {} bits but the max is {}", val, max_bit_size)
                        }
                        sub_bits.push(val);
                    }
                },
                _ => {},
            }
        }

        let bit_size = bit_size as u8;

        macro_rules! gen_prim {
            ($(
                ($key:ident, $ty:ty, $sty:ty) => ($emethod:ident, $dmethod:ident),
            )*) => {
                match *self {
                    $(
                    Prim::$key => {
                        if !sub_bits.is_empty() {
                            let num_states = if flags.contains(GenFlags::ALWAYS) { 0 } else { 1 } + sub_bits.len();
                            let required_bits = (num_states.next_power_of_two() - 1).count_ones() as u8;
                            let mut encode_vals: Vec<TokenStream> = vec![];
                            let mut encode_part_vals: Vec<TokenStream> = vec![];
                            let mut decode_vals: Vec<TokenStream> = vec![];
                            let mut decode_part_vals: Vec<TokenStream> = vec![];

                            let target_name = if flags.contains(GenFlags::DIFF) {
                                quote!(__diff_val)
                            } else {
                                name_self.clone()
                            };

                            let mut offset = 0u64;
                            if !flags.contains(GenFlags::ALWAYS) {
                                encode_part_vals.push(quote!(
                                    _ if #name_self == #name_base => w.write_unsigned(#offset, #required_bits)?,
                                ));
                                decode_part_vals.push(quote!(
                                    0 => #name_base,
                                ));
                                offset += 1;
                            }
                            for sub in &sub_bits {
                                let (min, max) = self.range(*sub);

                                let sub8 = *sub as u8;
                                let enc = quote!(
                                    #min ... #max => {
                                        w.write_unsigned(#offset, #required_bits)?;
                                        w.$emethod($ty::from(#target_name), #sub8)?;
                                    },
                                );
                                encode_part_vals.push(enc.clone());
                                encode_vals.push(enc);

                                let dec = quote!(
                                    #offset => {
                                        r.$dmethod(#sub8)? as $sty
                                    },
                                );
                                decode_part_vals.push(dec.clone());
                                decode_vals.push(dec);
                                offset += 1;

                            }
                            if flags.contains(GenFlags::DIFF) {
                                encode_part.push(quote!{
                                    let __diff_val = #name_self - #name_base;
                                    match __diff_val {
                                        #(#encode_part_vals)*
                                        _ => return Err(::std::io::Error::new(::std::io::ErrorKind::InvalidData, "Number out of range")),
                                    }
                                });
                                decode_part.push(quote!{
                                    #de_target {
                                        let __diff_val = match r.read_unsigned(#required_bits)? {
                                            #(#decode_part_vals)*
                                            _ => return Err(::std::io::Error::new(::std::io::ErrorKind::InvalidData, "Invalid subbit")),
                                        };
                                        #name_base + __diff_val
                                    }
                                });

                                encode.push(quote!{
                                    let __diff_val = #name_self;
                                    match __diff_val {
                                        #(#encode_vals)*
                                        _ => return Err(::std::io::Error::new(::std::io::ErrorKind::InvalidData, "Number out of range")),
                                    }
                                });
                                decode.push(quote!{
                                    #de_target match r.read_unsigned(#required_bits)? {
                                        #(#decode_vals)*
                                        _ => return Err(::std::io::Error::new(::std::io::ErrorKind::InvalidData, "Invalid subbit")),
                                    }
                                });
                            } else {
                                encode_part.push(quote!{
                                    match #name_self {
                                        #(#encode_part_vals)*
                                        _ => return Err(::std::io::Error::new(::std::io::ErrorKind::InvalidData, "Number out of range")),
                                    }
                                });
                                decode_part.push(quote!{
                                    #de_target match r.read_unsigned(#required_bits)? {
                                        #(#decode_part_vals)*
                                        _ => return Err(::std::io::Error::new(::std::io::ErrorKind::InvalidData, "Invalid subbit")),
                                    }
                                });

                                encode.push(quote!{
                                    match #name_self {
                                        #(#encode_vals)*
                                        _ => return Err(::std::io::Error::new(::std::io::ErrorKind::InvalidData, "Number out of range")),
                                    }
                                });
                                decode.push(quote!{
                                    #de_target match r.read_unsigned(#required_bits)? {
                                        #(#decode_vals)*
                                        _ => return Err(::std::io::Error::new(::std::io::ErrorKind::InvalidData, "Invalid subbit")),
                                    }
                                });
                            }
                        } else if flags.contains(GenFlags::ALWAYS) {
                            encode.push(quote!{
                                w.$emethod($ty::from(#name_self), #bit_size)?;
                            });
                            encode_part.push(quote!{
                                w.$emethod($ty::from(#name_self), #bit_size)?;
                            });
                            decode.push(quote!{
                                #de_target r.$dmethod(#bit_size)? as $sty
                            });
                            decode_part.push(quote!{
                                #de_target r.$dmethod(#bit_size)? as $sty
                            });
                        } else {
                            encode.push(quote!{
                                w.write_bool(true)?;
                                w.$emethod($ty::from(#name_self), #bit_size)?;
                            });
                            encode_part.push(quote!{
                                if #name_base != #name_self {
                                    w.write_bool(true)?;
                                    w.$emethod($ty::from(#name_self), #bit_size)?;
                                } else {
                                    w.write_bool(false)?
                                }
                            });
                            decode.push(quote!{
                                #de_target if r.read_bool()? {
                                    r.$dmethod(#bit_size)? as $sty
                                } else {
                                    return Err(::std::io::Error::new(::std::io::ErrorKind::InvalidData, "Mismatched decode, missing previous state"));
                                }
                            });
                            decode_part.push(quote!{
                                #de_target if r.read_bool()? {
                                    r.$dmethod(#bit_size)? as $sty
                                } else {
                                    #name_base
                                }
                            });
                        }
                    },
                    )*
                    _ => unreachable!(),
                }
            }
        }


        gen_prim!(
            (I8, i64, i8) => (write_signed, read_signed),
            (I16, i64, i16) => (write_signed, read_signed),
            (I32, i64, i32) => (write_signed, read_signed),
            (I64, i64, i64) => (write_signed, read_signed),
            (U8, u64, u8) => (write_unsigned, read_unsigned),
            (U16, u64, u16) => (write_unsigned, read_unsigned),
            (U32, u64, u32) => (write_unsigned, read_unsigned),
            (U64, u64, u64) => (write_unsigned, read_unsigned),
        );
    }
}