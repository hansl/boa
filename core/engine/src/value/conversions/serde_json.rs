//! This module implements the conversions from and into [`serde_json::Value`].

use super::JsValue;
use crate::{
    builtins::Array,
    error::JsNativeError,
    js_string,
    object::JsObject,
    property::{PropertyDescriptor, PropertyKey},
    Context, JsResult, JsVariant,
};
use serde_json::{Map, Value};
use std::collections::HashSet;

impl JsValue {
    /// Converts a [`serde_json::Value`] to a `JsValue`.
    ///
    /// # Example
    ///
    /// ```
    /// use boa_engine::{Context, JsValue};
    ///
    /// let data = r#"
    ///     {
    ///         "name": "John Doe",
    ///         "age": 43,
    ///         "phones": [
    ///             "+44 1234567",
    ///             "+44 2345678"
    ///         ]
    ///      }"#;
    ///
    /// let json: serde_json::Value = serde_json::from_str(data).unwrap();
    ///
    /// let mut context = Context::default();
    /// let value = JsValue::from_json(&json, &mut context).unwrap();
    /// #
    /// # assert_eq!(json, value.to_json(&mut context).unwrap());
    /// ```
    pub fn from_json(json: &Value, context: &mut Context) -> JsResult<Self> {
        /// Biggest possible integer, as i64.
        const MAX_INT: i64 = i32::MAX as i64;

        /// Smallest possible integer, as i64.
        const MIN_INT: i64 = i32::MIN as i64;

        match json {
            Value::Null => Ok(Self::null()),
            Value::Bool(b) => Ok(Self::new(*b)),
            Value::Number(num) => num
                .as_i64()
                .filter(|n| (MIN_INT..=MAX_INT).contains(n))
                .map(|i| Self::new(i as i32))
                .or_else(|| num.as_f64().map(Self::new))
                .ok_or_else(|| {
                    JsNativeError::typ()
                        .with_message(format!("could not convert JSON number {num} to JsValue"))
                        .into()
                }),
            Value::String(string) => Ok(Self::from(js_string!(string.as_str()))),
            Value::Array(vec) => {
                let mut arr = Vec::with_capacity(vec.len());
                for val in vec {
                    arr.push(Self::from_json(val, context)?);
                }
                Ok(Array::create_array_from_list(arr, context).into())
            }
            Value::Object(obj) => {
                let js_obj = JsObject::with_object_proto(context.intrinsics());
                for (key, value) in obj {
                    let property = PropertyDescriptor::builder()
                        .value(Self::from_json(value, context)?)
                        .writable(true)
                        .enumerable(true)
                        .configurable(true);
                    js_obj
                        .borrow_mut()
                        .insert(js_string!(key.clone()), property);
                }

                Ok(js_obj.into())
            }
        }
    }

    /// Converts the `JsValue` to a [`serde_json::Value`].
    ///
    /// # Example
    ///
    /// ```
    /// use boa_engine::{Context, JsValue};
    ///
    /// let data = r#"
    ///     {
    ///         "name": "John Doe",
    ///         "age": 43,
    ///         "phones": [
    ///             "+44 1234567",
    ///             "+44 2345678"
    ///         ]
    ///      }"#;
    ///
    /// let json: serde_json::Value = serde_json::from_str(data).unwrap();
    ///
    /// let mut context = Context::default();
    /// let value = JsValue::from_json(&json, &mut context).unwrap();
    ///
    /// let back_to_json = value.to_json(&mut context).unwrap();
    /// #
    /// # assert_eq!(json, back_to_json);
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if the `JsValue` is `Undefined`.
    pub fn to_json(&self, context: &mut Context) -> JsResult<Value> {
        let mut seen_objects = HashSet::new();
        self.to_json_inner(context, &mut seen_objects)
    }

