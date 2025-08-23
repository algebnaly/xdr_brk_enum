use quote::{format_ident, quote};
use syn::{Fields, Ident};

use crate::{VariantDiscriminant, VariantInfo};

pub(crate) fn generate_field_bindings(fields: &Fields) -> (Vec<Ident>, proc_macro2::TokenStream) {
    match fields {
        Fields::Unit => (Vec::new(), quote! {}),
        Fields::Unnamed(fields) => {
            let bindings: Vec<Ident> = (0..fields.unnamed.len())
                .map(|i| format_ident!("field_{}", i))
                .collect();
            let pattern = quote! { ( #(#bindings,)* ) };
            (bindings, pattern)
        }
        Fields::Named(fields) => {
            let bindings: Vec<Ident> = fields
                .named
                .iter()
                .filter_map(|f| f.ident.as_ref().cloned()) // use filter_map is ok here, since we already know that all fields are named, it has identifier
                .collect();
            let pattern = quote! { { #(#bindings),* } };
            (bindings, pattern)
        }
    }
}

pub(crate) fn generate_serialize_element(bindings: &[Ident]) -> proc_macro2::TokenStream {
    quote! { &(#(#bindings,)*) }
}

pub(crate) fn generate_match_arm(variant_info: &VariantInfo) -> proc_macro2::TokenStream {
    let variant_ident = &variant_info.variant.ident;
    let (field_bindings, field_pattern) = generate_field_bindings(&variant_info.variant.fields);

    match &variant_info.discriminant {
        VariantDiscriminant::Default => {
            let field_name = &field_bindings[0];
            quote! {
                Self::#variant_ident #field_pattern => {
                    let mut ser = serializer.serialize_tuple(2)?;
                    ::serde::ser::SerializeTuple::serialize_element(&mut ser, &(*#field_name as u32))?;
                    ::serde::ser::SerializeTuple::serialize_element(&mut ser, &())?;
                    ::serde::ser::SerializeTuple::end(ser)
                }
            }
        }
        VariantDiscriminant::Normal(discriminant) => {
            let serialize_element = generate_serialize_element(&field_bindings);
            quote! {
                Self::#variant_ident #field_pattern => {
                    let mut ser = serializer.serialize_tuple(2)?;
                    ::serde::ser::SerializeTuple::serialize_element(&mut ser, &((#discriminant) as u32))?;
                    ::serde::ser::SerializeTuple::serialize_element(&mut ser, #serialize_element)?;
                    ::serde::ser::SerializeTuple::end(ser)
                }
            }
        }
    }
}
