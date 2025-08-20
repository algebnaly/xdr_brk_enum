use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Expr, Fields, Ident, Type, Variant, parse_quote};

fn calculate_variants_with_discriminants<'a>(
    variants: impl IntoIterator<Item = &'a Variant>,
) -> Vec<(Expr, &'a Variant)> {
    let mut variants_with_discriminants: Vec<(Expr, &Variant)> = Vec::new();
    let mut next_discriminant: Expr = parse_quote! { 0 };
    for v in variants {
        let current_discriminant = match &v.discriminant {
            Some((_, expr)) => {
                next_discriminant = parse_quote! { (#expr + 1) };
                expr.clone()
            }
            None => {
                let current = next_discriminant.clone();
                next_discriminant = parse_quote! { (#current + 1) };
                current
            }
        };
        variants_with_discriminants.push((current_discriminant, v));
    }
    variants_with_discriminants
}

#[proc_macro_derive(XDREnumSerialize)]
pub fn derive_xdr_enum_serialize(input: TokenStream) -> TokenStream {
    let ast = syn::parse_macro_input!(input as DeriveInput);
    let name = &ast.ident;
    let variants = match &ast.data {
        Data::Enum(data) => data.variants.iter().collect::<Vec<&Variant>>(),
        _ => {
            let error_span = ast.ident.span();
            return syn::Error::new(error_span, "XDREnum can only be derived for enums")
                .to_compile_error()
                .into();
        }
    };

    let variants_with_discriminants = calculate_variants_with_discriminants(variants);

    let match_arms = variants_with_discriminants
        .iter()
        .map(|(discriminant, variant)| {
            let variant_ident = &variant.ident;

            let base_serialization = quote! {
                let mut ser = serializer.serialize_tuple(2)?;
                ::serde::ser::SerializeTuple::serialize_element(&mut ser, &((#discriminant) as u32))?;
            };

            match &variant.fields {
                Fields::Unit => {
                    quote! {
                        Self::#variant_ident => {
                            #base_serialization
                            ::serde::ser::SerializeTuple::serialize_element(&mut ser, &())?;
                            ::serde::ser::SerializeTuple::end(ser)
                        }
                    }
                }
                Fields::Unnamed(fields) => {
                    let num_fields = fields.unnamed.len();
                    let field_bindings: Vec<Ident> = (0..num_fields)
                        .map(|i| format_ident!("field_{}", i))
                        .collect();

                    quote! {
                        Self::#variant_ident( #(#field_bindings,)* ) => {
                            #base_serialization
                            ::serde::ser::SerializeTuple::serialize_element(&mut ser, &(#(#field_bindings,)*))?;
                            ::serde::ser::SerializeTuple::end(ser)
                        }
                    }
                }
                Fields::Named(fields) => {
                    let field_names = fields
                        .named
                        .iter()
                        .filter_map(
                            |f| f.ident.as_ref(), // all fields are named, so we can safely use filter_map here
                        )
                        .collect::<Vec<&Ident>>();
                    quote! {
                        Self::#variant_ident { #(#field_names),* } => {
                            #base_serialization
                            ::serde::ser::SerializeTuple::serialize_element(&mut ser, &(#(#field_names),*))?;
                            ::serde::ser::SerializeTuple::end(ser)
                        }
                    }
                }
            }
        });

    let expanded = quote! {
        const _: () = {
            impl ::serde::Serialize for #name{
                fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
                where
                    S: ::serde::Serializer,
                {
                    match self {
                        #(#match_arms,)*
                    }
                }
            }
        };
    };
    expanded.into()
}

#[proc_macro_derive(XDREnumDeserialize)]
pub fn derive_xdr_enum_deserialize(input: TokenStream) -> TokenStream {
    let ast = syn::parse_macro_input!(input as DeriveInput);
    let name = &ast.ident;
    let variants = match &ast.data {
        Data::Enum(data) => data.variants.iter().collect::<Vec<&Variant>>(),
        _ => {
            let error_span = ast.ident.span();
            return syn::Error::new(error_span, "XDREnum can only be derived for enums")
                .to_compile_error()
                .into();
        }
    };

    let variants_with_discriminants: Vec<(Expr, &Variant)> =
        calculate_variants_with_discriminants(variants.iter().copied());

    let deserialization_branches =
        variants_with_discriminants
            .iter()
            .map(|(discriminant, variant)| {
                let variant_ident = &variant.ident;
                let variant_body = match &variant.fields {
                    Fields::Unit => {
                        quote! {
                            let _ = data.next_element::<()>()?
                                    .ok_or_else(|| ::serde::de::Error::invalid_length(1, &self))?;
                            Ok(#name::#variant_ident)
                        }
                    }
                    Fields::Unnamed(fields) => {
                        let field_types = fields.unnamed.iter().map(|f| &f.ty);
                        let indices = 0..fields.unnamed.len();
                        quote! {
                                let fields = data.next_element::<(#( #field_types, )*)>()?
                                    .ok_or_else(|| ::serde::de::Error::invalid_length(1, &self))?;
                                Ok(#name::#variant_ident( #( fields.#indices, )* ))
                        }
                    }
                    Fields::Named(fields) => {
                        let field_names: Vec<&Ident> = fields
                            .named
                            .iter()
                            .filter_map(|f| f.ident.as_ref()) // filter_map is ok here, since we know that this is Named branch
                            .collect();
                        let field_types: Vec<&Type> = fields.named.iter().map(|f| &f.ty).collect();

                        let indices = 0..field_names.len();

                        quote! {
                                let fields = data.next_element::<(#( #field_types, )*)>()?
                                    .ok_or_else(|| ::serde::de::Error::invalid_length(1, &self))?;

                                Ok(#name::#variant_ident {
                                    #( #field_names: fields.#indices, )*
                                })
                        }
                    }
                };

                quote! {
                    if discriminant == (#discriminant) as u32 {
                        return {#variant_body};
                    }
                }
            });

    let visitor_struct_defs = quote! {
        struct __Visitor{}
        impl<'de> ::serde::de::Visitor<'de> for __Visitor {
            type Value = #name;

            fn expecting(&self, formatter: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
                formatter.write_str(concat!("a enum ", stringify!(#name)))
            }

            fn visit_seq<A>(self, mut data: A) -> Result<Self::Value, A::Error>
            where
                A: ::serde::de::SeqAccess<'de>,
            {
                let discriminant: u32 = data.next_element()?
                                            .ok_or_else(|| ::serde::de::Error::invalid_length(0, &self))?;

                #(#deserialization_branches)*
                Err(::serde::de::Error::custom(format!(
                    "unknown discriminant {} for enum {}",
                    discriminant, stringify!(#name)
                )))
            }
        }
    };

    let expanded = quote! {
        const _: () = {
            #visitor_struct_defs
            impl<'de> ::serde::Deserialize<'de> for #name {
                fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
                where
                    D: ::serde::Deserializer<'de>,
                {
                    deserializer.deserialize_tuple(2, __Visitor{})
                }
            }
        };
    };
    expanded.into()
}
