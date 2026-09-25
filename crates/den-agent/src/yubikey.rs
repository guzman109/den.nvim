//! The YubiKey copy of the vault key, through `age-plugin-yubikey`.
//!
//! The YubiKey holds a private key that never leaves it; age wraps the vault
//! key to its public half. Unwrapping asks the plugin, which asks the
//! YubiKey (a PIN once per session if its policy says so, and a touch).
//! The identity file saved beside the wrapped copy only says which YubiKey
//! and slot to ask; it holds no secret.

use std::path::Path;

use age::secrecy::SecretString;
use den_core::lock::{self, Method, VaultKey};
use den_core::{Error, Result};

const PLUGIN: &str = "yubikey";

fn identity_file(root: &Path) -> std::path::PathBuf {
    root.join(lock::KEYS_DIR).join("yubikey.identity")
}

/// Answers the plugin's questions without a screen: the PIN, if one was
/// given; nothing else.
#[derive(Clone)]
struct Answers {
    pin: Option<SecretString>,
}

impl age::Callbacks for Answers {
    fn display_message(&self, _: &str) {}

    fn confirm(&self, _: &str, _: &str, _: Option<&str>) -> Option<bool> {
        None
    }

    fn request_public_string(&self, _: &str) -> Option<String> {
        None
    }

    fn request_passphrase(&self, _: &str) -> Option<SecretString> {
        self.pin.clone()
    }
}

fn plugin_error(e: impl std::fmt::Display) -> Error {
    Error::Lock(format!(
        "age-plugin-yubikey: {e} (is it installed and on PATH?)"
    ))
}

pub fn wrap(root: &Path, key: &VaultKey, recipient: &str, identity: &str) -> Result<()> {
    let recipient: age::plugin::Recipient = recipient
        .trim()
        .parse()
        .map_err(|_| Error::Lock("not a YubiKey recipient (age1yubikey1…)".into()))?;
    let stub: age::plugin::Identity = identity
        .trim()
        .parse()
        .map_err(|_| Error::Lock("not a YubiKey identity (AGE-PLUGIN-YUBIKEY-1…)".into()))?;
    let wrapper =
        age::plugin::RecipientPluginV1::new(PLUGIN, &[recipient], &[], Answers { pin: None })
            .map_err(plugin_error)?;
    lock::wrap(root, key, Method::Yubikey, &wrapper)?;
    let path = identity_file(root);
    den_core::write::write_atomically(&path, format!("{stub}\n").as_bytes())
}

pub fn unwrap(root: &Path, pin: Option<SecretString>) -> Result<VaultKey> {
    let path = identity_file(root);
    let text = std::fs::read_to_string(&path).map_err(|e| Error::io(&path, e))?;
    let stub: age::plugin::Identity = text
        .trim()
        .parse()
        .map_err(|_| Error::Lock(format!("{} is not a YubiKey identity", path.display())))?;
    let identity = age::plugin::IdentityPluginV1::new(PLUGIN, &[stub], Answers { pin })
        .map_err(plugin_error)?;
    lock::unwrap(root, Method::Yubikey, &identity)
}
