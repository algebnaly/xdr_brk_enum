use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_quote, Data, DeriveInput, Expr, Fields, Ident, Lifetime, Type, Variant};

fn calculate_variants_with_discriminants<'a>(variants: &[&'a Variant]) -> Vec<(Expr, &'a Variant)> {
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
        _ => panic!("Only enums are supported"),
    };

    let variants_with_discriminants = calculate_variants_with_discriminants(&variants);

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
                    let field_types = fields.named.iter().map(|f| &f.ty);

                    let helper_struct_name = format_ident!("__{}_{}Payload", name, variant_ident);
                    let lifetime = Lifetime::new("'a", proc_macro2::Span::call_site());
                    // TODO: can we remove lifetime parameter?

                    quote! {
                        Self::#variant_ident { #(#field_names,)* } => {
                            #[allow(non_camel_case_types)]
                            #[doc(hidden)]
                            #[derive(::serde::Serialize)]
                            struct #helper_struct_name<#lifetime> {
                                #( pub #field_names: &'a #field_types, )*
                            }

                            let payload = #helper_struct_name {
                                #(#field_names,)*
                            };

                            #base_serialization
                            ::serde::ser::SerializeTuple::serialize_element(&mut ser, &payload)?;
                            ::serde::ser::SerializeTuple::end(ser)
                        }
                    }
                }
            }
        });

    let expanded = quote! {
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
    expanded.into()
}

#[proc_macro_derive(XDREnumDeserialize)]
pub fn derive_xdr_enum_deserialize(input: TokenStream) -> TokenStream {
    let ast = syn::parse_macro_input!(input as DeriveInput);
    let name = &ast.ident;
    let variants = match &ast.data {
        Data::Enum(data) => data.variants.iter().collect::<Vec<&Variant>>(),
        _ => panic!("Only enums are supported"),
    };

    let variants_with_discriminants: Vec<(Expr, &Variant)> =
        calculate_variants_with_discriminants(&variants);
    let variants_ident = variants
        .iter()
        .map(|variant| &variant.ident)
        .collect::<Vec<&Ident>>();
    let variants_str: Vec<String> = variants_ident
        .iter()
        .map(|ident| ident.to_string())
        .collect();

    let deserialization_branches = variants_with_discriminants.iter().map(|(discriminant, variant)| {
            let variant_ident = &variant.ident;
            let variant_body = match &variant.fields {
                Fields::Unit => {
                    quote! {
                        ::serde::de::VariantAccess::unit_variant(v)?;
                        Ok(#name::#variant_ident)
                    }
                }
                Fields::Unnamed(fields) => {
                    let fields_len = fields.unnamed.len();
                    let field_vars: Vec<Ident> = (0..fields_len)
                        .map(|i| format_ident!("field_{}", i))
                        .collect();
                    quote! {
                        #[doc(hidden)]
                        struct __TupleVisitor;
                        impl<'de> ::serde::de::Visitor<'de> for __TupleVisitor {
                            type Value = #name;

                            fn expecting(&self, formatter: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
                                formatter.write_str(concat!("a tuple variant ", stringify!(#name::#variant_ident)))
                            }

                            fn visit_seq<A>(self, mut data: A) -> Result<Self::Value, A::Error>
                            where
                                A: ::serde::de::SeqAccess<'de>,
                            {
                                #(
                                    let #field_vars = data.next_element()?.ok_or_else(|| ::serde::de::Error::custom(concat!("missing field ", stringify!(#field_vars))))?;
                                )*
                                Ok(#name::#variant_ident(#(#field_vars),*))
                            }
                        }
                        ::serde::de::VariantAccess::tuple_variant(v, #fields_len, __TupleVisitor{})
                    }
                }
                Fields::Named(fields) => {
                    let field_names: Vec<&Ident> = fields
                        .named
                        .iter()
                        .map(|f| f.ident.as_ref().unwrap())
                        .collect();
                    let field_types: Vec<&Type> = fields
                        .named
                        .iter()
                        .map(|f| &f.ty)
                        .collect();
                    let field_names_str: Vec<String> = field_names.iter().map(|f| f.to_string()).collect();
                    
                    
                    quote!{
                        #[doc(hidden)]
                        struct __StructVisitor{};
                        impl<'de> ::serde::de::Visitor<'de> for __StructVisitor {
                            type Value = #name;
                            
                            fn expecting(&self, formatter: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
                                formatter.write_str(concat!("a tuple variant ", stringify!(#name::#variant_ident)))
                            }
                            
                            fn visit_seq<V>(self, mut data: V) -> Result<Self::Value, V::Error>
                            where
                                V: ::serde::de::SeqAccess<'de>,
                            {
                                #( let #field_names = data.next_element::<#field_types>()?.ok_or_else(|| ::serde::de::Error::custom(concat!("missing field ", stringify!(#field_names))))?; )* 
                                Ok(#name::#variant_ident{#(#field_names),*})
                            }
                        }
                        ::serde::de::VariantAccess::struct_variant(v, &[#(#field_names_str),*], __StructVisitor{})
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
        #[doc(hidden)]
        struct __Visitor{}
        impl<'de> ::serde::de::Visitor<'de> for __Visitor {
            type Value = #name;

            fn expecting(&self, formatter: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
                formatter.write_str(concat!("a enum ", stringify!(#name)))
            }

            fn visit_enum<A>(self, data: A) -> Result<Self::Value, A::Error>
            where
                A: ::serde::de::EnumAccess<'de>,
            {
                let (discriminant, v): (u32, <A as ::serde::de::EnumAccess<'de>>::Variant) = data.variant()?;
                #(#deserialization_branches)*
                Err(::serde::de::Error::custom(format!("unknown variant discriminant {}", discriminant)))
            }
        }
    };
    let expanded = quote! {
        #visitor_struct_defs
        impl<'de> ::serde::Deserialize<'de> for #name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: ::serde::Deserializer<'de>,
            {
                deserializer.deserialize_enum(stringify!(#name), &[#(#variants_str),*], __Visitor{})
            }
        }
    };
    expanded.into()
}
