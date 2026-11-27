//! OS-backed cryptographic randomness and Windows DPAPI secret protection at rest.
//! Zero external crate dependencies: binds directly to `bcrypt.dll` and `crypt32.dll` on Windows.

use crate::{CoreError, CoreResult};
use std::sync::atomic::{Ordering, compiler_fence};
use std::sync::{Arc, OnceLock, RwLock};

pub const DPAPI_PREFIX: &str = "dpapi:v1:";
pub const KEYSTORE_PREFIX: &str = "keystore:v1:";
#[cfg(target_os = "windows")]
const DPAPI_ENTROPY: &[u8] = b"Ferric.Accounts.DPAPI.v1";

/// Pluggable platform secret cipher (e.g., Android Keystore AES-256-GCM vault).
pub trait SecretCipher: Send + Sync {
    fn prefix(&self) -> &'static str;
    fn encrypt(&self, plain: &[u8]) -> Result<Vec<u8>, String>;
    fn decrypt(&self, cipher: &[u8]) -> Result<Vec<u8>, String>;
}

static PLATFORM_SECRET_CIPHER: OnceLock<RwLock<Option<Arc<dyn SecretCipher>>>> = OnceLock::new();

fn platform_cipher_lock() -> &'static RwLock<Option<Arc<dyn SecretCipher>>> {
    PLATFORM_SECRET_CIPHER.get_or_init(|| RwLock::new(None))
}

/// Register a platform-native at-rest secret cipher (call before opening `LibraryDb` on Android).
pub fn register_platform_secret_cipher(cipher: Arc<dyn SecretCipher>) {
    let mut guard = platform_cipher_lock()
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = Some(cipher);
}

#[cfg(test)]
thread_local! {
    static TEST_SECRET_CIPHER: std::cell::RefCell<Option<Arc<dyn SecretCipher>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) struct ScopedTestCipherGuard;

#[cfg(test)]
impl Drop for ScopedTestCipherGuard {
    fn drop(&mut self) {
        TEST_SECRET_CIPHER.with(|slot| {
            *slot.borrow_mut() = None;
        });
    }
}

#[cfg(test)]
pub(crate) fn register_scoped_secret_cipher_for_test(
    cipher: Arc<dyn SecretCipher>,
) -> ScopedTestCipherGuard {
    TEST_SECRET_CIPHER.with(|slot| {
        *slot.borrow_mut() = Some(cipher);
    });
    ScopedTestCipherGuard
}

fn active_platform_cipher() -> Option<Arc<dyn SecretCipher>> {
    #[cfg(test)]
    if let Some(test_cipher) = TEST_SECRET_CIPHER.with(|slot| slot.borrow().clone()) {
        return Some(test_cipher);
    }
    platform_cipher_lock()
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

const B64_TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Best-effort volatile memory scrub with compiler barrier so dead-store elimination cannot strip it.
pub fn zeroize_bytes(buf: &mut [u8]) {
    for b in buf.iter_mut() {
        // SAFETY: `b` is a valid mutable reference to an element of `buf`.
        unsafe { std::ptr::write_volatile(b, 0) };
    }
    compiler_fence(Ordering::SeqCst);
}

/// Scrub the backing heap bytes of a `String` before clearing it.
pub fn zeroize_string(s: &mut String) {
    if !s.is_empty() {
        // SAFETY: Writing all 0x00 bytes is valid UTF-8, and `clear()` immediately resets length to 0.
        unsafe { zeroize_bytes(s.as_bytes_mut()) };
        s.clear();
    }
}

/// Fill `out` with cryptographically secure random bytes from the OS CSPRNG
/// (`BCryptGenRandom` on Windows, `/dev/urandom` on Unix). Fails closed on OS CSPRNG failure.
pub fn csprng_fill(out: &mut [u8]) {
    if out.is_empty() {
        return;
    }

    #[cfg(target_os = "windows")]
    {
        #[link(name = "bcrypt")]
        unsafe extern "system" {
            fn BCryptGenRandom(
                h_algorithm: *mut std::ffi::c_void,
                pb_buffer: *mut u8,
                cb_buffer: u32,
                dw_flags: u32,
            ) -> i32;
        }
        const BCRYPT_USE_SYSTEM_PREFERRED_RNG: u32 = 0x0000_0002;
        for chunk in out.chunks_mut(u32::MAX as usize) {
            let len = chunk.len() as u32;
            // SAFETY: `chunk` is a valid mutable byte slice of `len` bytes.
            let status = unsafe {
                BCryptGenRandom(
                    std::ptr::null_mut(),
                    chunk.as_mut_ptr(),
                    len,
                    BCRYPT_USE_SYSTEM_PREFERRED_RNG,
                )
            };
            assert!(
                status == 0,
                "OS CSPRNG (BCryptGenRandom) failed with NTSTATUS 0x{:08x}",
                status as u32
            );
        }
    }

    #[cfg(unix)]
    {
        use std::io::Read;
        let mut f =
            std::fs::File::open("/dev/urandom").expect("OS CSPRNG (/dev/urandom) failed to open");
        f.read_exact(out)
            .expect("OS CSPRNG (/dev/urandom) failed to fill buffer");
    }

    #[cfg(not(any(target_os = "windows", unix)))]
    {
        panic!("OS CSPRNG is unavailable on this platform");
    }
}

/// Generate `num_bytes` of OS CSPRNG randomness encoded as lowercase hex (`2 * num_bytes` chars).
pub fn csprng_hex(num_bytes: usize) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut raw = vec![0u8; num_bytes];
    csprng_fill(&mut raw);
    let mut out = String::with_capacity(num_bytes * 2);
    for &b in &raw {
        out.push(char::from(HEX[usize::from(b >> 4)]));
        out.push(char::from(HEX[usize::from(b & 0x0F)]));
    }
    zeroize_bytes(&mut raw);
    out
}

