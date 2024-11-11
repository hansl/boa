//! Module implementing JavaScript classes to handle text encoding and decoding.
//!
//! See <https://developer.mozilla.org/en-US/docs/Web/API/Encoding_API> for more information.

use boa_engine::object::builtins::JsUint8Array;
use boa_engine::string::CodePoint;
use boa_engine::value::TryFromJs;
use boa_engine::{
    js_error, js_str, js_string, Context, Finalize, JsData, JsNativeError, JsObject, JsResult,
    JsString, JsValue, Trace, TryIntoJsResult,
};
use boa_interop::{js_class, JsClass};
use std::fmt::Debug;

#[cfg(test)]
mod tests;

/// The `Encoding` enum represents the different types of encoding that can be used.
///
/// All encodings are case-insensitive. A feature flag protects any encodings that
/// require dependencies.
///
/// UTF8 and UTF16 are always available, and they have an optimization path that makes
/// them faster than using the encoding crate.
///
/// See <https://developer.mozilla.org/en-US/docs/Web/API/Encoding_API/Encodings> for
/// a full list of encodings supported by the standard, and more information.
#[derive(Default, Clone, Copy)]
enum Encoding {
    #[default]
    Utf8,
    Utf16,

    #[cfg(feature = "encoding")]
    Ref(&'static encoding_rs::Encoding),
}

impl Debug for Encoding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Utf8 => write!(f, "utf-8"),
            Self::Utf16 => write!(f, "utf-16"),

            #[cfg(feature = "encoding")]
            Self::Ref(r) => write!(f, "{}", r.name()),
        }
    }
}

impl TryFromJs for Encoding {
    fn try_from_js(value: &JsValue, context: &mut Context) -> JsResult<Self> {
        let value = value
            .to_string(context)?
            .to_std_string_lossy()
            .to_lowercase();

        // Short circuit with encoding feature, as this adds support for BOM and
        // streaming.
        #[cfg(feature = "encoding")]
        if let Some(x) = encoding_rs::Encoding::for_label(value.as_bytes()) {
            return Ok(Self::Ref(x));
        }

        match value.as_str() {
            "unicode-1-1-utf-8" | "utf-8" | "utf8" => Ok(Self::Utf8),
            "utf-16" | "utf-16le" => Ok(Self::Utf16),
            _ => Err(js_error!(TypeError: "Invalid encoding.")),
        }
    }
}

impl TryIntoJsResult for Encoding {
    fn try_into_js_result(self, _context: &mut Context) -> JsResult<JsValue> {
        match self {
            Self::Utf8 => Ok(js_string!("utf-8").into()),
            Self::Utf16 => Ok(js_string!("utf-16").into()),
            #[cfg(feature = "encoding")]
            Self::Ref(r) => Ok(JsString::from(r.name()).into()),
        }
    }
}

#[derive(Debug, Default, Clone, TryFromJs)]
struct TextDecoderOptions {
    fatal: Option<bool>,
    #[boa(rename = "ignoreBOM")]
    ignore_bom: Option<bool>,
}

impl TextDecoderOptions {
    fn fatal(&self) -> bool {
        self.fatal.unwrap_or(false)
    }

    fn ignore_bom(&self) -> bool {
        self.ignore_bom.unwrap_or(false)
    }
}

/// The `TextDecoder`[mdn] class represents an encoder for a specific method, that is
/// a specific character encoding, like `utf-8`.
///
/// [mdn]: https://developer.mozilla.org/en-US/docs/Web/API/TextDecoder
#[derive(Debug, Clone, JsData, Trace, Finalize)]
pub struct TextDecoder {
    #[unsafe_ignore_trace]
    encoding: Encoding,
    fatal: bool,
    ignore_bom: bool,
}

impl TextDecoder {
    /// Register the `TextDecoder` class into the realm.
    ///
    /// # Errors
    /// This will error if the context or realm cannot register the class.
    pub fn register(context: &mut Context) -> JsResult<()> {
        context.register_global_class::<Self>()?;
        Ok(())
    }

