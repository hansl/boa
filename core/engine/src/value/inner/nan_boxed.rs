//! TODO: Redocument this if this works.
//!
//! A NaN-boxed inner value for JavaScript values.
#![allow(clippy::inline_always)]

use crate::{
    JsBigInt, JsObject, JsSymbol, JsVariant, bigint::RawBigInt, object::ErasedVTableObject,
    symbol::RawJsSymbol,
};
use boa_gc::{Finalize, GcBox, Trace, custom_trace};
use boa_string::{JsString, RawJsString};
use boxing::nan::{NanBox, RawBox};
use core::fmt;
use enum_ptr::{Aligned, EnumPtr};
use std::{mem::ManuallyDrop, ptr::NonNull};
use boxing::nan::raw::{RawTag, Value};

unsafe impl Aligned for JsObject {
    const ALIGNMENT: usize = align_of::<JsObject>();
}
unsafe impl Aligned for JsSymbol {
    const ALIGNMENT: usize = align_of::<JsSymbol>();
}
unsafe impl Aligned for JsBigInt {
    const ALIGNMENT: usize = align_of::<JsBigInt>();
}

/// The tag of the value.
enum TagValue {
    Null = 0,
    Undefined,
    Boolean,
    Integer32,
    BigInt,
    Object,
    Symbol,
    String,
}

#[derive(Clone, EnumPtr, Debug)]
#[repr(C, usize)]
enum TaggedInner {
    Object(JsObject),
    String(JsString),
    Symbol(JsSymbol),
    BigInt(JsBigInt),
}

/// A NaN-boxed [`JsValue`]'s inner.
pub(crate) struct NanBoxedValue(RawBox);

impl fmt::Debug for NanBoxedValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.as_variant() {
            JsVariant::Undefined => f.debug_tuple("Undefined").finish(),
            JsVariant::Null => f.debug_tuple("Null").finish(),
            JsVariant::Boolean(b) => f.debug_tuple("Boolean").field(&b).finish(),
            JsVariant::Float64(n) => f.debug_tuple("Float64").field(&n).finish(),
            JsVariant::Integer32(n) => f.debug_tuple("Integer32").field(&n).finish(),
            JsVariant::BigInt(n) => f.debug_tuple("BigInt").field(&n).finish(),
            JsVariant::Object(n) => f.debug_tuple("Object").field(&n).finish(),
            JsVariant::Symbol(n) => f.debug_tuple("Symbol").field(&n).finish(),
            JsVariant::String(n) => f.debug_tuple("String").field(&n).finish(),
        }
    }
}

impl Finalize for NanBoxedValue {
    fn finalize(&self) {
        if let Some(o) = self.as_object() {
            o.finalize();
        }
    }
}

#[allow(unsafe_op_in_unsafe_fn)]
unsafe impl Trace for NanBoxedValue {
    custom_trace! {this, mark, {
        if let Some(o) = this.as_object() {
            mark(&o);
        }
    }}
}

impl Clone for NanBoxedValue {
    #[inline(always)]
    fn clone(&self) -> Self {
        if let Some(o) = self.as_object() {
            Self::object(o.clone())
        } else if let Some(s) = self.as_string() {
            Self::string(s.clone())
        } else if let Some(b) = self.as_bigint() {
            Self::bigint(b.clone())
        } else if let Some(s) = self.as_symbol() {
            Self::symbol(s.clone())
        } else {
            Self(self.0.clone())
        }
    }
}

impl NanBoxedValue {
    /// Returns a `InnerValue` from a Null.
    #[must_use]
    #[inline(always)]
    pub(crate) fn null() -> Self {
        Self(RawBox::from_value(Value::empty(RawTag::new(false, TagValue::Null as _))))
    }

    /// Returns a `InnerValue` from an undefined.
    #[must_use]
    #[inline(always)]
    pub(crate) fn undefined() -> Self {
        Self(NanBox::from(0u8))
    }

