//! Applying planned changes to disk, safely.
//!
//! Every precondition is checked before anything is written: each file must
//! still hold the text its change was planned against (or not exist yet, for a
//! new file), and no file may have unsaved changes in an editor. Then each
//! file is written atomically, in the order given.

use std::collections::BTreeSet;
use std::io::Write as _;
use std::path::{Component, Path, PathBuf};

use crate::error::{Error, Result};
use crate::ops::Change;

/// Writes `changes` under `root`. `dirty` lists vault paths an editor holds
/// unsaved changes for; touching one is refused.
///
/// A plan never contains two changes to the same file.
pub fn apply(root: &Path, changes: &[Change], dirty: &BTreeSet<String>) -> Result<()> {
    let targets = check(root, changes, dirty)?;
    for (change, abs) in changes.iter().zip(targets) {
        if change.delete {
            std::fs::remove_file(&abs).map_err(|e| Error::io(&abs, e))?;
            if let Some(parent) = abs.parent()
                && let Ok(dir) = std::fs::File::open(parent)
            {
                let _ = dir.sync_all();
            }
            continue;
        }
        if change.before.is_none()
            && let Some(parent) = abs.parent()
        {
            std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
        }
        write_atomically(&abs, change.after.as_bytes())?;
    }
    Ok(())
}

/// Checks `changes` the way [`apply`] does, writing nothing, and returns
/// each file's absolute path. An editor that applies part of a plan to its
/// own buffers checks the rest first, so a plan is applied whole or not at
/// all.
pub fn check(root: &Path, changes: &[Change], dirty: &BTreeSet<String>) -> Result<Vec<PathBuf>> {
    let mut targets = Vec::with_capacity(changes.len());
    for change in changes {
        let abs = resolve(root, &change.path)?;
        if dirty.contains(&change.path) {
            return Err(Error::Dirty(change.path.clone()));
        }
        match &change.before {
            Some(expected) => {
                let current = match std::fs::read(&abs) {
                    Ok(bytes) => bytes,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        return Err(Error::Stale(change.path.clone()));
                    }
                    Err(e) => return Err(Error::io(&abs, e)),
                };
                if current != expected.as_bytes() {
                    return Err(Error::Stale(change.path.clone()));
                }
            }
            None => {
                if std::fs::symlink_metadata(&abs).is_ok() {
                    return Err(Error::Exists(change.path.clone()));
                }
            }
        }
        targets.push(abs);
    }
    Ok(targets)
}

fn hidden(rel: &Path) -> bool {
    rel.components().any(|c| match c {
        Component::Normal(part) => part.to_string_lossy().starts_with('.'),
        _ => true,
    })
}

/// The absolute path for a vault path, refusing anything that could escape
/// the vault: `..`, absolute paths, and symlinks (to the file or any folder
/// on the way) that point outside it or into its hidden folders. A synced
/// vault can carry symlinks from anywhere: a note named `notes/x.md` that
/// is really `.git/hooks/pre-commit` would run whatever was written to it.
/// A link is followed only when it stays among the vault's visible files,
/// and a path Den names inside a hidden folder (`.den/keys/…`) is taken
/// only as it is, with no links on the way.
pub fn resolve(root: &Path, path: &str) -> Result<PathBuf> {
    let rel = Path::new(path);
    if path.is_empty() || rel.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err(Error::OutsideVault(path.to_string()));
    }
    let abs = root.join(rel);
    let real_root = std::fs::canonicalize(root).map_err(|e| Error::io(root, e))?;
    let asked_hidden = hidden(rel);
    // The file itself, or the deepest folder on its way that exists.
    let mut probe = abs.clone();
    loop {
        match std::fs::canonicalize(&probe) {
            Ok(real) => {
                let Ok(real_rel) = real.strip_prefix(&real_root) else {
                    return Err(Error::OutsideVault(path.to_string()));
                };
                let asked_rel = probe.strip_prefix(root).unwrap_or(&probe);
                let redirected = real_rel != asked_rel;
                if (asked_hidden && redirected) || (!asked_hidden && hidden(real_rel)) {
                    return Err(Error::OutsideVault(path.to_string()));
                }
                break;
            }
            Err(_) => {
                // A dangling link must not be written through.
                if std::fs::symlink_metadata(&probe).is_ok_and(|m| m.file_type().is_symlink()) {
                    return Err(Error::OutsideVault(path.to_string()));
                }
                if !probe.pop() || probe == root {
                    break;
                }
            }
        }
    }
    Ok(abs)
}

