//! Locked notes: encrypted at rest, readable only with the vault key.
//!
//! A locked note is `<name>.md.age`, the note encrypted with age in ASCII
//! armor. It stays a text file, so git stores it and Den's writer checks it
//! like any other file.
//!
//! Every vault has one key pair. Notes are encrypted to its public half
//! (`.den/keys/recipient`), so locking or saving a locked note never needs
//! the vault unlocked. The secret half exists only in wrapped copies, one
//! per way of unlocking, all in `.den/keys/` and synced with the vault:
//!
//! ```text
//! recipient      the public key
//! password.age   the key, encrypted with a passphrase (scrypt)
//! recovery.age   the key, encrypted to a recovery key shown once at setup
//! yubikey.age    the key, encrypted to a YubiKey (age-plugin-yubikey)
//! ```
//!
//! Touch ID keeps its copy in the macOS keychain of each machine instead.
//! Unwrapping happens in den-agent, which keeps the key and hands out
//! plaintext, never the key itself.
//!
//! Locking a note that was committed in the clear does not remove the clear
//! versions from git history; [`Vault::plan_lock`] says so.

use std::path::{Path, PathBuf};

use age::secrecy::{ExposeSecret, SecretString};
use age::x25519;
use serde::Serialize;
use zeroize::Zeroizing;

use crate::error::{Error, Result};
use crate::ops::Change;
use crate::vault::{Vault, classify};

/// Where the key files live, relative to the vault.
pub const KEYS_DIR: &str = ".den/keys";

/// A folder whose new notes are created locked holds this file.
pub const LOCKED_FOLDER_MARKER: &str = ".den-locked";

fn keys(root: &Path) -> PathBuf {
    root.join(KEYS_DIR)
}

fn crypto(e: impl std::fmt::Display) -> Error {
    Error::Lock(e.to_string())
}

/// The vault's secret key, unwrapped. Its memory is wiped when it is dropped.
pub struct VaultKey {
    identity: x25519::Identity,
}

impl std::fmt::Debug for VaultKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("VaultKey(…)")
    }
}

impl VaultKey {
    pub fn generate() -> VaultKey {
        VaultKey {
            identity: x25519::Identity::generate(),
        }
    }

    /// The key from its secret text (`AGE-SECRET-KEY-1…`), as the keychain
    /// holds it.
    pub fn from_secret(secret: &SecretString) -> Result<VaultKey> {
        let identity = secret
            .expose_secret()
            .trim()
            .parse::<x25519::Identity>()
            .map_err(|_| Error::Lock("not a vault key".to_string()))?;
        Ok(VaultKey { identity })
    }

    pub fn secret(&self) -> SecretString {
        self.identity.to_string()
    }

    pub fn recipient(&self) -> x25519::Recipient {
        self.identity.to_public()
    }

    /// A locked note's text.
    pub fn decrypt(&self, armored: &str) -> Result<Zeroizing<String>> {
        let bytes = Zeroizing::new(
            age::decrypt(&self.identity, armored.as_bytes()).map_err(|_| {
                Error::Lock("this note was not locked with this vault's key".into())
            })?,
        );
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| Error::Lock("the unlocked note is not text".into()))?;
        Ok(Zeroizing::new(text.to_string()))
    }
}

/// Encrypts a note's text for the vault, as armored text.
pub fn encrypt(recipient: &x25519::Recipient, text: &str) -> Result<String> {
    age::encrypt_and_armor(recipient, text.as_bytes()).map_err(crypto)
}

/// Ways of unlocking this vault has a wrapped key for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    Password,
    Recovery,
    Yubikey,
}

impl Method {
    pub fn file(self) -> &'static str {
        match self {
            Method::Password => "password.age",
            Method::Recovery => "recovery.age",
            Method::Yubikey => "yubikey.age",
        }
    }
}

/// Whether the vault has a key pair yet.
pub fn is_set_up(root: &Path) -> bool {
    keys(root).join("recipient").is_file()
}

/// The vault's public key, as the synced key file says, checked against the
/// copy this machine pinned when it last unlocked.
///
/// The key file travels with the vault, so a hostile remote could replace
/// it and have every note locked afterwards encrypted to someone else. Each
/// clone therefore keeps its own copy (in its local git config), taken from
/// the real key at setup or unlock, and Den encrypts nothing until the two
/// agree. A vault that is not a git repository has no remote to fear and
/// uses the file alone.
pub fn recipient(root: &Path) -> Result<x25519::Recipient> {
    let from_file = recipient_file(root)?;
    match pinned(root) {
        Some(pin) if pin == from_file.to_string() => Ok(from_file),
        Some(_) => Err(Error::Lock(
            "the vault's public key file (.den/keys/recipient) no longer matches the key this machine \
             unlocked with; Den locks nothing until that is sorted out (see git log for who changed it)"
                .into(),
        )),
        None if is_git_repo(root) => Err(Error::Lock(
            "unlock the vault once on this machine before locking notes here (den unlock)".into(),
        )),
        None => Ok(from_file),
    }
}