    fn to_json_inner(
        &self,
        context: &mut Context,
        seen_objects: &mut HashSet<JsObject>,
    ) -> JsResult<Value> {
        match self.variant() {
            JsVariant::Null => Ok(Value::Null),
            JsVariant::Undefined => todo!("undefined to JSON"),
            JsVariant::Boolean(b) => Ok(Value::from(b)),
            JsVariant::String(string) => Ok(string.to_std_string_escaped().into()),
            JsVariant::Float64(rat) => Ok(Value::from(rat)),
            JsVariant::Integer32(int) => Ok(Value::from(int)),
            JsVariant::BigInt(_bigint) => Err(JsNativeError::typ()
                .with_message("cannot convert bigint to JSON")
                .into()),
            JsVariant::Object(obj) => {
                if seen_objects.contains(obj) {
                    return Err(JsNativeError::typ()
                        .with_message("cyclic object value")
                        .into());
                }
                seen_objects.insert(obj.clone());
                let mut value_by_prop_key = |property_key, context: &mut Context| {
                    obj.borrow()
                        .properties()
                        .get(&property_key)
                        .and_then(|x| {
                            x.value()
                                .map(|val| val.to_json_inner(context, seen_objects))
                        })
                        .unwrap_or(Ok(Value::Null))
                };

                if obj.is_array() {
                    let len = obj.length_of_array_like(context)?;
                    let mut arr = Vec::with_capacity(len as usize);

                    for k in 0..len as u32 {
                        let val = value_by_prop_key(k.into(), context)?;
                        arr.push(val);
                    }
                    // Passing the object rather than its clone that was inserted to the set should be fine
                    // as they hash to the same value and therefore HashSet can still remove the clone
                    seen_objects.remove(obj);
                    Ok(Value::Array(arr))
                } else {
                    let mut map = Map::new();

                    for index in obj.borrow().properties().index_property_keys() {
                        let key = index.to_string();
                        let value = value_by_prop_key(index.into(), context)?;
                        map.insert(key, value);
                    }

                    for property_key in obj.borrow().properties().shape.keys() {
                        let key = match &property_key {
                            PropertyKey::String(string) => string.to_std_string_escaped(),
                            PropertyKey::Index(i) => i.get().to_string(),
                            PropertyKey::Symbol(_sym) => {
                                return Err(JsNativeError::typ()
                                    .with_message("cannot convert Symbol to JSON")
                                    .into())
                            }
                        };
                        let value = value_by_prop_key(property_key, context)?;
                        map.insert(key, value);
                    }
                    seen_objects.remove(obj);
                    Ok(Value::Object(map))
                }
            }
            JsVariant::Symbol(_sym) => Err(JsNativeError::typ()
                .with_message("cannot convert Symbol to JSON")
                .into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use boa_macros::js_str;
    use indoc::indoc;
    use serde_json::json;

    use crate::{
        js_string, object::JsArray, run_test_actions, Context, JsObject, JsValue, TestAction,
    };

    #[test]
    fn json_conversions() {
        const DATA: &str = indoc! {r#"
            {
                "name": "John Doe",
                "age": 43,
                "minor": false,
                "adult": true,
                "extra": {
                    "address": null
                },
                "phones": [
                    "+44 1234567",
                    -45,
                    {},
                    true
                ],
                "7.3": "random text",
                "100": 1000,
                "24": 42
            }
        "#};

        run_test_actions([TestAction::inspect_context(|ctx| {
            let json: serde_json::Value = serde_json::from_str(DATA).unwrap();
            assert!(json.is_object());

            let value = JsValue::from_json(&json, ctx).unwrap();
            let obj = value.as_object().unwrap();
            assert_eq!(
                obj.get(js_str!("name"), ctx).unwrap(),
                js_str!("John Doe").into()
            );
            assert_eq!(obj.get(js_str!("age"), ctx).unwrap(), 43_i32.into());
            assert_eq!(obj.get(js_str!("minor"), ctx).unwrap(), false.into());
            assert_eq!(obj.get(js_str!("adult"), ctx).unwrap(), true.into());

            assert_eq!(
                obj.get(js_str!("7.3"), ctx).unwrap(),
                js_string!("random text").into()
            );
            assert_eq!(obj.get(js_str!("100"), ctx).unwrap(), 1000.into());
            assert_eq!(obj.get(js_str!("24"), ctx).unwrap(), 42.into());

            {
                let extra = obj.get(js_str!("extra"), ctx).unwrap();
                let extra = extra.as_object().unwrap();
                assert!(extra.get(js_str!("address"), ctx).unwrap().is_null());
            }
            {
                let phones = obj.get(js_str!("phones"), ctx).unwrap();
                let phones = phones.as_object().unwrap();

                let arr = JsArray::from_object(phones.clone()).unwrap();
                assert_eq!(arr.at(0, ctx).unwrap(), js_str!("+44 1234567").into());
                assert_eq!(arr.at(1, ctx).unwrap(), JsValue::from(-45_i32));
                assert!(arr.at(2, ctx).unwrap().is_object());
                assert_eq!(arr.at(3, ctx).unwrap(), true.into());
            }

            assert_eq!(json, value.to_json(ctx).unwrap());
        })]);
    }

    #[test]
    fn integer_ops_to_json() {
        run_test_actions([
            TestAction::assert_with_op("1000000 + 500", |v, ctx| {
                v.to_json(ctx).unwrap() == json!(1_000_500)
            }),
            TestAction::assert_with_op("1000000 - 500", |v, ctx| {
                v.to_json(ctx).unwrap() == json!(999_500)
            }),
            TestAction::assert_with_op("1000000 * 500", |v, ctx| {
                v.to_json(ctx).unwrap() == json!(500_000_000)
            }),
            TestAction::assert_with_op("1000000 / 500", |v, ctx| {
                v.to_json(ctx).unwrap() == json!(2_000)
            }),
            TestAction::assert_with_op("233894 % 500", |v, ctx| {
                v.to_json(ctx).unwrap() == json!(394)
            }),
            TestAction::assert_with_op("36 ** 5", |v, ctx| {
                v.to_json(ctx).unwrap() == json!(60_466_176)
            }),
        ]);
    }

    #[test]
    fn to_json_cyclic() {
        let mut context = Context::default();
        let obj = JsObject::with_null_proto();
        obj.create_data_property(js_string!("a"), obj.clone(), &mut context)
            .expect("should create data property");
        assert_eq!(
            JsValue::from(obj)
                .to_json(&mut context)
                .unwrap_err()
                .to_string(),
            "TypeError: cyclic object value"
        );
    }
}
