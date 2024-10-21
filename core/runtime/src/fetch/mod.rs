//! Boa's implementation of JavaScript's `fetch` function.
//!
//! More information:
//!  - [MDN documentation][mdn]
//!  - [WHATWG `fetch` specification][spec]
//!
//! [spec]: https://fetch.spec.whatwg.org/
//! [mdn]: https://developer.mozilla.org/en-US/docs/Web/API/fetch

#[cfg(test)]
mod tests;

use crate::fetch::headers::JsHeaders;
use crate::fetch::request::{JsRequest, RequestInit};
use boa_engine::object::builtins::JsPromise;
use boa_engine::property::Attribute;
use boa_engine::realm::Realm;
use boa_engine::{
    js_error, js_string, Context, JsError, JsObject, JsResult, JsString, NativeObject,
};
use boa_interop::IntoJsFunctionCopied;
use either::Either;
use http::{Request as HttpRequest, Request, Response as HttpResponse};

pub mod headers;
pub mod request;

/// A trait for backend implementation of an HTTP fetcher.
/// This MUST implement clone to allow for the context not to be
/// borrowed when getting a reference to the fetcher.
// TODO: consider implementing an async version of this.
pub trait Fetcher: NativeObject + Sized + Clone {
    /// Fetch an HTTP document, returning an HTTP response.
    ///
    /// # Errors
    /// Any errors returned by the HTTP implementation must conform to
    /// [`boa_engine::JsError`].
    fn fetch_blocking(
        &self,
        request: &HttpRequest<Vec<u8>>,
        context: &mut Context,
    ) -> JsResult<HttpResponse<Vec<u8>>>;
}

/// The `fetch` function.
///
/// # Errors
/// If the fetcher is not registered in the context, an error is returned.
/// This function will also return any error that the fetcher returns, or
/// any conversion to/from JavaScript types.
pub fn fetch<T: Fetcher>(
    resource: Either<JsString, JsObject>,
    options: Option<RequestInit>,
    context: &mut Context,
) -> JsResult<JsPromise> {
    let Some(fetcher) = context.get_data::<T>().cloned() else {
        return Err(
            js_error!(Error: "implementation of fetch requires a fetcher registered in the context"),
        );
    };

    // The resource parsing is complicated, so we parse it in Rust here (instead of relying on
    // `TryFromJs` and friends).
    let request: Request<Option<Vec<u8>>> = match resource {
        Either::Left(url) => {
            let url = url.to_std_string().map_err(JsError::from_rust)?;
            let request = HttpRequest::get(url)
                .body(Some(Vec::new()))
                .map_err(JsError::from_rust)?;
            request
        }
        Either::Right(request) => {
            // This can be a [`JsRequest`] object.
            let Ok(request) = request.downcast::<JsRequest>() else {
                return Err(js_error!(TypeError: "resource must be a URL or Request object"));
            };
            let Ok(request_ref) = request.try_borrow() else {
                return Err(js_error!(TypeError: "Request object is already in use"));
            };

            request_ref.data().inner().clone().map(|_| Some(Vec::new()))
        }
    };

    let request = if let Some(options) = options {
        options.into_request_builder(Some(request))?
    } else {
        request
    };

    let request = request.map(|maybe_body| maybe_body.unwrap_or_default());
    let response = fetcher.fetch_blocking(&request, context)?;

    eprintln!("Response: {:?}", response);
    todo!()
}

/// Register the `fetch` function in the context, as well as ALL supporting classes.
///
/// # Errors
/// If any of the classes fail to register, an error is returned.
pub fn register<F: Fetcher>(fetcher: F, realm: &Realm, context: &mut Context) -> JsResult<()> {
    context.register_global_class::<JsHeaders>()?;
    context.register_global_class::<JsRequest>()?;

    let fetch_fn = fetch::<F>
        .into_js_function_copied(context)
        .to_js_function(realm);
    context.insert_data(fetcher);
    context.register_global_property(js_string!("fetch"), fetch_fn, Attribute::all())?;

    Ok(())
}
