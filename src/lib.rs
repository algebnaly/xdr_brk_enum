use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Expr, Fields, Variant, parse_quote};

use crate::deser::generate_deserialization_branch;
use crate::ser::generate_match_arm;

mod deser;
mod ser;

#[derive(Clone)]
enum VariantDiscriminant {
    Normal(Expr),
    Default,
}

struct VariantInfo<'a> {
    discriminant: VariantDiscriminant,
    variant: &'a Variant,
}

fn calculate_variant_discriminants<'a>(
    variants: impl IntoIterator<Item = &'a Variant>,
) -> Result<Vec<VariantInfo<'a>>, String> {
    let mut result = Vec::new();
    let mut next_discriminant: Expr = parse_quote! { 0 };
    let mut has_default = false;
    for v in variants {
        let discriminant = if has_default_attribute(v) {
            if has_default {
                return Err("Only one default arm is allowed".to_string());
            }
            validate_default_arm_fields(&v.fields)?;
            has_default = true;
            VariantDiscriminant::Default
        } else {
            let current = match &v.discriminant {
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
            VariantDiscriminant::Normal(current)
        };

        result.push(VariantInfo {
            discriminant,
            variant: v,
        });
    }
    Ok(result)
}

fn has_default_attribute(variant: &Variant) -> bool {
    variant
        .attrs
        .iter()
        .any(|attr| attr.path().is_ident("default_arm"))
}

fn validate_default_arm_fields(fields: &Fields) -> Result<(), String> {
    match fields {
        Fields::Unnamed(unnamed) if unnamed.unnamed.len() == 1 => Ok(()),
        _ => Err("Default arms must have exactly one unnamed field of type u32".to_string()),
    }
}

#[proc_macro_derive(XDREnumSerialize, attributes(default_arm))]
pub fn derive_xdr_enum_serialize(input: TokenStream) -> TokenStream {
    let ast = syn::parse_macro_input!(input as DeriveInput);
    let name = &ast.ident;

    let variants = match &ast.data {
        Data::Enum(data) => data.variants.iter().collect::<Vec<_>>(),
        _ => {
            return syn::Error::new(
                ast.ident.span(),
                "XDREnumSerialize can only be derived for enums",
            )
            .to_compile_error()
            .into();
        }
    };

    let variant_infos = match calculate_variant_discriminants(variants) {
        Ok(v) => v,
        Err(e) => {
            return syn::Error::new(ast.ident.span(), e)
                .to_compile_error()
                .into();
        }
    };

    let match_arms = variant_infos.iter().map(generate_match_arm);

    let expanded = quote! {
        const _: () = {
            impl ::serde::Serialize for #name {
                fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
                where
                    S: ::serde::Serializer,
                {
                    match self {
                        #(#match_arms)*
                    }
                }
            }
        };
    };

    expanded.into()
}

#[proc_macro_derive(XDREnumDeserialize, attributes(default_arm))]
pub fn derive_xdr_enum_deserialize(input: TokenStream) -> TokenStream {
    let ast = syn::parse_macro_input!(input as DeriveInput);
    let name = &ast.ident;

    let variants = match &ast.data {
        Data::Enum(data) => data.variants.iter().collect::<Vec<_>>(),
        _ => {
            return syn::Error::new(
                ast.ident.span(),
                "XDREnumDeserialize can only be derived for enums",
            )
            .to_compile_error()
            .into();
        }
    };

    let variant_infos = match calculate_variant_discriminants(variants) {
        Ok(infos) => infos,
        Err(error) => {
            return syn::Error::new(ast.ident.span(), error)
                .to_compile_error()
                .into();
        }
    };

    let (normal_branches, default_branch): (Vec<_>, Vec<_>) = variant_infos
        .iter()
        .partition(|vi| matches!(vi.discriminant, VariantDiscriminant::Normal(_)));

    let normal_deserialization_branches = normal_branches
        .iter()
        .map(|vi| generate_deserialization_branch(vi, name));

    let default_handling = if let Some(default_variant) = default_branch.first() {
        generate_deserialization_branch(default_variant, name)
    } else {
        quote! {
            return Err(::serde::de::Error::custom(format!(
                "Unknown discriminant {} for enum {}",
                discriminant, stringify!(#name)
            )));
        }
    };

    let visitor_struct_defs = quote! {
        struct __Visitor;

        impl<'de> ::serde::de::Visitor<'de> for __Visitor {
            type Value = #name;

            fn expecting(&self, formatter: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
                formatter.write_str(concat!("enum ", stringify!(#name)))
            }

            fn visit_seq<A>(self, mut data: A) -> Result<Self::Value, A::Error>
            where
                A: ::serde::de::SeqAccess<'de>,
            {
                let discriminant: u32 = data.next_element()?
                    .ok_or_else(|| ::serde::de::Error::invalid_length(0, &self))?;

                #(#normal_deserialization_branches)*

                #default_handling
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
                    deserializer.deserialize_tuple(2, __Visitor)
                }
            }
        };
    };

    expanded.into()
}
