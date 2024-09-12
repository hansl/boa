//! The [`Headers`] JavaScript class.
//!
//! See <https://developer.mozilla.org/en-US/docs/Web/API/Headers>.
use boa_engine::value::Convert;
use boa_engine::{js_error, Finalize, JsData, JsResult, JsString, Trace};
use boa_interop::{js_class, JsClass};
use http::header::HeaderMap as HttpHeaderMap;
use http::HeaderName;
use std::str::FromStr;

#[derive(Debug, Clone, JsData, Trace, Finalize)]
pub struct JsHeaders {
    #[unsafe_ignore_trace]
    headers: HttpHeaderMap,
}

impl JsHeaders {
    pub fn append(&mut self, key: &JsString, value: &JsString) -> JsResult<()> {
        let key = key
            .to_std_string()
            .map_err(|_| ())
            .and_then(|s| HeaderName::from_str(&s).map_err(|_| ()))
            .map_err(|_| {
                js_error!("Cannot convert key to header string as it is not valid ASCII.")
            })?;
        let value = value
            .to_std_string()
            .map_err(|_| ())
            .and_then(|s| s.parse().map_err(|_| ()))
            .map_err(|_| {
                js_error!("Cannot convert value to header string as it is not valid ASCII.")
            })?;

        self.headers.append(key, value);
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

        fn append(this: JsClass<JsHeaders>, name: Convert<JsString>, value: Convert<JsString>) -> JsResult<()> {
            this.borrow_mut().append(&name.0, &value.0)
        }
    }
}
