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

/// Whether the person's screen is locked right now. Asked only while a key
/// is held; any doubt reads as "not locked", so a broken check never locks
/// the person out, it only keeps the idle and sleep rules.
pub fn screen_locked() -> bool {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("/usr/sbin/ioreg")
            .args(["-n", "Root", "-d1", "-a"])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .is_ok_and(|out| macos_locked(&String::from_utf8_lossy(&out.stdout)))
    }
    #[cfg(target_os = "linux")]
    {
        let me = nix::unistd::getuid().as_raw().to_string();
        let run = |args: &[&str]| {
            std::process::Command::new("loginctl")
                .args(args)
                .env("PATH", "/usr/bin:/bin")
                .stdin(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        };
        let Some(list) = run(&["list-sessions", "--no-legend"]) else {
            return false;
        };
        linux_sessions(&list, &me).iter().any(|id| {
            run(&["show-session", id, "-p", "LockedHint", "--value"])
                .is_some_and(|v| v.trim() == "yes")
        })
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        false
    }
}

/// macOS lists `CGSSessionScreenIsLocked` = true for a console session
/// while its screen is locked (the key is absent otherwise).
pub fn macos_locked(plist: &str) -> bool {
    let key = "<key>CGSSessionScreenIsLocked</key>";
    plist
        .match_indices(key)
        .any(|(at, _)| plist[at + key.len()..].trim_start().starts_with("<true/>"))
}

/// This user's session ids from `loginctl list-sessions --no-legend`.
pub fn linux_sessions(list: &str, uid: &str) -> Vec<String> {
    list.lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let id = words.next()?;
            (words.next()? == uid).then(|| id.to_string())
        })
        .collect()
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

    /// Asks the person for a fingerprint (or their login password). The
    /// system dialog reads "den-agent is trying to <reason>".
    pub fn confirm(reason: &str) -> Result<(), String> {
        let policy = PolicyBuilder::new()
            .biometrics(Some(BiometricStrength::Strong))
            .password(true)
            .build()
            .ok_or("Touch ID is not available")?;
        let text = Text {
            android: AndroidText {
                title: "Den",
                subtitle: None,
                description: None,
            },
            apple: reason,
            windows: WindowsText::new_truncated("Den", reason),
        };
        let (tx, rx) = mpsc::channel();
        Context::new(())
            .authenticate(text, &policy, move |result| {
                let _ = tx.send(result.is_ok());
            })
            .map_err(|e| format!("Touch ID: {e:?}"))?;
        match rx.recv_timeout(Duration::from_secs(120)) {
            Ok(true) => Ok(()),
            _ => Err("Touch ID did not confirm it was you".into()),
        }
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
        confirm("unlock your Den vault")?;
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

/// The person confirms, in the operating system's own dialog, that
/// something may happen now. Test builds can answer through
/// `DEN_TEST_CONFIRM` (yes or no) instead, so tests never raise a dialog.
pub fn confirm_presence(reason: &str) -> Result<(), String> {
    if cfg!(debug_assertions)
        && let Ok(answer) = std::env::var("DEN_TEST_CONFIRM")
    {
        return if answer == "yes" {
            Ok(())
        } else {
            Err("Touch ID did not confirm it was you".into())
        };
    }
    #[cfg(target_os = "macos")]
    {
        touch_id::confirm(reason)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = reason;
        Err("this needs a fingerprint or password check, which Den has only on macOS so far".into())
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_locked_mac_screen_is_recognised() {
        let unlocked = "<dict>\n\t<key>kCGSSessionOnConsoleKey</key>\n\t<true/>\n</dict>";
        assert!(!macos_locked(unlocked));
        let locked = "<dict>\n\t<key>CGSSessionScreenIsLocked</key>\n\t<true/>\n\t<key>kCGSSessionOnConsoleKey</key>\n\t<true/>\n</dict>";
        assert!(macos_locked(locked));
        let explicit_no = "<key>CGSSessionScreenIsLocked</key>\n<false/>";
        assert!(!macos_locked(explicit_no));
    }

    #[test]
    fn only_this_users_linux_sessions_are_asked_about() {
        let list = "  2 1000 sam  seat0 tty2\n  5 1001 kim  seat0 tty3\n c7 1000 sam        \n";
        assert_eq!(linux_sessions(list, "1000"), vec!["2", "c7"]);
    }
}
