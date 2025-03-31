//------------------------------------------------------------------------------
// Copyright (c) 2025 orgrinrt (orgrinrt@ikiuni.dev)
//                    Hiisi Digital Oy (contact@hiisi.digital)
// SPDX-License-Identifier: MPL-2.0
//------------------------------------------------------------------------------

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::token::Brace;
use syn::{
    braced, parse::Parse, parse::ParseStream, parse_macro_input, punctuated::Punctuated,
    GenericParam, Ident, ItemStruct, Path, Token,
};

struct ExtraGenerics {
    params: Punctuated<GenericParam, Token![,]>,
}

impl Parse for ExtraGenerics {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let params = Punctuated::<GenericParam, Token![,]>::parse_terminated(input)?;
        Ok(ExtraGenerics {
            params,
        })
    }
}

#[proc_macro_attribute]
pub fn with_generics(attrs: TokenStream, item: TokenStream) -> TokenStream {
    let extra: ExtraGenerics = parse_macro_input!(attrs as ExtraGenerics);
    let mut input_struct = parse_macro_input!(item as ItemStruct);
    let struct_name = &input_struct.ident;

    // Merge extra generics with struct's generics ensuring defaults come last
    let mut combined_generics = input_struct.generics.clone();

    // Collect params without defaults from both sources
    let mut params_without_defaults = input_struct
        .generics
        .params
        .iter()
        .filter(|p| !has_default(p))
        .cloned()
        .collect::<Vec<_>>();
    params_without_defaults.extend(extra.params.iter().filter(|p| !has_default(p)).cloned());

    // Collect params with defaults from both sources
    let mut params_with_defaults = input_struct
        .generics
        .params
        .iter()
        .filter(|p| has_default(p))
        .cloned()
        .collect::<Vec<_>>();
    params_with_defaults.extend(extra.params.iter().filter(|p| has_default(p)).cloned());

    // Clear and re-add params in correct order
    combined_generics.params.clear();
    for param in params_without_defaults {
        combined_generics.params.push(param);
    }
    for param in params_with_defaults {
        combined_generics.params.push(param);
    }

    input_struct.generics = combined_generics.clone();

    let generic_args: Vec<_> = combined_generics
        .params
        .iter()
        .map(|param| match param {
            GenericParam::Type(type_param) => {
                let ident = &type_param.ident;
                quote!(#ident)
            },
            GenericParam::Const(const_param) => {
                let ident = &const_param.ident;
                quote!(#ident)
            },
            GenericParam::Lifetime(lifetime_param) => {
                let lifetime = &lifetime_param.lifetime;
                quote!(#lifetime)
            },
        })
        .collect();

    let impl_for_trait_macro = impl_trait_macro_name(struct_name);
    let impl_for_self_macro = impl_self_macro_name(struct_name);
    let impl_generics = {
        let mut gen = combined_generics.clone();
        // Remove defaults from const parameters for impl blocks
        for param in &mut gen.params {
            if let GenericParam::Const(const_param) = param {
                const_param.default = None;
            }
        }
        gen
    };

    let module_name = format_ident!("__generics_impl_for_{}", struct_name);

    let output = quote! {
        #input_struct

        #[allow(non_snake_case)]
        #[doc(hidden)]
        mod #module_name {
            #[allow(non_snake_case)]
            #[macro_export]
            macro_rules! #impl_for_trait_macro {
                ($trait_path:path, $body:tt) => {
                    impl #impl_generics $trait_path for #struct_name<#(#generic_args),*> $body
                }
            }

            #[allow(non_snake_case)]
            #[macro_export]
            macro_rules! #impl_for_self_macro {
                ($body:tt) => {
                    impl #impl_generics #struct_name<#(#generic_args),*> $body
                }
            }

            // Re-export the main implementation macro
            pub use #impl_for_trait_macro;
            pub use #impl_for_self_macro;
        }
    };

    output.into()
}

struct ImplInput {
    trait_path: Option<syn::Path>,
    for_token: Token![for],
    struct_name: Ident,
    body: proc_macro2::TokenStream,
}

impl Parse for ImplInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        // Check if this is a Self impl or trait impl
        let lookahead = input.lookahead1();
        let trait_path = if lookahead.peek(syn::token::Brace) || lookahead.peek(Token![Self]) {
            None
        } else {
            Some(input.parse()?)
        };

        // Parse the 'for' token
        let for_token = input.parse()?;

        // Parse the struct name
        let struct_name: Ident = input.parse()?;

        // Parse the body
        let content;
        braced!(content in input);
        let body = content.parse()?;

        Ok(ImplInput {
            trait_path,
            for_token,
            struct_name,
            body,
        })
    }
}

struct MultiImplInput {
    impls: Vec<ImplItem>,
}

impl Parse for MultiImplInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut impls = Vec::new();

        while !input.is_empty() {
            impls.push(input.parse()?);

            // Allow optional semicolons between implementations
            let _ = input.parse::<Token![;]>();
        }

        Ok(MultiImplInput {
            impls,
        })
    }
}

enum ImplItem {
    TraitImpl {
        trait_path: Path,
        for_token: Token![for],
        struct_name: Ident,
        brace_token: Brace,
        body: proc_macro2::TokenStream,
    },
    SelfImpl {
        struct_name: Ident,
        brace_token: Brace,
        body: proc_macro2::TokenStream,
    },
}

impl Parse for ImplItem {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        // Try a lookahead to check if we have a "TRAIT for STRUCT" pattern
        // or just a "STRUCT" pattern
        let fork = input.fork();

        // Try to parse as if it's just a struct name followed by a brace
        let success = fork.parse::<Ident>().is_ok() && fork.peek(syn::token::Brace);

        if success {
            // It's a self impl
            let struct_name = input.parse()?;

            let content;
            let brace_token = braced!(content in input);
            let body = content.parse()?;

            return Ok(ImplItem::SelfImpl {
                struct_name,
                brace_token,
                body,
            });
        }

        // If not, it must be a trait impl
        let trait_path = input.parse()?;
        let for_token = input.parse()?;
        let struct_name = input.parse()?;

        let content;
        let brace_token = braced!(content in input);
        let body = content.parse()?;

        Ok(ImplItem::TraitImpl {
            trait_path,
            for_token,
            struct_name,
            brace_token,
            body,
        })
    }
}

#[proc_macro]
pub fn impl_(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as MultiImplInput);
    let mut expanded = proc_macro2::TokenStream::new();

    for impl_item in input.impls {
        match impl_item {
            ImplItem::TraitImpl {
                trait_path,
                struct_name,
                body,
                ..
            } => {
                let impl_trait_macro = impl_trait_macro_name(&struct_name);
                expanded.extend(quote! {
                    #impl_trait_macro!(#trait_path, { #body });
                });
            },
            ImplItem::SelfImpl {
                struct_name,
                body,
                ..
            } => {
                let impl_self_macro = impl_self_macro_name(&struct_name);
                expanded.extend(quote! {
                    #impl_self_macro!({ #body });
                });
            },
        }
    }

    expanded.into()
}

fn impl_trait_macro_name(struct_name: &Ident) -> Ident {
    format_ident!("__impl_for_{}_trait", struct_name)
}

fn impl_self_macro_name(struct_name: &Ident) -> Ident {
    format_ident!("__impl_for_{}_self", struct_name)
}

fn has_default(param: &GenericParam) -> bool {
    match param {
        GenericParam::Type(type_param) => type_param.default.is_some(),
        GenericParam::Const(const_param) => const_param.default.is_some(),
        GenericParam::Lifetime(_) => false,
    }
}
