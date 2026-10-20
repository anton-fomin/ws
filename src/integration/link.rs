//! Hard-linked copies of dependency directories.
//!
//! Installing `node_modules` from scratch in every worktree is the slowest
//! part of starting work on a branch, and almost all of it is already on the
//! disk in the worktree next door. Replicating that tree with hard links costs
//! one inode per file instead of its bytes, and the package managers cope with
//! it: they replace a file by writing a new one and renaming it over the old,
//! which breaks the link rather than editing the original.
//!
//! The copy is a replica, not a resolution: a symlink is recreated as a
//! symlink, so pnpm's store of relative links into `node_modules/.pnpm` keeps
//! pointing at the copy's own store rather than back at the one it came from.

use anyhow::{bail, Context, Result};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};

/// How deep below a repository a dependency directory is still looked for.
///
/// The root one, plus the ones a monorepo's packages carry — `packages/ui`,
/// `apps/web/node_modules` and the like. Deeper than this and it is a nested
/// dependency of a dependency, which comes along with its parent anyway.
pub const MAX_DEPTH: usize = 3;

/// What [`tree`] did, for the one line it reports.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    pub dirs: u64,
    pub linked: u64,
    pub copied: u64,
    pub symlinks: u64,
}

impl Stats {
    pub fn files(&self) -> u64 {
        self.linked + self.copied + self.symlinks
    }
}

/// Replicate the tree at `from` under `to`.
///
/// Directories are recreated, symlinks are recreated as symlinks without being
/// followed, and regular files are hard-linked — or copied where a link cannot
/// be made, which is what happens across a filesystem boundary.
///
/// `to` must not exist yet: the point is a fresh worktree, and merging into
/// half an installation is how a package manager ends up confused.
pub fn tree(from: &Path, to: &Path) -> Result<Stats> {
    replicate(from, to, true)
}

/// [`tree`], with every regular file copied rather than linked — for files
/// that are meant to be edited in one worktree without the edit showing up
/// in all the others.
pub fn copy_tree(from: &Path, to: &Path) -> Result<Stats> {
    replicate(from, to, false)
}

/// One file, hard-linked when `hard` and a link can be made, copied
/// otherwise. Reports whether it ended up linked.
pub fn file(from: &Path, to: &Path, hard: bool) -> Result<bool> {
    if hard && std::fs::hard_link(from, to).is_ok() {
        return Ok(true);
    }
    std::fs::copy(from, to).with_context(|| format!("cannot copy {}", from.display()))?;
    Ok(false)
}

fn replicate(from: &Path, to: &Path, hard: bool) -> Result<Stats> {
    if to.exists() {
        bail!("{} exists already", to.display());
    }
    let mut stats = Stats::default();
    let mut pending = VecDeque::from([(from.to_path_buf(), to.to_path_buf())]);

    while let Some((src, dst)) = pending.pop_front() {
        std::fs::create_dir_all(&dst)
            .with_context(|| format!("cannot create {}", dst.display()))?;
        stats.dirs += 1;
        let entries =
            std::fs::read_dir(&src).with_context(|| format!("cannot read {}", src.display()))?;
        for entry in entries {
            let entry = entry.with_context(|| format!("cannot read {}", src.display()))?;
            let kind = entry
                .file_type()
                .with_context(|| format!("cannot stat {}", entry.path().display()))?;
            let target = dst.join(entry.file_name());
            // Checked before `is_dir`: a symlink to a directory reports both,
            // and following it would copy the tree it points at instead.
            if kind.is_symlink() {
                let pointee = std::fs::read_link(entry.path())
                    .with_context(|| format!("cannot read {}", entry.path().display()))?;
                symlink(&pointee, &target)?;
                stats.symlinks += 1;
            } else if kind.is_dir() {
                pending.push_back((entry.path(), target));
            } else if file(&entry.path(), &target, hard)? {
                stats.linked += 1;
            } else {
                stats.copied += 1;
            }
        }
    }
    Ok(stats)
}

#[cfg(unix)]
pub fn symlink(pointee: &Path, at: &Path) -> Result<()> {
    std::os::unix::fs::symlink(pointee, at)
        .with_context(|| format!("cannot create the symlink {}", at.display()))
}

