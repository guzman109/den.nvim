//! What differs between macOS and Linux: who is on the other end of the
//! socket, keeping memory out of swap and crash dumps, and Touch ID.

use std::os::unix::net::UnixStream;
use std::path::Path;

use den_core::lock::VaultKey;

/// No crash dumps (they would hold the key), and on Linux no attaching a
/// debugger or reading this process's memory from outside.
pub fn harden() {
    use nix::sys::resource::{Resource, setrlimit};
    let _ = setrlimit(Resource::RLIMIT_CORE, 0, 0);
    #[cfg(target_os = "linux")]
    {
        let _ = nix::sys::prctl::set_dumpable(false);
    }
}

/// Whether the connected process runs as the same user.
pub fn same_user(stream: &UnixStream, me: nix::unistd::Uid) -> bool {
    #[cfg(any(target_os = "macos", target_os = "freebsd", target_os = "openbsd"))]
    {
        nix::unistd::getpeereid(stream).is_ok_and(|(uid, _)| uid == me)
    }
    #[cfg(target_os = "linux")]
    {
        use nix::sys::socket::{getsockopt, sockopt::PeerCredentials};
        getsockopt(stream, PeerCredentials).is_ok_and(|c| c.uid() == me.as_raw())
    }
}

/// Keeps a value's memory out of swap.
#[allow(unsafe_code)]
pub fn keep_out_of_swap<T>(value: &T) {
    let ptr = std::ptr::NonNull::from(value).cast::<std::ffi::c_void>();
    // SAFETY: the pointer and length describe `value`, which lives in a Box
    // the agent keeps until it forgets the key. mlock only pins those pages
    // in memory; it neither reads nor writes them.
    let _ = unsafe { nix::sys::mman::mlock(ptr, std::mem::size_of::<T>()) };
}

#[cfg(target_os = "macos")]
mod touch_id {
    use std::path::Path;
    use std::sync::mpsc;
    use std::time::Duration;

    use age::secrecy::{ExposeSecret, SecretString};
    use den_core::lock::{self, VaultKey};
    use robius_authentication::{
        AndroidText, BiometricStrength, Context, PolicyBuilder, Text, WindowsText,
    };
    use security_framework::item::{ItemClass, ItemSearchOptions};
    use security_framework::passwords;

    const SERVICE: &str = "Den vault key";

    fn account(root: &Path) -> Result<String, String> {
        lock::recipient(root)
            .map(|r| r.to_string())
            .map_err(|e| e.to_string())
    }

    /// A copy exists in the keychain (checked without reading it, so no
    /// prompt).
    pub fn ready(root: &Path) -> bool {
        let Ok(account) = account(root) else {
            return false;
        };
        ItemSearchOptions::new()
            .class(ItemClass::generic_password())
            .service(SERVICE)
            .account(&account)
            .load_attributes(true)
            .search()
            .is_ok_and(|found| !found.is_empty())
    }

    pub fn store(root: &Path, key: &VaultKey) -> Result<(), String> {
        let account = account(root)?;
        let secret = key.secret();
        let _ = passwords::delete_generic_password(SERVICE, &account);
        passwords::set_generic_password(SERVICE, &account, secret.expose_secret().as_bytes())
            .map_err(|e| format!("keychain: {e}"))
    }

    /// Asks for a fingerprint (or the login password), then reads the
    /// keychain copy.
    pub fn unlock(root: &Path) -> Result<VaultKey, String> {
        let account = account(root)?;
        if !ready(root) {
            return Err(
                "Touch ID is not set up for this vault on this Mac (den lock touch-id)".into(),
            );
        }
        let policy = PolicyBuilder::new()
            .biometrics(Some(BiometricStrength::Strong))
            .password(true)
            .build()
            .ok_or("Touch ID is not available")?;
        let text = Text {
            android: AndroidText {
                title: "Unlock Den",
                subtitle: None,
                description: None,
            },
            apple: "unlock your Den vault",
            windows: WindowsText::new_truncated("Unlock Den", "Unlock your Den vault"),
        };
        let (tx, rx) = mpsc::channel();
        Context::new(())
            .authenticate(text, &policy, move |result| {
                let _ = tx.send(result.is_ok());
            })
            .map_err(|e| format!("Touch ID: {e:?}"))?;
        match rx.recv_timeout(Duration::from_secs(120)) {
            Ok(true) => {}
            _ => return Err("Touch ID did not confirm it was you".into()),
        }
        let bytes = zeroize::Zeroizing::new(
            passwords::get_generic_password(SERVICE, &account)
                .map_err(|e| format!("keychain: {e}"))?,
        );
        let text = std::str::from_utf8(&bytes).map_err(|_| "keychain: not a key".to_string())?;
        let key = VaultKey::from_secret(&SecretString::from(text.to_string()))
            .map_err(|e| e.to_string())?;
        if key.recipient().to_string() != account {
            return Err("the keychain holds a key for a different vault".into());
        }
        Ok(key)
    }
}

#[cfg(target_os = "macos")]
pub fn touch_id_ready(root: &Path) -> bool {
    touch_id::ready(root)
}

#[cfg(target_os = "macos")]
pub fn touch_id_store(root: &Path, key: &VaultKey) -> Result<(), String> {
    touch_id::store(root, key)
}

#[cfg(target_os = "macos")]
pub fn touch_id_unlock(root: &Path) -> Result<VaultKey, String> {
    touch_id::unlock(root)
}

#[cfg(not(target_os = "macos"))]
pub fn touch_id_ready(_: &Path) -> bool {
    false
}

#[cfg(not(target_os = "macos"))]
pub fn touch_id_store(_: &Path, _: &VaultKey) -> Result<(), String> {
    Err("Touch ID is only on macOS; use a YubiKey or the password here".into())
}

#[cfg(not(target_os = "macos"))]
pub fn touch_id_unlock(_: &Path) -> Result<VaultKey, String> {
    Err("Touch ID is only on macOS; use a YubiKey or the password here".into())
}
