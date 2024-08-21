//! Boa's implementation of JavaScript's `fetch` function.
//!
//! More information:
//!  - [MDN documentation][mdn]
//!  - [WHATWG `fetch` specification][spec]
//!
//! [spec]: https://fetch.spec.whatwg.org/
//! [mdn]: https://developer.mozilla.org/en-US/docs/Web/API/fetch

use boa_engine::object::builtins::JsPromise;
use boa_engine::value::{Convert, TryFromJs};
use boa_engine::{js_error, Context, Finalize, JsData, JsResult, JsString, JsValue};
use boa_gc::Trace;
use boa_interop::js_class;
use http::{Request as HttpRequest, Response as HttpResponse};
use std::collections::BTreeMap;

/// A trait for backend implementation of an HTTP fetcher.
// TODO: consider implementing an async version of this.
pub trait Fetcher<Body>: Trace + Sized {
    /// Fetch an HTTP document, returning an HTTP response.
    ///
    /// # Errors
    /// Any errors returned by the HTTP implementation must conform to
    /// [`boa_engine::JsError`].
    fn fetch_blocking(
        request: &HttpRequest<Body>,
        context: &mut Context,
    ) -> JsResult<HttpResponse<Body>>;
}

/// A [RequestInit][mdn] object. This is a JavaScript object (not a
/// class) that can be used as options for creating a [`JsRequest`].
///
/// [mdn]:https://developer.mozilla.org/en-US/docs/Web/API/RequestInit
// TODO: This class does not contain all fields that are defined in the spec.
#[derive(Clone, Trace, Finalize)]
pub struct RequestInit {
    body: Option<JsValue>,
    headers: Option<BTreeMap<JsString, Convert<JsString>>>,
    method: Option<Convert<JsString>>,
}

impl RequestInit {
    /// Create a [`http::request::Builder`] object and return both the
    /// body specified by JavaScript and the builder.
    fn into_request_builder(
        self,
        maybe_request: Option<HttpRequest<()>>,
    ) -> JsResult<(Option<JsValue>, http::request::Builder)> {
        let mut builder = HttpRequest::builder();
        if let Some(r) = maybe_request {
            let (parts, body) = r.into_parts();
            builder = builder
                .method(parts.method)
                .uri(parts.uri)
                .version(parts.version);

            for (key, value) in &parts.headers {
                builder = builder.header(key, value);
            }
        }

        if let Some(headers) = self.headers {
            for (key, Convert(ref value)) in &headers {
                // Make sure key and value can be represented by regular strings.
                // Keys also cannot have any extended characters (>128).
                // Values cannot have unpaired surrogates.
                let key = key.to_std_string().map_err(|_| {
                    js_error!(TypeError: "Request constructor: {} is an invalid header name", key.to_std_string_escaped())
                })?;
                if key.chars().any(|c| !c.is_ascii()) {
                    return Err(
                        js_error!(TypeError: "Request constructor: {} is an invalid header name", key),
                    );
                }
                let value = value.to_std_string().map_err(|_| {
                    js_error!(
                        TypeError: "Request constructor: {:?} is an invalid header value",
                        value
                    )
                })?;

                builder = builder.header(key, value);
            }
        }

        if let Some(method) = self.method {
            builder = builder.method(method.0.to_std_string().map_err(
                |_| js_error!(TypeError: "Requestion constructor: {} is an invalid method", method),
            ))
        }

        todo!()
    }
}

/// The JavaScript Response class.
///
/// The `Request` interface of the [Fetch API][mdn] represents a resource request.
///
/// [mdn]: https://developer.mozilla.org/en-US/docs/Web/API/Fetch_API
#[derive(Clone, JsData, Trace, Finalize)]
pub struct JsRequest {
    #[unsafe_ignore_trace]
    inner: HttpRequest<()>,
    body: Option<JsValue>,
}

impl JsRequest {
    /// Create a [`JsRequest`] instance from JavaScript arguments, similar to
    /// calling its constructor in JavaScript.
    pub fn create_from_js(
        input: either::Either<JsString, JsRequest>,
        options: Option<RequestInit>,
        context: &mut Context,
    ) -> JsResult<Self> {
        todo!()
    }
}

js_class! {
    class JsRequest as "Request" {
        constructor(
            input: either::Either<JsString, JsRequest>,
            options: Option<RequestInit>,
            context: &mut Context
        ) {
            JsRequest::create_from_js(input, options, context)
        }
    }
}

pub(crate) fn fetch<Body, T: Fetcher<Body>>(context: &mut Context) -> JsPromise {
    todo!()
}
