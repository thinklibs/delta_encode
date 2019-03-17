use super::*;

pub(crate) fn build_ty(
    ty: syn::Type, flags: GenFlags,
    encode: &mut Vec<TokenStream>,
    encode_part: &mut Vec<TokenStream>,
    decode: &mut Vec<TokenStream>,
    decode_part: &mut Vec<TokenStream>,
    de_target: TokenStream,
    name_self: &TokenStream,
    name_base: &TokenStream,
    attrs: &[syn::Attribute]
) {
    if decode_flags(&attrs).contains(GenFlags::DEFAULT) {
        decode.push(quote! {
            #de_target ::std::default::Default::default()
        });
        decode_part.push(quote! {
            #de_target ::std::default::Default::default()
        });
        return;
    }
    match ty {
        syn::Type::Path(syn::TypePath{path, ..}) => {
            if let Some(prim) = path.segments.first() {
                let prim = prim.value();
                if let Some(prim) = Prim::from_ident(&prim.ident) {
                    prim.build(
                        flags,
                        encode,
                        encode_part,
                        decode,
                        decode_part,
                        name_self,
                        name_base,
                        de_target.clone(),
                        attrs
                    );
                    return;
                }
                if prim.ident == "f32" || prim.ident == "f64" {
                    build_float(
                        &prim.ident,
                        flags,
                        encode,
                        encode_part,
                        decode,
                        decode_part,
                        name_self,
                        name_base,
                        de_target.clone(),
                        attrs
                    );
                    return;
                }
            }
            // let field_flags = flags | decode_flags(&field.attrs);

            encode.push(quote!{
                crate::delta_encode::bitio::DeltaEncodable::encode(&#name_self, None, w)?;
            });
            encode_part.push(quote!{
                crate::delta_encode::bitio::DeltaEncodable::encode(&#name_self, Some(&#name_base), w)?;
            });
            decode.push(quote!{
                #de_target crate::delta_encode::bitio::DeltaEncodable::decode(None, r)?
            });
            decode_part.push(quote!{
                #de_target crate::delta_encode::bitio::DeltaEncodable::decode(Some(&#name_base), r)?
            });
        },
        syn::Type::Array(syn::TypeArray{elem: sub_ty, ..}) => {

            let mut sencode: Vec<TokenStream> = vec![];
            let mut sencode_part: Vec<TokenStream> = vec![];
            let mut sdecode: Vec<TokenStream> = vec![];
            let mut sdecode_part: Vec<TokenStream> = vec![];

            let sname_self = quote!(*curr);
            let sname_base = quote!(*base);
            build_ty(
                *sub_ty, flags,
                &mut sencode, &mut sencode_part,
                &mut sdecode, &mut sdecode_part,
                quote!(),
                &sname_self, &sname_base,
                attrs,
            );
            encode.push(quote!{
                for curr in (#name_self).iter() {
                     #(#sencode)*
                }
            });
            encode_part.push(quote!{
                for (curr, base) in (#name_self).iter().zip((#name_base).iter()) {
                    #(#sencode_part)*
                }
            });
            decode.push(quote!{
                #de_target crate::delta_encode::CreateArray::create::<_, ::std::io::Error>(|_offset| {
                    Ok(#(#sdecode)*)
                })?
            });
            decode_part.push(quote!{
                #de_target crate::delta_encode::CreateArray::create::<_, ::std::io::Error>(|offset| {
                    let base = &(#name_base)[offset];
                    Ok(#(#sdecode_part)*)
                })?
            });
        },
        ty => unimplemented!("Other type: {:?}", ty),
    }
}