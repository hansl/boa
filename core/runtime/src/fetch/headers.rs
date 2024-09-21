//! The [`Headers`] JavaScript class.
//!
//! See <https://developer.mozilla.org/en-US/docs/Web/API/Headers>.
use boa_engine::object::builtins::{JsArray, JsFunction, TypedJsFunction};
use boa_engine::value::Convert;
use boa_engine::{
    js_error, Context, Finalize, JsData, JsObject, JsResult, JsString, JsValue, Trace,
};
use boa_interop::{js_class, JsClass};
use http::header::HeaderMap as HttpHeaderMap;
use http::{HeaderName, HeaderValue};
use std::str::FromStr;

pub type ForEachCallback = TypedJsFunction<(JsString, JsString, JsObject), JsResult<()>>;

/// Converts a JavaScript string to a valid header name (or error).
///
/// # Errors
/// If the key is not valid ASCII, an error is returned.
#[inline]
fn to_header_name(key: &JsString) -> JsResult<HeaderName> {
    key.to_std_string()
        .map_err(|_| ())
        .and_then(|s| HeaderName::from_str(&s).map_err(|_| ()))
        .map_err(|()| js_error!("Cannot convert key to header string as it is not valid ASCII."))
}

/// Converts a JavaScript string to a valid header value (or error).
///
/// # Errors
/// If the value is not valid ASCII, an error is returned.
#[inline]
fn to_header_value(value: &JsString) -> JsResult<HeaderValue> {
    value
        .to_std_string()
        .map_err(|_| ())
        .and_then(|s| s.parse().map_err(|_| ()))
        .map_err(|()| js_error!("Cannot convert value to header string as it is not valid ASCII."))
}

/// A JavaScript wrapper for the `Headers` object.
#[derive(Debug, Clone, JsData, Trace, Finalize)]
pub struct JsHeaders {
    #[unsafe_ignore_trace]
    headers: HttpHeaderMap,
}

impl JsHeaders {
    /// Appends a new value onto an existing header inside a Headers object,
    /// or adds the header if it does not already exist.
    ///
    /// # Errors
    /// If the key or value is not valid ASCII, an error is returned.
    pub fn append(&mut self, key: &JsString, value: &JsString) -> JsResult<()> {
        let key = to_header_name(key)?;
        let value = to_header_value(value)?;
        self.headers.append(key, value);
        Ok(())
    }

    /// Deletes a header from a Headers object.
    ///
    /// # Errors
    /// If the key is not valid ASCII, an error is returned.
    pub fn delete(&mut self, key: &JsString) -> JsResult<()> {
        let key = to_header_name(key)?;
        self.headers.remove(key);
        Ok(())
    }

    /// Returns an iterator allowing to go through all key/value pairs contained in this object.
    // TODO: This should return a JsIterator, but not such thing exists yet.
    pub fn entries(&self, context: &mut Context) -> JsValue {
        JsArray::from_iter(
            self.headers
                .iter()
                .map(|(k, v)| {
                    let k: JsValue = JsString::from(k.as_str()).into();
                    let v: JsValue = JsString::from(v.to_str().unwrap_or("")).into();
                    JsArray::from_iter([k, v], context).into()
                })
                .collect::<Vec<_>>(),
            context,
        )
        .into()
    }

    /// Executes a provided function once for each key/value pair in the Headers object.
    pub fn for_each(
        &self,
        callback: ForEachCallback,
        this_arg: Option<JsValue>,
        object: JsObject,
        context: &mut Context,
    ) -> JsResult<()> {
        for (k, v) in self.headers.iter() {
            let k: JsValue = JsString::from(k.as_str()).into();
            let v: JsValue = JsString::from(v.to_str().unwrap_or("")).into();
            callback.call(context, this_arg, &[v, k, object])?;
        }
        Ok(())
    }
}

js_class! {
    class JsHeaders as "Headers" {
        constructor() {
            Ok(JsHeaders {
                headers: HttpHeaderMap::new(),
            })
        }

        fn append(
            this: JsClass<JsHeaders>,
            name: Convert<JsString>,
            value: Convert<JsString>,
        ) -> JsResult<()> {
            this.borrow_mut().append(&name.0, &value.0)
        }

        fn delete(
            this: JsClass<JsHeaders>,
            name: Convert<JsString>,
        ) -> JsResult<()> {
            this.borrow_mut().delete(&name.0)
        }

        fn entries(
            this: JsClass<JsHeaders>,
            context: &mut Context,
        ) -> JsValue {
            this.borrow().entries(context)
        }

        fn forEach(
            this: JsClass<JsHeaders>,
            callback: ForEachCallback,
            this_arg: Option<JsValue>,
        ) -> JsResult<()> {
            this.borrow().for_each(callback, this_arg, &this.inner())
        }

        fn get(
            this: JsClass<JsHeaders>,
            name: Convert<JsString>,
        ) -> JsResult<JsValue> {
            unimplemented!("Headers.prototype.get")
        }

        fn getSetCookie(
            this: JsClass<JsHeaders>,
            name: Convert<JsString>,
        ) -> JsResult<JsValue> {
            unimplemented!("Headers.prototype.get")
        }

        fn has(
            this: JsClass<JsHeaders>,
            name: Convert<JsString>,
        ) -> JsResult<bool> {
            unimplemented!("Headers.prototype.has")
        }

        fn keys(
            this: JsClass<JsHeaders>,
        ) -> JsValue {
            unimplemented!("Headers.prototype.keys")
        }

        fn set(
            this: JsClass<JsHeaders>,
            name: Convert<JsString>,
            value: Convert<JsString>,
        ) -> JsResult<()> {
            unimplemented!("Headers.prototype.set")
        }

        fn values(
            this: JsClass<JsHeaders>,
        ) -> JsValue {
            unimplemented!("Headers.prototype.values")
        }
    }
}
