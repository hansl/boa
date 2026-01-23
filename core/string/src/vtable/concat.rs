//! `VTable` implementations for [`ConcatString`].

use crate::iter::CodePointsIter;
use crate::r#type::{InternalStringType, Latin1, Utf16};
use crate::vtable::{JsStringVTable, SequenceString};
use crate::{JsStr, JsStrVariant, JsString, JsStringKind, StaticJsStrings, alloc_overflow};
use std::alloc::{Layout, alloc, dealloc};
use std::cell::Cell;
use std::marker::PhantomData;
use std::process::abort;
use std::ptr;
use std::ptr::NonNull;

/// A sequential memory array of `T::Char` elements.
#[repr(C)]
pub(crate) struct ConcatString {
    /// Embedded `VTable` - must be the first field for vtable dispatch.
    vtable: JsStringVTable,
    refcount: Cell<usize>,
    // The string can either be normalized or an array of substrings concatenated.
    // If the string is normalized, the strings will be dropped and invalidated.
    normalized: Cell<Option<JsString>>,
    /// The number of strings concatenated.
    count: usize,
    strings: [JsString; 0],
}

impl ConcatString {
    /// Creates a [`ConcatString`] without data. This should only be used to write to
    /// an allocation which contains all the information.
    #[inline]
    #[must_use]
    pub(crate) fn new(strings: &[JsString]) -> Self {
        ConcatString {
            vtable: JsStringVTable {
                clone,
                drop,
                as_str,
                code_points,
                refcount,
                len: strings.iter().map(JsString::len).sum(),
                kind: JsStringKind::Concat,
            },
            refcount: Cell::new(1),
            normalized: Cell::new(None),
            count: strings.len(),
            strings: [],
        }
    }

    /// Allocates a new [`ConcatString`] with an internal capacity of `len` substrings.
    ///
    /// # Panics
    ///
    /// Panics if `try_allocate_seq` returns `Err`.
    pub(crate) fn allocate(array: &[JsString]) -> NonNull<ConcatString> {
        match Self::try_allocate(array) {
            Ok(v) => v,
            Err(None) => alloc_overflow(),
            Err(Some(layout)) => std::alloc::handle_alloc_error(layout),
        }
    }

    /// Allocates a new [`ConcatString`] with an internal capacity of `len` substrings.
    ///
    /// # Errors
    ///
    /// Returns `Err(None)` on integer overflows `usize::MAX`.
    /// Returns `Err(Some(Layout))` on allocation error.
    pub(crate) fn try_allocate(array: &[JsString]) -> Result<NonNull<Self>, Option<Layout>> {
        let (layout, offset) = Layout::array::<JsString>(array.len())
            .and_then(|arr| Layout::new::<Self>().extend(arr))
            .map(|(layout, offset)| (layout.pad_to_align(), offset))
            .map_err(|_| None)?;

        debug_assert_eq!(offset, size_of::<Self>());
        debug_assert_eq!(layout.align(), align_of::<Self>());

        #[allow(clippy::cast_ptr_alignment)]
        // SAFETY:
        // The layout size of `ConcatString` is never zero, since it has to store
        // the length of the string and the reference count.
        let inner = unsafe { alloc(layout).cast::<Self>() };

        // We need to verify that the pointer returned by `alloc` is not null, otherwise
        // we should abort, since an allocation error is pretty unrecoverable for us
        // right now.
        let inner = NonNull::new(inner).ok_or(Some(layout))?;

        // SAFETY:
        // `NonNull` verified for us that the pointer returned by `alloc` is valid,
        // meaning we can write to its pointed memory.
        unsafe {
            // Write the first part, the `ConcatString`.
            inner.as_ptr().write(Self::new(array));
        }

        debug_assert!({
            let inner = inner.as_ptr();
            // SAFETY:
            // - `inner` must be a valid pointer, since it comes from a `NonNull`,
            // meaning we can safely dereference it to `ConcatString`.
            // - `offset` should point us to the beginning of the array,
            // and since we requested a `ConcatString` layout with a trailing
            // `[T::Byte; str_len]`, the memory of the array must be in the `usize`
            // range for the allocation to succeed.
            unsafe {
                // This is `<u8>` as the offset is in bytes.
                ptr::eq(
                    inner.cast::<u8>().add(offset).cast(),
                    (*inner).strings().as_ptr(),
                )
            }
        });

        Ok(inner)
    }

    pub(crate) fn populate(&mut self, array: &[JsString]) -> Self {
        // SAFETY: We verify all this when constructing the string and populating it.
        unsafe { std::slice::from_raw_parts(self.strings.as_ptr(), self.count) }
    }

