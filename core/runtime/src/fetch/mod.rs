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

use crate::request::{JsRequest, RequestInit};
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
// TODO: consider implementing an async version of this.
pub trait Fetcher: NativeObject + Sized {
    /// Fetch an HTTP document, returning an HTTP response.
    ///
    /// # Errors
    /// Any errors returned by the HTTP implementation must conform to
    /// [`boa_engine::JsError`].
    fn fetch_blocking(
        &self,
        request: &HttpRequest<&[u8]>,
        context: &mut Context,
    ) -> JsResult<HttpResponse<Vec<u8>>>;
}

/// The `fetch` function.
pub fn fetch<T: Fetcher>(
    resource: Either<JsString, JsObject>,
    options: Option<RequestInit>,
    context: &mut Context,
) -> JsResult<JsPromise> {
    let Some(fetcher) = context.get_data::<T>() else {
        return Err(
            js_error!(Error: "implementation of fetch requires a fetcher registered in the context"),
        );
    };

    // The resource parsing is complicated, so we parse it in Rust here (instead of relying on
    // `TryFromJs` and friends).
    let request: Request<Vec<u8>> = match resource {
        Either::Left(url) => {
            let url = url.to_std_string().map_err(JsError::from_rust)?;
            let request = HttpRequest::get(url)
                .body(Vec::new())
                .map_err(JsError::from_rust)?;
            request
        }
        Either::Right(request) => {
            // This can be a [`JsRequest`] object.
            if let Ok(request) = request.downcast::<JsRequest>() {
                request.borrow().data().inner().clone().map(|_| Vec::new())
            } else {
                return Err(js_error!(TypeError: "resource must be a URL or Request object"));
            }
        }
    };
    let request = request.map()

    let response = fetcher.fetch_blocking(&request, context);
    todo!()
}

pub fn register_fetch<F: Fetcher>(
    fetcher: F,
    realm: &Realm,
    context: &mut Context,
) -> JsResult<()> {
    context.register_global_class::<JsRequest>()?;

    let fetch_fn = fetch::<F>
        .into_js_function_copied(context)
        .to_js_function(realm);
    context.insert_data(fetcher);
    context.register_global_property(js_string!("fetch"), fetch_fn, Attribute::all())?;

    Ok(())
}