/// Replaces a file's contents so that a crash leaves either the old file or
/// the new one, never a mix.
///
/// The new text goes to a temporary file beside the target (a rename is only
/// atomic within one filesystem), is flushed to disk, takes the old file's
/// permissions, and is renamed over it; then the folder is flushed so the
/// rename itself survives a crash. A symlink is written through: the file it
/// points to changes and the link stays a link.
pub fn write_atomically(path: &Path, content: &[u8]) -> Result<()> {
    let target = if std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        std::fs::canonicalize(path).map_err(|e| Error::io(path, e))?
    } else {
        path.to_path_buf()
    };
    let parent = target
        .parent()
        .ok_or_else(|| Error::OutsideVault(target.display().to_string()))?;
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let temp = parent.join(format!(".{name}.den-{}-{unique}.tmp", std::process::id()));

    let result = (|| -> std::io::Result<()> {
        let mut options = std::fs::File::options();
        options.write(true).create_new(true);
        // Private until it takes the target's permissions below.
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let mut file = options.open(&temp)?;
        file.write_all(content)?;
        file.sync_all()?;
        drop(file);
        if let Ok(meta) = std::fs::metadata(&target) {
            std::fs::set_permissions(&temp, meta.permissions())?;
        }
        std::fs::rename(&temp, &target)?;
        sync_dir(parent)
    })();

    if let Err(e) = result {
        let _ = std::fs::remove_file(&temp);
        return Err(Error::io(&target, e));
    }
    Ok(())
}