    pub(crate) fn strings(&self) -> &'_ [JsString] {
        // SAFETY: We verify all this when constructing the string and populating it.
        unsafe { std::slice::from_raw_parts(self.strings.as_ptr(), self.count) }
    }

    pub(crate) fn normalize(&self) -> &'_ JsString {
        // SAFETY: This is the only method that uses or builds the normalized field.
        if let Some(Some(ref s)) = unsafe { self.normalized.as_ptr().as_ref() } {
            return s;
        }

        let strings = self.strings();

        let mut latin1_encoding = true;
        let mut full_count = 0usize;
        for string in strings {
            let Some(sum) = full_count.checked_add(string.len()) else {
                alloc_overflow()
            };
            if !matches!(string.variant(), JsStrVariant::Latin1(_)) {
                latin1_encoding = false;
            }
            full_count = sum;
        }

        let (ptr, data_offset) = if latin1_encoding {
            let p = SequenceString::<Latin1>::allocate(full_count);
            (p.cast::<u8>(), size_of::<SequenceString<Latin1>>())
        } else {
            let p = SequenceString::<Utf16>::allocate(full_count);
            (p.cast::<u8>(), size_of::<SequenceString<Utf16>>())
        };

        let string = {
            // SAFETY: `allocate_*_seq` guarantees that `ptr` is a valid pointer to a sequence string.
            let mut data = unsafe {
                let seq_ptr = ptr.as_ptr();
                seq_ptr.add(data_offset)
            };
            for string in strings {
                // SAFETY:
                // The sum of all `count` for each `string` equals `full_count`, and since we're
                // iteratively writing each of them to `data`, `copy_non_overlapping` always stays
                // in-bounds for `count` reads of each string and `full_count` writes to `data`.
                //
                // Each `string` must be properly aligned to be a valid slice, and `data` must be
                // properly aligned by `allocate_seq`.
                //
                // `allocate_seq` must return a valid pointer to newly allocated memory, meaning
                // `ptr` and all `string`s should never overlap.
                unsafe {
                    // NOTE: The alignment is checked when we allocate the array.
                    #[allow(clippy::cast_ptr_alignment)]
                    match (latin1_encoding, string.variant()) {
                        (true, JsStrVariant::Latin1(s)) => {
                            let count = s.len();
                            ptr::copy_nonoverlapping(s.as_ptr(), data.cast::<u8>(), count);
                            data = data.cast::<u8>().add(count).cast::<u8>();
                        }
                        (false, JsStrVariant::Latin1(s)) => {
                            let count = s.len();
                            for (i, byte) in s.iter().enumerate() {
                                *data.cast::<u16>().add(i) = u16::from(*byte);
                            }
                            data = data.cast::<u16>().add(count).cast::<u8>();
                        }
                        (false, JsStrVariant::Utf16(s)) => {
                            let count = s.len();
                            ptr::copy_nonoverlapping(s.as_ptr(), data.cast::<u16>(), count);
                            data = data.cast::<u16>().add(count).cast::<u8>();
                        }
                        (true, JsStrVariant::Utf16(_)) => {
                            unreachable!("Already checked that it's latin1 encoding")
                        }
                    }
                }
            }

            JsString { ptr: ptr.cast() }
        };

        StaticJsStrings::get_string(&string.as_str()).unwrap_or(string)
    }
}

#[inline]
fn clone(vtable: NonNull<JsStringVTable>) -> JsString {
    // SAFETY: This is part of the correct vtable which is validated on construction.
    let this: &ConcatString = unsafe { vtable.cast().as_ref() };
    let Some(strong) = this.refcount.get().checked_add(1) else {
        abort();
    };
    this.refcount.set(strong);
    // SAFETY: validated the string outside this function.
    unsafe { JsString::from_ptr(vtable) }
}

#[inline]
fn drop(vtable: NonNull<JsStringVTable>) {
    // SAFETY: This is part of the correct vtable which is validated on construction.
    let this: &ConcatString = unsafe { vtable.cast().as_ref() };
    let Some(new) = this.refcount.get().checked_sub(1) else {
        abort();
    };
    this.refcount.set(new);
    if new != 0 {
        return;
    }

    // SAFETY: All the checks for the validity of the layout have already been made on allocation.
    let layout = unsafe {
        Layout::for_value(this)
            .extend(Layout::array::<T::Byte>(this.vtable.len).unwrap_unchecked())
            .unwrap_unchecked()
            .0
            .pad_to_align()
    };

    // SAFETY: If refcount is 0, this is the last reference, so deallocating is safe.
    unsafe {
        dealloc(vtable.as_ptr().cast(), layout);
    }
}

#[inline]
fn as_str(vtable: NonNull<JsStringVTable>) -> JsStr<'static> {
    // SAFETY: This is part of the correct vtable which is validated on construction.
    let this: &ConcatString = unsafe { vtable.cast().as_ref() };
    this.normalize().as_str()
}

#[inline]
fn code_points(vtable: NonNull<JsStringVTable>) -> CodePointsIter<'static> {
    CodePointsIter::new(as_str(vtable))
}

/// `VTable` function for refcount, need to return an `Option<usize>`.
#[inline]
#[allow(clippy::unnecessary_wraps)]
fn refcount(vtable: NonNull<JsStringVTable>) -> Option<usize> {
    // SAFETY: This is part of the correct vtable which is validated on construction.
    let this: &ConcatString = unsafe { vtable.cast().as_ref() };
    Some(this.refcount.get())
}
