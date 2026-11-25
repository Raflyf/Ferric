//! OS-backed cryptographic randomness and Windows DPAPI secret protection at rest.
//! Zero external crate dependencies: binds directly to `bcrypt.dll` and `crypt32.dll` on Windows.

pub const DPAPI_PREFIX: &str = "dpapi:v1:";
#[cfg(target_os = "windows")]
const DPAPI_ENTROPY: &[u8] = b"Ferric.Accounts.DPAPI.v1";

const B64_TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Fill `out` with cryptographically secure random bytes from the OS CSPRNG
/// (`BCryptGenRandom` on Windows, `/dev/urandom` on Unix).
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
        if let Ok(len) = u32::try_from(out.len()) {
            // SAFETY: `out` is a valid mutable byte slice of `len` bytes.
            let status = unsafe {
                BCryptGenRandom(
                    std::ptr::null_mut(),
                    out.as_mut_ptr(),
                    len,
                    BCRYPT_USE_SYSTEM_PREFERRED_RNG,
                )
            };
            if status == 0 {
                return;
            }
        }
    }

    #[cfg(unix)]
    {
        use std::io::Read;
        if let Ok(mut f) = std::fs::File::open("/dev/urandom")
            && f.read_exact(out).is_ok()
        {
            return;
        }
    }

    // Emergency fallback only if OS RNG call unexpectedly fails: mix RandomState (OS-seeded) per block.
    use std::collections::hash_map::RandomState;
    use std::hash::BuildHasher;
    let mut written = 0usize;
    let mut round = 0u64;
    while written < out.len() {
        let state = RandomState::new();
        let block = state.hash_one(round).to_le_bytes();
        let take = (out.len() - written).min(block.len());
        out[written..written + take].copy_from_slice(&block[..take]);
        written += take;
        round = round.wrapping_add(1);
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
        for b in &mut in_copy {
            // SAFETY: Zeroize temporary plaintext buffer in place.
            unsafe { ptr::write_volatile(b, 0) };
        }
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
        if ok == 0 || out_blob.pb_data.is_null() {
            return Err("DPAPI CryptUnprotectData failed".to_string());
        }
        // SAFETY: `pb_data` is valid for `cb_data` bytes; zeroize before calling `LocalFree`.
        let plain = unsafe {
            let slice = std::slice::from_raw_parts_mut(out_blob.pb_data, out_blob.cb_data as usize);
            let out = slice.to_vec();
            for b in slice {
                ptr::write_volatile(b, 0);
            }
            let _ = LocalFree(out_blob.pb_data.cast());
            out
        };
        Ok(plain)
    }
}

/// Encrypt a non-empty secret string for storage at rest using per-user Windows DPAPI.
/// On non-Windows targets, returns the string unchanged.
pub fn protect_secret(plain: &str) -> String {
    if plain.is_empty() || plain.starts_with(DPAPI_PREFIX) {
        return plain.to_string();
    }
    #[cfg(target_os = "windows")]
    {
        match win_dpapi::protect_bytes(plain.as_bytes()) {
            Ok(cipher) => format!("{DPAPI_PREFIX}{}", b64_encode(&cipher)),
            Err(_) => String::new(),
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = b64_encode(&[]);
        plain.to_string()
    }
}

/// Decrypt a stored secret string.
/// Returns `(plaintext, needs_migration)` where `needs_migration == true` indicates a legacy
/// plaintext value that should be re-saved under DPAPI encryption immediately.
/// Corrupted or cross-user DPAPI envelopes fail closed to `("", false)`.
pub fn unprotect_secret(stored: &str) -> (String, bool) {
    if stored.is_empty() {
        return (String::new(), false);
    }
    if let Some(b64_payload) = stored.strip_prefix(DPAPI_PREFIX) {
        #[cfg(target_os = "windows")]
        {
            let Ok(cipher) = b64_decode(b64_payload) else {
                return (String::new(), false);
            };
            let Ok(plain_bytes) = win_dpapi::unprotect_bytes(&cipher) else {
                return (String::new(), false);
            };
            match String::from_utf8(plain_bytes) {
                Ok(plain) => (plain, false),
                Err(_) => (String::new(), false),
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = b64_decode(b64_payload);
            (String::new(), false)
        }
    } else {
        // Legacy plaintext entry — return in-memory value and flag for automatic DPAPI upgrade.
        (stored.to_string(), cfg!(target_os = "windows"))
    }
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
        let protected = protect_secret(secret);
        if cfg!(target_os = "windows") {
            assert!(protected.starts_with(DPAPI_PREFIX));
            assert!(!protected.contains("super_secret"));
        }
        let (restored, migrated) = unprotect_secret(&protected);
        assert_eq!(restored, secret);
        assert!(!migrated);

        // Corrupted DPAPI blob fails closed to empty string (never returns raw ciphertext)
        let (corrupt, _) = unprotect_secret("dpapi:v1:AAAA");
        assert!(corrupt.is_empty());

        // Legacy plaintext is detected for transparent migration on Windows
        let (legacy_plain, needs_mig) = unprotect_secret("legacy_plain_token");
        assert_eq!(legacy_plain, "legacy_plain_token");
        assert_eq!(needs_mig, cfg!(target_os = "windows"));
    }
}
