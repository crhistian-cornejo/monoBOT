//! The few CoreFoundation and IOKit calls the notch's Mac readers share:
//! the power sources and the graphics card's statistics come as
//! CoreFoundation dictionaries.
use std::ffi::{c_void, CStr};

pub(crate) type Ref = *const c_void;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFStringCreateWithCString(allocator: Ref, text: *const i8, encoding: u32) -> Ref;
    fn CFDictionaryGetValue(dictionary: Ref, key: Ref) -> Ref;
    fn CFGetTypeID(object: Ref) -> usize;
    fn CFNumberGetTypeID() -> usize;
    fn CFStringGetTypeID() -> usize;
    fn CFNumberGetValue(number: Ref, kind: isize, out: *mut c_void) -> bool;
    fn CFEqual(one: Ref, other: Ref) -> bool;
    fn CFRelease(object: Ref);
    fn CFArrayGetCount(array: Ref) -> isize;
    fn CFArrayGetValueAtIndex(array: Ref, index: isize) -> Ref;
}

const UTF8: u32 = 0x0800_0100;
/// kCFNumberDoubleType.
const DOUBLE: isize = 13;

/// An object this code owns (a Create or Copy call's result), released
/// when dropped.
pub(crate) struct Owned(pub Ref);

impl Owned {
    pub(crate) fn new(object: Ref) -> Option<Self> {
        (!object.is_null()).then_some(Self(object))
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: owned and not null.
        unsafe { CFRelease(self.0) }
    }
}

pub(crate) fn string(text: &CStr) -> Option<Owned> {
    // SAFETY: a NUL-terminated string copied into a new CFString.
    Owned::new(unsafe { CFStringCreateWithCString(std::ptr::null(), text.as_ptr(), UTF8) })
}

/// The value under `key`, borrowed from `dictionary`.
///
/// SAFETY: `dictionary` is a live CFDictionary.
unsafe fn value(dictionary: Ref, key: &CStr) -> Option<Ref> {
    let key = string(key)?;
    // SAFETY: as the caller says; the key lives for the call.
    let found = unsafe { CFDictionaryGetValue(dictionary, key.0) };
    (!found.is_null()).then_some(found)
}

/// The number under `key`.
///
/// SAFETY: `dictionary` is a live CFDictionary.
pub(crate) unsafe fn number(dictionary: Ref, key: &CStr) -> Option<f64> {
    // SAFETY: as the caller says, and the type is checked before reading.
    unsafe {
        let found = value(dictionary, key)?;
        if CFGetTypeID(found) != CFNumberGetTypeID() {
            return None;
        }
        let mut out = 0f64;
        CFNumberGetValue(found, DOUBLE, (&raw mut out).cast()).then_some(out)
    }
}

/// Whether the string under `key` is `expected`.
///
/// SAFETY: `dictionary` is a live CFDictionary.
pub(crate) unsafe fn says(dictionary: Ref, key: &CStr, expected: &CStr) -> bool {
    // SAFETY: as the caller says, and the type is checked before comparing.
    unsafe {
        let (Some(found), Some(expected)) = (value(dictionary, key), string(expected)) else {
            return false;
        };
        CFGetTypeID(found) == CFStringGetTypeID() && CFEqual(found, expected.0)
    }
}

/// The items of a CFArray, borrowed from it.
///
/// SAFETY: `array` is a live CFArray.
pub(crate) unsafe fn items(array: Ref) -> Vec<Ref> {
    // SAFETY: as the caller says; every index is in range.
    unsafe {
        (0..CFArrayGetCount(array))
            .map(|index| CFArrayGetValueAtIndex(array, index))
            .collect()
    }
}
