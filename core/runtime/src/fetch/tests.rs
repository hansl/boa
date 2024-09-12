use crate::request::JsRequest;
use crate::test::{run_test_actions, TestAction};
use boa_engine::js_string;
use either::Either;

#[test]
fn request_constructor() {
    run_test_actions([TestAction::inspect_context(|ctx| {
        let request =
            JsRequest::create_from_js(Either::Left(js_string!("http://example.com")), None)
                .unwrap();
        assert_eq!(request.uri().to_string(), "http://example.com/");
    })]);
}
