
#![recursion_limit = "1024"]
#![allow(clippy::all)]

extern crate proc_macro;
extern crate proc_macro2;
extern crate syn;
#[macro_use]
extern crate quote;
#[macro_use]
extern crate bitflags;

mod prim;
use prim::*;
mod float;
use float::*;
mod ty;
use ty::*;

use proc_macro2::{TokenStream, Span};

use syn::punctuated::Punctuated;
use syn::token::Comma;

bitflags!{
    struct GenFlags: u32 {
        const COMPLETE = 0b0000_0001;
        const ALWAYS   = 0b0000_0010;
        const DIFF     = 0b0000_0100;
        const FIXED    = 0b0000_1000;
        const DEFAULT  = 0b0001_0000;
    }
}

// delta_bits = number of bits used for an integer type
// delta_subbits = Try and use the smallest number of bits from the list
// delta_always = always send this value instead of only changes
// delta_complete = compare the whole struct and only send if changed
// delta_diff = sends the difference between the values, only useful when
//              used with `delta_subbits`
// delta_fixed - Causes the floating point number to be sent as a fixed point number

#[proc_macro_derive(DeltaEncode, attributes(
    delta_bits,
    delta_subbits,
    delta_diff,
    delta_always,
    delta_complete,
    delta_fixed,
    delta_default,
))]
pub fn delta_encode(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let ast: syn::DeriveInput = syn::parse(input).expect("Failed to parse input");
    let ts: proc_macro::TokenStream = delta_encode_impl(ast).into();
    ts
}

fn delta_encode_impl(ast: syn::DeriveInput) -> TokenStream {
    let name = &ast.ident;

    let flags = decode_flags(&ast.attrs);

    let (enc, dec) = match ast.data {
        syn::Data::Struct(syn::DataStruct{fields: syn::Fields::Named(fields), ..}) => {
            build_struct(name, &syn::Ident::new("self", Span::call_site()), &syn::Ident::new("base", Span::call_site()), flags, fields.named)
        },
        syn::Data::Struct(syn::DataStruct{fields: syn::Fields::Unnamed(fields), ..}) => {
            build_tuple(name, &syn::Ident::new("self", Span::call_site()), &syn::Ident::new("base", Span::call_site()), flags, fields.unnamed)
        },
        syn::Data::Enum(e) => {
            build_enum(name, &syn::Ident::new("self", Span::call_site()), &syn::Ident::new("base", Span::call_site()), flags, e.variants)
        }
        _ => unimplemented!("body type"),
    };

    quote! {
        #[allow(unused_variables, non_snake_case, unreachable_patterns, clippy::float_cmp)]
        impl crate::delta_encode::bitio::DeltaEncodable for #name {
            #[inline]
            fn encode<W>(&self, base: Option<&Self>, w: &mut crate::delta_encode::bitio::Writer<W>) -> ::std::io::Result<()>
                where W: std::io::Write
            {
                #enc
                Ok(())
            }

            #[inline]
            fn decode<R>(base: Option<&Self>, r: &mut crate::delta_encode::bitio::Reader<R>) -> ::std::io::Result<Self>
                where R: std::io::Read
            {
                Ok(#dec)
            }
        }
    }
}

fn decode_flags(attrs: &[syn::Attribute]) -> GenFlags {
    let mut flags = GenFlags::empty();
    for attr in attrs.into_iter().filter_map(|v| v.interpret_meta()) {
        match attr {
            syn::Meta::Word(ref ident) if ident == "delta_complete" => {
                flags |= GenFlags::COMPLETE;
            },
            syn::Meta::Word(ref ident) if ident == "delta_always" => {
                flags |= GenFlags::ALWAYS;
            },
            syn::Meta::Word(ref ident) if ident == "delta_diff" => {
                flags |= GenFlags::DIFF;
            },
            syn::Meta::Word(ref ident) if ident == "delta_fixed" => {
                flags |= GenFlags::FIXED;
            },
            syn::Meta::Word(ref ident) if ident == "delta_default" => {
                flags |= GenFlags::DEFAULT;
            },
            _ => {},
        }
    }
    flags
}

