//! Finding the main checkout behind a git worktree.
//!
//! A project's `root:` names its main checkout, but you often work in a linked
//! worktree elsewhere on disk. A linked worktree has a `.git` *file* pointing
//! at its private git directory, which in turn names the shared (common) git
//! directory in a `commondir` file. The main checkout is the folder holding
//! that common `.git` directory.

use std::path::{Path, PathBuf};

/// Where `dir` sits in git terms: the top of its checkout, and the top of the
/// main checkout it belongs to. Both are the same for an ordinary checkout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkout {
    pub top: PathBuf,
    pub main: PathBuf,
}

pub fn checkout(dir: &Path) -> Option<Checkout> {
    for top in dir.ancestors() {
        let dotgit = top.join(".git");
        if dotgit.is_dir() {
            return Some(Checkout {
                top: top.to_path_buf(),
                main: top.to_path_buf(),
            });
        }
        if dotgit.is_file() {
            let main = linked_main(top, &dotgit).unwrap_or_else(|| top.to_path_buf());
            return Some(Checkout {
                top: top.to_path_buf(),
                main,
            });
        }
    }
    None
}

fn linked_main(top: &Path, dotgit: &Path) -> Option<PathBuf> {
    let content = std::fs::read_to_string(dotgit).ok()?;
    let gitdir = content.trim().strip_prefix("gitdir:")?.trim();
    let gitdir = top.join(gitdir);
    let common = std::fs::read_to_string(gitdir.join("commondir")).ok()?;
    let common = std::fs::canonicalize(gitdir.join(common.trim())).ok()?;
    if common.file_name()? == ".git" {
        Some(common.parent()?.to_path_buf())
    } else {
        None
    }
}

/// The same place as `dir`, but in the main checkout when `dir` is inside a
/// linked worktree.
pub fn in_main_checkout(dir: &Path) -> Option<PathBuf> {
    let c = checkout(dir)?;
    if c.top == c.main {
        return None;
    }
    let inner = dir.strip_prefix(&c.top).ok()?;
    Some(c.main.join(inner))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// Runs git with no user or system config, so a developer's signing or
    /// hook settings cannot change what the test does.
    fn git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .status()
            .expect("git must be installed to run these tests");
        assert!(status.success(), "git {args:?}");
    }

    #[test]
    fn a_linked_worktree_maps_back_to_its_main_checkout() {
        let tmp = tempfile::tempdir().unwrap();
        let base = std::fs::canonicalize(tmp.path()).unwrap();
        let main = base.join("main");
        std::fs::create_dir(&main).unwrap();
        git(&main, &["init", "-q"]);
        git(
            &main,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "init",
            ],
        );
        git(&main, &["worktree", "add", "-q", "../wt"]);
        let wt = base.join("wt");
        std::fs::create_dir(wt.join("src")).unwrap();

        let c = checkout(&wt.join("src")).unwrap();
        assert_eq!(c.top, wt);
        assert_eq!(c.main, main);
        assert_eq!(in_main_checkout(&wt.join("src")), Some(main.join("src")));
        assert_eq!(in_main_checkout(&main), None);
    }
}
