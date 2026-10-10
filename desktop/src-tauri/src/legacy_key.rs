//! Explicitly selected legacy DPAPI file. No directory scanning, plaintext IPC or secret logs.
use crate::{ai, engine::Engine};
use library_core::*;
use serde_json::{json, Value};
use std::{fs, io::Read, path::Path};

fn invalid(message: &str) -> Failure {
    Failure::new("LEGACY_KEY_INVALID", message)
}
fn ciphertext(path: &Path) -> Result<Vec<u8>> {
    if path.file_name().and_then(|s| s.to_str()) != Some("model_api_key.dpapi") {
        return Err(invalid(
            "请选择旧版 runtime 中的 model_api_key.dpapi 文件。",
        ));
    }
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() == 0 || meta.len() > 16 * 1024
    {
        return Err(invalid("旧密钥须为实际加密文件，不能为空或超过 16 KB。"));
    }
    let mut data = vec![];
    fs::File::open(path)?
        .take(16 * 1024 + 1)
        .read_to_end(&mut data)?;
    if data.is_empty() || data.len() > 16 * 1024 {
        return Err(invalid("旧加密文件在读取期间变化，请重新选择。"));
    }
    Ok(data)
}
pub(crate) fn preview(engine: &Engine, path: &Path) -> Result<Value> {
    let data = ciphertext(path)?;
    let config = ai::settings(engine)?;
    Ok(
        json!({"path":path.canonicalize()?.to_string_lossy(),"fingerprint":hash(&data),"bytes":data.len(),
        "base":config["base"],"model":config["model"],"configured":config["configured"],
        "supported":cfg!(windows),"target_base":"https://models.sjtu.edu.cn/api/v1"}),
    )
}
pub(crate) fn import(
    engine: &Engine,
    path: &Path,
    fingerprint: &str,
    base: &str,
    model: &str,
) -> Result<Value> {
    let mut data = ciphertext(path)?;
    if hash(&data) != fingerprint {
        return Err(invalid("旧加密文件在预览后变化，请重新选择。"));
    }
    with_key(&mut data, |key| {
        if hash(&ciphertext(path)?) != fingerprint {
            return Err(invalid("旧文件在解密后变化，未保存密钥。"));
        }
        ai::import_legacy_key(engine, key, base, model)
    })
}
fn validate_key(bytes: &[u8]) -> Result<&str> {
    if !(12..=512).contains(&bytes.len())
        || !bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(b))
    {
        return Err(invalid("旧加密内容不是受支持的 API 密钥格式，未导入。"));
    }
    std::str::from_utf8(bytes).map_err(|_| invalid("旧密钥编码无效，未导入。"))
}

#[cfg(not(windows))]
fn with_key<T>(_cipher: &mut [u8], _action: impl FnOnce(&str) -> Result<T>) -> Result<T> {
    Err(invalid(
        "旧版 Windows 加密密钥只能在原 Windows 账号下导入。",
    ))
}

#[cfg(windows)]
fn with_key<T>(cipher: &mut [u8], action: impl FnOnce(&str) -> Result<T>) -> Result<T> {
    use std::{ffi::c_void, ptr};
    #[repr(C)]
    struct Blob {
        len: u32,
        data: *mut u8,
    }
    // ABI and ownership documented by Microsoft:
    // https://learn.microsoft.com/en-us/windows/win32/api/dpapi/nf-dpapi-cryptunprotectdata
    #[link(name = "crypt32")]
    extern "system" {
        fn CryptUnprotectData(
            input: *mut Blob,
            description: *mut *mut u16,
            entropy: *mut Blob,
            reserved: *mut c_void,
            prompt: *mut c_void,
            flags: u32,
            output: *mut Blob,
        ) -> i32;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn LocalFree(memory: *mut c_void) -> *mut c_void;
    }
    struct Output(Blob);
    impl Drop for Output {
        fn drop(&mut self) {
            if !self.0.data.is_null() {
                // The successful OS call owns exactly len writable bytes. Clear
                // them even when validation/the credential write fails.
                unsafe {
                    for index in 0..self.0.len as usize {
                        ptr::write_volatile(self.0.data.add(index), 0);
                    }
                    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
                    LocalFree(self.0.data.cast());
                }
            }
        }
    }
    let mut input = Blob {
        len: cipher.len() as u32,
        data: cipher.as_mut_ptr(),
    };
    let mut output = Output(Blob {
        len: 0,
        data: ptr::null_mut(),
    });
    // Exactly the legacy Python contract: no optional entropy, description or
    // UI prompt, current-user DPAPI, CRYPTPROTECT_UI_FORBIDDEN (1).
    let success = unsafe {
        CryptUnprotectData(
            &mut input,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            1,
            &mut output.0,
        )
    };
    if success == 0 {
        return Err(invalid(
            "Windows 无法解密旧密钥，请使用原 Windows 账号或手动重新配置。",
        ));
    }
    if output.0.data.is_null() || !(12..=512).contains(&output.0.len) {
        return Err(invalid("旧密钥内容格式异常，未导入。"));
    }
    let bytes = unsafe { std::slice::from_raw_parts(output.0.data, output.0.len as usize) };
    action(validate_key(bytes)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_format_accepts_only_bounded_ascii_token_and_never_echoes_invalid_content() {
        assert!(validate_key(b"isolated_fixture-token.123").is_ok());
        for data in [
            b"short".as_slice(),
            b"token with spaces",
            b"token_fixture\n",
            &[255; 12],
            &[b'a'; 513],
        ] {
            let error = validate_key(data).unwrap_err();
            assert_eq!(error.code, "LEGACY_KEY_INVALID");
            assert!(!error.message.contains("token_fixture"));
        }
    }
}