fn is_git_repo(root: &Path) -> bool {
    root.join(".git").exists()
}

fn git_config(root: &Path, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["config", "--local"])
        .args(args)
        .current_dir(root)
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// This clone's pinned copy of the vault's public key.
pub fn pinned(root: &Path) -> Option<String> {
    if !is_git_repo(root) {
        return None;
    }
    git_config(root, &["--get", "den.recipient"]).filter(|s| !s.is_empty())
}

/// Pins the public key of a key that was just set up or unwrapped. Only
/// the real key's own public half is ever pinned.
pub fn pin(root: &Path, key: &VaultKey) -> Result<()> {
    if !is_git_repo(root) {
        return Ok(());
    }
    let value = key.recipient().to_string();
    if pinned(root).as_deref() == Some(value.as_str()) {
        return Ok(());
    }
    git_config(root, &["den.recipient", &value])
        .map(|_| ())
        .ok_or_else(|| {
            Error::Lock("could not record the vault's key in this clone's git config".into())
        })
}

/// The public key as the synced key file says, unchecked.
fn recipient_file(root: &Path) -> Result<x25519::Recipient> {
    let path = keys(root).join("recipient");
    let text = std::fs::read_to_string(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            Error::Lock("locking is not set up for this vault (run: den lock setup)".into())
        } else {
            Error::io(&path, e)
        }
    })?;
    text.trim()
        .parse::<x25519::Recipient>()
        .map_err(|_| Error::Lock(format!("{} is not an age public key", path.display())))
}

/// The unlock methods with a wrapped copy in the vault.
pub fn methods(root: &Path) -> Vec<Method> {
    [Method::Password, Method::Yubikey, Method::Recovery]
        .into_iter()
        .filter(|m| keys(root).join(m.file()).is_file())
        .collect()
}

fn write_key_file(root: &Path, name: &str, text: &str) -> Result<()> {
    let dir = keys(root);
    std::fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
    let path = crate::write::resolve(root, &format!("{KEYS_DIR}/{name}"))?;
    crate::write::write_atomically(&path, text.as_bytes())
}

/// Stores a wrapped copy of `key`, readable with `recipient`.
pub fn wrap(
    root: &Path,
    key: &VaultKey,
    method: Method,
    recipient: &dyn age::Recipient,
) -> Result<()> {
    let secret = key.secret();
    let encryptor = age::Encryptor::with_recipients(std::iter::once(recipient)).map_err(crypto)?;
    let mut out = Vec::new();
    {
        use std::io::Write;
        let armor =
            age::armor::ArmoredWriter::wrap_output(&mut out, age::armor::Format::AsciiArmor)
                .map_err(crypto)?;
        let mut writer = encryptor.wrap_output(armor).map_err(crypto)?;
        writer
            .write_all(secret.expose_secret().as_bytes())
            .map_err(crypto)?;
        writer
            .finish()
            .and_then(|armor| armor.finish())
            .map_err(crypto)?;
    }
    let text = String::from_utf8(out).map_err(crypto)?;
    write_key_file(root, method.file(), &text)
}

/// Stores the password copy. `work_factor` is scrypt's log2(N); `None`
/// lets age pick about a second's work on this machine.
pub fn wrap_password(
    root: &Path,
    key: &VaultKey,
    password: SecretString,
    work_factor: Option<u8>,
) -> Result<()> {
    if password.expose_secret().chars().count() < 8 {
        return Err(Error::Lock(
            "choose a password of at least 8 characters".into(),
        ));
    }
    let mut recipient = age::scrypt::Recipient::new(password);
    if let Some(n) = work_factor {
        recipient.set_work_factor(n);
    }
    wrap(root, key, Method::Password, &recipient)
}

