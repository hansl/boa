use crate::utils::{take_name_value_attr, take_path_attr, RenameScheme};
use proc_macro::TokenStream;
use quote::quote;
use std::str::FromStr;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{Expr, Item, ItemImpl, ItemMod, Lit, Meta, MetaNameValue, Token, Type};

#[derive(Debug)]
struct ModuleArguments {
    name: Option<String>,
}

impl Parse for ModuleArguments {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let args: Punctuated<Meta, Token![,]> = Punctuated::parse_terminated(input)?;
        let mut name = None;

        for arg in &args {
            match arg {
                Meta::NameValue(MetaNameValue {
                    path,
                    value: Expr::Lit(lit),
                    ..
                }) if path.is_ident("name") => {
                    name = Some(match &lit.lit {
                        Lit::Str(s) => Ok(s.value()),
                        _ => Err(syn::Error::new(lit.span(), "Expected a string literal")),
                    }?);
                }
                _ => return Err(syn::Error::new(arg.span(), "Unrecognize argument.")),
            }
        }

        Ok(Self { name })
    }
}

pub(crate) fn module_impl(attr: TokenStream, input: TokenStream) -> TokenStream {
    // Parse the attribute arguments.
    let args = syn::parse_macro_input!(attr as ModuleArguments);

    // Parse the input.
    let mut mod_ = syn::parse_macro_input!(input as ItemMod);

    let renaming = match RenameScheme::from_attrs(&mut mod_.attrs) {
        Ok(r) => r,
        Err((span, msg)) => {
            return syn::Error::new(span, msg).to_compile_error().into();
        }
    };

    // Iterate through all top-level content. If the module is empty, still
    // iterate to create an empty JS module.
    let mut module_decl = quote! {};
    let mut module_fn = quote! {};

    for item in mod_.content.map_or_else(Vec::new, |c| c.1).as_mut_slice() {
        match item {
            Item::Const(c) => {}
            Item::Fn(f) => {}
            Item::Static(_) => {}
            Item::Use(_) => {}
            i => {
                return syn::Error::new(i.span(), "Invalid boa_module top-level item.")
                    .to_compile_error()
                    .into();
            }
        }

        module_decl = quote! {
            #module_decl
            #item
        }
    }

    let debug = take_path_attr(&mut impl_.attrs, "debug");

    let vis = mod_.vis;
    let name = mod_.ident;
    let attrs = mod_.attrs;
    let safety = mod_.unsafety;

    let module_name = args.name.unwrap_or_else(|| name.to_string());

    let tokens = quote! {
        #(#attrs)*
        #vis #safety mod #name {
            #module_decl

            fn boa_module_name() -> boa_engine::JsString {
                boa_engine::js_string!( #module_name )
            }

            fn boa_module() -> boa_engine::Module {
                let module = boa_engine::Module::new();
            }
            #module_fn
        }
    };

    #[allow(clippy::print_stderr)]
    if debug {
        eprintln!("---------\n{tokens}\n---------\n");
    }

    tokens.into()
}