/// Without symlinks there is no faithful replica to make — a dependency tree
/// is full of them — so the caller falls back to a plain install.
#[cfg(not(unix))]
pub fn symlink(_pointee: &Path, at: &Path) -> Result<()> {
    bail!(
        "cannot create the symlink {} on this platform",
        at.display()
    )
}

/// Dependency directories named `name` inside `root`, relative to it.
///
/// The walk stops at one rather than descending into it, skips dotted
/// directories — `.git` above all — and goes no deeper than [`MAX_DEPTH`].
/// An unreadable directory is simply not descended into: this is a search for
/// something to copy, and finding less of it is not an error.
pub fn find(root: &Path, name: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = VecDeque::from([(PathBuf::new(), 0usize)]);

    while let Some((relative, depth)) = pending.pop_front() {
        let Ok(entries) = std::fs::read_dir(root.join(&relative)) else {
            continue;
        };
        let mut children = Vec::new();
        for entry in entries.flatten() {
            if !entry.file_type().is_ok_and(|k| k.is_dir()) {
                continue;
            }
            let file_name = entry.file_name();
            let Some(file_name) = file_name.to_str() else {
                continue;
            };
            if file_name == name {
                found.push(relative.join(file_name));
            } else if !file_name.starts_with('.') && depth < MAX_DEPTH {
                children.push(relative.join(file_name));
            }
        }
        // Sorted so the result does not depend on the order the filesystem
        // happens to hand the entries back in.
        children.sort();
        pending.extend(children.into_iter().map(|child| (child, depth + 1)));
    }

    found.sort();
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn replicates_files_as_links_and_symlinks_as_symlinks() {
        let tmp = TempDir::new().unwrap();
        let from = tmp.path().join("source/node_modules");
        write(&from.join("left-pad/index.js").to_path_buf(), "pad");
        write(&from.join(".pnpm/left-pad@1/x").to_path_buf(), "store");
        std::fs::create_dir_all(from.join("nested")).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("../.pnpm/left-pad@1", from.join("nested/link")).unwrap();

        let to = tmp.path().join("target/node_modules");
        let stats = tree(&from, &to).unwrap();

        assert_eq!(stats.linked, 2);
        assert_eq!(stats.copied, 0);
        assert_eq!(stats.symlinks, 1);
        assert_eq!(
            std::fs::read_to_string(to.join("left-pad/index.js")).unwrap(),
            "pad"
        );

        // The same inode, not a second copy of the bytes.
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let original = std::fs::metadata(from.join("left-pad/index.js")).unwrap();
            let replica = std::fs::metadata(to.join("left-pad/index.js")).unwrap();
            assert_eq!(original.ino(), replica.ino());
        }

        // Recreated as a symlink, pointing where the original pointed — which
        // is inside the copy, not back into the directory it came from.
        let pointee = std::fs::read_link(to.join("nested/link")).unwrap();
        assert_eq!(pointee, Path::new("../.pnpm/left-pad@1"));
        assert!(to.join("nested/link/x").is_file());
    }

    #[test]
    fn an_existing_target_is_left_alone() {
        let tmp = TempDir::new().unwrap();
        let from = tmp.path().join("source");
        let to = tmp.path().join("target");
        std::fs::create_dir_all(&from).unwrap();
        std::fs::create_dir_all(&to).unwrap();
        assert!(tree(&from, &to).is_err());
    }

    #[test]
    fn finds_the_root_and_the_packages_of_a_monorepo() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        for dir in [
            "node_modules/left-pad",
            "packages/ui/node_modules/react",
            "apps/web/node_modules",
            ".git/objects",
            "deep/one/two/three/node_modules",
        ] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        // Inside a dependency directory, so never reached on its own.
        std::fs::create_dir_all(root.join("node_modules/left-pad/node_modules")).unwrap();

        assert_eq!(
            find(root, "node_modules"),
            [
                PathBuf::from("apps/web/node_modules"),
                PathBuf::from("node_modules"),
                PathBuf::from("packages/ui/node_modules"),
            ]
        );
    }

    #[test]
    fn nothing_to_find_is_an_empty_list() {
        let tmp = TempDir::new().unwrap();
        assert!(find(tmp.path(), "node_modules").is_empty());
        assert!(find(&tmp.path().join("gone"), "node_modules").is_empty());
    }
}
