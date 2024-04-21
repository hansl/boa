//! This module contains the [`TryFromJs`] trait, and conversions to basic Rust types.

use num_bigint::BigInt;
use rustc_hash::FxHashMap;
use std::hash::Hash;

use crate::object::JsMap;
use crate::{
    js_string, Context, JsArgs, JsBigInt, JsError, JsNativeError, JsObject, JsResult, JsString,
    JsValue,
};

/// This trait adds a fallible and efficient conversions from a [`JsValue`] to Rust types.
pub trait TryFromJs: Sized {
    /// This function tries to convert a JavaScript value into `Self`.
    fn try_from_js(value: &JsValue, context: &mut Context) -> JsResult<Self>;
}

impl JsValue {
    /// This function is the inverse of [`TryFromJs`]. It tries to convert a [`JsValue`] to a given
    /// Rust type.
    pub fn try_js_into<T>(&self, context: &mut Context) -> JsResult<T>
    where
        T: TryFromJs,
    {
        T::try_from_js(self, context)
    }
}

impl TryFromJs for bool {
    fn try_from_js(value: &JsValue, _context: &mut Context) -> JsResult<Self> {
        match value {
            JsValue::Boolean(b) => Ok(*b),
            _ => Err(JsNativeError::typ()
                .with_message("cannot convert value to a boolean")
                .into()),
        }
    }
}

impl TryFromJs for String {
    fn try_from_js(value: &JsValue, _context: &mut Context) -> JsResult<Self> {
        match value {
            JsValue::String(s) => s.to_std_string().map_err(|e| {
                JsNativeError::typ()
                    .with_message(format!("could not convert JsString to Rust string: {e}"))
                    .into()
            }),
            _ => Err(JsNativeError::typ()
                .with_message("cannot convert value to a String")
                .into()),
        }
    }
}

impl TryFromJs for JsString {
    fn try_from_js(value: &JsValue, _context: &mut Context) -> JsResult<Self> {
        match value {
            JsValue::String(s) => Ok(s.clone()),
            _ => Err(JsNativeError::typ()
                .with_message("cannot convert value to a String")
                .into()),
        }
    }
}

impl<T> TryFromJs for Option<T>
where
    T: TryFromJs,
{
    fn try_from_js(value: &JsValue, context: &mut Context) -> JsResult<Self> {
        match value {
            JsValue::Null | JsValue::Undefined => Ok(None),
            value => Ok(Some(T::try_from_js(value, context)?)),
        }
    }
}

impl<T> TryFromJs for Vec<T>
where
    T: TryFromJs,
{
    fn try_from_js(value: &JsValue, context: &mut Context) -> JsResult<Self> {
        let JsValue::Object(object) = value else {
            return Err(JsNativeError::typ()
                .with_message("cannot convert value to a Vec")
                .into());
        };

        let length = object
            .get(js_string!("length"), context)?
            .to_length(context)?;
        let length = match usize::try_from(length) {
            Ok(length) => length,
            Err(e) => {
                return Err(JsNativeError::typ()
                    .with_message(format!("could not convert length to usize: {e}"))
                    .into());
            }
        };
        let mut vec = Vec::with_capacity(length);
        for i in 0..length {
            let value = object.get(i, context)?;
            vec.push(T::try_from_js(&value, context)?);
        }

        Ok(vec)
    }
}

fn fx_hash_from_map<K: TryFromJs + Eq + Hash, V: TryFromJs>(
    map: &JsMap,
    context: &mut Context,
) -> JsResult<FxHashMap<K, V>> {
    let mut result = FxHashMap::default();

    let it = map.entries(context)?;
    loop {
        let item = it.next(context)?;
        let inner = item.create_list_from_array_like(&[], context)?;

        let k: K = K::try_from_js(inner.get_or_undefined(0), context)?;
        let v: V = V::try_from_js(inner.get_or_undefined(1), context)?;

        result.insert(k, v);
    }
    Ok(result)
}

fn fx_hash_from_properties<K: TryFromJs + Eq + Hash, V: TryFromJs>(
    object: &JsObject,
    context: &mut Context,
) -> JsResult<FxHashMap<K, V>> {
    let properties = object.__own_property_keys__(context)?;
    let mut result = FxHashMap::default();

    for key in properties {
        let value = object.get(key.clone(), context)?;
        let k = K::try_from_js(&key.into(), context)?;
        let v = V::try_from_js(&value, context)?;

        result.insert(k, v);
    }

    Ok(result)
}