/// Unwraps the key from one of its copies, and checks it is this vault's.
pub fn unwrap(root: &Path, method: Method, identity: &dyn age::Identity) -> Result<VaultKey> {
    let path = keys(root).join(method.file());
    let text = std::fs::read_to_string(&path).map_err(|e| Error::io(&path, e))?;
    let decryptor = age::Decryptor::new_buffered(age::armor::ArmoredReader::new(text.as_bytes()))
        .map_err(crypto)?;
    let mut reader = decryptor
        .decrypt(std::iter::once(identity))
        .map_err(|_| match method {
            Method::Password => Error::Lock("wrong password".into()),
            Method::Recovery => Error::Lock("that recovery key does not open this vault".into()),
            Method::Yubikey => Error::Lock("the YubiKey did not unlock the vault".into()),
        })?;
    let mut secret = Zeroizing::new(String::new());
    std::io::Read::read_to_string(&mut reader, &mut secret).map_err(crypto)?;
    let key = VaultKey::from_secret(&SecretString::from(secret.as_str().to_string()))?;
    if key.recipient().to_string() != recipient_file(root)?.to_string() {
        return Err(Error::Lock(format!(
            "{} holds a key for a different vault",
            method.file()
        )));
    }
    Ok(key)
}

pub fn unwrap_password(root: &Path, password: SecretString) -> Result<VaultKey> {
    let mut identity = age::scrypt::Identity::new(password);
    identity.set_max_work_factor(22);
    unwrap(root, Method::Password, &identity)
}

pub fn unwrap_recovery(root: &Path, recovery: &SecretString) -> Result<VaultKey> {
    let identity = recovery
        .expose_secret()
        .trim()
        .parse::<x25519::Identity>()
        .map_err(|_| Error::Lock("that is not a recovery key (AGE-SECRET-KEY-1…)".into()))?;
    unwrap(root, Method::Recovery, &identity)
}

/// What setting up locking produced. The recovery key is shown once and
/// never stored.
pub struct Setup {
    pub key: VaultKey,
    pub recovery: SecretString,
}

/// Creates the vault's key pair with a password copy and a recovery copy.
pub fn setup(root: &Path, password: SecretString, work_factor: Option<u8>) -> Result<Setup> {
    if is_set_up(root) {
        return Err(Error::Lock(
            "locking is already set up for this vault".into(),
        ));
    }
    if password.expose_secret().chars().count() < 8 {
        return Err(Error::Lock(
            "choose a password of at least 8 characters".into(),
        ));
    }
    let key = VaultKey::generate();
    let recovery = x25519::Identity::generate();
    let dir = keys(root);
    std::fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
    write_key_file(root, "recipient", &format!("{}\n", key.recipient()))?;
    if let Err(e) = wrap_password(root, &key, password, work_factor) {
        let _ = std::fs::remove_file(dir.join("recipient"));
        return Err(e);
    }
    wrap(root, &key, Method::Recovery, &recovery.to_public())?;
    Ok(Setup {
        key,
        recovery: recovery.to_string(),
    })
}

/// Whether new notes in `dir` (a vault folder) are created locked.
pub fn folder_is_locked(root: &Path, dir: &str) -> bool {
    let mut path = root.to_path_buf();
    for part in dir.split('/').filter(|p| !p.is_empty()) {
        path.push(part);
        if path.join(LOCKED_FOLDER_MARKER).is_file() {
            return true;
        }
    }
    false
}

