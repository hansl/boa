//! The [`Request`] JavaScript class and adjacent types.
//!
//! See the [Request interface documentation][mdn] for more information.
//!
//! [mdn]: https://developer.mozilla.org/en-US/docs/Web/API/Request
use super::HttpRequest;
use boa_engine::value::{Convert, TryFromJs};
use boa_engine::{js_error, Finalize, JsData, JsObject, JsResult, JsString, JsValue, Trace};
use boa_interop::js_class;
use either::Either;
use std::collections::BTreeMap;
use std::mem;

/// A [RequestInit][mdn] object. This is a JavaScript object (not a
/// class) that can be used as options for creating a [`JsRequest`].
///
/// [mdn]:https://developer.mozilla.org/en-US/docs/Web/API/RequestInit
// TODO: This class does not contain all fields that are defined in the spec.
#[derive(Debug, Clone, TryFromJs, Trace, Finalize)]
pub struct RequestInit {
    body: Option<JsValue>,
    headers: Option<BTreeMap<JsString, Convert<JsString>>>,
    method: Option<Convert<JsString>>,
}

impl RequestInit {
    /// Create a [`http::request::Builder`] object and return both the
    /// body specified by JavaScript and the builder.
    fn into_request_builder(
        mut self,
        maybe_request: Option<HttpRequest<()>>,
    ) -> JsResult<(Option<JsValue>, http::request::Builder)> {
        let mut builder = HttpRequest::builder();
        if let Some(r) = maybe_request {
            let (parts, _body) = r.into_parts();
            builder = builder
                .method(parts.method)
                .uri(parts.uri)
                .version(parts.version);

            for (key, value) in &parts.headers {
                builder = builder.header(key, value);
            }
        }

        if let Some(ref headers) = self.headers.take() {
            for (hkey, Convert(ref hvalue)) in headers {
                // Make sure key and value can be represented by regular strings.
                // Keys also cannot have any extended characters (>128).
                // Values cannot have unpaired surrogates.
                let key = hkey.to_std_string().map_err(|_| {
                    js_error!(TypeError: "Request constructor: {} is an invalid header name", hkey.to_std_string_escaped())
                })?;
                if key.chars().any(|c| !c.is_ascii()) {
                    return Err(
                        js_error!(TypeError: "Request constructor: {} is an invalid header name", hkey.to_std_string_escaped()),
                    );
                }
                let value = hvalue.to_std_string().map_err(|_| {
                    js_error!(
                        TypeError: "Request constructor: {:?} is an invalid header value",
                        hvalue.to_std_string_escaped()
                    )
                })?;

                builder = builder.header(key, value);
            }
        }

        if let Some(Convert(method)) = self.method.take() {
            builder = builder.method(method.to_std_string().map_err(
                |_| js_error!(TypeError: "Requestion constructor: {} is an invalid method", method.to_std_string_escaped()),
            )?.as_str())
        }

        Ok((self.body.take(), builder))
    }
}

/// The JavaScript Response class.
///
/// The `Request` interface of the [Fetch API][mdn] represents a resource request.
///
/// [mdn]: https://developer.mozilla.org/en-US/docs/Web/API/Fetch_API
#[derive(Debug, Clone, JsData, Trace, Finalize)]
pub struct JsRequest {
    #[unsafe_ignore_trace]
    inner: HttpRequest<()>,
    body: Option<JsValue>,
}

impl JsRequest {
    /// Get the inner `http::Request` object. This drops the body (if any).
    fn into_inner(mut self) -> HttpRequest<()> {
        let inner = mem::replace(&mut self.inner, HttpRequest::new(()));
        inner
    }

    /// Create a [`JsRequest`] instance from JavaScript arguments, similar to
    /// calling its constructor in JavaScript.
    pub fn create_from_js(
        input: Either<JsString, JsRequest>,
        options: Option<RequestInit>,
    ) -> JsResult<Self> {
        let request = match input {
            Either::Left(uri) => {
                let uri = http::Uri::try_from(
                    uri.to_std_string()
                        .map_err(|_| js_error!(URIError: "URI cannot have unpaired surrogates"))?,
                )
                .map_err(|_| js_error!(URIError: "Invalid URI"))?;
                http::request::Request::builder()
                    .uri(uri)
                    .body(())
                    .map_err(|_| js_error!(Error: "Cannot construct request"))?
            }
            Either::Right(r) => r.into_inner(),
        };

        if let Some(options) = options {
            let (body, builder) = options.into_request_builder(Some(request))?;
            Ok(Self {
                inner: builder
                    .body(())
                    .map_err(|_| js_error!(Error: "Cannot construct request"))?,
                body,
            })
        } else {
            Ok(Self {
                inner: request,
                body: None,
            })
        }
    }
}

js_class! {
    class JsRequest as "Request" {
        constructor(
            input: Either<JsString, JsObject>,
            options: Option<RequestInit>
        ) {
            let input = match input {
                Either::Left(i) => Either::Left(i),
                Either::Right(r) => {
                    if let Ok(request) = r.clone().downcast::<JsRequest>() {
                        // TODO: why do we need to clone? We can just drop the `JsObject`.
                        Either::Right(request.borrow().data().clone())
                    } else {
                        return Err(js_error!(TypeError: "invalid input argument"));
                    }
                }
            };
            JsRequest::create_from_js(input, options)
        }
    }
}