//! Git synchronisation for corpus clones.
//!
//! The guideline servers index a local clone of a corpus repository. Before
//! this module existed, the update path read `git rev-parse HEAD` and re-parsed
//! whatever was on disk — it never contacted the remote. A server could
//! therefore sit on an old commit indefinitely while reporting that an update
//! had succeeded.
//!
//! Two properties matter here and are deliberate:
//!
//! 1. **Fast-forward only.** A corpus clone is a cache, but it may also have
//!    been placed deliberately. Nothing here resets, rebases, force-updates, or
//!    discards local modifications. Divergence is reported, not resolved.
//! 2. **A failure to reach the remote is not a failure to serve.** The caller
//!    re-indexes local content and reports the sync outcome, so a stale index
//!    is visible rather than silent.

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// What happened when a clone was synchronised with its remote.
///
/// This is an enum rather than a bool because the caller has to be able to tell
/// "already up to date" from "never looked" — conflating them is the defect
/// this module was written to remove.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncOutcome {
    /// The remote had new commits and the clone was advanced to them.
    FastForwarded { from: String, to: String },
    /// The remote was contacted; the clone already matched it.
    AlreadyCurrent,
    /// Auto-pull is switched off by configuration.
    Disabled,
    /// Nothing to sync against: no remote, no upstream, or a detached HEAD.
    /// A legitimate deployment shape, e.g. a pinned or vendored clone.
    Skipped(String),
    /// The remote could not be reached, or the clone could not be advanced.
    /// Local content is still usable; the index may be behind.
    Failed(String),
}

impl SyncOutcome {
    /// Whether the working tree moved. Distinct from "the remote was reached".
    pub fn advanced(&self) -> bool {
        matches!(self, SyncOutcome::FastForwarded { .. })
    }

    /// Whether the remote was successfully contacted.
    pub fn reached_remote(&self) -> bool {
        matches!(
            self,
            SyncOutcome::FastForwarded { .. } | SyncOutcome::AlreadyCurrent
        )
    }
}

impl fmt::Display for SyncOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SyncOutcome::FastForwarded { from, to } => {
                write!(f, "fast-forwarded {}..{}", short(from), short(to))
            }
            SyncOutcome::AlreadyCurrent => write!(f, "already current with remote"),
            SyncOutcome::Disabled => write!(f, "auto-pull disabled"),
            SyncOutcome::Skipped(reason) => write!(f, "skipped: {reason}"),
            SyncOutcome::Failed(reason) => write!(f, "failed: {reason}"),
        }
    }
}

fn short(commit: &str) -> &str {
    if commit.len() >= 7 { &commit[..7] } else { commit }
}

/// Errors that prevent reading the repository at all, as opposed to sync
/// outcomes, which are reported rather than raised.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("failed to run git: {0}")]
    Spawn(String),
    #[error("git {command} failed: {stderr}")]
    Command { command: String, stderr: String },
}

