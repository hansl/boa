#![cfg(feature = "fetch")]

use boa_engine::object::builtins::{JsPromise, JsTypedArray};
use boa_engine::value::TryFromJs;
use boa_engine::{
    js_string, Context, Finalize, JsArgs, JsError, JsNativeError, JsResult, JsValue,
    NativeFunction, Trace,
};
use boa_interop::{IntoJsFunctionCopied, UnsafeIntoJsFunction};
use reqwest;
use rustc_hash::FxHashMap;

/// The first argument of the fetch function is the resource to fetch.
///
/// This defines the resource that you wish to fetch. This can either be:
/// * A string or any other object with a stringifier — including a URL
///   object — that provides the URL of the resource you want to fetch.
///
/// * A Request object.
#[derive(Debug)]
#[non_exhaustive]
enum FetchResource {
    Url(reqwest::Url),
}

impl FetchResource {
    fn create_request(&self, client: &reqwest::Client) -> JsResult<reqwest::Request> {
        match self {
            Self::Url(url) => client
                .request(reqwest::Method::GET, url.clone())
                .build()
                .map_err(|e| {
                    JsError::from_opaque(
                        js_string!(format!("Could not build request: {}", e.to_string())).into(),
                    )
                }),
        }
    }
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
                    JsNativeError::typ().with_message("Request Object as Resource not supported"),
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
pub struct RequestBody {
    bytes: Vec<u8>,
}

impl TryFromJs for RequestBody {
    fn try_from_js(value: &JsValue, _ctx: &mut Context) -> JsResult<Self> {
        let o = value.as_object().ok_or_else(|| {
            JsError::from_native(JsNativeError::typ().with_message("Invalid body object"))
        })?;
        if !o.is_array() {
            return Err(JsError::from_native(
                JsNativeError::typ().with_message("Invalid body object"),
            ));
        }

        // let array = JsTypedArray

        Ok(Self { bytes: Vec::new() })
    }
}

impl Into<reqwest::Body> for RequestBody {
    fn into(self) -> reqwest::Body {
        self.bytes.into()
    }
}

#[derive(Debug, Clone, Finalize, Trace, TryFromJs)]
pub struct FetchOptions {
    method: Option<String>,
    headers: Option<FxHashMap<String, String>>,
    body: Option<JsValue>,
}

impl FetchOptions {
    fn create_request(&self, mut request: reqwest::Request) -> JsResult<reqwest::Request> {
        if let Some(method) = &self.method {
            let method = method.parse::<reqwest::Method>().map_err(|e| {
                JsError::from_native(JsNativeError::typ().with_message(e.to_string().into()))
            })?;
            *request.method_mut() = method;
        }
        if let Some(headers) = &self.headers {
            headers.iter().for_each(|(k, v)| {
                request.headers_mut().insert(k, v.into());
            });
        }

        Ok(request)
    }
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
    #[unsafe_ignore_trace]
    client: reqwest::Client,
}

impl Default for Fetch {
    fn default() -> Self {
        // SAFETY: This will not error, by Reqwest's documentation.
        // Client creation only fails when TLS backend cannot be found.
        // Using the default fetch implementation, there are not TLS
        // backends.
        match Self::new("BoaJS Fetch".to_string()) {
            Ok(c) => c,
            _ => unreachable!("Default impl cannot fail"),
        }
    }
}

impl Fetch {
    pub fn new(agent_string: String) -> Result<Self, reqwest::Error> {
        Ok(Self {
            client: reqwest::ClientBuilder::new()
                .user_agent(agent_string)
                .build()?,
        })
    }

    fn fetch(
        &self,
        resource: FetchResource,
        options: Option<FetchOptions>,
        ctx: &mut Context,
    ) -> JsResult<JsPromise> {
        let mut request = resource.create_request(&self.client)?;
        if let Some(o) = options {
            request = o.create_request(&self.client, request)?;
        }

        let (promise, resolvers) = JsPromise::new_pending(ctx);
        let fetch_job = async move {};
        ctx.job_queue().enqueue_future_job(Box::pin(fetch_job), ctx);

        Ok(promise)
    }

    /// Create a [`NativeFunction`] from this object. Use this to add the [`fetch()`]
    /// API to your module, or preferably global object.
    ///
    /// # Example
    /// ```
    /// # use boa_runtime::Fetch;
    /// # use boa_engine::{Context, js_string};
    /// use boa_engine::property::Attribute;
    /// # let mut context = Context::default();
    /// let fetch_fn = Fetch::default().into_js_function(&mut context);
    /// context.register_global_property(js_string!("fetch"), fetch_fn.into(), Attribute::all()).unwrap();
    /// ```
    pub fn into_js_function(self, ctx: &mut Context) -> NativeFunction {
        // Inner function to typecast the `FnMut` properly. Apparently the compiler
        // cannot infer the type correctly without this.
        #[inline]
        fn into_fn_mut(
            s: Fetch,
        ) -> impl FnMut(FetchResource, Option<FetchOptions>, &mut Context) -> JsResult<JsValue>
        {
            move |resource, options, ctx| s.fetch(resource, options, ctx)
        }

        unsafe { into_fn_mut(self).into_js_function_unsafe(ctx) }
    }
}
