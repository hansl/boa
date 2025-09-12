use proc_macro2::{Ident, Span as Span2};
use quote::ToTokens;
use std::fmt::Display;
use std::str::FromStr;
use syn::ext::IdentExt;
use syn::spanned::Spanned;
use syn::{Attribute, Expr, ExprLit, Lit, MetaNameValue};

pub(crate) type SpannedResult<T> = Result<T, (Span2, String)>;

/// A function to make it easier to return error messages.
pub(crate) fn error<T>(span: &impl Spanned, message: impl Display) -> SpannedResult<T> {
    Err((span.span(), message.to_string()))
}

/// Look (and remove from AST) a `path` version of the attribute `boa`, e.g. `#[boa(something)]`.
pub(crate) fn take_path_attr(attrs: &mut Vec<Attribute>, name: &str) -> bool {
    if let Some((i, _)) = attrs
        .iter()
        .enumerate()
        .filter(|(_, a)| a.path().is_ident("boa"))
        .filter_map(|(i, a)| a.meta.require_list().ok().map(|nv| (i, nv)))
        .filter_map(|(i, m)| m.parse_args_with(Ident::parse_any).ok().map(|p| (i, p)))
        .find(|(_, path)| path == name)
    {
        attrs.remove(i);
        true
    } else {
        false
    }
}

/// Look (and remove from AST) for a `#[boa(rename = ...)]` attribute, where `...`
/// is a literal. The validation of the literal's type should be done separately.
pub(crate) fn take_name_value_attr(attrs: &mut Vec<Attribute>, name: &str) -> Option<Lit> {
    if let Some((i, lit)) = attrs
        .iter()
        .enumerate()
        .filter(|(_, a)| a.meta.path().is_ident("boa"))
        .filter_map(|(i, a)| a.meta.require_list().ok().map(|nv| (i, nv)))
        .filter_map(|(i, a)| {
            syn::parse2::<MetaNameValue>(a.tokens.to_token_stream())
                .ok()
                .map(|nv| (i, nv))
        })
        .filter(|(_, nv)| nv.path.is_ident(name))
        .find_map(|(i, nv)| match &nv.value {
            Expr::Lit(ExprLit { lit, .. }) => Some((i, lit.clone())),
            _ => None,
        })
    {
        attrs.remove(i);
        Some(lit)
    } else {
        None
    }
}

/// Take the length name-value from the list of attributes.
pub(crate) fn take_length_from_attrs(attrs: &mut Vec<Attribute>) -> SpannedResult<Option<usize>> {
    match take_name_value_attr(attrs, "length") {
        None => Ok(None),
        Some(lit) => match lit {
            Lit::Int(int) if int.base10_parse::<usize>().is_ok() => int
                .base10_parse::<usize>()
                .map(Some)
                .map_err(|e| (int.span(), format!("Invalid literal: {e}"))),
            l => error(&l, "Invalid literal type. Was expecting a number")?,
        },
    }
}

pub(crate) fn take_name_value_string(
    attrs: &mut Vec<Attribute>,
    name: &str,
) -> SpannedResult<Option<String>> {
    match take_name_value_attr(attrs, name) {
        None => Ok(None),
        Some(lit) => match lit {
            Lit::Str(s) => Ok(Some(s.value())),
            l => Err((
                l.span(),
                "Invalid literal type. Was expecting a string".to_string(),
            )),
        },
    }
}

/// Take the last `#[boa(error = "...")]` statement if found, remove it from the list
/// of attributes, and return the literal string.
pub(crate) fn take_error_from_attrs(attrs: &mut Vec<Attribute>) -> SpannedResult<Option<String>> {
    take_name_value_string(attrs, "error")
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) enum RenameScheme {
    #[default]
    None,
    CamelCase,
    PascalCase,
    ScreamingSnakeCase,
}

impl FromStr for RenameScheme {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.eq_ignore_ascii_case("none") {
            Ok(Self::None)
        } else if s.eq_ignore_ascii_case("camelcase") {
            Ok(Self::CamelCase)
        } else if s.eq_ignore_ascii_case("pascalcase") {
            Ok(Self::PascalCase)
        } else if s.eq_ignore_ascii_case("screamingsnakecase")
            || s.eq_ignore_ascii_case("screaming_snake_case")
        {
            Ok(Self::ScreamingSnakeCase)
        } else {
            Err(format!(
                r#"Invalid rename scheme: {s:?}. Accepted values are "none" or "camelCase"."#
            ))
        }
    }
}

