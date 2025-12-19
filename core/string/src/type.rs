//! Module containing string types public and crate-specific.
use crate::vtable::SequenceString;
use crate::{JsStr, JsStringKind};
use std::alloc::Layout;

pub(crate) mod sealed {
    use crate::{JsStr, JsStringKind};
    use std::alloc::Layout;

    /// Seal to prevent others from implementing their own string types.
    pub trait Sealed {}

    /// Internal trait for crate-specific usage. Contains implementation details
    /// that should not leak through the API.
    #[allow(private_interfaces)]
    pub trait InternalStringType {
        /// The offset to the data field in the sequence string struct.
        const DATA_OFFSET: usize;

        /// The kind of string produced by this string type.
        const KIND: JsStringKind;

        /// The type of one character for this string type.
        type Byte: Copy + Eq + 'static;

        /// Create the base layout for the sequence string header.
        fn base_layout() -> Layout;

        /// Construct a [`JsStr`] from a slice of characters.
        fn str_ctor(slice: &[Self::Byte]) -> JsStr<'_>;

        /// Write the sequence string header to the given pointer.
        ///
        /// # Safety
        /// The pointer must be valid and properly aligned for writing.
        unsafe fn write_header(ptr: *mut (), len: usize);
    }
}
use sealed::{InternalStringType, Sealed};

/// Trait that maps the data type to the appropriate internal types and constants.
pub trait StringType: InternalStringType + Sealed {
    type Char: Copy + Eq + 'static;
}

#[allow(missing_copy_implementations)]
#[derive(Debug)]
pub struct Ascii;

impl Sealed for Ascii {}
impl StringType for Ascii {
    type Char = u8;
}

#[allow(private_interfaces)]
impl InternalStringType for Ascii {
    const DATA_OFFSET: usize = size_of::<AsciiSequenceString>();
    const KIND: JsStringKind = JsStringKind::AsciiSequence;
    type Byte = u8;

    fn base_layout() -> Layout {
        Layout::new::<AsciiSequenceString>()
    }

    fn str_ctor(slice: &[Self::Byte]) -> JsStr<'_> {
        // SAFETY: This is valid UTF8 since it is ASCII.
        let str = unsafe { str::from_utf8_unchecked(slice) };
        JsStr::ascii(str)
    }

    unsafe fn write_header(ptr: *mut (), len: usize) {
        // SAFETY: Caller must ensure ptr is valid and aligned.
        unsafe {
            ptr.cast::<AsciiSequenceString>()
                .write(AsciiSequenceString::new(len));
        }
    }
}

#[allow(missing_copy_implementations)]
#[derive(Debug)]
pub struct Latin1;

impl Sealed for Latin1 {}
impl StringType for Latin1 {
    type Char = u8;
}

#[allow(private_interfaces)]
impl InternalStringType for Latin1 {
    const DATA_OFFSET: usize = size_of::<Latin1SequenceString>();
    const KIND: JsStringKind = JsStringKind::Latin1Sequence;
    type Byte = u8;

    fn base_layout() -> Layout {
        Layout::new::<Latin1SequenceString>()
    }

    fn str_ctor(slice: &[Self::Byte]) -> JsStr<'_> {
        JsStr::latin1(slice)
    }

    unsafe fn write_header(ptr: *mut (), len: usize) {
        // SAFETY: Caller must ensure ptr is valid and aligned.
        unsafe {
            ptr.cast::<Latin1SequenceString>()
                .write(Latin1SequenceString::new(len));
        }
    }
}

#[allow(missing_copy_implementations)]
#[derive(Debug)]
pub struct Utf16;

impl Sealed for Utf16 {}
impl StringType for Utf16 {
    type Char = u16;
}

#[allow(private_interfaces)]
impl InternalStringType for Utf16 {
    const DATA_OFFSET: usize = size_of::<Utf16SequenceString>();
    const KIND: JsStringKind = JsStringKind::Utf16Sequence;
    type Byte = u16;

    fn base_layout() -> Layout {
        Layout::new::<Utf16SequenceString>()
    }

    fn str_ctor(slice: &[Self::Byte]) -> JsStr<'_> {
        JsStr::utf16(slice)
    }

    unsafe fn write_header(ptr: *mut (), len: usize) {
        // SAFETY: Caller must ensure ptr is valid.
        unsafe {
            ptr.cast::<Utf16SequenceString>()
                .write(Utf16SequenceString::new(len));
        }
    }
}

pub(crate) type AsciiSequenceString = SequenceString<Ascii>;
pub(crate) type Latin1SequenceString = SequenceString<Latin1>;
pub(crate) type Utf16SequenceString = SequenceString<Utf16>;
