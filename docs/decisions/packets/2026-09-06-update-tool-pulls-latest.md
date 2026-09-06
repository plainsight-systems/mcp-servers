# 2026-09-06-update-tool-pulls-latest: Make `update_*` fetch from the remote

**Status:** completed
**Change class:** cross-cutting - shared crate plus four server crates and the
documented tool contract

## Intent

- **What is changing:** `update_guidelines` and its siblings currently re-index
  whatever is already on disk. They never contact the remote, so a server can
  sit on an old commit indefinitely while reporting success. Add a shared
  `mcp_common::git` module that fetches and fast-forwards the corpus clone
  before the commit check, and wire all four servers to it.
- **Why the change is necessary:** The tool's own description says it triggers
  a re-index "from the git repository", and the server instructions tell agents
  to use it "to refresh from the repository". Neither is true. Observed
  directly: after pushing four commits to `cpp-perf-guidelines`, the tool
  returned `{"updated": true, "commit": "271694a", "guideline_count": 87}` —
  the pre-push commit — and continued serving ten categories when the corpus
  had eleven. Callers cannot distinguish "already current" from "never looked".
- **Expected behavior changes:**
  - The update path fetches and fast-forwards before deciding whether to
    re-index. A newly pushed commit is picked up without operator action.
  - The response gains a `remote_sync` field reporting what happened at the
    remote: fast-forwarded, already current, disabled, or a stated failure.
  - A fetch failure no longer silently masquerades as success, and also does
    not fail the whole update: the server re-indexes local content and says so.
- **Guaranteed invariants/contracts:**
  - **Fast-forward only.** The clone is never reset, rebased, or force-updated,
    and local modifications are never discarded. A non-fast-forward is reported,
    not resolved.
  - Existing response fields (`updated`, `commit`, `guideline_count`) keep their
    meaning; `remote_sync` is additive.
  - Servers still start and serve when the network is unavailable.
  - No change to parsing, embedding, search, or the corpus format.

## Design

Single implementation in `mcp-common`, four call sites. The alternative —
copying the logic into each crate — was rejected because the defect being fixed
is itself a consequence of that duplication: `get_repo_commit` is already
copy-pasted four times, and all four copies lacked the fetch.

`GitSync::sync()` returns an outcome rather than a bare bool, because the
caller must be able to tell the difference between the cases and report it:

| Outcome | Meaning |
|---|---|
| `FastForwarded { from, to }` | Remote had new commits; clone advanced |
| `AlreadyCurrent` | Fetched; clone already matched the remote |
| `Disabled` | Auto-pull turned off by configuration |
| `Skipped(reason)` | No remote, no upstream, or detached HEAD |
| `Failed(reason)` | Fetch or fast-forward failed; local content used |

`Skipped` and `Failed` are deliberately distinct: the first is a legitimate
deployment shape (a pinned or vendored clone), the second is something wrong.

Auto-pull can be disabled with `<SERVER>_REPO_AUTO_PULL=0` for pinned or
air-gapped deployments. Default is enabled, because the current default is the
behaviour that caused this packet.

## Scope

**In:** the shared git module, its unit tests, wiring in the four update
services, the added response field, and the README and tool-description text
that currently overstate what the tool does.

**Explicitly out:**

- Cloning a repository that is not already present. The servers require an
  existing clone today and that is unchanged.
- Any authentication handling. The corpora are public over HTTPS.
- Scheduled or background polling. This packet makes the explicit trigger
  honest; it does not add an implicit one.
- Changing what `updated` means. It still reports whether a re-index occurred.

## Acceptance Criteria

- [x] `mcp_common::git` fetches and fast-forwards a clone, and reports the
      outcome as a distinguishable value rather than a bool.
- [x] A non-fast-forward, a detached HEAD, a missing upstream, and a failed
      fetch are each reported without discarding local state.
- [x] All four servers call it before the commit check.
- [x] The update response carries the sync outcome.
- [x] A fetch failure does not fail the update, and is visible in the response.
- [x] Auto-pull can be disabled by configuration.
- [x] `cargo test` passes; `cargo clippy` is clean.
- [x] README tool contracts and server descriptions match actual behaviour.

## Verification Plan

- **V1** — `cargo test --workspace` and `cargo clippy --workspace -- -D warnings`.
- **V2** — Unit tests over real temporary git repositories covering
  fast-forward, already-current, no-upstream, detached HEAD, non-fast-forward
  divergence, and a bad remote.
- **V3** — End-to-end against the running `cpp-perf-guidelines` server: rewind
  its clone, call `update_guidelines`, and confirm it recovers the current
  commit and reports the fast-forward.
- **V4** — Confirm the README no longer describes behaviour the code does not
  have.

## Verification Evidence

- **V1** — `cargo test --workspace` -> all suites pass (8 new git tests plus the
  existing ones). `cargo clippy --workspace --all-targets -- -D warnings` -> the
  only findings are three pre-existing `clippy::get_first` hits in
  `mcp-common/src/openai.rs`, confirmed present before this change by stashing
  it and re-running. Not introduced here and not in scope.
- **V2** — Eight unit tests over real temporary git repositories:
  fast-forward with a new remote commit, already-current, refusal to discard
  local commits, detached HEAD left pinned, no upstream, unreachable remote, the
  disabled toggle, and the env-var parsing. The divergence test asserts the
  local commit and file still exist after the refusal.
- **V3** — Against the live `data/cpp-perf-guidelines` clone: reset it to
  `271694a` (the commit at which the tool had reported false success), ran the
  new sync, and it returned `fast-forwarded 271694a..7c57af8` with `HEAD` back
  at `7c57af8`. This reproduces and fixes the original failure.
- **V4** — README `update_guidelines` contracts now show
  `{ updated, commit, guideline_count, remote_sync }` and link to a new
  "Updating a corpus" section documenting the fetch, the fast-forward-only
  guarantee, every `remote_sync` value, and the opt-out variables. Tool
  descriptions in all four servers rewritten to state that they fetch first and
  report a failed sync rather than hiding it.

## Records

- 2026-09-06 - Packet opened after the tool reported success at a stale commit
  following a push to the corpus repository.
- 2026-09-06 - Implemented as a shared module rather than four copies; the
  defect existed in four places precisely because `get_repo_commit` had been
  copy-pasted four times.
- 2026-09-06 - `parses_real_corpus_when_present` was asserting a hardcoded
  category count that the corpus push broke. The literal had already been
  bumped 8 -> 9 once; rather than bump it to 11 and re-arm the trap, it now
  compares against what `categories.toml` declares, which is the property the
  assertion was reaching for.
- 2026-09-06 - Code change complete and verified. **Not deployed**: the servers
  run as Docker containers built months ago, so the running instances still
  have the old binary until the images are rebuilt and restarted.