fn sync_dir(dir: &Path) -> std::io::Result<()> {
    match std::fs::File::open(dir).and_then(|d| d.sync_all()) {
        Ok(()) => Ok(()),
        // Some filesystems cannot flush a directory; the rename has still
        // happened, so there is nothing further to do.
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::InvalidInput | std::io::ErrorKind::Unsupported
            ) =>
        {
            Ok(())
        }
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn change(path: &str, before: Option<&str>, after: &str) -> Change {
        Change {
            path: path.to_string(),
            before: before.map(str::to_string),
            after: after.to_string(),
            delete: false,
        }
    }

    #[test]
    fn writes_when_the_file_is_as_expected() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("inbox.md"), "old").unwrap();
        apply(
            dir.path(),
            &[change("inbox.md", Some("old"), "new")],
            &BTreeSet::new(),
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("inbox.md")).unwrap(),
            "new"
        );
    }

    #[test]
    fn refuses_a_file_that_moved_underneath() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("inbox.md"), "someone else").unwrap();
        let err = apply(
            dir.path(),
            &[change("inbox.md", Some("old"), "new")],
            &BTreeSet::new(),
        );
        assert!(matches!(err, Err(Error::Stale(_))));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("inbox.md")).unwrap(),
            "someone else"
        );
    }

    #[test]
    fn refuses_a_dirty_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("inbox.md"), "old").unwrap();
        let dirty = BTreeSet::from(["inbox.md".to_string()]);
        let err = apply(
            dir.path(),
            &[change("inbox.md", Some("old"), "new")],
            &dirty,
        );
        assert!(matches!(err, Err(Error::Dirty(_))));
    }

    #[test]
    fn checks_every_file_before_writing_any() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("inbox.md"), "a").unwrap();
        std::fs::create_dir(dir.path().join("notes")).unwrap();
        std::fs::write(dir.path().join("notes/b.md"), "changed").unwrap();
        let err = apply(
            dir.path(),
            &[
                change("inbox.md", Some("a"), "A"),
                change("notes/b.md", Some("b"), "B"),
            ],
            &BTreeSet::new(),
        );
        assert!(matches!(err, Err(Error::Stale(_))));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("inbox.md")).unwrap(),
            "a"
        );
    }

    #[test]
    fn creates_new_files_but_never_over_existing_ones() {
        let dir = tempfile::tempdir().unwrap();
        apply(
            dir.path(),
            &[change("notes/new.md", None, "hi")],
            &BTreeSet::new(),
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("notes/new.md")).unwrap(),
            "hi"
        );
        let err = apply(
            dir.path(),
            &[change("notes/new.md", None, "again")],
            &BTreeSet::new(),
        );
        assert!(matches!(err, Err(Error::Exists(_))));
    }

    #[test]
    fn keeps_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("inbox.md");
        std::fs::write(&file, "old").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        apply(
            dir.path(),
            &[change("inbox.md", Some("old"), "new")],
            &BTreeSet::new(),
        )
        .unwrap();
        let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn writes_through_a_symlink_and_keeps_the_link() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real.md");
        std::fs::write(&real, "old").unwrap();
        std::fs::create_dir(dir.path().join("notes")).unwrap();
        let link = dir.path().join("notes/link.md");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        apply(
            dir.path(),
            &[change("notes/link.md", Some("old"), "new")],
            &BTreeSet::new(),
        )
        .unwrap();
        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(std::fs::read_to_string(&real).unwrap(), "new");
    }

    #[test]
    fn refuses_paths_that_leave_the_vault() {
        let dir = tempfile::tempdir().unwrap();
        for path in ["../x.md", "/etc/x.md", "notes/../../x.md", ""] {
            let err = apply(dir.path(), &[change(path, None, "x")], &BTreeSet::new());
            assert!(matches!(err, Err(Error::OutsideVault(_))), "{path}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_out_of_the_vault_are_refused() {
        let outside = tempfile::tempdir().unwrap();
        let victim = outside.path().join("rc");
        std::fs::write(&victim, "keep me\n").unwrap();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::os::unix::fs::symlink(&victim, root.join("inbox.md")).unwrap();
        std::fs::create_dir_all(root.join("notes")).unwrap();
        std::os::unix::fs::symlink(outside.path(), root.join("notes/away")).unwrap();
        std::os::unix::fs::symlink(root.join("nowhere"), root.join("dangling.md")).unwrap();

        let none = BTreeSet::new();
        let into_file = change("inbox.md", Some("keep me\n"), "captured\n");
        assert!(matches!(
            apply(root, &[into_file], &none),
            Err(Error::OutsideVault(_))
        ));
        let into_folder = change("notes/away/new.md", None, "x\n");
        assert!(matches!(
            apply(root, &[into_folder], &none),
            Err(Error::OutsideVault(_))
        ));
        let dangling = change("dangling.md", None, "x\n");
        assert!(matches!(
            apply(root, &[dangling], &none),
            Err(Error::OutsideVault(_))
        ));
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "keep me\n");

        // Nor into the vault's own hidden folders.
        std::fs::create_dir_all(root.join(".git/hooks")).unwrap();
        std::fs::write(root.join(".git/hooks/pre-commit"), "#!/bin/sh\n").unwrap();
        std::os::unix::fs::symlink(
            root.join(".git/hooks/pre-commit"),
            root.join("notes/hook.md"),
        )
        .unwrap();
        let hook = change("notes/hook.md", Some("#!/bin/sh\n"), "#!/bin/sh\nevil\n");
        assert!(matches!(
            apply(root, &[hook], &none),
            Err(Error::OutsideVault(_))
        ));
        std::os::unix::fs::symlink(root.join(".git"), root.join("notes/g")).unwrap();
        let through = change("notes/g/config", None, "x\n");
        assert!(apply(root, &[through], &none).is_err());
        // A hidden path Den names itself is fine as it is, not through links.
        std::fs::create_dir_all(root.join(".den")).unwrap();
        std::os::unix::fs::symlink(root.join(".git/hooks"), root.join(".den/keys")).unwrap();
        assert!(resolve(root, ".den/keys/password.age").is_err());
        assert!(resolve(root, ".den/log/mac.jsonl").is_ok());

        // A link that stays inside the vault is still written through.
        std::fs::write(root.join("real.md"), "a\n").unwrap();
        std::os::unix::fs::symlink(root.join("real.md"), root.join("alias.md")).unwrap();
        apply(root, &[change("alias.md", Some("a\n"), "b\n")], &none).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("real.md")).unwrap(),
            "b\n"
        );
    }

    #[test]
    fn leaves_no_temporary_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("inbox.md"), "old").unwrap();
        apply(
            dir.path(),
            &[change("inbox.md", Some("old"), "new")],
            &BTreeSet::new(),
        )
        .unwrap();
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["inbox.md"]);
    }
}
