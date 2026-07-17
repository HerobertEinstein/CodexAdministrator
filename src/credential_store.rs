use anyhow::{Result, bail};
use sha2::{Digest, Sha256};

pub const PROVIDER_CREDENTIAL_TARGET: &str = "CodexAdministrator/provider-api-key";
const BOUND_CREDENTIAL_PREFIX: &str = "v1";
const MAX_PROVIDER_SECRET_BYTES: usize = 2048;
const MAX_STORED_CREDENTIAL_BYTES: usize = 2304;

pub trait CredentialStore {
    fn read(&self) -> Result<Option<String>>;
    fn write(&self, secret: &str) -> Result<()>;
    fn delete(&self) -> Result<bool>;
}

pub fn bind_provider_credential(base_url: &str, action_path: &str, secret: &str) -> Result<String> {
    validate_endpoint_component(base_url, "base URL", 2048)?;
    validate_endpoint_component(action_path, "action path", 256)?;
    validate_provider_secret(secret)?;
    let fingerprint = endpoint_fingerprint(base_url, action_path);
    let stored = format!("{BOUND_CREDENTIAL_PREFIX}:{fingerprint}:{secret}");
    if stored.len() > MAX_STORED_CREDENTIAL_BYTES {
        bail!("bound provider credential exceeds its storage limit");
    }
    Ok(stored)
}

pub fn resolve_bound_provider_credential(
    base_url: &str,
    action_path: &str,
    stored: &str,
) -> Result<Option<String>> {
    validate_endpoint_component(base_url, "base URL", 2048)?;
    validate_endpoint_component(action_path, "action path", 256)?;
    if !stored.starts_with(&format!("{BOUND_CREDENTIAL_PREFIX}:")) {
        return Ok(None);
    }
    let mut parts = stored.splitn(3, ':');
    if parts.next() != Some(BOUND_CREDENTIAL_PREFIX) {
        return Ok(None);
    }
    let fingerprint = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("stored provider credential binding is invalid"))?;
    let secret = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("stored provider credential binding is invalid"))?;
    if fingerprint.len() != 64 || !fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("stored provider credential binding is invalid");
    }
    validate_provider_secret(secret)?;
    if fingerprint != endpoint_fingerprint(base_url, action_path) {
        return Ok(None);
    }
    Ok(Some(secret.to_owned()))
}

fn endpoint_fingerprint(base_url: &str, action_path: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update((base_url.len() as u64).to_le_bytes());
    hasher.update(base_url.as_bytes());
    hasher.update((action_path.len() as u64).to_le_bytes());
    hasher.update(action_path.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn validate_endpoint_component(value: &str, label: &str, max_len: usize) -> Result<()> {
    if value.is_empty()
        || value.len() > max_len
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        bail!("provider credential {label} is invalid");
    }
    Ok(())
}

fn validate_provider_secret(secret: &str) -> Result<()> {
    if secret.is_empty()
        || secret.len() > MAX_PROVIDER_SECRET_BYTES
        || secret.trim() != secret
        || secret.chars().any(char::is_control)
    {
        bail!("provider credential is invalid");
    }
    Ok(())
}

#[cfg(windows)]
mod windows {
    use std::{ffi::OsStr, os::windows::ffi::OsStrExt, ptr::null_mut, slice};

    use anyhow::{Context, Result, bail};
    use windows_sys::Win32::{
        Foundation::{ERROR_NOT_FOUND, GetLastError},
        Security::Credentials::{
            CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW, CredFree,
            CredReadW, CredWriteW,
        },
    };

    use super::CredentialStore;

    pub struct WindowsCredentialStore {
        target: String,
    }

    impl WindowsCredentialStore {
        pub fn new(target: impl Into<String>) -> Self {
            Self {
                target: target.into(),
            }
        }

        fn target_wide(&self) -> Result<Vec<u16>> {
            if self.target.is_empty()
                || self.target.len() > 256
                || self.target.chars().any(char::is_control)
            {
                bail!("credential target is invalid");
            }
            Ok(OsStr::new(&self.target)
                .encode_wide()
                .chain(Some(0))
                .collect())
        }
    }

    impl CredentialStore for WindowsCredentialStore {
        fn read(&self) -> Result<Option<String>> {
            let target = self.target_wide()?;
            let mut raw = null_mut();
            if unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut raw) } == 0 {
                let error = unsafe { GetLastError() };
                if error == ERROR_NOT_FOUND {
                    return Ok(None);
                }
                return Err(std::io::Error::from_raw_os_error(error as i32))
                    .context("failed to read provider credential");
            }
            let result = (|| {
                let credential = unsafe { &*raw };
                let size = credential.CredentialBlobSize as usize;
                if size == 0
                    || size > super::MAX_STORED_CREDENTIAL_BYTES
                    || credential.CredentialBlob.is_null()
                {
                    bail!("stored provider credential has an invalid size");
                }
                let bytes = unsafe { slice::from_raw_parts(credential.CredentialBlob, size) };
                let secret = String::from_utf8(bytes.to_vec())
                    .context("stored provider credential is not valid UTF-8")?;
                validate_stored_credential(&secret)?;
                Ok(Some(secret))
            })();
            unsafe { CredFree(raw.cast()) };
            result
        }

        fn write(&self, secret: &str) -> Result<()> {
            validate_stored_credential(secret)?;
            let mut target = self.target_wide()?;
            let mut username: Vec<u16> = OsStr::new("Codex Administrator")
                .encode_wide()
                .chain(Some(0))
                .collect();
            let mut blob = secret.as_bytes().to_vec();
            let credential = CREDENTIALW {
                Type: CRED_TYPE_GENERIC,
                TargetName: target.as_mut_ptr(),
                CredentialBlobSize: blob.len() as u32,
                CredentialBlob: blob.as_mut_ptr(),
                Persist: CRED_PERSIST_LOCAL_MACHINE,
                UserName: username.as_mut_ptr(),
                ..Default::default()
            };
            let written = unsafe { CredWriteW(&credential, 0) };
            blob.fill(0);
            if written == 0 {
                let error = unsafe { GetLastError() };
                return Err(std::io::Error::from_raw_os_error(error as i32))
                    .context("failed to store provider credential");
            }
            Ok(())
        }

        fn delete(&self) -> Result<bool> {
            let target = self.target_wide()?;
            if unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) } != 0 {
                return Ok(true);
            }
            let error = unsafe { GetLastError() };
            if error == ERROR_NOT_FOUND {
                return Ok(false);
            }
            Err(std::io::Error::from_raw_os_error(error as i32))
                .context("failed to delete provider credential")
        }
    }

    fn validate_stored_credential(stored: &str) -> Result<()> {
        if stored.is_empty()
            || stored.len() > super::MAX_STORED_CREDENTIAL_BYTES
            || stored.trim() != stored
            || stored.chars().any(char::is_control)
        {
            bail!("stored provider credential is invalid");
        }
        Ok(())
    }

    pub use WindowsCredentialStore as Store;
}

#[cfg(windows)]
pub use windows::Store as WindowsCredentialStore;