impl RenameScheme {
    pub(crate) fn from_named_attrs(
        attrs: &mut Vec<Attribute>,
        name: &str,
    ) -> SpannedResult<Option<Self>> {
        match take_name_value_attr(attrs, name) {
            None => Ok(None),
            Some(Lit::Str(lit_str)) => Self::from_str(lit_str.value().as_str())
                .map_err(|e| (lit_str.span(), e))
                .map(Some),
            Some(lit) => Err((
                lit.span(),
                "Invalid attribute value literal, expected a string.".to_string(),
            )),
        }
    }

    pub(crate) fn rename(self, s: String) -> String {
        use convert_case::Casing;

        match self {
            Self::None => s,
            Self::CamelCase => s.to_case(convert_case::Case::Camel),
            Self::PascalCase => s.to_case(convert_case::Case::Pascal),
            Self::ScreamingSnakeCase => s.to_case(convert_case::Case::UpperSnake),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RenameScheme;
    use test_case::test_case;

    #[rustfmt::skip]
    #[test_case("HelloWorld", "helloWorld" ; "camel_case_1")]
    #[test_case("Hello_World", "helloWorld" ; "camel_case_2")]
    #[test_case("hello_world", "helloWorld" ; "camel_case_3")]
    #[test_case("__hello_world__", "helloWorld" ; "camel_case_4")]
    #[test_case("HELLOWorld", "helloWorld" ; "camel_case_5")]
    #[test_case("helloWORLD", "helloWorld" ; "camel_case_6")]
    #[test_case("HELLO_WORLD", "helloWorld" ; "camel_case_7")]
    #[test_case("hello_beautiful_world", "helloBeautifulWorld" ; "camel_case_8")]
    #[test_case("helloBeautifulWorld", "helloBeautifulWorld" ; "camel_case_9")]
    #[test_case("switch_to_term", "switchToTerm" ; "camel_case_10")]
    #[test_case("_a_b_c_", "aBC" ; "camel_case_11")]
    fn camel_case(input: &str, expected: &str) {
        assert_eq!(RenameScheme::CamelCase.rename(input.to_string()).as_str(), expected);
    }

    #[rustfmt::skip]
    #[test_case("HelloWorld", "HelloWorld" ; "pascal_case_1")]
    #[test_case("Hello_World", "HelloWorld" ; "pascal_case_2")]
    #[test_case("hello_world", "HelloWorld" ; "pascal_case_3")]
    #[test_case("__hello_world__", "HelloWorld" ; "pascal_case_4")]
    #[test_case("HELLOWorld", "HelloWorld" ; "pascal_case_5")]
    #[test_case("helloWORLD", "HelloWorld" ; "pascal_case_6")]
    #[test_case("HELLO_WORLD", "HelloWorld" ; "pascal_case_7")]
    fn pascal_case(input: &str, expected: &str) {
        assert_eq!(RenameScheme::PascalCase.rename(input.to_string()).as_str(), expected);
    }

    #[rustfmt::skip]
    #[test_case("HelloWorld", "HELLO_WORLD" ; "screaming_snake_case_1")]
    #[test_case("Hello_World", "HELLO_WORLD" ; "screaming_snake_case_2")]
    #[test_case("hello_world", "HELLO_WORLD" ; "screaming_snake_case_3")]
    #[test_case("__hello_world__", "HELLO_WORLD" ; "screaming_snake_case_4")]
    #[test_case("HELLOWorld", "HELLO_WORLD" ; "screaming_snake_case_5")]
    #[test_case("helloWORLD", "HELLO_WORLD" ; "screaming_snake_case_6")]
    #[test_case("HELLO_WORLD", "HELLO_WORLD" ; "screaming_snake_case_7")]
    fn screaming_snake_case(input: &str, expected: &str) {
        assert_eq!(RenameScheme::ScreamingSnakeCase.rename(input.to_string()).as_str(), expected);
    }
}
