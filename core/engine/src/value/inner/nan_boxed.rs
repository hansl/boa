//! TODO: Redocument this if this works.
//!
//! A NaN-boxed inner value for JavaScript values.
#![allow(clippy::inline_always)]

use crate::{JsBigInt, JsObject, JsSymbol, JsVariant};
use boa_gc::{Finalize, Trace, custom_trace};
use boa_string::JsString;
use core::fmt;
use std::mem::ManuallyDrop;
use std::ptr::NonNull;

mod raw;
mod singlenan;
use raw::*;

/// The tag of the value.
enum PrimitiveTag {
    Null,
    Undefined,
    Boolean,
    Integer32,
    BigInt,
    Object,
    Symbol,
    String,
}

impl PrimitiveTag {
    const fn raw(self) -> RawTag {
        // SAFETY: Cannot fail as we define all valid variants of this enum.
        unsafe {
            match self {
                PrimitiveTag::Null => RawTag::new_unchecked(false, 1),
                PrimitiveTag::Undefined => RawTag::new_unchecked(false, 2),
                PrimitiveTag::Boolean => RawTag::new_unchecked(false, 3),
                PrimitiveTag::Integer32 => RawTag::new_unchecked(false, 4),
                PrimitiveTag::BigInt => RawTag::new_unchecked(true, 1),
                PrimitiveTag::Object => RawTag::new_unchecked(true, 2),
                PrimitiveTag::Symbol => RawTag::new_unchecked(true, 3),
                PrimitiveTag::String => RawTag::new_unchecked(true, 4),
            }
        }
    }
}

impl Into<RawTag> for PrimitiveTag {
    #[inline(always)]
    fn into(self) -> RawTag {
        self.raw()
    }
}

impl Into<PrimitiveTag> for RawTag {
    #[inline(always)]
    fn into(self) -> PrimitiveTag {
        match self.neg_val() {
            (false, 1) => PrimitiveTag::Null,
            (false, 2) => PrimitiveTag::Undefined,
            (false, 3) => PrimitiveTag::Boolean,
            (false, 4) => PrimitiveTag::Integer32,
            (true, 1) => PrimitiveTag::BigInt,
            (true, 2) => PrimitiveTag::Object,
            (true, 3) => PrimitiveTag::Symbol,
            (true, 4) => PrimitiveTag::String,
            _ => unsafe { std::hint::unreachable_unchecked() },
        }
    }
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
    pub(crate) const fn null() -> Self {
        Self(RawBox::from_value(Value::empty(PrimitiveTag::Null.raw())))
    }

    /// Returns a `InnerValue` from an undefined.
    #[must_use]
    #[inline(always)]
    pub(crate) const fn undefined() -> Self {
        Self(RawBox::from_value(Value::empty(
            PrimitiveTag::Undefined.raw(),
        )))
    }

    /// Returns a `InnerValue` from a 64-bits float. If the float is `NaN`,
    /// it will be reduced to a canonical `NaN` representation.
    #[must_use]
    #[inline(always)]
    pub(crate) const fn float64(value: f64) -> Self {
        Self(RawBox::from_float(value))
    }

    /// Returns a `InnerValue` from a 32-bits integer.
    #[must_use]
    #[inline(always)]
    pub(crate) fn integer32(value: i32) -> Self {
        Self(RawBox::from_value(Value::store(
            PrimitiveTag::Integer32.raw(),
            value,
        )))
    }

    /// Returns a `InnerValue` from a boolean.
    #[must_use]
    #[inline(always)]
    pub(crate) fn boolean(value: bool) -> Self {
        Self(RawBox::from_value(Value::store(
            PrimitiveTag::Boolean.into(),
            value,
        )))
    }

    /// Returns a `InnerValue` from a boxed [`JsBigInt`].
    #[must_use]
    #[inline(always)]
    pub(crate) fn bigint(value: JsBigInt) -> Self {
        let raw = value.into_raw();
        Self(RawBox::from_value(Value::store(
            PrimitiveTag::BigInt.into(),
            raw,
        )))
    }

    /// Returns a `InnerValue` from a boxed [`JsObject`].
    #[must_use]
    #[inline(always)]
    pub(crate) fn object(value: JsObject) -> Self {
        let raw = value.into_raw();
        Self(RawBox::from_value(Value::store(
            PrimitiveTag::Object.into(),
            raw.as_ptr(),
        )))
    }

    /// Returns a `InnerValue` from a boxed [`JsSymbol`].
    #[must_use]
    #[inline(always)]
    pub(crate) fn symbol(value: JsSymbol) -> Self {
        let raw = value.into_raw();
        Self(RawBox::from_value(Value::store(
            PrimitiveTag::Symbol.into(),
            raw.as_ptr(),
        )))
    }

