use crate::utils::{take_path_attr, RenameScheme};
use proc_macro::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::spanned::Spanned;
use syn::{Item, ItemMod};

#[derive(Debug)]
struct ModuleArguments {}

impl Parse for ModuleArguments {
    fn parse(_input: ParseStream<'_>) -> syn::Result<Self> {
        Ok(Self {})
    }
}

pub(crate) fn module_impl(attr: TokenStream, input: TokenStream) -> TokenStream {
    // Parse the attribute arguments.
    let _args = syn::parse_macro_input!(attr as ModuleArguments);

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
    let mut original_module_decl = quote! {};
    let mut module_fn = quote! {};
    let mut module_exports = quote! {};

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

        original_module_decl = quote! {
            #original_module_decl
            #item
        }
    }

    let debug = take_path_attr(&mut mod_.attrs, "debug");

    let vis = mod_.vis;
    let name = mod_.ident;
    let attrs = mod_.attrs;
    let safety = mod_.unsafety;

    let tokens = quote! {
        #(#attrs)*
        #vis #safety mod #name {
            #original_module_decl

            fn boa_module() -> boa_engine::Module {
                let module = boa_engine::Module::synthetic(
                    #module_exports,

                );

                #module_fn

                module
            }
        }
    };

    #[allow(clippy::print_stderr)]
    if debug {
        eprintln!("---------\n{tokens}\n---------\n");
    }

    tokens.into()
}