    /// Returns a `InnerValue` from a 64-bits float. If the float is `NaN`,
    /// it will be reduced to a canonical `NaN` representation.
    #[must_use]
    #[inline(always)]
    pub(crate) const fn float64(value: f64) -> Self {
        Self(NanBox::from(value))
    }

    /// Returns a `InnerValue` from a 32-bits integer.
    #[must_use]
    #[inline(always)]
    pub(crate) const fn integer32(value: i32) -> Self {
        Self(NanBox::from(value))
    }

    /// Returns a `InnerValue` from a boolean.
    #[must_use]
    #[inline(always)]
    pub(crate) const fn boolean(value: bool) -> Self {
        Self(NanBox::from(value as u16))
    }

    /// Returns a `InnerValue` from a boxed [`JsBigInt`].
    #[must_use]
    #[inline(always)]
    pub(crate) fn bigint(value: JsBigInt) -> Self {
        Self(NanBox::from(Box::new(TaggedInner::BigInt(value))))
    }

    /// Returns a `InnerValue` from a boxed [`JsObject`].
    #[must_use]
    #[inline(always)]
    pub(crate) fn object(value: JsObject) -> Self {
        Self(NanBox::from(Box::new(TaggedInner::Object(value))))
    }

    /// Returns a `InnerValue` from a boxed [`JsSymbol`].
    #[must_use]
    #[inline(always)]
    pub(crate) fn symbol(value: JsSymbol) -> Self {
        Self(NanBox::from(Box::new(TaggedInner::Symbol(value))))
    }

    /// Returns a `InnerValue` from a boxed [`JsString`].
    #[must_use]
    #[inline(always)]
    pub(crate) fn string(value: JsString) -> Self {
        Self(NanBox::from(Box::new(TaggedInner::String(value))))
    }

    /// Returns true if a value is undefined.
    #[must_use]
    #[inline(always)]
    pub(crate) fn is_undefined(&self) -> bool {
        self.0.try_ref_inline::<u8>() == Some(&0)
    }

    /// Returns true if a value is null.
    #[must_use]
    #[inline(always)]
    pub(crate) fn is_null(&self) -> bool {
        self.0.try_ref_inline::<u8>() == Some(&1)
    }

    /// Returns true if a value is a boolean.
    #[must_use]
    #[inline(always)]
    pub(crate) fn is_bool(&self) -> bool {
        self.0.try_ref_inline::<u16>().is_some()
    }

    /// Returns true if a value is a 64-bits float.
    #[must_use]
    #[inline(always)]
    pub(crate) fn is_float64(&self) -> bool {
        self.0.is_float()
    }

    /// Returns true if a value is a 32-bits integer.
    #[must_use]
    #[inline(always)]
    pub(crate) fn is_integer32(&self) -> bool {
        self.0.try_ref_inline::<i32>().is_some()
    }

    /// Returns true if a value is a [`JsBigInt`]. A `NaN` will not match here.
    #[must_use]
    #[inline(always)]
    pub(crate) fn is_bigint(&self) -> bool {
        matches!(self.0.try_ref_boxed(), Some(TaggedInner::BigInt(_)))
    }

    /// Returns true if a value is a boxed Object.
    #[must_use]
    #[inline(always)]
    pub(crate) fn is_object(&self) -> bool {
        matches!(self.0.try_ref_boxed(), Some(TaggedInner::Object(_)))
    }

    /// Returns true if a value is a boxed Symbol.
    #[must_use]
    #[inline(always)]
    pub(crate) fn is_symbol(&self) -> bool {
        matches!(self.0.try_ref_boxed(), Some(TaggedInner::Symbol(_)))
    }

    /// Returns true if a value is a boxed String.
    #[must_use]
    #[inline(always)]
    pub(crate) fn is_string(&self) -> bool {
        matches!(self.0.try_ref_boxed(), Some(TaggedInner::String(_)))
    }

    /// Returns the value as a f64 if it is a float.
    #[must_use]
    #[inline(always)]
    pub(crate) fn as_float64(&self) -> Option<f64> {
        self.0.try_ref_float().copied()
    }