fn b64_encode(input: &[u8]) -> String {
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    let (chunks, rem) = input.as_chunks::<3>();
    for &[b0, b1, b2] in chunks {
        out.push(char::from(B64_TABLE[usize::from(b0 >> 2)]));
        out.push(char::from(
            B64_TABLE[usize::from(((b0 & 0x03) << 4) | (b1 >> 4))],
        ));
        out.push(char::from(
            B64_TABLE[usize::from(((b1 & 0x0F) << 2) | (b2 >> 6))],
        ));
        out.push(char::from(B64_TABLE[usize::from(b2 & 0x3F)]));
    }
    match rem {
        [b0] => {
            out.push(char::from(B64_TABLE[usize::from(b0 >> 2)]));
            out.push(char::from(B64_TABLE[usize::from((b0 & 0x03) << 4)]));
            out.push('=');
            out.push('=');
        }
        [b0, b1] => {
            out.push(char::from(B64_TABLE[usize::from(b0 >> 2)]));
            out.push(char::from(
                B64_TABLE[usize::from(((b0 & 0x03) << 4) | (b1 >> 4))],
            ));
            out.push(char::from(B64_TABLE[usize::from((b1 & 0x0F) << 2)]));
            out.push('=');
        }
        _ => {}
    }
    out
}