    /// The `decode()` method of the `TextDecoder` interface returns a `JsString` containing
    /// the given `Uint8Array` decoded in the specific method. This will replace any
    /// invalid characters with the Unicode replacement character.
    ///
    /// # Errors
    /// If the `fatal` option is set to `true` and an invalid character is found, this will
    /// return a `TypeError`.
    pub fn decode(&self, text: Option<JsUint8Array>, context: &mut Context) -> JsResult<JsString> {
        let bytes = text.map(|t| t.to_vec(context)).unwrap_or_default();
        let fatal = self.fatal;
        let ignore_bom = self.ignore_bom;

        match self.encoding {
            Encoding::Utf8 => {
                if fatal {
                    let Some(chunk) = bytes.utf8_chunks().next() else {
                        return Ok(JsString::default());
                    };
                    if chunk.invalid().is_empty() {
                        return Ok(JsString::from(chunk.valid()));
                    }
                    return Err(js_error!(TypeError: "Decoding failed"));
                }
                Ok(JsString::from(
                    String::from_utf8_lossy(bytes.as_slice()).as_ref(),
                ))
            }
            Encoding::Utf16 => {
                // Safety: We are aligning to u16, so this is safe.
                let (front, slice, back) = unsafe { bytes.align_to::<u16>() };
                if fatal && (!front.is_empty() || !back.is_empty()) {
                    return Err(js_error!(TypeError: "Decoding failed"));
                }

                let main = JsString::from(slice);

                match (front.is_empty(), back.is_empty()) {
                    (true, true) => Ok(main),
                    (true, false) => Ok(JsString::concat(main.as_str(), js_str!("\u{FFFD}"))),
                    (false, true) => Ok(JsString::concat(js_str!("\u{FFFD}"), main.as_str())),
                    (false, false) => Ok(JsString::concat(
                        js_str!("\u{FFFD}"),
                        JsString::concat(main.as_str(), js_str!("\u{FFFD}")).as_str(),
                    )),
                }
            }

            #[cfg(feature = "encoding")]
            Encoding::Ref(e) => {
                let mut decoder = e.new_decoder();
                let capacity = decoder
                    .max_utf16_buffer_length(bytes.len())
                    .ok_or(js_error!(TypeError: "Decoding failed"))?;
                let mut utf16 = Vec::with_capacity(capacity);

                let (a, b, c) = decoder.decode_to_utf16(&bytes, &mut utf16, text.is_none());

                let trap = if fatal {
                    encoding::DecoderTrap::Strict
                } else {
                    encoding::DecoderTrap::Replace
                };

                e.decode(&bytes, trap)
                    .map(JsString::from)
                    .map_err(|e| js_error!(TypeError: "Decoding failed: {}", e))
            }
        }
    }
}

js_class! {
    class TextDecoder {
        property encoding {
            fn get(this: JsClass<TextDecoder>) -> Encoding {
                this.borrow().encoding
            }
        }

        property fatal {
            fn get(this: JsClass<TextDecoder>) -> bool {
                this.borrow().fatal
            }
        }

        // Creates a new `TextEncoder` object. Encoding is optional but MUST BE
        // "utf-8" if specified. Options is ignored.
        constructor(encoding: Option<Encoding>, options: Option<TextDecoderOptions>) {
            let encoding = encoding.unwrap_or_default();
            let options = options.unwrap_or_default();
            Ok(TextDecoder {
                encoding,
                fatal: options.fatal(),
                ignore_bom: options.ignore_bom(),
            })
        }

        fn decode(this: JsClass<TextDecoder>, array: Option<JsUint8Array>, context: &mut Context) -> JsResult<JsString> {
            this.borrow().decode(array, context)
        }

    }
}

/// The `TextEncoder`[mdn] class represents an encoder for a specific method, that is
/// a specific character encoding, like `utf-8`.
///
/// [mdn]: https://developer.mozilla.org/en-US/docs/Web/API/TextEncoder
#[derive(Debug, Clone, JsData, Trace, Finalize)]
pub struct TextEncoder;

impl TextEncoder {
    /// Register the `TextEncoder` class into the realm.
    ///
    /// # Errors
    /// This will error if the context or realm cannot register the class.
    pub fn register(context: &mut Context) -> JsResult<()> {
        context.register_global_class::<Self>()?;
        Ok(())
    }

    /// The `encode()` method of the `TextEncoder` interface returns a `Uint8Array` containing
    /// the given string encoded in the specific method.
    ///
    /// # Errors
    /// This will error if there is an issue creating the `Uint8Array`.
    pub fn encode(text: &JsString, context: &mut Context) -> JsResult<JsUint8Array> {
        // TODO: move this logic to JsString.
        JsUint8Array::from_iter(
            text.code_points().flat_map(|s| match s {
                CodePoint::Unicode(c) => c.to_string().as_bytes().to_vec(),
                CodePoint::UnpairedSurrogate(_) => "\u{FFFD}".as_bytes().to_vec(),
            }),
            context,
        )
    }
}

js_class! {
    class TextEncoder {
        property encoding {
            fn get() -> JsString {
                js_string!("utf-8")
            }
        }

        // Creates a new `TextEncoder` object. Encoding is optional but MUST BE
        // "utf-8" if specified. Options is ignored.
        constructor(encoding: Option<JsString>, _options: Option<JsObject>) {
            if let Some(e) = encoding {
                if e != js_string!("utf-8") {
                    return Err(JsNativeError::typ().with_message("Only utf-8 encoding is supported").into());
                }
            }
            Ok(TextEncoder)
        }

        fn encode(text: JsString, context: &mut Context) -> JsResult<JsUint8Array> {
            TextEncoder::encode(&text, context)
        }
    }
}