impl<K: TryFromJs + Eq + Hash, V: TryFromJs> TryFromJs for FxHashMap<K, V> {
    fn try_from_js(value: &JsValue, context: &mut Context) -> JsResult<Self> {
        let o = value.as_object().ok_or_else(|| {
            JsError::from_native(JsNativeError::typ().with_message("Value must be an object"))
        })?;

        match JsMap::from_object(o.clone()) {
            Ok(map) => fx_hash_from_map(&map, context),
            Err(_e) => fx_hash_from_properties(o, context),
        }
    }
}

impl TryFromJs for JsObject {
    fn try_from_js(value: &JsValue, _context: &mut Context) -> JsResult<Self> {
        match value {
            JsValue::Object(o) => Ok(o.clone()),
            _ => Err(JsNativeError::typ()
                .with_message("cannot convert value to a Object")
                .into()),
        }
    }
}

impl TryFromJs for JsBigInt {
    fn try_from_js(value: &JsValue, _context: &mut Context) -> JsResult<Self> {
        match value {
            JsValue::BigInt(b) => Ok(b.clone()),
            _ => Err(JsNativeError::typ()
                .with_message("cannot convert value to a BigInt")
                .into()),
        }
    }
}

impl TryFromJs for BigInt {
    fn try_from_js(value: &JsValue, _context: &mut Context) -> JsResult<Self> {
        match value {
            JsValue::BigInt(b) => Ok(b.as_inner().clone()),
            _ => Err(JsNativeError::typ()
                .with_message("cannot convert value to a BigInt")
                .into()),
        }
    }
}

impl TryFromJs for JsValue {
    fn try_from_js(value: &JsValue, _context: &mut Context) -> JsResult<Self> {
        Ok(value.clone())
    }
}

impl TryFromJs for f64 {
    fn try_from_js(value: &JsValue, _context: &mut Context) -> JsResult<Self> {
        match value {
            JsValue::Integer(i) => Ok((*i).into()),
            JsValue::Rational(r) => Ok(*r),
            _ => Err(JsNativeError::typ()
                .with_message("cannot convert value to a f64")
                .into()),
        }
    }
}

impl TryFromJs for i8 {
    fn try_from_js(value: &JsValue, _context: &mut Context) -> JsResult<Self> {
        match value {
            JsValue::Integer(i) => (*i).try_into().map_err(|e| {
                JsNativeError::typ()
                    .with_message(format!("cannot convert value to a i8: {e}"))
                    .into()
            }),
            _ => Err(JsNativeError::typ()
                .with_message("cannot convert value to a i8")
                .into()),
        }
    }
}

impl TryFromJs for u8 {
    fn try_from_js(value: &JsValue, _context: &mut Context) -> JsResult<Self> {
        match value {
            JsValue::Integer(i) => (*i).try_into().map_err(|e| {
                JsNativeError::typ()
                    .with_message(format!("cannot convert value to a u8: {e}"))
                    .into()
            }),
            _ => Err(JsNativeError::typ()
                .with_message("cannot convert value to a u8")
                .into()),
        }
    }
}

impl TryFromJs for i16 {
    fn try_from_js(value: &JsValue, _context: &mut Context) -> JsResult<Self> {
        match value {
            JsValue::Integer(i) => (*i).try_into().map_err(|e| {
                JsNativeError::typ()
                    .with_message(format!("cannot convert value to a i16: {e}"))
                    .into()
            }),
            _ => Err(JsNativeError::typ()
                .with_message("cannot convert value to a i16")
                .into()),
        }
    }
}

impl TryFromJs for u16 {
    fn try_from_js(value: &JsValue, _context: &mut Context) -> JsResult<Self> {
        match value {
            JsValue::Integer(i) => (*i).try_into().map_err(|e| {
                JsNativeError::typ()
                    .with_message(format!("cannot convert value to a iu16: {e}"))
                    .into()
            }),
            _ => Err(JsNativeError::typ()
                .with_message("cannot convert value to a u16")
                .into()),
        }
    }
}

