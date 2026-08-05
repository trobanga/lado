//! Watches the git directory and reports changes that can alter the diff.

use anyhow::{Context, Result};
use notify::{EventKind, RecursiveMode};
use notify_debouncer_full::{new_debouncer, DebounceEventResult, Debouncer, RecommendedCache};
use std::path::Path;
use std::time::Duration;

/// How long the git directory must be quiet before the watcher reports. A
/// rebase or a fetch writes many files in a burst; one report at the end of the
/// burst is worth more than one report per file.
const QUIET_PERIOD: Duration = Duration::from_millis(300);

/// Reports git directory changes for as long as it is alive. Dropping it stops
/// the background thread.
pub struct DiffWatcher {
    _debouncer: Debouncer<notify::RecommendedWatcher, RecommendedCache>,
}

impl DiffWatcher {
    /// Call `on_change` after a change under `git_dir` that can alter the diff.
    ///
    /// `on_change` runs on a background thread, so it must not touch the
    /// repository: `git2::Repository` is not `Send`. Hand the signal to the UI
    /// thread instead.
    pub fn spawn(git_dir: &Path, on_change: impl Fn() + Send + 'static) -> Result<Self> {
        let root = git_dir.to_path_buf();
        let mut debouncer = new_debouncer(QUIET_PERIOD, None, move |res: DebounceEventResult| {
            let Ok(events) = res else { return };
            if events.iter().any(|e| is_relevant(&root, e)) {
                on_change();
            }
        })
        .context("Failed to start the git directory watcher")?;

        debouncer
            .watch(git_dir, RecursiveMode::Recursive)
            .with_context(|| format!("Failed to watch {}", git_dir.display()))?;

        Ok(Self {
            _debouncer: debouncer,
        })
    }
}

/// Whether `event` can alter the diff.
///
/// Two filters, and both are necessary:
///
/// - The kind. inotify reports a plain *read* as an event, and answering
///   "did the range move?" reads the ref files. Without this test the watcher
///   feeds itself: read a ref, get an event, read the ref again, forever.
/// - The path. Only the references and the pseudo-refs move the endpoints
///   `lado` diffs between. Object writes and index writes never do.
fn is_relevant(git_dir: &Path, event: &notify::Event) -> bool {
    if matches!(event.kind, EventKind::Access(_)) {
        return false;
    }
    event.paths.iter().any(|p| moves_a_ref(git_dir, p))
}

/// Whether a write at `event_path` can move one of the refs the diff resolves.
fn moves_a_ref(git_dir: &Path, event_path: &Path) -> bool {
    let Ok(rel) = event_path.strip_prefix(git_dir) else {
        return false;
    };
    if rel.extension().is_some_and(|ext| ext == "lock") {
        return false;
    }
    rel.starts_with("refs") || rel == Path::new("HEAD") || rel == Path::new("packed-refs")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::mpsc;
    use tempfile::TempDir;

    fn git_dir() -> PathBuf {
        PathBuf::from("/home/user/project/.git")
    }

    /// A write event for one path, the shape inotify reports for a ref update.
    fn write_to(path: PathBuf) -> notify::Event {
        notify::Event {
            kind: EventKind::Modify(notify::event::ModifyKind::Data(
                notify::event::DataChange::Any,
            )),
            paths: vec![path],
            attrs: Default::default(),
        }
    }

    /// Commit `file.txt` with the given content, on top of whatever HEAD is.
    fn commit(repo: &git2::Repository, content: &str) {
        let blob = repo.blob(content.as_bytes()).expect("write blob");
        let mut builder = repo.treebuilder(None).expect("tree builder");
        builder
            .insert("file.txt", blob, git2::FileMode::Blob.into())
            .expect("insert blob");
        let tree_oid = builder.write().expect("write tree");
        let tree = repo.find_tree(tree_oid).expect("find tree");

        let sig = git2::Signature::now("Tester", "tester@example.com").expect("signature");
        let parents: Vec<git2::Commit> = repo
            .head()
            .ok()
            .and_then(|h| h.peel_to_commit().ok())
            .into_iter()
            .collect();
        let parent_refs: Vec<&git2::Commit> = parents.iter().collect();
        repo.commit(Some("HEAD"), &sig, &sig, content, &tree, &parent_refs)
            .expect("commit");
    }

    #[test]
    fn branch_update_is_relevant() {
        assert!(is_relevant(
            &git_dir(),
            &write_to(git_dir().join("refs/heads/main"))
        ));
    }

    /// A checkout moves HEAD without touching any file under `refs/`.
    #[test]
    fn head_move_is_relevant() {
        assert!(is_relevant(&git_dir(), &write_to(git_dir().join("HEAD"))));
    }

    /// `git gc` and `git pack-refs` delete the loose refs and write this file
    /// instead. The refs still move; only their storage changes.
    #[test]
    fn packed_refs_is_relevant() {
        assert!(is_relevant(
            &git_dir(),
            &write_to(git_dir().join("packed-refs"))
        ));
    }

    /// git writes `<ref>.lock` first and renames it into place after. The lock
    /// holds no ref the diff can read, and the rename raises its own event.
    #[test]
    fn ref_lock_is_not_relevant() {
        assert!(!is_relevant(
            &git_dir(),
            &write_to(git_dir().join("refs/heads/main.lock"))
        ));
    }

    /// inotify reports a plain read. Answering "did the range move?" reads the
    /// ref files, so counting a read as a change makes the watcher feed itself
    /// and spin without end.
    #[test]
    fn reading_a_ref_is_not_relevant() {
        let read = notify::Event {
            kind: EventKind::Access(notify::event::AccessKind::Open(
                notify::event::AccessMode::Read,
            )),
            paths: vec![git_dir().join("refs/heads/main")],
            attrs: Default::default(),
        };
        assert!(!is_relevant(&git_dir(), &read));
    }

    /// The behavior that matters: a commit made while `lado` runs reaches the
    /// callback. Uses a real repository and a real filesystem event, because a
    /// test against `is_relevant` alone would not prove the watcher is wired to
    /// the right directory.
    #[test]
    fn a_commit_reaches_the_callback() {
        let dir = TempDir::new().expect("temp dir");
        let repo = git2::Repository::init(dir.path()).expect("git init");
        commit(&repo, "first");

        let (tx, rx) = mpsc::channel();
        let _watcher = DiffWatcher::spawn(repo.path(), move || {
            let _ = tx.send(());
        })
        .expect("spawn watcher");

        commit(&repo, "second");

        rx.recv_timeout(Duration::from_secs(5))
            .expect("a commit must reach the callback");
    }

    /// Guards the allowlist against a later widening. Object writes and index
    /// writes are the loudest events in the directory — every commit, every
    /// fetch, every `git status` — and none of them move a ref.
    #[test]
    fn object_and_index_writes_are_not_relevant() {
        for noise in ["objects/ab/cdef0123", "index", "COMMIT_EDITMSG"] {
            assert!(
                !is_relevant(&git_dir(), &write_to(git_dir().join(noise))),
                "{noise} must not trigger a reload"
            );
        }
    }
}
