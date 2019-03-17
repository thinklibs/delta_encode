use super::*;

pub(crate) fn build_float(
    ty: &syn::Ident,
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
    let max_bit_size = match ty.to_string().as_str() {
        "f32" => 32,
        "f64" => 64,
        _ => panic!("Invalid float type"),
    };

    let mut bit_size: Option<(i32, i32)> = None;
    let mut sub_bits = vec![];
    flags |= decode_flags(attrs);
    for attr in attrs {
        match attr.interpret_meta().unwrap() {
            syn::Meta::NameValue(syn::MetaNameValue{ref ident, lit: syn::Lit::Str(ref val), ..}) if ident == "delta_bits" => {
                let val = val.value();
                let mut parts = val.split(":");
                let int: i32 = parts.next().unwrap().parse().unwrap();
                if int > max_bit_size {
                    panic!("Wanted {} bits but the max is {}", val, max_bit_size)
                }
                let fract: i32 = parts.next().unwrap().parse().unwrap();
                if fract > max_bit_size {
                    panic!("Wanted {} bits but the max is {}", val, max_bit_size)
                }
                bit_size = Some((int, fract));
            },
            syn::Meta::NameValue(syn::MetaNameValue{ref ident, lit: syn::Lit::Str(ref val), ..})  if ident == "delta_subbits" => {
                let val = val.value();
                for val in val.split(",").map(|v| v.trim()) {
                    let mut parts = val.split(":");
                    let int: i32 = parts.next().unwrap().parse().unwrap();
                    if int > max_bit_size {
                        panic!("Wanted {} bits but the max is {}", val, max_bit_size)
                    }
                    let fract: i32 = parts.next().unwrap().parse().unwrap();
                    if fract > max_bit_size {
                        panic!("Wanted {} bits but the max is {}", val, max_bit_size)
                    }
                    sub_bits.push((int, fract));
                }
            },
            _ => {},
        }
    }

    let (emethod, dmethod) = if ty == "f32" {
        (syn::Ident::new("write_f32", Span::call_site()), syn::Ident::new("read_f32", Span::call_site()))
    } else {
        (syn::Ident::new("write_f64", Span::call_site()), syn::Ident::new("read_f64", Span::call_site()))
    };

    if flags.contains(GenFlags::FIXED) {
        if let Some((int, fract)) = bit_size {
            let bits = (int + fract) as u8;
            if flags.contains(GenFlags::ALWAYS) {
                let enc = quote!{
                    w.write_signed((#name_self * (1 << #fract) as #ty) as i64, #bits)?;
                };
                encode.push(enc.clone());
                encode_part.push(enc);
                let dec = quote!{
                    #de_target r.read_signed(#bits)? as #ty / ((1 << #fract) as #ty)
                };
                decode.push(dec.clone());
                decode_part.push(dec);
            } else {
                encode.push(quote!{
                    w.write_bool(true)?;
                    w.write_signed((#name_self * (1 << #fract) as #ty) as i64, #bits)?;
                });
                encode_part.push(quote!{
                    let __orig = (#name_base * (1 << #fract) as #ty) as i64;
                    let __val = (#name_self * (1 << #fract) as #ty) as i64;
                    if __orig != __val {
                        w.write_bool(true)?;
                        w.write_signed(__val, #bits)?;
                    } else {
                        w.write_bool(false)?;
                    }
                });
                decode.push(quote!{
                    #de_target if r.read_bool()? {
                        r.read_signed(#bits)? as #ty / ((1 << #fract) as #ty)
                    } else {
                        return Err(::std::io::Error::new(::std::io::ErrorKind::InvalidData, "Mismatched decode, missing previous state"));
                    }
                });
                decode_part.push(quote!{
                    #de_target if r.read_bool()? {
                        r.read_signed(#bits)? as #ty / ((1 << #fract) as #ty)
                    } else {
                        #name_base
                    }
                });
            }
        } else if !sub_bits.is_empty() {
            let num_states = if flags.contains(GenFlags::ALWAYS) { 0 } else { 1 } + sub_bits.len();
            let required_bits = (num_states.next_power_of_two() - 1).count_ones() as u8;
            let mut encode_vals: Vec<TokenStream> = vec![];
            let mut encode_part_vals: Vec<TokenStream> = vec![];
            let mut decode_vals: Vec<TokenStream> = vec![];
            let mut decode_part_vals: Vec<TokenStream> = vec![];

            let target_name = if flags.contains(GenFlags::DIFF) {
                quote!(__diff_val)
            } else {
                quote!(__abs_val)
            };

            let mut offset = 0u64;
            if !flags.contains(GenFlags::ALWAYS) {
                encode_part_vals.push(quote!(
                    if #name_self == #name_base {
                        w.write_unsigned(#offset, #required_bits)?;
                    }
                ));
                if flags.contains(GenFlags::DIFF) {
                    decode_part_vals.push(quote!(
                        0 => #name_base,
                    ));
                } else {
                    decode_part_vals.push(quote!(
                        0 => #name_base,
                    ));
                }
                offset += 1;
            }
            let fract = sub_bits.first().unwrap().1;
            for &(int, ofract) in &sub_bits {
                if int == -1 || fract == -1 {
                    let enc = quote!(
                        if true {
                            w.write_unsigned(#offset, #required_bits)?;
                            w.#emethod(#name_self)?;
                        }
                    );
                    encode_part_vals.push(enc.clone());
                    encode_vals.push(enc);

                    let dec = quote!(
                        #offset => {
                            r.#dmethod()?
                        },
                    );
                    decode_part_vals.push(dec.clone());
                    decode_vals.push(dec);
                } else {
                    assert!(fract == ofract);
                    let bits = (int + fract) as u8;
                    let min = i64::min_value() >> (64 - bits);
                    let max = i64::max_value() >> (64 - bits);
                    let (min, max) = (quote!(#min), quote!(#max));

                    let enc = quote!(
                        if let val @ #min ... #max = #target_name {
                            w.write_unsigned(#offset, #required_bits)?;
                            w.write_signed(val, #bits)?;
                        }
                    );
                    encode_part_vals.push(enc.clone());
                    encode_vals.push(enc);

                    if flags.contains(GenFlags::DIFF) {
                        decode_part_vals.push(quote!(
                            #offset => {
                                let __diff_val = r.read_signed(#bits)?;
                                let __diff_val_b = (#name_base * (1 << #fract) as #ty) as i64;
                                (__diff_val_b + __diff_val) as #ty / ((1 << #fract) as #ty)
                            },
                        ));
                        decode_vals.push(quote!(
                            #offset => {
                                r.read_signed(#bits)? as #ty / ((1 << #fract) as #ty)
                            },
                        ));
                    } else {
                        decode_part_vals.push(quote!(
                            #offset => {
                                r.read_signed(#bits)? as #ty / ((1 << #fract) as #ty)
                            },
                        ));
                        decode_vals.push(quote!(
                            #offset => {
                                r.read_signed(#bits)? as #ty / ((1 << #fract) as #ty)
                            },
                        ));
                    }
                }
                offset += 1;

            }
            if flags.contains(GenFlags::DIFF) {
                encode_part.push(quote!{
                    let __diff_val_s = (#name_self * (1 << #fract) as #ty) as i64;
                    let __diff_val_b = (#name_base * (1 << #fract) as #ty) as i64;
                    let __diff_val = __diff_val_s - __diff_val_b;
                    #(#encode_part_vals else)*
                    {
                        return Err(::std::io::Error::new(::std::io::ErrorKind::InvalidData, "Number out of range"))
                    }
                });
                decode_part.push(quote!{
                    #de_target {
                        match r.read_unsigned(#required_bits)? {
                            #(#decode_part_vals)*
                            _ => return Err(::std::io::Error::new(::std::io::ErrorKind::InvalidData, "Invalid subbit")),
                        }
                    }
                });

                encode.push(quote!{
                    let __diff_val = (#name_self * (1 << #fract) as #ty) as i64;
                    #(#encode_vals else)*
                    {
                        return Err(::std::io::Error::new(::std::io::ErrorKind::InvalidData, "Number out of range"))
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
                    let __abs_val = (#name_self * (1 << #fract) as #ty) as i64;
                    #(#encode_part_vals else)*
                    {
                        return Err(::std::io::Error::new(::std::io::ErrorKind::InvalidData, "Number out of range"))
                    }
                });
                decode_part.push(quote!{
                    #de_target match r.read_unsigned(#required_bits)? {
                        #(#decode_part_vals)*
                        _ => return Err(::std::io::Error::new(::std::io::ErrorKind::InvalidData, "Invalid subbit")),
                    }
                });

                encode.push(quote!{
                    let __abs_val = (#name_self * (1 << #fract) as #ty) as i64;
                    #(#encode_vals else)*
                    {
                        return Err(::std::io::Error::new(::std::io::ErrorKind::InvalidData, "Number out of range"))
                    }
                });
                decode.push(quote!{
                    #de_target match r.read_unsigned(#required_bits)? {
                        #(#decode_vals)*
                        _ => return Err(::std::io::Error::new(::std::io::ErrorKind::InvalidData, "Invalid subbit")),
                    }
                });
            }

        } else {
            panic!("`delta_fixed` requires either `delta_bits` or `delta_subbits`")
        }
    } else {
        if flags.contains(GenFlags::ALWAYS) {
            encode.push(quote!{
                w.#emethod(#name_self)?;
            });
            encode_part.push(quote!{
                w.#emethod(#name_self)?;
            });
            decode.push(quote!{
                #de_target r.#dmethod()?
            });
            decode_part.push(quote!{
                #de_target r.#dmethod()?
            });
        } else {
            encode.push(quote!{
                w.write_bool(true)?;
                w.#emethod(#name_self)?;
            });
            encode_part.push(quote!{
                if #name_base != #name_self {
                    w.write_bool(true)?;
                    w.#emethod(#name_self)?;
                } else {
                    w.write_bool(false)?;
                }
            });
            decode.push(quote!{
                #de_target if r.read_bool()? {
                    r.#dmethod()?
                } else {
                    return Err(::std::io::Error::new(::std::io::ErrorKind::InvalidData, "Mismatched decode, missing previous state"));
                }
            });
            decode_part.push(quote!{
                #de_target if r.read_bool()? {
                    r.#dmethod()?
                } else {
                    #name_base
                }
            });
        }
    }
}