#![cfg(feature = "fetch")]

use boa_engine::value::TryFromJs;
use boa_engine::{
    Context, Finalize, JsArgs, JsError, JsNativeError, JsResult, JsValue, NativeFunction, Trace,
};
use reqwest::Body;

/// The first argument of the fetch function is the resource to fetch.
///
/// This defines the resource that you wish to fetch. This can either be:
/// * A string or any other object with a stringifier — including a URL
///   object — that provides the URL of the resource you want to fetch.
///
/// * A Request object.
#[derive(Debug)]
enum FetchResource {
    Url(reqwest::Url),
    Request(reqwest::Request),
}

impl TryFromJs for FetchResource {
    fn try_from_js(value: &JsValue, ctx: &mut Context) -> JsResult<Self> {
        match value.to_string(ctx) {
            Ok(url) => {
                let url = match reqwest::Url::parse(&url.to_std_string_escaped()) {
                    Ok(url) => url,
                    Err(_) => {
                        return Err(JsError::from_native(
                            JsNativeError::typ().with_message("Invalid URL"),
                        ));
                    }
                };
                Ok(Self::Url(url))
            }
            Err(_) => {
                // TODO: implement this for Request object.
                return Err(JsError::from_native(
                    JsNativeError::typ().with_message("Reqwest Object as Resource not supported"),
                ));
            }
        }
    }
}

/// The `body` field of the fetch options object.
///
/// Any body that you want to add to your request: this can be a
/// Blob, an ArrayBuffer, a TypedArray, a DataView, a FormData, a
/// URLSearchParams, string object or literal, or a ReadableStream
/// object. This latest possibility is still experimental; check
/// the compatibility information to verify you can use it. Note
/// that a request using the GET or HEAD method cannot have a body.
#[derive(Debug, Clone)]
pub struct FetchBody {
    bytes: Vec<u8>,
}

impl TryFromJs for FetchBody {
    fn try_from_js(_value: &JsValue, _ctx: &mut Context) -> JsResult<Self> {
        Ok(Self { bytes: Vec::new() })
    }
}

impl Into<Body> for FetchBody {
    fn into(self) -> Body {
        self.bytes.into()
    }
}

#[derive(Debug, Clone, TryFromJs)]
pub struct FetchOptions {
    method: Option<String>,
    headers: Option<JsValue>,
    body: Option<JsValue>,
}

/// Implementation of the `fetch()` API. See more:
/// <https://developer.mozilla.org/en-US/docs/Web/API/Fetch_API>.
///
/// To include this API in your code, use
/// ```
///
/// ```
#[derive(Debug, Clone, Finalize, Trace)]
pub struct Fetch {
    agent_string: String,
}

impl Default for Fetch {
    fn default() -> Self {
        Self::new("BoaJS Fetch".to_string())
    }
}

impl Fetch {
    pub fn new(agent_string: String) -> Self {
        Self { agent_string }
    }

    fn fetch(&self, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
        let url = args.get_or_undefined(0).to_string(ctx)?;

        Ok(JsValue::undefined())
    }
}

impl Into<NativeFunction> for Fetch {
    fn into(self) -> NativeFunction {
        NativeFunction::from_copy_closure_with_captures(
            |_this, args, s, ctx| s.fetch(args, ctx),
            self,
        )
    }
}