    /// Returns a `InnerValue` from a boxed [`JsString`].
    #[must_use]
    #[inline(always)]
    pub(crate) fn string(value: JsString) -> Self {
        let raw = value.into_raw();
        Self(RawBox::from_value(Value::store(
            PrimitiveTag::String.into(),
            raw.as_ptr(),
        )))
    }

    /// Returns true if a value is undefined.
    #[must_use]
    #[inline(always)]
    pub(crate) fn is_undefined(&self) -> bool {
        self.0.tag() == Some(PrimitiveTag::Undefined.into())
    }

    /// Returns true if a value is null.
    #[must_use]
    #[inline(always)]
    pub(crate) fn is_null(&self) -> bool {
        self.0.tag() == Some(PrimitiveTag::Null.into())
    }

    /// Returns true if a value is a boolean.
    #[must_use]
    #[inline(always)]
    pub(crate) fn is_bool(&self) -> bool {
        self.0.tag() == Some(PrimitiveTag::Boolean.into())
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
        self.0.tag() == Some(PrimitiveTag::Integer32.into())
    }

    /// Returns true if a value is a [`JsBigInt`]. A `NaN` will not match here.
    #[must_use]
    #[inline(always)]
    pub(crate) fn is_bigint(&self) -> bool {
        self.0.tag() == Some(PrimitiveTag::BigInt.into())
    }

    /// Returns true if a value is a boxed Object.
    #[must_use]
    #[inline(always)]
    pub(crate) fn is_object(&self) -> bool {
        self.0.tag() == Some(PrimitiveTag::Object.into())
    }

    /// Returns true if a value is a boxed Symbol.
    #[must_use]
    #[inline(always)]
    pub(crate) fn is_symbol(&self) -> bool {
        self.0.tag() == Some(PrimitiveTag::Symbol.into())
    }

    /// Returns true if a value is a boxed String.
    #[must_use]
    #[inline(always)]
    pub(crate) fn is_string(&self) -> bool {
        self.0.tag() == Some(PrimitiveTag::String.into())
    }

    /// Returns the value as a f64 if it is a float.
    #[must_use]
    #[inline(always)]
    pub(crate) fn as_float64(&self) -> Option<f64> {
        self.0.float().copied()
    }

    /// Returns the value as an i32 if it is an integer.
    #[must_use]
    #[inline(always)]
    pub(crate) fn as_integer32(&self) -> Option<i32> {
        if self.is_integer32() {
            self.0.value().map(i32::from_val)
        } else {
            None
        }
    }

    /// Returns the value as a boolean if it is a boolean.
    #[must_use]
    #[inline(always)]
    pub(crate) fn as_bool(&self) -> Option<bool> {
        if self.is_bool() {
            self.0.value().map(bool::from_val)
        } else {
            None
        }
    }

    /// Returns the value as a boxed [`JsBigInt`].
    #[must_use]
    #[inline(always)]
    pub(crate) fn as_bigint(&self) -> Option<JsBigInt> {
        if self.is_bigint() {
            self.0.value().map(|v| {
                let raw = RawStore::from_val(v);
                unsafe { JsBigInt::from_raw(raw) }
            })
        } else {
            None
        }
    }

    /// Returns the value as a boxed [`JsObject`].
    #[must_use]
    #[inline(always)]
    pub(crate) fn as_object(&self) -> Option<JsObject> {
        if self.is_object() {
            self.0.value().map(|v| {
                let raw: *mut _ = RawStore::from_val(v);
                let raw = unsafe { NonNull::new_unchecked(raw) };
                unsafe { JsObject::from_raw(raw) }
            })
        } else {
            None
        }
    }

    /// Returns the value as a [`JsSymbol`].
    #[must_use]
    #[inline(always)]
    pub(crate) fn as_symbol(&self) -> Option<JsSymbol> {
        if self.is_symbol() {
            self.0.value().map(|v| {
                let raw: *mut _ = RawStore::from_val(v);
                let raw = unsafe { NonNull::new_unchecked(raw) };
                unsafe { JsSymbol::from_raw(raw) }
            })
        } else {
            None
        }
    }

    /// Returns the value as a boxed [`JsString`].
    #[must_use]
    #[inline(always)]
    pub(crate) fn as_string(&self) -> Option<JsString> {
        if self.is_string() {
            self.0.value().map(|v| {
                let raw: *mut _ = RawStore::from_val(v);
                let raw = unsafe { NonNull::new_unchecked(raw) };
                unsafe { JsString::from_raw(raw) }
            })
        } else {
            None
        }
    }