    /// Returns the value as an i32 if it is an integer.
    #[must_use]
    #[inline(always)]
    pub(crate) fn as_integer32(&self) -> Option<i32> {
        self.0.try_ref_inline().copied()
    }

    /// Returns the value as a boolean if it is a boolean.
    #[must_use]
    #[inline(always)]
    pub(crate) fn as_bool(&self) -> Option<bool> {
        self.0.try_ref_inline::<u16>().copied().map(|v| v != 0)
    }

    /// Returns the value as a boxed [`JsBigInt`].
    #[must_use]
    #[inline(always)]
    pub(crate) fn as_bigint(&self) -> Option<JsBigInt> {
        self.0.try_ref_boxed().and_then(|inner| {
            if let TaggedInner::BigInt(v) = inner {
                Some(v.clone())
            } else {
                None
            }
        })
    }

    /// Returns the value as a boxed [`JsObject`].
    #[must_use]
    #[inline(always)]
    pub(crate) fn as_object(&self) -> Option<JsObject> {
        self.0.try_ref_boxed().and_then(|inner| {
            if let TaggedInner::Object(v) = inner {
                Some(v.clone())
            } else {
                None
            }
        })
    }

    /// Returns the value as a [`JsSymbol`].
    #[must_use]
    #[inline(always)]
    pub(crate) fn as_symbol(&self) -> Option<JsSymbol> {
        self.0.try_ref_boxed().and_then(|inner| {
            if let TaggedInner::Symbol(v) = inner {
                Some(v.clone())
            } else {
                None
            }
        })
    }

    /// Returns the value as a boxed [`JsString`].
    #[must_use]
    #[inline(always)]
    pub(crate) fn as_string(&self) -> Option<JsString> {
        self.0.try_ref_boxed().and_then(|inner| {
            if let TaggedInner::String(v) = inner {
                Some(v.clone())
            } else {
                None
            }
        })
    }

    /// Returns the [`JsVariant`] of this inner value.
    #[must_use]
    #[inline(always)]
    pub(crate) fn as_variant(&self) -> JsVariant {
        if self.is_null() {
            JsVariant::Null
        } else if self.is_undefined() {
            JsVariant::Undefined
        }
        if let Some(b) = self.as_bool() {
            JsVariant::Boolean(b)
        } else if let
    }
}

impl Drop for NanBoxedValue {
    #[inline(always)]
    fn drop(&mut self) {
        match self.value() & bits::MASK_KIND {
            bits::MASK_OBJECT => {
                unsafe { ManuallyDrop::into_inner(self.as_object_unchecked()) };
            }
            bits::MASK_STRING => {
                unsafe { ManuallyDrop::into_inner(self.as_string_unchecked()) };
            }
            bits::MASK_SYMBOL => {
                unsafe { ManuallyDrop::into_inner(self.as_symbol_unchecked()) };
            }
            bits::MASK_BIGINT => {
                unsafe { ManuallyDrop::into_inner(self.as_bigint_unchecked()) };
            }
            _ => {}
        }
    }
}

