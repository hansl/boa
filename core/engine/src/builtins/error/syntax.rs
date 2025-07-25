//! This module implements the global `SyntaxError` object.
//!
//! The `SyntaxError` object represents an error when trying to interpret syntactically invalid code.
//! It is thrown when the JavaScript context encounters tokens or token order that does not conform
//! to the syntax of the language when parsing code.
//!
//! More information:
//!  - [MDN documentation][mdn]
//!  - [ECMAScript reference][spec]
//!
//! [spec]: https://tc39.es/ecma262/#sec-native-error-types-used-in-this-standard-syntaxerror
//! [mdn]: https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/SyntaxError

use crate::{
    Context, JsArgs, JsResult, JsString, JsValue,
    builtins::{BuiltInBuilder, BuiltInConstructor, BuiltInObject, IntrinsicObject},
    context::intrinsics::{Intrinsics, StandardConstructor, StandardConstructors},
    js_string,
    object::{JsObject, internal_methods::get_prototype_from_constructor},
    property::Attribute,
    realm::Realm,
    string::StaticJsStrings,
};

use super::{Error, ErrorKind};

/// JavaScript `SyntaxError` implementation.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SyntaxError;

impl IntrinsicObject for SyntaxError {
    fn init(realm: &Realm) {
        let attribute = Attribute::WRITABLE | Attribute::NON_ENUMERABLE | Attribute::CONFIGURABLE;
        BuiltInBuilder::from_standard_constructor::<Self>(realm)
            .prototype(realm.intrinsics().constructors().error().constructor())
            .inherits(Some(realm.intrinsics().constructors().error().prototype()))
            .property(js_string!("name"), Self::NAME, attribute)
            .property(js_string!("message"), js_string!(), attribute)
            .build();
    }

    fn get(intrinsics: &Intrinsics) -> JsObject {
        Self::STANDARD_CONSTRUCTOR(intrinsics.constructors()).constructor()
    }
}

impl BuiltInObject for SyntaxError {
    const NAME: JsString = StaticJsStrings::SYNTAX_ERROR;
}

impl BuiltInConstructor for SyntaxError {
    const LENGTH: usize = 1;
    const P: usize = 2;
    const SP: usize = 0;

    const STANDARD_CONSTRUCTOR: fn(&StandardConstructors) -> &StandardConstructor =
        StandardConstructors::syntax_error;

    /// Create a new error object.
    fn constructor(
        new_target: &JsValue,
        args: &[JsValue],
        context: &mut Context,
    ) -> JsResult<JsValue> {
        // 1. If NewTarget is undefined, let newTarget be the active function object; else let newTarget be NewTarget.
        let new_target = &if new_target.is_undefined() {
            context
                .active_function_object()
                .unwrap_or_else(|| {
                    context
                        .intrinsics()
                        .constructors()
                        .syntax_error()
                        .constructor()
                })
                .into()
        } else {
            new_target.clone()
        };
        // 2. Let O be ? OrdinaryCreateFromConstructor(newTarget, "%NativeError.prototype%", « [[ErrorData]] »).
        let prototype = get_prototype_from_constructor(
            new_target,
            StandardConstructors::syntax_error,
            context,
        )?;
        let o = JsObject::from_proto_and_data_with_shared_shape(
            context.root_shape(),
            prototype,
            Error::with_caller_position(ErrorKind::Syntax, context),
        );

        // 3. If message is not undefined, then
        let message = args.get_or_undefined(0);
        if !message.is_undefined() {
            // a. Let msg be ? ToString(message).
            let msg = message.to_string(context)?;

            // b. Perform CreateNonEnumerableDataPropertyOrThrow(O, "message", msg).
            o.create_non_enumerable_data_property_or_throw(js_string!("message"), msg, context);
        }

        // 4. Perform ? InstallErrorCause(O, options).
        Error::install_error_cause(&o, args.get_or_undefined(1), context)?;

        // 5. Return O.
        Ok(o.into())
    }
}