fn run(repo: &Path, args: &[&str]) -> Result<String, GitError> {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .map_err(|e| GitError::Spawn(format!("{} ({e})", args.join(" "))))?;

    if !output.status.success() {
        return Err(GitError::Command {
            command: args.join(" "),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Read the current `HEAD` commit of a clone.
pub fn head_commit(repo: impl AsRef<Path>) -> Result<String, GitError> {
    run(repo.as_ref(), &["rev-parse", "HEAD"])
}

/// Fetch the clone's upstream and fast-forward onto it.
///
/// Never fails the caller: every remote-side problem is returned as a
/// [`SyncOutcome`] so it can be reported alongside the index state. Only an
/// inability to read the repository at all is an error.
pub fn sync(repo: impl AsRef<Path>, enabled: bool) -> Result<SyncOutcome, GitError> {
    let repo = repo.as_ref();

    if !enabled {
        return Ok(SyncOutcome::Disabled);
    }

    // A detached HEAD is a deliberate pin. Advancing it would discard the
    // operator's choice, so report and leave it alone.
    match run(repo, &["symbolic-ref", "--quiet", "HEAD"]) {
        Ok(_) => {}
        Err(_) => return Ok(SyncOutcome::Skipped("detached HEAD".to_string())),
    }

    // No configured upstream means there is nothing to fast-forward onto. This
    // is normal for a vendored copy.
    let upstream = match run(repo, &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"]) {
        Ok(u) => u,
        Err(_) => return Ok(SyncOutcome::Skipped("no upstream configured".to_string())),
    };

    let before = head_commit(repo)?;

    if let Err(e) = run(repo, &["fetch", "--quiet"]) {
        return Ok(SyncOutcome::Failed(format!("fetch: {e}")));
    }

    let target = match run(repo, &["rev-parse", &upstream]) {
        Ok(t) => t,
        Err(e) => return Ok(SyncOutcome::Failed(format!("resolve {upstream}: {e}"))),
    };

    if target == before {
        return Ok(SyncOutcome::AlreadyCurrent);
    }

    // --ff-only is the whole safety story: if the clone has diverged or has
    // local commits, this refuses rather than rewriting anything.
    match run(repo, &["merge", "--ff-only", "--quiet", &target]) {
        Ok(_) => Ok(SyncOutcome::FastForwarded {
            from: before,
            to: target,
        }),
        Err(e) => Ok(SyncOutcome::Failed(format!(
            "cannot fast-forward onto {upstream}: {e}"
        ))),
    }
}

/// Read an auto-pull toggle from the environment.
///
/// Enabled unless explicitly switched off, because "off" is the behaviour that
/// let a server serve stale content while reporting success.
pub fn auto_pull_from_env(var: &str) -> bool {
    match std::env::var(var) {
        Ok(v) => !matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "no" | "off"
        ),
        Err(_) => true,
    }
}

/// Path helper used by tests and callers that hold a `String` path.
pub fn repo_path(path: &str) -> PathBuf {
    PathBuf::from(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;

    fn git(repo: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(repo)
            .status()
            .expect("git should run");
        assert!(status.success(), "git {args:?} failed");
    }

    fn init_repo(dir: &Path) {
        fs::create_dir_all(dir).unwrap();
        git(dir, &["init", "--quiet", "--initial-branch=main"]);
        git(dir, &["config", "user.email", "test@example.invalid"]);
        git(dir, &["config", "user.name", "Test"]);
    }

    fn commit(dir: &Path, name: &str, body: &str) {
        fs::write(dir.join(name), body).unwrap();
        git(dir, &["add", name]);
        git(dir, &["commit", "--quiet", "-m", name]);
    }

    /// Returns (origin, clone) in a fresh temp dir.
    fn origin_and_clone(label: &str) -> (PathBuf, PathBuf, PathBuf) {
        let base = std::env::temp_dir().join(format!("mcp-git-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let origin = base.join("origin");
        let clone = base.join("clone");

        init_repo(&origin);
        commit(&origin, "a.txt", "one");

        let status = Command::new("git")
            .args(["clone", "--quiet"])
            .arg(&origin)
            .arg(&clone)
            .status()
            .expect("clone should run");
        assert!(status.success());
        git(&clone, &["config", "user.email", "test@example.invalid"]);
        git(&clone, &["config", "user.name", "Test"]);

        (base, origin, clone)
    }

    #[test]
    fn fast_forwards_when_remote_has_new_commits() {
        let (base, origin, clone) = origin_and_clone("ff");
        let before = head_commit(&clone).unwrap();
        commit(&origin, "b.txt", "two");

        let outcome = sync(&clone, true).unwrap();
        match &outcome {
            SyncOutcome::FastForwarded { from, to } => {
                assert_eq!(from, &before);
                assert_eq!(to, &head_commit(&clone).unwrap());
            }
            other => panic!("expected fast-forward, got {other:?}"),
        }
        assert!(outcome.advanced());
        assert!(clone.join("b.txt").exists(), "new file should be present");
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn reports_already_current_without_moving() {
        let (base, _origin, clone) = origin_and_clone("current");
        let before = head_commit(&clone).unwrap();

        let outcome = sync(&clone, true).unwrap();
        assert_eq!(outcome, SyncOutcome::AlreadyCurrent);
        assert!(!outcome.advanced());
        assert!(outcome.reached_remote());
        assert_eq!(head_commit(&clone).unwrap(), before);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn refuses_to_discard_local_commits() {
        let (base, origin, clone) = origin_and_clone("diverged");
        commit(&origin, "b.txt", "remote change");
        commit(&clone, "c.txt", "local change");
        let local_head = head_commit(&clone).unwrap();

        let outcome = sync(&clone, true).unwrap();
        match &outcome {
            SyncOutcome::Failed(reason) => assert!(
                reason.contains("fast-forward"),
                "reason should name the refusal: {reason}"
            ),
            other => panic!("expected refusal, got {other:?}"),
        }
        // The point of the test: local work is still there.
        assert_eq!(head_commit(&clone).unwrap(), local_head);
        assert!(clone.join("c.txt").exists());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn skips_detached_head_rather_than_moving_it() {
        let (base, _origin, clone) = origin_and_clone("detached");
        let pinned = head_commit(&clone).unwrap();
        git(&clone, &["checkout", "--quiet", "--detach", "HEAD"]);

        let outcome = sync(&clone, true).unwrap();
        assert!(
            matches!(&outcome, SyncOutcome::Skipped(r) if r.contains("detached")),
            "got {outcome:?}"
        );
        assert_eq!(head_commit(&clone).unwrap(), pinned);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn skips_repository_with_no_upstream() {
        let base = std::env::temp_dir().join(format!("mcp-git-noup-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        init_repo(&base);
        commit(&base, "a.txt", "one");

        let outcome = sync(&base, true).unwrap();
        assert!(
            matches!(&outcome, SyncOutcome::Skipped(r) if r.contains("upstream")),
            "got {outcome:?}"
        );
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn reports_failure_when_remote_is_unreachable() {
        let (base, _origin, clone) = origin_and_clone("badremote");
        let before = head_commit(&clone).unwrap();
        git(
            &clone,
            &[
                "remote",
                "set-url",
                "origin",
                "/nonexistent/path/to/nowhere.git",
            ],
        );

        let outcome = sync(&clone, true).unwrap();
        assert!(
            matches!(&outcome, SyncOutcome::Failed(r) if r.contains("fetch")),
            "got {outcome:?}"
        );
        // Serving continues from local content.
        assert_eq!(head_commit(&clone).unwrap(), before);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn disabled_does_not_touch_the_remote() {
        let (base, origin, clone) = origin_and_clone("disabled");
        commit(&origin, "b.txt", "two");
        let before = head_commit(&clone).unwrap();

        let outcome = sync(&clone, false).unwrap();
        assert_eq!(outcome, SyncOutcome::Disabled);
        assert_eq!(head_commit(&clone).unwrap(), before);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn auto_pull_defaults_on_and_respects_explicit_off() {
        let var = "MCP_TEST_AUTO_PULL_TOGGLE";
        unsafe { std::env::remove_var(var) };
        assert!(auto_pull_from_env(var), "absent means enabled");

        for off in ["0", "false", "NO", " off "] {
            unsafe { std::env::set_var(var, off) };
            assert!(!auto_pull_from_env(var), "{off:?} should disable");
        }
        for on in ["1", "true", "yes", "anything-else"] {
            unsafe { std::env::set_var(var, on) };
            assert!(auto_pull_from_env(var), "{on:?} should enable");
        }
        unsafe { std::env::remove_var(var) };
    }
}
