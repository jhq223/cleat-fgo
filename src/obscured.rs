//! Raw memory manipulation of FGO's `ObscuredString` type.
//!
//! ## Memory layout (arm64, 64-bit)
//!
//! ```text
//! offset  size  field
//! 0x00    8     klass (ObscuredString__Class*)
//! 0x08    8     monitor (MonitorData*)
//! 0x10    8     currentCryptoKey (Il2CppString*)
//! 0x18    8     hiddenValue (byte[] / Il2CppArray<u8>*)
//! 0x20    8     fakeValue (Il2CppString*)
//! 0x28    1+7   inited (bool, padded to 8)
//! total: 0x30
//! ```
//!
//! ## Crypto
//!
//! XOR each byte of `hiddenValue` with the corresponding byte of
//! `currentCryptoKey` (cycling). The result is the plaintext UTF-16LE
//! bytes. Encryption is the same operation — XOR is its own inverse.

use cleat::prelude::*;
use std::ffi::c_void;

// ── Offsets inside an ObscuredString ─────────────────────────────────────
const OFF_CURRENT_CRYPTO_KEY: isize = 0x10;
const OFF_HIDDEN_VALUE: isize = 0x18;

// ── Public API ────────────────────────────────────────────────────────────

/// Decrypt `hiddenValue` and return the plaintext.
///
/// Returns `None` if `hiddenValue` is null or empty, or if `currentCryptoKey`
/// is null.
pub fn obscured_str(obj: &Il2CppObject) -> Option<String> {
    let key_ptr = read_ptr(obj, OFF_CURRENT_CRYPTO_KEY)?;
    let hv_ptr = read_ptr(obj, OFF_HIDDEN_VALUE)?;

    let key_bytes = il2cpp_string_raw_bytes(key_ptr)?;
    if key_bytes.is_empty() {
        return None;
    }

    let arr = unsafe { Il2CppArray::<u8>::from_raw(hv_ptr) };
    let len = arr.len();
    if len == 0 {
        return None;
    }

    // XOR decrypt
    let decrypted: Vec<u8> = (0..len)
        .map(|i| arr.get(i) ^ key_bytes[i % key_bytes.len()])
        .collect();

    // Convert UTF-16LE bytes → Rust String
    if decrypted.len() % 2 != 0 {
        log::warn!("obscured_str: odd decrypted length {}", decrypted.len());
        return None;
    }
    let u16s: Vec<u16> = decrypted
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16(&u16s).ok()
}

/// Encrypt `text` using the same key as this ObscuredString and write the
/// result into a **newly allocated** `Il2CppArray<u8>`.
///
/// Returns `None` if `currentCryptoKey` is null or empty.
pub fn obscured_encrypt(obj: &Il2CppObject, text: &str) -> Option<Il2CppArray<u8>> {
    let key_ptr = read_ptr(obj, OFF_CURRENT_CRYPTO_KEY)?;
    let key_bytes = il2cpp_string_raw_bytes(key_ptr)?;
    if key_bytes.is_empty() {
        return None;
    }

    let plain: Vec<u8> = text
        .encode_utf16()
        .flat_map(|c| c.to_le_bytes())
        .collect();

    let byte_class = match Il2CppClass::find("System.Byte") {
        Ok(c) => c,
        Err(_) => return None,
    };
    let mut arr = match Il2CppArray::<u8>::new(&byte_class, plain.len()) {
        Ok(a) => a,
        Err(_) => return None,
    };

    for (i, &b) in plain.iter().enumerate() {
        arr.set(i, b ^ key_bytes[i % key_bytes.len()]);
    }

    Some(arr)
}

/// Replace the ObscuredString's hidden value with a new encrypted value
/// derived from `new_text`. This **mutates the managed heap**.
///
/// Returns `true` on success.
pub fn obscured_replace(obj: &Il2CppObject, new_text: &str) -> bool {
    let Some(arr) = obscured_encrypt(obj, new_text) else {
        return false;
    };
    let arr_ptr = arr.as_ptr();
    std::mem::forget(arr); // ownership transfers to the managed heap
    write_ptr(obj, OFF_HIDDEN_VALUE, arr_ptr);
    true
}

// ── Low-level helpers ────────────────────────────────────────────────────

/// Read a `*mut c_void` from `obj` at the given byte offset.
fn read_ptr(obj: &Il2CppObject, offset: isize) -> Option<*mut c_void> {
    let base = obj.raw_ptr();
    if base.is_null() {
        return None;
    }
    let addr = unsafe { (base as *const u8).add(offset as usize) as *const *mut c_void };
    let ptr = unsafe { std::ptr::read(addr) };
    if ptr.is_null() {
        None
    } else {
        Some(ptr)
    }
}

/// Write a `*mut c_void` into `obj` at the given byte offset.
fn write_ptr(obj: &Il2CppObject, offset: isize, val: *mut c_void) {
    let base = obj.raw_ptr();
    if base.is_null() {
        return;
    }
    let addr = unsafe { (base as *mut u8).add(offset as usize) as *mut *mut c_void };
    unsafe { std::ptr::write(addr, val) };
}

/// Read the raw UTF-16LE bytes from an Il2CppString without allocations.
///
/// Il2CppString layout (simplified):
/// ```text
/// offset  size  field
/// 0x00    8     klass
/// 0x08    8     monitor
/// 0x10    4     length (i32, number of chars)
/// 0x14    ...   chars (u16[length])
/// ```
fn il2cpp_string_raw_bytes(str_ptr: *mut c_void) -> Option<Vec<u8>> {
    if str_ptr.is_null() {
        return None;
    }
    // Read length at offset 0x10
    let len_addr = unsafe { (str_ptr as *const u8).add(0x10) as *const i32 };
    let len = unsafe { std::ptr::read(len_addr) };
    if len <= 0 {
        return None;
    }
    // Read raw chars at offset 0x14 — each char is 2 bytes (u16 LE)
    let chars_addr = unsafe { (str_ptr as *const u8).add(0x14) };
    let byte_len = len as usize * 2;
    let bytes = unsafe { std::slice::from_raw_parts(chars_addr, byte_len) };
    Some(bytes.to_vec())
}