fn build_enum(name: &syn::Ident, self_name: &syn::Ident, base_name: &syn::Ident, flags: GenFlags, variants: Punctuated<syn::Variant, Comma>) -> (TokenStream, TokenStream) {
    let mut encode: Vec<TokenStream> = vec![];
    let mut encode_part: Vec<TokenStream> = vec![];
    let mut decode: Vec<TokenStream> = vec![];
    let mut decode_part: Vec<TokenStream> = vec![];

    let self_ref = if self_name == "self" {
        syn::Ident::new("self", Span::call_site())
    } else {
        syn::Ident::new(
            &format!("&{}", self_name),
            Span::call_site()
        )
    };

    let variant_bits = (variants.len().next_power_of_two() - 1).count_ones() as u8;

    for (idx, variant) in variants.into_iter().enumerate() {
        let idxu = idx as u64;
        let encode_variant = quote! {
            w.write_unsigned(#idxu, #variant_bits)?;
        };

        let ident = &variant.ident;
        let variant_flags = flags | decode_flags(&variant.attrs);

        match variant.fields {
            syn::Fields::Unit => {
                encode.push(quote! {
                    &#name::#ident => {
                        #encode_variant
                    }
                });
                encode_part.push(quote! {
                    (&#name::#ident, _) => {
                        #encode_variant
                    }
                });
                decode.push(quote! {
                    #idxu => { #name::#ident }
                });
                decode_part.push(quote! {
                    (#idxu, _) => { #name::#ident }
                });
            },
            syn::Fields::Named(fields) => {
                let mut sencode: Vec<TokenStream> = vec![];
                let mut sencode_part: Vec<TokenStream> = vec![];
                let mut sdecode: Vec<TokenStream> = vec![];
                let mut sdecode_part: Vec<TokenStream> = vec![];

                let mut field_info = vec![];
                let mut field_info_base = vec![];
                for field in fields.named {
                    let fname = field.ident.unwrap();
                    let name_self = quote!(*#fname);
                    let name_base_orig = syn::Ident::new(
                        &format!("__enumbase__{}", fname),
                        Span::call_site()
                    );
                    field_info.push(fname.clone());
                    field_info_base.push(quote!(#fname: ref #name_base_orig));
                    let name_base = quote!(*#name_base_orig);
                    build_ty(
                        field.ty, variant_flags,
                        &mut sencode, &mut sencode_part,
                        &mut sdecode, &mut sdecode_part,
                        quote!(#fname :),
                        &name_self, &name_base,
                        &field.attrs,
                    );
                }
                {
                    let field_info = field_info.clone();
                    let sencode = sencode.clone();
                    encode.push(quote!(
                        &#name::#ident{#(ref #field_info),*} => {
                            #encode_variant
                            #(#sencode)*
                        }
                    ));
                }
                {
                    let field_info = field_info.clone();
                    let field_info_base = field_info_base.clone();
                    encode_part.push(quote!(
                        (
                            &#name::#ident{#(ref #field_info),*},
                            &#name::#ident{#(#field_info_base),*},
                        ) => {
                            #encode_variant
                            #(#sencode_part)*
                        }
                    ));
                }
                {
                    let field_info = field_info.clone();
                    let sencode = sencode.clone();
                    encode_part.push(quote!(
                        (
                            &#name::#ident{#(ref #field_info),*},
                            _,
                        ) => {
                            #encode_variant
                            #(#sencode)*
                        }
                    ));
                }
                {
                    let sdecode = sdecode.clone();
                    decode.push(quote!(
                        #idxu => {
                            #name::#ident {
                                #(#sdecode,)*
                            }
                        }
                    ));
                }
                {
                    let field_info_base = field_info_base.clone();
                    decode_part.push(quote!(
                        (
                            #idxu,
                            &#name::#ident{#(#field_info_base),*},
                        ) => {
                            #name::#ident {
                                #(#sdecode_part,)*
                            }
                        }
                    ));
                }
                {
                    let sdecode = sdecode.clone();
                    decode_part.push(quote!(
                        (
                            #idxu,
                            _,
                        ) => {
                            #name::#ident {
                                #(#sdecode,)*
                            }
                        }
                    ));
                }
            },
            syn::Fields::Unnamed(fields) => {
                let mut sencode: Vec<TokenStream> = vec![];
                let mut sencode_part: Vec<TokenStream> = vec![];
                let mut sdecode: Vec<TokenStream> = vec![];
                let mut sdecode_part: Vec<TokenStream> = vec![];

                let mut field_info = vec![];
                let mut field_info_base = vec![];
                for (idx, field) in fields.unnamed.into_iter().enumerate() {
                    let name_self_orig = syn::Ident::new(&format!("__enumcur__{}", idx), Span::call_site());
                    let name_self = quote!(*#name_self_orig);
                    let name_base_orig = syn::Ident::new(&format!("__enumbase__{}", idx), Span::call_site());
                    let name_base = quote!(*#name_base_orig);

                    field_info.push(quote!(ref #name_self_orig));
                    field_info_base.push(quote!(ref #name_base_orig));
                    build_ty(
                        field.ty, flags,
                        &mut sencode, &mut sencode_part,
                        &mut sdecode, &mut sdecode_part,
                        quote!(),
                        &name_self, &name_base,
                        &field.attrs,
                    );
                }
                {
                    let field_info = field_info.clone();
                    let sencode = sencode.clone();
                    encode.push(quote!(
                        &#name::#ident(#(#field_info),*) => {
                            #encode_variant
                            #(#sencode)*
                        }
                    ));
                }
                {
                    let field_info = field_info.clone();
                    let field_info_base = field_info_base.clone();
                    encode_part.push(quote!(
                        (
                            &#name::#ident(#(#field_info),*),
                            &#name::#ident(#(#field_info_base),*),
                        ) => {
                            #encode_variant
                            #(#sencode_part)*
                        }
                    ));
                }
                {
                    let field_info = field_info.clone();
                    let sencode = sencode.clone();
                    encode_part.push(quote!(
                        (
                            &#name::#ident(#(#field_info),*),
                            _,
                        ) => {
                            #encode_variant
                            #(#sencode)*
                        }
                    ));
                }
                {
                    let sdecode = sdecode.clone();
                    decode.push(quote!(
                        #idxu => {
                            #name::#ident (
                                #(#sdecode,)*
                            )
                        }
                    ));
                }
                {
                    let field_info_base = field_info_base.clone();
                    decode_part.push(quote!(
                        (
                            #idxu,
                            &#name::#ident(#(#field_info_base),*),
                        ) => {
                            #name::#ident (
                                #(#sdecode_part,)*
                            )
                        }
                    ));
                }
                {
                    let sdecode = sdecode.clone();
                    decode_part.push(quote!(
                        (
                            #idxu,
                            _,
                        ) => {
                            #name::#ident (
                                #(#sdecode,)*
                            )
                        }
                    ));
                }
            }
        }
    }

    if flags.contains(GenFlags::COMPLETE) {
        (quote! {
            if #base_name.map_or(false, |v| *v == *self) {
                w.write_bool(false);
            } else {
                w.write_bool(true);
                if let Some(#base_name) = #base_name {
                    match (#self_ref, #base_name) {
                        #(#encode_part),*
                    }
                } else {
                    match #self_ref {
                        #(#encode),*
                    }
                }
            }
        }, quote! {{
            let changed = r.read_bool()?;
            match (#base_name, changed) {
                (Some(#base_name), false) => #base_name.clone(),
                (Some(#base_name), true) => {
                    match (r.read_unsigned(#variant_bits)?, #base_name) {
                        #(#decode_part,)*
                        _ => return Err(::std::io::Error::new(::std::io::ErrorKind::InvalidData, "Invalid enum variant")),
                    }
                },
                (None, true) => {
                    match r.read_unsigned(#variant_bits)? {
                        #(#decode,)*
                        _ => return Err(::std::io::Error::new(::std::io::ErrorKind::InvalidData, "Invalid enum variant")),
                    }
                },
                (None, false) => return Err(::std::io::Error::new(::std::io::ErrorKind::InvalidData, "Mismatched decode, missing previous state")),
            }
        }})
    } else {
        (quote! {
            if let Some(#base_name) = #base_name {
                match (#self_ref, #base_name) {
                    #(#encode_part),*
                }
            } else {
                match #self_ref {
                    #(#encode),*
                }
            }
        }, quote! {{
            match #base_name {
                Some(#base_name) => {
                    match (r.read_unsigned(#variant_bits)?, #base_name) {
                        #(#decode_part,)*
                        _ => return Err(::std::io::Error::new(::std::io::ErrorKind::InvalidData, "Invalid enum variant")),
                    }
                },
                None => {
                    match r.read_unsigned(#variant_bits)? {
                        #(#decode,)*
                        _ => return Err(::std::io::Error::new(::std::io::ErrorKind::InvalidData, "Invalid enum variant")),
                    }
                },
            }
        }})
    }
}

fn build_struct(name: &syn::Ident, self_name: &syn::Ident, base_name: &syn::Ident, flags: GenFlags, fields: Punctuated<syn::Field, Comma>) -> (TokenStream, TokenStream) {
    let mut encode: Vec<TokenStream> = vec![];
    let mut encode_part: Vec<TokenStream> = vec![];
    let mut decode: Vec<TokenStream> = vec![];
    let mut decode_part: Vec<TokenStream> = vec![];

    for field in fields {
        let fname = field.ident.unwrap();
        let name_self = quote!(#self_name . #fname);
        let name_base = quote!(#base_name . #fname);
        build_ty(
            field.ty, flags,
            &mut encode, &mut encode_part,
            &mut decode, &mut decode_part,
            quote!(#fname :),
            &name_self, &name_base,
            &field.attrs,
        );
    }

    if flags.contains(GenFlags::COMPLETE) {
        (quote! {
            if #base_name.map_or(false, |v| *v == *self) {
                w.write_bool(false)?;
            } else {
                w.write_bool(true)?;
                if let Some(#base_name) = #base_name {
                    #(#encode_part)*
                } else {
                    #(#encode)*
                }
            }
        }, quote! {{
            let changed = r.read_bool()?;
            match (#base_name, changed) {
                (Some(#base_name), false) => (*#base_name).clone(),
                (Some(#base_name), true) => {
                    #name {
                        #(#decode_part,)*
                    }
                },
                (None, true) => {
                    #name {
                        #(#decode,)*
                    }
                },
                (None, false) => return Err(::std::io::Error::new(::std::io::ErrorKind::InvalidData, "Mismatched decode, missing previous state")),
            }
        }})
    } else {
        (quote! {
            if let Some(#base_name) = #base_name {
                #(#encode_part)*
            } else {
                #(#encode)*
            }
        }, quote! {{
            if let Some(#base_name) = #base_name {
                #name {
                    #(#decode_part,)*
                }
            } else {
                #name {
                    #(#decode,)*
                }
            }
        }})
    }
}

fn build_tuple(name: &syn::Ident, self_name: &syn::Ident, base_name: &syn::Ident, flags: GenFlags, fields: Punctuated<syn::Field, Comma>) -> (TokenStream, TokenStream) {
    let mut encode: Vec<TokenStream> = vec![];
    let mut encode_part: Vec<TokenStream> = vec![];
    let mut decode: Vec<TokenStream> = vec![];
    let mut decode_part: Vec<TokenStream> = vec![];

    for (idx, field) in fields.into_iter().enumerate() {
        let index = syn::Index::from(idx);
        let name_self = quote!(#self_name.#index);
        let name_base = quote!(#base_name.#index);
        build_ty(
            field.ty, flags,
            &mut encode, &mut encode_part,
            &mut decode, &mut decode_part,
            quote!(),
            &name_self, &name_base,
            &field.attrs,
        );
    }

    if flags.contains(GenFlags::COMPLETE) {
        (quote! {
            if #base_name.map_or(false, |v| *v == *self) {
                w.write_bool(false);
            } else {
                w.write_bool(true);
                if let Some(#base_name) = #base_name {
                    #(#encode_part)*
                } else {
                    #(#encode)*
                }
            }
        }, quote! {{
            let changed = r.read_bool()?;
            match (#base_name, changed) {
                (Some(#base_name), false) => #base_name.clone(),
                (Some(#base_name), true) => {
                    #name (
                        #(#decode_part,)*
                    )
                },
                (None, true) => {
                    #name (
                        #(#decode,)*
                    )
                },
                (None, false) => return Err(::std::io::Error::new(::std::io::ErrorKind::InvalidData, "Mismatched decode, missing previous state")),
            }
        }})
    } else {
        (quote! {
            if let Some(#base_name) = #base_name {
                #(#encode_part)*
            } else {
                #(#encode)*
            }
        }, quote! {{
            if let Some(#base_name) = #base_name {
                #name (
                    #(#decode_part,)*
                )
            } else {
                #name (
                    #(#decode,)*
                )
            }
        }})
    }
}