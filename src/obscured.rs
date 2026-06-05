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
    let key_ptr = unsafe { obj.read_ptr_at(OFF_CURRENT_CRYPTO_KEY)? };
    let hv_ptr = unsafe { obj.read_ptr_at(OFF_HIDDEN_VALUE)? };

    let key_bytes = raw_utf16le_bytes(key_ptr)?;
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
    if !decrypted.len().is_multiple_of(2) {
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
    let key_ptr = unsafe { obj.read_ptr_at(OFF_CURRENT_CRYPTO_KEY)? };
    let key_bytes = raw_utf16le_bytes(key_ptr)?;
    if key_bytes.is_empty() {
        return None;
    }

    let plain: Vec<u8> = text.encode_utf16().flat_map(|c| c.to_le_bytes()).collect();

    let byte_class = Il2CppClass::find("System.Byte").ok()?;
    let mut arr = Il2CppArray::<u8>::new(&byte_class, plain.len()).ok()?;

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
    unsafe { obj.write_ptr_at(OFF_HIDDEN_VALUE, arr_ptr) };
    true
}

// ── Internal helpers ──────────────────────────────────────────────────────

/// Read the raw UTF-16LE bytes from an Il2CppString pointer.
///
/// Used for the ObscuredString XOR key — we need the raw bytes,
/// not a decoded Rust string.
fn raw_utf16le_bytes(str_ptr: *mut c_void) -> Option<Vec<u8>> {
    if str_ptr.is_null() {
        return None;
    }
    // Il2CppString layout: klass(8) monitor(8) length(i32 at 0x10) chars(u16[] at 0x14)
    let len = unsafe { std::ptr::read::<i32>((str_ptr as *const u8).add(0x10) as *const i32) };
    if len <= 0 {
        return None;
    }
    let chars_addr = unsafe { (str_ptr as *const u8).add(0x14) };
    let byte_len = len as usize * 2;
    unsafe { Some(std::slice::from_raw_parts(chars_addr, byte_len).to_vec()) }
}
