use proc_macro::TokenStream;
use quote::quote;
use std::collections::BTreeMap;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::visit_mut::VisitMut;
use syn::{ImplItemFn, ItemImpl, Meta, MetaNameValue, Token};

#[derive(Debug, Default)]
struct ClassVisitor {
    constructor: Option<TokenStream>,
    methods: BTreeMap<String, TokenStream>,
    getters: BTreeMap<String, TokenStream>,
    errors: Option<syn::Error>,
}

impl ClassVisitor {
    fn method(&mut self, name: String, fn_: ImplItemFn) {
        let has_self = fn_.sig.receiver().is_some();
        self.methods.insert(name, quote! { #fn_ }.into());
    }

    fn getter(&mut self, name: String, fn_: ImplItemFn) {
        self.getters.insert(name, quote! { #fn_ }.into());
    }

    fn constructor(&mut self, name: String, fn_: ImplItemFn) {
        self.constructor = Some(quote! { #fn_ }.into());
    }

    fn error(&mut self, node: impl Spanned, message: impl std::fmt::Display) {
        let error = syn::Error::new(node.span(), message);

        match &mut self.errors {
            None => {
                self.errors = Some(error);
            }
            Some(e) => {
                e.combine(error);
            }
        }
    }
}

impl VisitMut for ClassVisitor {
    fn visit_impl_item_fn_mut(&mut self, item: &mut ImplItemFn) {
        // If there's a `boa` argument, parse it.
        let mut i = 0;

        let mut is_ctor = false;
        let mut is_getter = false;
        let mut is_method = true;

        while i < item.attrs.len() {
            let attr = &item.attrs[i];

            let attrs = match &attr.meta {
                Meta::List(list) if list.path.is_ident("boa") => {
                    list.parse_args_with(Punctuated::<Meta, Token![=]>::parse_terminated)
                }
                _ => {
                    i += 1;

                    continue;
                }
            };

            let Ok(attrs) = attrs else {
                self.error(attr, "Cannot parse arguments.");
                item.attrs.remove(i);
                continue;
            };

            let has_ctor_attr = attrs.iter().any(|i| i.path().is_ident("constructor"));
            let has_getter_attr = attrs.iter().any(|i| i.path().is_ident("getter"));
            let has_method_attr = attrs.iter().any(|i| i.path().is_ident("method"));

            if has_ctor_attr && is_ctor {
                self.error(attr, "Constructor already declared.");
                continue;
            }

            is_ctor = is_ctor || has_ctor_attr;
            is_getter = is_getter || has_getter_attr;
            is_method = is_method || has_method_attr;

            item.attrs.remove(i);
        }

        if is_getter {
            self.getter(item.sig.ident.to_string(), item.clone());
        } else if is_ctor {
            self.constructor(item.sig.ident.to_string(), item.clone());
        } else if is_method {
            self.method(item.sig.ident.to_string(), item.clone());
        }

        syn::visit_mut::visit_impl_item_fn_mut(self, item);
    }
}

#[derive(Debug)]
struct ClassArguments {}

impl Parse for ClassArguments {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let args: Punctuated<MetaNameValue, Token![,]> = Punctuated::parse_terminated(input)?;

        Ok(Self {})
    }
}

pub(crate) fn class_impl(attr: TokenStream, input: TokenStream) -> TokenStream {
    // Parse the attribute arguments.
    let args = syn::parse_macro_input!(attr as ClassArguments);

    // Parse the input.
    let mut impl_ = syn::parse_macro_input!(input as ItemImpl);

    // Get all methods from the input.
    let mut visitor = ClassVisitor::default();
    syn::visit_mut::visit_item_impl_mut(&mut visitor, &mut impl_);

    if let Some(err) = visitor.errors {
        return err.to_compile_error().into();
    };

    eprintln!("methods: {:#?}", visitor.methods);

    eprintln!("impl: {impl_:?}");

    quote! { #impl_ }.into()
}
