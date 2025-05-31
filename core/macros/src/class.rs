use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use std::collections::BTreeMap;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::visit_mut::VisitMut;
use syn::{ImplItemFn, ItemImpl, Meta, MetaNameValue, Token, Type};

#[derive(Debug, Default)]
struct ClassVisitor {
    constructor: Option<TokenStream>,
    methods: BTreeMap<String, TokenStream>,
    getters: BTreeMap<String, TokenStream>,
    errors: Option<syn::Error>,
}

impl ClassVisitor {
    fn method(&mut self, span: impl Spanned, name: String, fn_: ImplItemFn) {
        if fn_.sig.asyncness.is_some() {
            self.error(span, "Async methods are not supported.");
            return;
        }

        if !fn_.sig.generics.params.is_empty() {
            self.error(span, "Generic methods are not supported.");
            return;
        }

        if fn_.sig.receiver().is_some() {}

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

    fn serialize_class_impl(&self, class_ty: &Type, class_name: &str) -> TokenStream2 {
        quote! {
            impl boa_engine::class::Class for #class_ty {
                const NAME: &'static str = #class_name;

                fn data_constructor(
                    new_target: &boa_engine::JsValue,
                    args: &[boa_engine::JsValue],
                    context: &mut boa_engine::Context
                ) -> boa_engine::JsResult<Self> {
                    todo!();
                }

                fn init(builder: &mut boa_engine::class::ClassBuilder) -> boa_engine::JsResult<()> {
                    todo!();
                }
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
            self.method(item.sig.span(), item.sig.ident.to_string(), item.clone());
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

    let name = match impl_.self_ty.as_ref() {
        Type::Path(pa) if pa.path.get_ident().is_some() => pa.path.get_ident().unwrap().to_string(),
        _ => {
            return syn::Error::new(impl_.span(), "Impossible to find the name of the class.")
                .to_compile_error()
                .into()
        }
    };

    let class_impl = visitor.serialize_class_impl(&impl_.self_ty, &name);

    quote! {
        #impl_

        #class_impl
    }
    .into()
}