#[cfg(test)]
macro_rules! assert_type {
    (@@is $value: ident, $u: literal, $n: literal, $b: literal, $i: literal, $f: literal, $bi: literal, $s: literal, $o: literal, $sy: literal) => {
        assert_eq!($u  != 0, $value.is_undefined());
        assert_eq!($n  != 0, $value.is_null());
        assert_eq!($b  != 0, $value.is_bool());
        assert_eq!($i  != 0, $value.is_integer32());
        assert_eq!($f  != 0, $value.is_float64());
        assert_eq!($bi != 0, $value.is_bigint());
        assert_eq!($s  != 0, $value.is_string());
        assert_eq!($o  != 0, $value.is_object());
        assert_eq!($sy != 0, $value.is_symbol());
    };
    (@@as $value: ident, $u: literal, $n: literal, $b: literal, $i: literal, $f: literal, $bi: literal, $s: literal, $o: literal, $sy: literal) => {
        if $b  == 0 { assert_eq!($value.as_bool(), None); }
        if $i  == 0 { assert_eq!($value.as_integer32(), None); }
        if $f  == 0 { assert_eq!($value.as_float64(), None); }
        if $bi == 0 { assert_eq!($value.as_bigint(), None); }
        if $s  == 0 { assert_eq!($value.as_string(), None); }
        if $o  == 0 { assert_eq!($value.as_object(), None); }
        if $sy == 0 { assert_eq!($value.as_symbol(), None); }
    };
    ($value: ident is undefined) => {
        assert_type!(@@is $value, 1, 0, 0, 0, 0, 0, 0, 0, 0);
        assert_eq!($value.as_variant(), JsVariant::Undefined);
    };
    ($value: ident is null) => {
        assert_type!(@@is $value, 0, 1, 0, 0, 0, 0, 0, 0, 0);
        assert_eq!($value.as_variant(), JsVariant::Null);
    };
    ($value: ident is bool($scalar: ident)) => {
        assert_type!(@@is $value, 0, 0, 1, 0, 0, 0, 0, 0, 0);
        assert_type!(@@as $value, 0, 0, 1, 0, 0, 0, 0, 0, 0);
        assert_eq!(Some($scalar), $value.as_bool());
        assert_eq!($value.as_variant(), JsVariant::Boolean($scalar));
    };
    ($value: ident is integer($scalar: ident)) => {
        assert_type!(@@is $value, 0, 0, 0, 1, 0, 0, 0, 0, 0);
        assert_type!(@@as $value, 0, 0, 0, 1, 0, 0, 0, 0, 0);
        assert_eq!(Some($scalar), $value.as_integer32());
        assert_eq!($value.as_variant(), JsVariant::Integer32($scalar));
    };
    ($value: ident is float($scalar: ident)) => {
        assert_type!(@@is $value, 0, 0, 0, 0, 1, 0, 0, 0, 0);
        assert_type!(@@as $value, 0, 0, 0, 0, 1, 0, 0, 0, 0);
        assert_eq!(Some($scalar), $value.as_float64());
        // Verify parity.
        assert_eq!(Some(1.0 / $scalar), $value.as_float64().map(|f| 1.0 / f));
        assert_eq!($value.as_variant(), JsVariant::Float64($scalar));

        // Verify that the clone is still the same.
        let new_value = $value.clone();

        assert_eq!(Some($scalar), new_value.as_float64());
        assert_eq!($value.as_float64(), new_value.as_float64());
        // Verify parity.
        assert_eq!(Some(1.0 / $scalar), new_value.as_float64().map(|f| 1.0 / f));
        assert_eq!(new_value.as_variant(), JsVariant::Float64($scalar));

        let JsVariant::Float64(new_scalar) = new_value.as_variant() else {
            panic!("Expected Float64, got {:?}", new_value.as_variant());
        };
        assert_eq!(Some(new_scalar), new_value.as_float64());
        assert_eq!($value.as_float64(), new_value.as_float64());
        // Verify parity.
        assert_eq!(Some(1.0 / new_scalar), new_value.as_float64().map(|f| 1.0 / f));
        assert_eq!(new_value.as_variant(), JsVariant::Float64(new_scalar));
    };
    ($value: ident is nan) => {
        assert_type!(@@is $value, 0, 0, 0, 0, 1, 0, 0, 0, 0);
        assert_type!(@@as $value, 0, 0, 0, 0, 1, 0, 0, 0, 0);
        assert!($value.as_float64().unwrap().is_nan());
        assert!(matches!($value.as_variant(), JsVariant::Float64(f) if f.is_nan()));
    };
    ($value: ident is bigint($scalar: ident)) => {
        assert_type!(@@is $value, 0, 0, 0, 0, 0, 1, 0, 0, 0);
        assert_type!(@@as $value, 0, 0, 0, 0, 0, 1, 0, 0, 0);
        assert_eq!(Some(&$scalar), $value.as_bigint().as_ref());
        assert_eq!($value.as_variant(), JsVariant::BigInt($scalar));
    };
    ($value: ident is object($scalar: ident)) => {
        assert_type!(@@is $value, 0, 0, 0, 0, 0, 0, 0, 1, 0);
        assert_type!(@@as $value, 0, 0, 0, 0, 0, 0, 0, 1, 0);
        assert_eq!(Some(&$scalar), $value.as_object().as_ref());
        assert_eq!($value.as_variant(), JsVariant::Object($scalar));
    };
    ($value: ident is symbol($scalar: ident)) => {
        assert_type!(@@is $value, 0, 0, 0, 0, 0, 0, 0, 0, 1);
        assert_type!(@@as $value, 0, 0, 0, 0, 0, 0, 0, 0, 1);
        assert_eq!(Some(&$scalar), $value.as_symbol().as_ref());
        assert_eq!($value.as_variant(), JsVariant::Symbol($scalar));
    };
    ($value: ident is string($scalar: ident)) => {
        assert_type!(@@is $value, 0, 0, 0, 0, 0, 0, 1, 0, 0);
        assert_type!(@@as $value, 0, 0, 0, 0, 0, 0, 1, 0, 0);
        assert_eq!(Some(&$scalar), $value.as_string().as_ref());
        assert_eq!($value.as_variant(), JsVariant::String($scalar));
    };
}

