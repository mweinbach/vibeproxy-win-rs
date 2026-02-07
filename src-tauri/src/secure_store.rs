#[cfg(target_os = "windows")]
use std::ptr::{null, null_mut};

#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::LocalFree;
#[cfg(target_os = "windows")]
use windows_sys::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};

pub fn encrypt_secret(secret: &str) -> Result<String, String> {
    if secret.is_empty() {
        return Ok(String::new());
    }

    #[cfg(target_os = "windows")]
    {
        let mut in_bytes = secret.as_bytes().to_vec();
        let mut in_blob = CRYPT_INTEGER_BLOB {
            cbData: in_bytes.len() as u32,
            pbData: in_bytes.as_mut_ptr(),
        };
        let mut out_blob = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: null_mut(),
        };

        let ok = unsafe {
            CryptProtectData(
                &mut in_blob,
                null(),
                null_mut(),
                null_mut(),
                null_mut(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out_blob,
            )
        };

        if ok == 0 {
            return Err(format!(
                "Failed to encrypt secret: {}",
                std::io::Error::last_os_error()
            ));
        }

        let encoded = unsafe {
            let bytes = std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize);
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes)
        };

        unsafe {
            LocalFree(out_blob.pbData as _);
        }

        return Ok(encoded);
    }

    #[cfg(not(target_os = "windows"))]
    {
        Ok(base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            secret.as_bytes(),
        ))
    }
}

pub fn decrypt_secret(secret: &str) -> Result<String, String> {
    if secret.is_empty() {
        return Ok(String::new());
    }

    #[cfg(target_os = "windows")]
    {
        let mut in_bytes =
            base64::Engine::decode(&base64::engine::general_purpose::STANDARD, secret)
                .map_err(|e| format!("Failed to decode encrypted secret: {}", e))?;

        let mut in_blob = CRYPT_INTEGER_BLOB {
            cbData: in_bytes.len() as u32,
            pbData: in_bytes.as_mut_ptr(),
        };
        let mut out_blob = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: null_mut(),
        };

        let ok = unsafe {
            CryptUnprotectData(
                &mut in_blob,
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out_blob,
            )
        };

        if ok == 0 {
            return Err(format!(
                "Failed to decrypt secret: {}",
                std::io::Error::last_os_error()
            ));
        }

        let decrypted = unsafe {
            let bytes = std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize);
            String::from_utf8(bytes.to_vec())
                .map_err(|e| format!("Invalid decrypted UTF-8: {}", e))?
        };

        unsafe {
            LocalFree(out_blob.pbData as _);
        }

        return Ok(decrypted);
    }

    #[cfg(not(target_os = "windows"))]
    {
        let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, secret)
            .map_err(|e| format!("Failed to decode encrypted secret: {}", e))?;
        String::from_utf8(bytes).map_err(|e| format!("Invalid decrypted UTF-8: {}", e))
    }
}
