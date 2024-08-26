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

use crate::request::JsRequest;
use boa_engine::object::builtins::JsPromise;
use boa_engine::property::Attribute;
use boa_engine::realm::Realm;
use boa_engine::{js_string, Context, Finalize, JsData, JsResult, NativeObject};
use boa_gc::Trace;
use boa_interop::IntoJsFunctionCopied;
use http::{Request as HttpRequest, Response as HttpResponse};

pub mod headers;
pub mod request;

/// A trait for backend implementation of an HTTP fetcher.
// TODO: consider implementing an async version of this.
pub trait Fetcher<Body>: NativeObject + Sized {
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

/// The `fetch` function.
pub fn fetch<Body, T: Fetcher<Body>>(context: &mut Context) -> JsPromise {
    todo!()
}

pub fn register_fetch<F: Fetcher<()>>(
    fetcher: F,
    realm: &Realm,
    context: &mut Context,
) -> JsResult<()> {
    context.register_global_class::<JsRequest>()?;

    let fetch_fn = fetch::<(), F>
        .into_js_function_copied(context)
        .to_js_function(realm);
    context.register_global_property(js_string!("fetch"), fetch_fn, Attribute::all())?;
    context.insert_data(fetcher);

    Ok(())
}