fn b64_val(b: u8) -> Option<u8> {
    match b {
        b'A'..=b'Z' => Some(b - b'A'),
        b'a'..=b'z' => Some(b - b'a' + 26),
        b'0'..=b'9' => Some(b - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

fn b64_decode(input: &str) -> Result<Vec<u8>, String> {
    let bytes = input.trim().as_bytes();
    if bytes.is_empty() || !bytes.len().is_multiple_of(4) {
        return Err("Invalid base64 length".to_string());
    }
    let mut out = Vec::with_capacity((bytes.len() / 4) * 3);
    let (chunks, _) = bytes.as_chunks::<4>();
    for (idx, &[c0, c1, c2, c3]) in chunks.iter().enumerate() {
        let is_last = idx + 1 == chunks.len();
        let v0 = b64_val(c0).ok_or_else(|| "Invalid base64 char".to_string())?;
        let v1 = b64_val(c1).ok_or_else(|| "Invalid base64 char".to_string())?;
        out.push((v0 << 2) | (v1 >> 4));
        if c2 == b'=' {
            if !is_last || c3 != b'=' {
                return Err("Invalid base64 padding".to_string());
            }
            break;
        }
        let v2 = b64_val(c2).ok_or_else(|| "Invalid base64 char".to_string())?;
        out.push((v1 << 4) | (v2 >> 2));
        if c3 == b'=' {
            if !is_last {
                return Err("Invalid base64 padding".to_string());
            }
            break;
        }
        let v3 = b64_val(c3).ok_or_else(|| "Invalid base64 char".to_string())?;
        out.push((v2 << 6) | v3);
    }
    Ok(out)
}

#[cfg(target_os = "windows")]
mod win_dpapi {
    use super::DPAPI_ENTROPY;
    use std::ffi::c_void;
    use std::ptr;

    const CRYPTPROTECT_UI_FORBIDDEN: u32 = 0x01;

    #[repr(C)]
    struct CryptDataBlob {
        cb_data: u32,
        pb_data: *mut u8,
    }

    #[link(name = "crypt32")]
    unsafe extern "system" {
        fn CryptProtectData(
            p_data_in: *const CryptDataBlob,
            sz_data_descr: *const u16,
            p_optional_entropy: *const CryptDataBlob,
            pv_reserved: *mut c_void,
            p_prompt_struct: *mut c_void,
            dw_flags: u32,
            p_data_out: *mut CryptDataBlob,
        ) -> i32;
        fn CryptUnprotectData(
            p_data_in: *const CryptDataBlob,
            ppsz_data_descr: *mut *mut u16,
            p_optional_entropy: *const CryptDataBlob,
            pv_reserved: *mut c_void,
            p_prompt_struct: *mut c_void,
            dw_flags: u32,
            p_data_out: *mut CryptDataBlob,
        ) -> i32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LocalFree(h_mem: *mut c_void) -> *mut c_void;
    }

    pub fn protect_bytes(plain: &[u8]) -> Result<Vec<u8>, String> {
        let mut in_copy = plain.to_vec();
        let mut entropy = DPAPI_ENTROPY.to_vec();
        let in_blob = CryptDataBlob {
            cb_data: u32::try_from(in_copy.len()).map_err(|_| "Secret exceeds DPAPI size limit")?,
            pb_data: in_copy.as_mut_ptr(),
        };
        let ent_blob = CryptDataBlob {
            cb_data: entropy.len() as u32,
            pb_data: entropy.as_mut_ptr(),
        };
        let mut out_blob = CryptDataBlob {
            cb_data: 0,
            pb_data: ptr::null_mut(),
        };

        // SAFETY: Valid stack descriptors and heap buffers; `out_blob.pb_data` is allocated by DPAPI.
        let ok = unsafe {
            CryptProtectData(
                &in_blob,
                ptr::null(),
                &ent_blob,
                ptr::null_mut(),
                ptr::null_mut(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out_blob,
            )
        };
        super::zeroize_bytes(&mut in_copy);
        super::zeroize_bytes(&mut entropy);
        if ok == 0 || out_blob.pb_data.is_null() {
            return Err("DPAPI CryptProtectData failed".to_string());
        }
        // SAFETY: `pb_data` is valid for `cb_data` bytes until freed with `LocalFree`.
        let cipher = unsafe {
            let slice = std::slice::from_raw_parts(out_blob.pb_data, out_blob.cb_data as usize);
            let out = slice.to_vec();
            let _ = LocalFree(out_blob.pb_data.cast());
            out
        };
        Ok(cipher)
    }

    pub fn unprotect_bytes(cipher: &[u8]) -> Result<Vec<u8>, String> {
        let mut in_copy = cipher.to_vec();
        let mut entropy = DPAPI_ENTROPY.to_vec();
        let in_blob = CryptDataBlob {
            cb_data: u32::try_from(in_copy.len())
                .map_err(|_| "Ciphertext exceeds DPAPI size limit")?,
            pb_data: in_copy.as_mut_ptr(),
        };
        let ent_blob = CryptDataBlob {
            cb_data: entropy.len() as u32,
            pb_data: entropy.as_mut_ptr(),
        };
        let mut out_blob = CryptDataBlob {
            cb_data: 0,
            pb_data: ptr::null_mut(),
        };

        // SAFETY: Valid ciphertext & entropy blobs; decrypted buffer is zeroized and freed via `LocalFree`.
        let ok = unsafe {
            CryptUnprotectData(
                &in_blob,
                ptr::null_mut(),
                &ent_blob,
                ptr::null_mut(),
                ptr::null_mut(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out_blob,
            )
        };
        super::zeroize_bytes(&mut entropy);
        if ok == 0 || out_blob.pb_data.is_null() {
            return Err("DPAPI CryptUnprotectData failed".to_string());
        }
        // SAFETY: `pb_data` is valid for `cb_data` bytes; zeroize before calling `LocalFree`.
        let plain = unsafe {
            let slice = std::slice::from_raw_parts_mut(out_blob.pb_data, out_blob.cb_data as usize);
            let out = slice.to_vec();
            super::zeroize_bytes(slice);
            let _ = LocalFree(out_blob.pb_data.cast());
            out
        };
        Ok(plain)
    }
}

/// Outcome of decrypting a stored secret string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnprotectOutcome {
    pub plaintext: String,
    pub needs_migration: bool,
    pub failed: bool,
}

/// Encrypt a non-empty secret string for storage at rest using either the registered platform
/// cipher (e.g., Android Keystore `keystore:v1:`) or per-user Windows DPAPI (`dpapi:v1:`).
/// Fails closed with `CoreError::SecretProtection` on failure so callers never clobber credentials.
pub fn protect_secret(plain: &str) -> CoreResult<String> {
    if plain.is_empty() {
        return Ok(String::new());
    }
    if let Some(cipher) = active_platform_cipher() {
        let prefix = cipher.prefix();
        if plain.starts_with(prefix) {
            return Ok(plain.to_string());
        }
        let ct = cipher
            .encrypt(plain.as_bytes())
            .map_err(CoreError::SecretProtection)?;
        return Ok(format!("{prefix}{}", b64_encode(&ct)));
    }
    if plain.starts_with(DPAPI_PREFIX) || plain.starts_with(KEYSTORE_PREFIX) {
        return Ok(plain.to_string());
    }
    #[cfg(target_os = "windows")]
    {
        let cipher =
            win_dpapi::protect_bytes(plain.as_bytes()).map_err(CoreError::SecretProtection)?;
        Ok(format!("{DPAPI_PREFIX}{}", b64_encode(&cipher)))
    }
    #[cfg(target_os = "android")]
    {
        let _ = b64_encode(&[]);
        Err(CoreError::SecretProtection(
            "Android Keystore SecretCipher not registered".to_string(),
        ))
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "android")))]
    {
        let _ = b64_encode(&[]);
        Ok(plain.to_string())
    }
}

/// Decrypt a stored secret string and report whether decryption failed (e.g., transient Keystore/DPAPI
/// error or corrupted envelope) so callers avoid overwriting existing DB entries during auto-migration.
pub fn unprotect_secret_checked(stored: &str) -> UnprotectOutcome {
    if stored.is_empty() {
        return UnprotectOutcome {
            plaintext: String::new(),
            needs_migration: false,
            failed: false,
        };
    }
    if let Some(cipher) = active_platform_cipher() {
        let prefix = cipher.prefix();
        if let Some(b64_payload) = stored.strip_prefix(prefix) {
            let Ok(ct) = b64_decode(b64_payload) else {
                return UnprotectOutcome {
                    plaintext: String::new(),
                    needs_migration: false,
                    failed: true,
                };
            };
            let Ok(plain_bytes) = cipher.decrypt(&ct) else {
                return UnprotectOutcome {
                    plaintext: String::new(),
                    needs_migration: false,
                    failed: true,
                };
            };
            return match String::from_utf8(plain_bytes) {
                Ok(plain) => UnprotectOutcome {
                    plaintext: plain,
                    needs_migration: false,
                    failed: false,
                },
                Err(err) => {
                    let mut raw = err.into_bytes();
                    zeroize_bytes(&mut raw);
                    UnprotectOutcome {
                        plaintext: String::new(),
                        needs_migration: false,
                        failed: true,
                    }
                }
            };
        }
        if stored.starts_with(DPAPI_PREFIX) || stored.starts_with(KEYSTORE_PREFIX) {
            return UnprotectOutcome {
                plaintext: String::new(),
                needs_migration: false,
                failed: true,
            };
        }
        return UnprotectOutcome {
            plaintext: stored.to_string(),
            needs_migration: true,
            failed: false,
        };
    }
    if stored.starts_with(KEYSTORE_PREFIX) {
        return UnprotectOutcome {
            plaintext: String::new(),
            needs_migration: false,
            failed: true,
        };
    }
    if let Some(b64_payload) = stored.strip_prefix(DPAPI_PREFIX) {
        #[cfg(target_os = "windows")]
        {
            let Ok(cipher) = b64_decode(b64_payload) else {
                return UnprotectOutcome {
                    plaintext: String::new(),
                    needs_migration: false,
                    failed: true,
                };
            };
            let Ok(plain_bytes) = win_dpapi::unprotect_bytes(&cipher) else {
                return UnprotectOutcome {
                    plaintext: String::new(),
                    needs_migration: false,
                    failed: true,
                };
            };
            match String::from_utf8(plain_bytes) {
                Ok(plain) => UnprotectOutcome {
                    plaintext: plain,
                    needs_migration: false,
                    failed: false,
                },
                Err(err) => {
                    let mut raw = err.into_bytes();
                    zeroize_bytes(&mut raw);
                    UnprotectOutcome {
                        plaintext: String::new(),
                        needs_migration: false,
                        failed: true,
                    }
                }
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = b64_decode(b64_payload);
            UnprotectOutcome {
                plaintext: String::new(),
                needs_migration: false,
                failed: true,
            }
        }
    } else {
        // Legacy plaintext entry: return in-memory value and flag for automatic encryption upgrade.
        UnprotectOutcome {
            plaintext: stored.to_string(),
            needs_migration: cfg!(target_os = "windows"),
            failed: false,
        }
    }
}

/// Decrypt a stored secret string.
/// Returns `(plaintext, needs_migration)` where `needs_migration == true` indicates a legacy
/// plaintext value that should be re-saved under DPAPI encryption immediately.
/// Corrupted or cross-user DPAPI envelopes fail closed to `("", false)`.
pub fn unprotect_secret(stored: &str) -> (String, bool) {
    let outcome = unprotect_secret_checked(stored);
    (outcome.plaintext, outcome.needs_migration)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csprng_produces_distinct_non_zero_hex_tokens() {
        let a = csprng_hex(16);
        let b = csprng_hex(16);
        assert_eq!(a.len(), 32);
        assert_eq!(b.len(), 32);
        assert_ne!(a, b);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn base64_roundtrips_all_padding_lengths() {
        for sample in [
            b"".as_slice(),
            b"f",
            b"fo",
            b"foo",
            b"foob",
            b"fooba",
            b"foobar",
        ] {
            if sample.is_empty() {
                assert_eq!(b64_encode(sample), "");
                continue;
            }
            let enc = b64_encode(sample);
            let dec = b64_decode(&enc).expect("decode b64");
            assert_eq!(dec, sample);
        }
        assert!(b64_decode("not_valid!").is_err());
    }

    #[test]
    fn dpapi_secret_roundtrip_and_tamper_fail_closed() {
        let secret = "SAPISID=super_secret_cookie_value_12345";
        let protected = protect_secret(secret).expect("protect secret");
        if cfg!(target_os = "windows") {
            assert!(protected.starts_with(DPAPI_PREFIX));
            assert!(!protected.contains("super_secret"));
        }
        let outcome = unprotect_secret_checked(&protected);
        assert_eq!(outcome.plaintext, secret);
        assert!(!outcome.needs_migration);
        assert!(!outcome.failed);

        // Corrupted DPAPI blob fails closed and flags failure so callers do not clobber stored secrets
        let corrupt = unprotect_secret_checked("dpapi:v1:AAAA");
        assert!(corrupt.plaintext.is_empty());
        assert!(corrupt.failed);

        // Legacy plaintext is detected for transparent migration on Windows
        let (legacy_plain, needs_mig) = unprotect_secret("legacy_plain_token");
        assert_eq!(legacy_plain, "legacy_plain_token");
        assert_eq!(needs_mig, cfg!(target_os = "windows"));
    }

    #[test]
    fn custom_secret_cipher_roundtrips_and_fails_closed_on_tamper() {
        struct XorTestCipher;
        impl SecretCipher for XorTestCipher {
            fn prefix(&self) -> &'static str {
                KEYSTORE_PREFIX
            }
            fn encrypt(&self, plain: &[u8]) -> Result<Vec<u8>, String> {
                Ok(plain.iter().map(|b| b ^ 0x5A).collect())
            }
            fn decrypt(&self, cipher: &[u8]) -> Result<Vec<u8>, String> {
                if cipher.is_empty() {
                    return Err("empty ciphertext".to_string());
                }
                Ok(cipher.iter().map(|b| b ^ 0x5A).collect())
            }
        }

        let _guard = register_scoped_secret_cipher_for_test(std::sync::Arc::new(XorTestCipher));
        let secret = "sp_dc=android_keystore_secret_token";
        let protected = protect_secret(secret).expect("protect with keystore cipher");
        assert!(protected.starts_with(KEYSTORE_PREFIX));
        assert!(!protected.contains("android_keystore_secret_token"));

        let ok = unprotect_secret_checked(&protected);
        assert_eq!(ok.plaintext, secret);
        assert!(!ok.needs_migration);
        assert!(!ok.failed);

        let corrupt = unprotect_secret_checked("keystore:v1:NOT_VALID_BASE64!");
        assert!(corrupt.plaintext.is_empty());
        assert!(corrupt.failed);
        assert!(!corrupt.needs_migration);

        let legacy = unprotect_secret_checked("plaintext_before_keystore");
        assert_eq!(legacy.plaintext, "plaintext_before_keystore");
        assert!(legacy.needs_migration);
        assert!(!legacy.failed);
    }
}