impl TryFromJs for i32 {
    fn try_from_js(value: &JsValue, _context: &mut Context) -> JsResult<Self> {
        match value {
            JsValue::Integer(i) => Ok(*i),
            _ => Err(JsNativeError::typ()
                .with_message("cannot convert value to a i32")
                .into()),
        }
    }
}

impl TryFromJs for u32 {
    fn try_from_js(value: &JsValue, _context: &mut Context) -> JsResult<Self> {
        match value {
            JsValue::Integer(i) => (*i).try_into().map_err(|e| {
                JsNativeError::typ()
                    .with_message(format!("cannot convert value to a u32: {e}"))
                    .into()
            }),
            _ => Err(JsNativeError::typ()
                .with_message("cannot convert value to a u32")
                .into()),
        }
    }
}

impl TryFromJs for i64 {
    fn try_from_js(value: &JsValue, _context: &mut Context) -> JsResult<Self> {
        match value {
            JsValue::Integer(i) => Ok((*i).into()),
            _ => Err(JsNativeError::typ()
                .with_message("cannot convert value to a i64")
                .into()),
        }
    }
}

impl TryFromJs for u64 {
    fn try_from_js(value: &JsValue, _context: &mut Context) -> JsResult<Self> {
        match value {
            JsValue::Integer(i) => (*i).try_into().map_err(|e| {
                JsNativeError::typ()
                    .with_message(format!("cannot convert value to a u64: {e}"))
                    .into()
            }),
            _ => Err(JsNativeError::typ()
                .with_message("cannot convert value to a u64")
                .into()),
        }
    }
}

impl TryFromJs for usize {
    fn try_from_js(value: &JsValue, _context: &mut Context) -> JsResult<Self> {
        match value {
            JsValue::Integer(i) => (*i).try_into().map_err(|e| {
                JsNativeError::typ()
                    .with_message(format!("cannot convert value to a usize: {e}"))
                    .into()
            }),
            _ => Err(JsNativeError::typ()
                .with_message("cannot convert value to a usize")
                .into()),
        }
    }
}

impl TryFromJs for i128 {
    fn try_from_js(value: &JsValue, _context: &mut Context) -> JsResult<Self> {
        match value {
            JsValue::Integer(i) => Ok((*i).into()),
            _ => Err(JsNativeError::typ()
                .with_message("cannot convert value to a i128")
                .into()),
        }
    }
}

impl TryFromJs for u128 {
    fn try_from_js(value: &JsValue, _context: &mut Context) -> JsResult<Self> {
        match value {
            JsValue::Integer(i) => (*i).try_into().map_err(|e| {
                JsNativeError::typ()
                    .with_message(format!("cannot convert value to a u128: {e}"))
                    .into()
            }),
            _ => Err(JsNativeError::typ()
                .with_message("cannot convert value to a u128")
                .into()),
        }
    }
}

#[test]
fn value_into_vec() {
    use boa_engine::{run_test_actions, TestAction};
    use indoc::indoc;

    #[derive(Debug, PartialEq, Eq, boa_macros::TryFromJs)]
    struct TestStruct {
        inner: bool,
        my_int: i16,
        my_vec: Vec<String>,
    }

    run_test_actions([
        TestAction::assert_with_op(
            indoc! {r#"
            let value = {
                inner: true,
                my_int: 11,
                my_vec: ["a", "b", "c"]
            };
            value
        "#},
            |value, context| {
                let value = TestStruct::try_from_js(&value, context);

                match value {
                    Ok(value) => {
                        value
                            == TestStruct {
                                inner: true,
                                my_int: 11,
                                my_vec: vec!["a".to_string(), "b".to_string(), "c".to_string()],
                            }
                    }
                    _ => false,
                }
            },
        ),
        TestAction::assert_with_op(
            indoc!(
                r#"
            let wrong = {
                inner: false,
                my_int: 22,
                my_vec: [{}, "e", "f"]
            };
            wrong"#
            ),
            |value, context| {
                let Err(value) = TestStruct::try_from_js(&value, context) else {
                    return false;
                };
                assert!(value.to_string().contains("TypeError"));
                true
            },
        ),
    ]);
}
