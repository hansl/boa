use proc_macro::TokenStream;
use quote::{quote, ToTokens};
use syn::parse::{Parse, ParseStream};
use syn::parse_macro_input;

#[derive(Debug, Clone)]
struct ClassArgs {
    name: Option<String>,
}

impl Parse for ClassArgs {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut name = None;

        while input.peek(syn::Ident) && input.peek2(syn::Token![=]) {
            let attr = input.parse::<syn::Ident>()?;

            match attr.to_string().as_str() {
                "name" => {
                    input.parse::<syn::Token![=]>()?;
                    let value = input.parse::<syn::LitStr>()?;
                    name = Some(value.value());
                }
                _ => return Err(syn::Error::new_spanned(attr, "Unknown attribute")),
            };
        }

        Ok(Self { name })
    }
}

struct ImplBlock {
    block: syn::ItemImpl,
}

impl Parse for ImplBlock {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let block = input.parse::<syn::ItemImpl>()?;
        if block.trait_.is_some() {
            return Err(syn::Error::new_spanned(block, "Cannot implement trait for class").to_compile_error().into());
        }
        Ok(Self { block })
    }
}

impl ImplBlock {
    fn name(&self) -> String {
        self.block.self_ty.to_token_stream().to_string()
    }
    
    fn constructor(&self) -> Option<&syn::ImplItemMethod> {
        self.block.items.iter().find_map(|item| {
            if let syn::ImplItem::Method(method) = item {
                if method.sig.ident == "new" {
                    Some(method)
                } else {
                    None
                }
            } else {
                None
            }
        })
    }
}

pub(crate) fn class(args: TokenStream, input: TokenStream) -> TokenStream {
    let attrs = parse_macro_input!(args as ClassArgs);

    eprintln!("{:?}", attrs);

    let impl_block = parse_macro_input!(input as ImplBlock);


    let name = syn::Ident::new(
        &attrs.name.unwrap_or_else(|| {
            impl_block.name()
        }),
        proc_macro2::Span::call_site(),
    );
    let name_str = syn::LitStr::new(&name.to_string(), proc_macro2::Span::call_site());

    quote! {
        #impl_block

        impl ::boa_engine::class::Class for #name {
            const NAME: &'static str = #name_str;

            const LENGTH: usize = 123;
        }
    }.into()
}