    /// Returns the [`JsVariant`] of this inner value.
    #[must_use]
    #[inline(always)]
    pub(crate) fn as_variant(&self) -> JsVariant {
        match self.0.tag().map(|t: RawTag| -> PrimitiveTag { t.into() }) {
            None => JsVariant::Float64(self.as_float64().unwrap()),
            Some(PrimitiveTag::Null) => JsVariant::Null,
            Some(PrimitiveTag::Undefined) => JsVariant::Undefined,
            Some(PrimitiveTag::Boolean) => {
                JsVariant::Boolean(bool::from_val(self.0.value().unwrap()))
            }
            Some(PrimitiveTag::Integer32) => {
                JsVariant::Integer32(i32::from_val(self.0.value().unwrap()))
            }
            Some(PrimitiveTag::BigInt) => JsVariant::BigInt(self.as_bigint().unwrap()),
            Some(PrimitiveTag::Object) => JsVariant::Object(self.as_object().unwrap()),
            Some(PrimitiveTag::Symbol) => JsVariant::Symbol(self.as_symbol().unwrap()),
            Some(PrimitiveTag::String) => JsVariant::String(self.as_string().unwrap()),
        }
    }

    /// Returns the value as a [`JsBigInt`] without checking the inner tag.
    ///
    /// # Safety
    ///
    /// The inner value must be a valid `JsBigInt`.
    #[must_use]
    #[inline(always)]
    unsafe fn as_bigint_unchecked(&self) -> ManuallyDrop<JsBigInt> {
        // let addr = bits::untag_pointer(self.value());
        // // SAFETY: This is guaranteed by the caller.
        // unsafe {
        //     ManuallyDrop::new(JsBigInt::from_raw(
        //         self.ptr.with_addr(addr).cast::<RawBigInt>().cast_const(),
        //     ))
        // }
        todo!();
    }

    /// Returns the value as a boxed [`JsObject`] without checking the inner tag.
    ///
    /// # Safety
    ///
    /// The inner value must be a valid `JsObject`.
    #[must_use]
    #[inline(always)]
    unsafe fn as_object_unchecked(&self) -> ManuallyDrop<JsObject> {
        // let addr = self.0.value().unwrap();
        // let addr:
        // // SAFETY: This is guaranteed by the caller.
        // unsafe {
        //     ManuallyDrop::new(JsObject::from_raw(NonNull::new_unchecked(
        //         self.ptr.with_addr(addr).cast::<GcBox<ErasedVTableObject>>(),
        //     )))
        // }
        todo!();
    }

    /// Returns the value as a [`JsSymbol`] without checking the inner tag.
    ///
    /// # Safety
    ///
    /// The inner value must be a valid `JsSymbol`.
    #[must_use]
    #[inline(always)]
    unsafe fn as_symbol_unchecked(&self) -> ManuallyDrop<JsSymbol> {
        // let addr = bits::untag_pointer(self.value());
        // // SAFETY: This is guaranteed by the caller.
        // unsafe {
        //     ManuallyDrop::new(JsSymbol::from_raw(NonNull::new_unchecked(
        //         self.ptr.with_addr(addr).cast::<RawJsSymbol>(),
        //     )))
        // }
        todo!();
    }

    /// Returns the value as a [`JsString`] without checking the inner tag.
    ///
    /// # Safety
    ///
    /// The inner value must be a valid `JsString`.
    #[must_use]
    #[inline(always)]
    unsafe fn as_string_unchecked(&self) -> ManuallyDrop<JsString> {
        // let addr = bits::untag_pointer(self.value());
        // // SAFETY: the inner address must hold a valid, non-null JsString.
        // unsafe {
        //     ManuallyDrop::new(JsString::from_raw(NonNull::new_unchecked(
        //         self.ptr.with_addr(addr).cast::<RawJsString>(),
        //     )))
        // }
        todo!();
    }
}

impl Drop for NanBoxedValue {
    #[inline(always)]
    fn drop(&mut self) {
        // if let Some(obj) = self.as_object() {
        //     unsafe { ManuallyDrop::into_inner(obj) };
        // }
        //
        // match self.value() & bits::MASK_KIND {
        //     bits::MASK_OBJECT => {
        //         unsafe { ManuallyDrop::into_inner(self.as_object_unchecked()) };
        //     }
        //     bits::MASK_STRING => {
        //         unsafe { ManuallyDrop::into_inner(self.as_string_unchecked()) };
        //     }
        //     bits::MASK_SYMBOL => {
        //         unsafe { ManuallyDrop::into_inner(self.as_symbol_unchecked()) };
        //     }
        //     bits::MASK_BIGINT => {
        //         unsafe { ManuallyDrop::into_inner(self.as_bigint_unchecked()) };
        //     }
        //     _ => {}
        // }
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
