use crate::test::{run_test_actions, TestAction};

#[test]
fn request_constructor() {
    run_test_actions([TestAction::inspect_context(|ctx| {
        let request =
            JsRequest::create_from_js(Either::Left(js_string!("http://example.com")), None)
                .unwrap();
        assert_eq!(request.inner.uri().to_string(), "http://example.com/");
    })]);
}
