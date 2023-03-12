#![feature(error_generic_member_access)]
#![feature(provide_any)]

use proc_macro::TokenStream;

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::parse_macro_input;
use syn::DeriveInput;

#[proc_macro_derive(JsonLoadable)]
pub fn derive_json_loadable(input: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(input as DeriveInput);
    expand_derive_json_loadable(&mut input)
        .unwrap_or_else(to_compile_errors)
        .into()
}

#[proc_macro_derive(TomlLoadable)]
pub fn derive_toml_loadable(input: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(input as DeriveInput);
    expand_derive_toml_loadable(&mut input)
        .unwrap_or_else(to_compile_errors)
        .into()
}

#[proc_macro_derive(JsonSavable)]
pub fn derive_json_savable(input: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(input as DeriveInput);
    expand_derive_json_savable(&mut input)
        .unwrap_or_else(to_compile_errors)
        .into()
}

#[proc_macro_derive(TomlSavable)]
pub fn derive_toml_savable(input: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(input as DeriveInput);
    expand_derive_toml_savable(&mut input)
        .unwrap_or_else(to_compile_errors)
        .into()
}

fn to_compile_errors(errors: Vec<syn::Error>) -> proc_macro2::TokenStream {
    let compile_errors = errors.iter().map(syn::Error::to_compile_error);
    quote!(#(#compile_errors)*)
}

fn expand_derive_json_loadable(input: &mut DeriveInput) -> Result<TokenStream2, Vec<syn::Error>> {
    let name = &input.ident;
    let gen = quote! {
        impl crate::data::Loadable<Self> for #name {
            fn from_str(s: &str) -> crate::error::Result<Self> {
                Ok(serde_json::from_str(s).map_err(crate::error::DeserializedError::from)?)
            }
        }
    };
    Ok(gen)
}

fn expand_derive_toml_loadable(input: &mut DeriveInput) -> Result<TokenStream2, Vec<syn::Error>> {
    let name = &input.ident;
    let gen = quote! {
        impl crate::data::Loadable<Self> for #name {
            fn from_str(s: &str) -> crate::error::Result<Self> {
                Ok(toml::from_str(s).map_err(crate::error::DeserializedError::from)?)
            }
        }
    };
    Ok(gen)
}

fn expand_derive_toml_savable(input: &mut DeriveInput) -> Result<TokenStream2, Vec<syn::Error>> {
    let name = &input.ident;
    let gen = quote! {
        impl crate::data::Savable for #name {
            fn to_string(&self) -> crate::error::Result<String> {
                Ok(toml::to_string(self).map_err(crate::error::SerializedError::from)?)
            }
        }
    };
    Ok(gen)
}

fn expand_derive_json_savable(input: &mut DeriveInput) -> Result<TokenStream2, Vec<syn::Error>> {
    let name = &input.ident;
    let gen = quote! {
        impl crate::data::Savable for #name {
            fn to_string(&self) -> crate::error::Result<String> {
                Ok(serde_json::to_string(self).map_err(crate::error::SerializedError::from)?)
            }
        }
    };
    Ok(gen)
}