/// Teaches this clone's git about locked notes: diffs show the decrypted
/// text (when the vault is unlocked), and merges combine task edits inside
/// them. `den` is the absolute path of the `den` program. Safe to repeat.
pub fn git_setup(root: &Path, den: &Path) -> Result<()> {
    let attributes = root.join(".gitattributes");
    let line = "*.md.age diff=den-age merge=den-age";
    let current = std::fs::read_to_string(&attributes).unwrap_or_default();
    if !current.lines().any(|l| l.trim() == line) {
        let mut text = current;
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(line);
        text.push('\n');
        crate::write::write_atomically(&attributes, text.as_bytes())?;
    }
    let den = den.display().to_string().replace('\'', "'\\''");
    let settings = [
        ("diff.den-age.textconv", format!("'{den}' git-textconv")),
        ("merge.den-age.name", "Den locked notes".to_string()),
        (
            "merge.den-age.driver",
            format!("'{den}' git-merge %O %A %B %P"),
        ),
    ];
    for (key, value) in settings {
        let current = std::process::Command::new("git")
            .args(["config", "--get", key])
            .current_dir(root)
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();
        if current == value {
            continue;
        }
        let out = std::process::Command::new("git")
            .args(["config", key, &value])
            .current_dir(root)
            .output()
            .map_err(|e| Error::io(root, e))?;
        if !out.status.success() {
            return Err(Error::Lock(format!(
                "git config {key}: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
    }
    Ok(())
}

/// A three-way merge of a locked note's decrypted versions: one side's
/// text when only it changed, task edits combined when they do not
/// overlap, else `None` (the person chooses).
pub fn merge_text(base: &str, ours: &str, theirs: &str) -> Option<String> {
    if ours == theirs || base == theirs {
        return Some(ours.to_string());
    }
    if base == ours {
        return Some(theirs.to_string());
    }
    let (b, o, t) = (
        crate::text::TextBuf::parse(base),
        crate::text::TextBuf::parse(ours),
        crate::text::TextBuf::parse(theirs),
    );
    let lines = crate::conflict::combine(&o.lines, Some(&b.lines), &t.lines)?;
    let mut out = o;
    out.lines = lines;
    Some(out.render())
}

impl Vault {
    /// Locks a note: `x.md` becomes `x.md.age`. Needs no unlock.
    pub fn plan_lock(&self, path: &str) -> Result<Vec<Change>> {
        let doc = self
            .doc(path)
            .ok_or_else(|| Error::OutsideVault(path.to_string()))?;
        if doc.locked {
            return Err(Error::Lock(format!("{path} is already locked")));
        }
        let target = format!("{path}.age");
        if self.abs(&target).exists() {
            return Err(Error::Exists(target));
        }
        let armored = encrypt(&recipient(self.root())?, &doc.text)?;
        Ok(vec![
            Change::write(target, None, armored),
            Change::remove(path, doc.text.clone()),
        ])
    }

    /// Unlocks a note for good: `x.md.age` becomes `x.md`. `text` is the
    /// note as the agent decrypted it; `armored` is the file as read.
    pub fn plan_unlock(&self, path: &str, armored: &str, text: &str) -> Result<Vec<Change>> {
        let plain = path
            .strip_suffix(".age")
            .filter(|p| classify(path).is_some_and(|(_, locked)| locked) && p.ends_with(".md"))
            .ok_or_else(|| Error::Lock(format!("{path} is not a locked note")))?;
        if self.abs(plain).exists() {
            return Err(Error::Exists(plain.to_string()));
        }
        Ok(vec![
            Change::write(plain, None, text.to_string()),
            Change::remove(path, armored.to_string()),
        ])
    }

    /// Saves new text into a locked note. `armored` is the file as read, so a
    /// note changed elsewhere meanwhile is refused.
    pub fn plan_write_locked(
        &self,
        path: &str,
        armored: Option<&str>,
        text: &str,
    ) -> Result<Vec<Change>> {
        if !classify(path).is_some_and(|(_, locked)| locked) {
            return Err(Error::Lock(format!("{path} is not a locked note")));
        }
        let after = encrypt(&recipient(self.root())?, text)?;
        Ok(vec![Change::write(
            path,
            armored.map(str::to_string),
            after,
        )])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn password(s: &str) -> SecretString {
        SecretString::from(s.to_string())
    }

    /// A vault folder with locking set up (a fast scrypt, for tests).
    fn vault_with_keys() -> (tempfile::TempDir, Setup) {
        let dir = tempfile::tempdir().unwrap();
        let setup = setup(dir.path(), password("correct horse"), Some(10)).unwrap();
        (dir, setup)
    }

    #[test]
    fn setup_writes_the_public_key_and_two_wrapped_copies() {
        let (dir, s) = vault_with_keys();
        assert!(is_set_up(dir.path()));
        assert_eq!(
            methods(dir.path()),
            vec![Method::Password, Method::Recovery]
        );
        assert_eq!(
            recipient(dir.path()).unwrap().to_string(),
            s.key.recipient().to_string()
        );
        assert!(s.recovery.expose_secret().starts_with("AGE-SECRET-KEY-1"));
        let stored = std::fs::read_to_string(dir.path().join(".den/keys/password.age")).unwrap();
        assert!(stored.starts_with("-----BEGIN AGE ENCRYPTED FILE-----"));
        assert!(!stored.contains(s.key.secret().expose_secret()));
        assert!(
            setup(dir.path(), password("another one"), Some(10)).is_err(),
            "only once"
        );
    }

    #[test]
    fn a_short_password_is_refused_and_nothing_is_written() {
        let dir = tempfile::tempdir().unwrap();
        assert!(setup(dir.path(), password("short"), Some(10)).is_err());
        assert!(!is_set_up(dir.path()));
    }

    #[test]
    fn the_password_and_the_recovery_key_both_unlock() {
        let (dir, s) = vault_with_keys();
        let want = s.key.recipient().to_string();
        let k = unwrap_password(dir.path(), password("correct horse")).unwrap();
        assert_eq!(k.recipient().to_string(), want);
        assert!(unwrap_password(dir.path(), password("wrong horse")).is_err());
        let k = unwrap_recovery(dir.path(), &s.recovery).unwrap();
        assert_eq!(k.recipient().to_string(), want);
        let other = x25519::Identity::generate().to_string();
        assert!(unwrap_recovery(dir.path(), &other).is_err());
    }

    #[test]
    fn a_new_password_replaces_the_old_one() {
        let (dir, s) = vault_with_keys();
        wrap_password(dir.path(), &s.key, password("battery staple"), Some(10)).unwrap();
        assert!(unwrap_password(dir.path(), password("correct horse")).is_err());
        assert!(unwrap_password(dir.path(), password("battery staple")).is_ok());
    }

    #[test]
    fn a_copy_from_another_vault_is_refused() {
        let (a, _) = vault_with_keys();
        let (b, _) = vault_with_keys();
        std::fs::copy(
            b.path().join(".den/keys/password.age"),
            a.path().join(".den/keys/password.age"),
        )
        .unwrap();
        let err = unwrap_password(a.path(), password("correct horse")).unwrap_err();
        assert!(err.to_string().contains("different vault"), "{err}");
    }

    #[test]
    fn notes_round_trip_and_only_the_right_key_reads_them() {
        let (dir, s) = vault_with_keys();
        let armored = encrypt(&recipient(dir.path()).unwrap(), "# Secret\n\nmy note\n").unwrap();
        assert!(armored.starts_with("-----BEGIN AGE ENCRYPTED FILE-----"));
        assert!(!armored.contains("my note"));
        assert_eq!(
            s.key.decrypt(&armored).unwrap().as_str(),
            "# Secret\n\nmy note\n"
        );
        assert!(VaultKey::generate().decrypt(&armored).is_err());
    }

    #[test]
    fn folders_can_make_new_notes_locked() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("notes/health/visits")).unwrap();
        std::fs::write(
            dir.path().join("notes/health").join(LOCKED_FOLDER_MARKER),
            "",
        )
        .unwrap();
        assert!(folder_is_locked(dir.path(), "notes/health"));
        assert!(folder_is_locked(dir.path(), "notes/health/visits"));
        assert!(!folder_is_locked(dir.path(), "notes"));
    }

    #[test]
    fn locked_notes_merge_task_edits_and_leave_real_clashes_alone() {
        let base = "# Plan\n\n- [ ] Book the room\n- [ ] Send invites\n";
        let ours = "# Plan\n\n- [x] Book the room @done(2026-09-24)\n- [ ] Send invites\n";
        let theirs = "# Plan\n\n- [ ] Book the room\n- [ ] Send invites #today\n";
        assert_eq!(
            merge_text(base, ours, theirs).unwrap(),
            "# Plan\n\n- [x] Book the room @done(2026-09-24)\n- [ ] Send invites #today\n"
        );
        assert_eq!(merge_text(base, base, theirs).unwrap(), theirs);
        assert_eq!(merge_text(base, ours, base).unwrap(), ours);
        let reworded = "# Plan, revised\n\n- [ ] Book the room\n- [ ] Send invites\n";
        let retitled = "# The plan\n\n- [ ] Book the room\n- [ ] Send invites\n";
        assert_eq!(merge_text(base, reworded, retitled), None);
    }

    #[test]
    fn a_swapped_public_key_stops_locking_in_a_git_clone() {
        let (dir, s) = vault_with_keys();
        let root = dir.path();
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(root)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .output()
                .unwrap()
        };
        git(&["init", "-q"]);
        // A fresh clone has no pin yet: nothing is encrypted until an unlock.
        assert!(
            recipient(root)
                .unwrap_err()
                .to_string()
                .contains("unlock the vault once")
        );
        pin(root, &s.key).unwrap();
        assert_eq!(
            recipient(root).unwrap().to_string(),
            s.key.recipient().to_string()
        );

        // Someone replaces the synced key file with their own key.
        let theirs = VaultKey::generate();
        std::fs::write(
            root.join(".den/keys/recipient"),
            format!("{}\n", theirs.recipient()),
        )
        .unwrap();
        let err = recipient(root).unwrap_err().to_string();
        assert!(err.contains("no longer matches"), "{err}");
        // And an unlock notices too: the real key is not the file's.
        let err = unwrap_password(root, password("correct horse"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("different vault"), "{err}");
    }

    #[test]
    fn the_key_prints_no_secret() {
        let key = VaultKey::generate();
        assert_eq!(format!("{key:?}"), "VaultKey(…)");
    }
}
