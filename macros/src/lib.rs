use std::collections::HashSet;

use proc_macro::{Span, TokenStream};
use quote::quote;
use syn::{parse_macro_input, parse_quote, Data, DeriveInput, Error, Fields, Ident};
use syn::meta::parser as meta_parser;
use syn::spanned::Spanned;

#[proc_macro_attribute]
pub fn render_node(args: TokenStream, input: TokenStream) -> TokenStream {
    // Process args.

    let mut struct_name_opt: Option<Ident> = None; 
    let mut field_name_opt: Option<Ident> = None;
    let mut in_names: Vec<Ident> = Vec::new();
    let mut out_names: Vec<Ident> = Vec::new();

    let args_parser = meta_parser(|meta| {
        if meta.path.is_ident("struct") {
            struct_name_opt = Some(meta.value()?.parse()?);
            Ok(())
        } else if meta.path.is_ident("field") {
            field_name_opt = Some(meta.value()?.parse()?);
            Ok(())
        } else if meta.path.is_ident("in") {
            meta.parse_nested_meta(|meta| {
                in_names.push(meta.path.require_ident()?.clone());
                Ok(())
            })
        } else if meta.path.is_ident("out") {
            meta.parse_nested_meta(|meta| {
                out_names.push(meta.path.require_ident()?.clone());
                Ok(())
            })
        } else {
            Err(meta.error("unsupported attribute"))
        }
    });

    parse_macro_input!(args with args_parser);

    let struct_name = match struct_name_opt {
        Some(struct_name) => struct_name,
        None => return Error::new(Span::call_site().into(), "struct attribute is mandatory").into_compile_error().into(),
    };

    let field_name = match field_name_opt {
        Some(field_name) => field_name,
        None => return Error::new(Span::call_site().into(), "field attribute is mandatory").into_compile_error().into(),
    };

    if out_names.is_empty() {
        return Error::new(Span::call_site().into(), "at least one output is mandatory").into_compile_error().into();
    }

    let mut in_out_names = HashSet::new();
    in_out_names.extend(&in_names);
    in_out_names.extend(&out_names);

    if in_out_names.len() != (in_names.len() + out_names.len()) {
        return Error::new(Span::call_site().into(), "duplicated names are not allowed in in/out").into_compile_error().into();
    }

    // Process node. Use full names (e.g. crate::render::*) to reference
    // types.

    let mut node = parse_macro_input!(input as DeriveInput);
    let node_name = &node.ident;

    match node.data {
        Data::Struct(ref mut data) => {
            match data.fields {
                Fields::Named(ref mut fields) => {
                    let field = parse_quote! {
                        #field_name: #struct_name
                    };

                    fields.named.push(field);
                },
                _ => return Error::new(node.span(), "struct with named fields expected").into_compile_error().into(),
            }
        },
        _ => return Error::new(node.span(), "struct expected").into_compile_error().into(),
    };

    let struct_fields_it = 
        in_names.iter()
            .map(|in_name| quote! { #in_name: crate::render::RenderTextureIn })
        .chain(out_names.iter()
            .map(|out_name| quote! { #out_name: crate::render::RenderTextureOut }));

    let in_names_it = in_names.iter().map(|in_name| in_name.to_string());

    let in_out_fields_it = 
        in_names.iter()
            .map(|in_name| {
                let in_name_s = in_name.to_string();
                let err = format!("Input {} is unconnected", in_name_s);
                quote! { #in_name: ins.remove(#in_name_s).unwrap().expect(#err) }
            })
        .chain(out_names.iter()
            .map(|out_name| quote! { #out_name: crate::render::RenderTextureOut::new() }));

    let outs_it = out_names.iter()
        .map(|out_name| {
            let out_name_s = out_name.to_string();
            quote! { #out_name_s => &self.#field_name.#out_name }
        });

    // Construct output.

    quote! {
        #node

        pub struct #struct_name {
            #(#struct_fields_it),*
        }

        impl crate::render::RenderNodeInOut for #node_name {
            type InOut = #struct_name;

            fn get_in_names() -> &'static [&'static str] {
                &[#(#in_names_it),*]
            }

            fn build(mut ins: crate::render::RenderTextureNamedInputs) -> Self::InOut {
                #struct_name {
                    #(#in_out_fields_it),*
                }
            }

            fn get_out(&self, out_name: &str) -> &crate::render::RenderTextureOut {
                match out_name {
                    #(#outs_it),*,
                    _ => panic!("No such output"),
                }
            }
        }
    }.into()
}
