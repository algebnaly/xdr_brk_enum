use quote::quote;
use syn::Fields;
use syn::Ident;
use syn::Index;
use syn::spanned::Spanned;

use crate::VariantDiscriminant;
use crate::VariantInfo;

fn generate_deserialize_fields(fields: &Fields) -> proc_macro2::TokenStream {
    match fields {
        Fields::Unit => {
            quote! { <()> }
        }
        Fields::Unnamed(fields) => {
            let field_types = fields.unnamed.iter().map(|f| &f.ty);
            quote! { <(#(#field_types,)*)> }
        }
        Fields::Named(fields) => {
            let field_types = fields.named.iter().map(|f| &f.ty);
            quote! { <(#(#field_types,)*)> }
        }
    }
}

fn generate_variant_construction(
    variant_ident: &Ident,
    fields: &Fields,
    name: &Ident,
) -> proc_macro2::TokenStream {
    match fields {
        Fields::Unit => {
            quote! {
                #name::#variant_ident
            }
        }
        Fields::Unnamed(fields) => {
            let indices = (0..fields.unnamed.len()).map(Index::from);
            quote! {
                #name::#variant_ident(#(fields.#indices,)*)
            }
        }
        Fields::Named(fields) => {
            let field_names: Vec<&Ident> = fields
                .named
                .iter()
                .filter_map(|f| f.ident.as_ref()) // use filter_map is ok here, since this is named fields, all fields are named.
                .collect();
            let indices = (0..field_names.len()).map(Index::from);
            quote! {
                #name::#variant_ident {
                    #(#field_names: fields.#indices,)*
                }
            }
        }
    }
}

/// 生成单个反序列化分支
pub(crate) fn generate_deserialization_branch(
    variant_info: &VariantInfo,
    name: &Ident,
) -> proc_macro2::TokenStream {
    let variant_ident = &variant_info.variant.ident;
    let fields = &variant_info.variant.fields;

    match &variant_info.discriminant {
        VariantDiscriminant::Default => {
            let default_variant_type = variant_info
                .variant
                .fields
                .iter()
                .map(|field| &field.ty)
                .next();
            match default_variant_type {
                Some(default_variant_ty) => {
                    let default_variant_ty = default_variant_ty.clone();
                    quote! {
                        let _ = data.next_element::<()>()?
                            .ok_or_else(|| ::serde::de::Error::invalid_length(1, &self))?;
                        Ok(#name::#variant_ident(discriminant as #default_variant_ty))
                    }
                }
                None => {
                    return syn::Error::new(
                        variant_info.variant.span(),
                        "Internal error: default_arm validation failed".to_string(),
                    )
                    .to_compile_error();
                }
            }
        }
        VariantDiscriminant::Normal(discriminant_expr) => {
            let field_types = generate_deserialize_fields(fields);
            let variant_construction = generate_variant_construction(variant_ident, fields, name);

            let deserialization_body = if matches!(fields, Fields::Unit) {
                quote! {
                    let _ = data.next_element::<()>()?
                        .ok_or_else(|| ::serde::de::Error::invalid_length(1, &self))?;
                    Ok(#variant_construction)
                }
            } else {
                quote! {
                    let fields = data.next_element::#field_types()?
                        .ok_or_else(|| ::serde::de::Error::invalid_length(1, &self))?;
                    Ok(#variant_construction)
                }
            };

            quote! {
                if discriminant == (#discriminant_expr) as u32 {
                    return {#deserialization_body};
                }
            }
        }
    }
}
