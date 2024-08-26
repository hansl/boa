//! The [`Headers`] JavaScript class.
use boa_engine::{Finalize, JsData, Trace};
use boa_interop::js_class;
use http::header::HeaderMap as HttpHeaderMap;

#[derive(Debug, Clone, JsData, Trace, Finalize)]
pub struct JsHeaders {
    #[unsafe_ignore_trace]
    headers: HttpHeaderMap,
}

js_class! {
    class JsHeaders as "Headers" {
        constructor() {
            Ok(JsHeaders {
                headers: HttpHeaderMap::new(),
            })
        }
    }
}