#[test]
fn null() {
    let v = NanBoxedValue::null();
    assert_type!(v is null);
}

#[test]
fn undefined() {
    let v = NanBoxedValue::undefined();
    assert_type!(v is undefined);
}

#[test]
fn boolean() {
    let v = NanBoxedValue::boolean(true);
    assert_type!(v is bool(true));

    let v = NanBoxedValue::boolean(false);
    assert_type!(v is bool(false));
}

#[test]
fn integer() {
    fn assert_integer(i: i32) {
        let v = NanBoxedValue::integer32(i);
        assert_type!(v is integer(i));
    }

    assert_integer(0);
    assert_integer(1);
    assert_integer(-1);
    assert_integer(42);
    assert_integer(-42);
    assert_integer(i32::MAX);
    assert_integer(i32::MIN);
    assert_integer(i32::MAX - 1);
    assert_integer(i32::MIN + 1);
}

#[test]
#[allow(clippy::float_cmp)]
fn float() {
    fn assert_float(f: f64) {
        let v = NanBoxedValue::float64(f);
        assert_type!(v is float(f));
    }

    assert_float(0.0);
    assert_float(-0.0);
    assert_float(0.1 + 0.2);
    assert_float(-42.123);
    assert_float(f64::INFINITY);
    assert_float(f64::NEG_INFINITY);

    // Some edge cases around zeroes.
    let neg_zero = NanBoxedValue::float64(-0.0);
    assert!(neg_zero.as_float64().unwrap().is_sign_negative());
    assert_eq!(0.0f64, neg_zero.as_float64().unwrap());

    let pos_zero = NanBoxedValue::float64(0.0);
    assert!(!pos_zero.as_float64().unwrap().is_sign_negative());
    assert_eq!(0.0f64, pos_zero.as_float64().unwrap());

    assert_eq!(pos_zero.as_float64(), neg_zero.as_float64());

    let nan = NanBoxedValue::float64(f64::NAN);
    assert_type!(nan is nan);
}

#[test]
fn bigint() {
    let bigint = JsBigInt::from(42);
    let v = NanBoxedValue::bigint(bigint.clone());
    assert_type!(v is bigint(bigint));
}

#[test]
fn object() {
    let object = JsObject::with_null_proto();
    let v = NanBoxedValue::object(object.clone());
    assert_type!(v is object(object));
}

#[test]
fn string() {
    let str = crate::js_string!("Hello World");
    let v = NanBoxedValue::string(str.clone());
    assert_type!(v is string(str));
}

#[test]
fn symbol() {
    let sym = JsSymbol::new(Some(JsString::from("Hello World"))).unwrap();
    let v = NanBoxedValue::symbol(sym.clone());
    assert_type!(v is symbol(sym));

    let sym = JsSymbol::new(None).unwrap();
    let v = NanBoxedValue::symbol(sym.clone());
    assert_type!(v is symbol(sym));
}
