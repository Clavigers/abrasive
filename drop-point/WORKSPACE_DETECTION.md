# Switch workspace detection from env-var path-prefix to RUSTC_WORKSPACE_WRAPPER chain

Status: open
Component: drop-point + daemon

## Summary

Replace the current `DROP_POINT_WORKSPACE_MEMBERS` mechanism (env var of colon-separated workspace dirs, path-prefix-matched against the input source path) with cargo's own workspace classification via `RUSTC_WORKSPACE_WRAPPER`. Drop-point detects whether the current invocation is a workspace member by checking its position in the wrapper chain, and short-circuits to direct-exec rustc for workspace members.

## Motivation

Cargo already classifies workspace vs non-workspace crates and surfaces the distinction through the wrapper chain (rust-lang/cargo#7533). The current path-prefix heuristic is a less authoritative reimplementation of that classification, and it requires the daemon to run `cargo metadata` and stuff the result into an env var. Switching to the wrapper chain:

- Removes a class of misclassification bugs (symlink layouts, path-deps outside the workspace tree, registry sources mis-resolved through $HOME).
- Drops the `cargo metadata` call from the daemon.
- Drops the env-var construction and refresh-on-Cargo.toml-change logic from the daemon.
- Composes correctly with sccache and clippy if anyone ever chains them.

## Design

For workspace crates cargo invokes: `$RUSTC_WRAPPER $RUSTC_WORKSPACE_WRAPPER $RUSTC <args>`. For non-workspace crates it skips the workspace wrapper.

- Install drop-point as `RUSTC_WRAPPER`.
- Install a pass-through binary (`drop-point-skip`, a few lines that `execvp(argv[1], argv+1)`) as `RUSTC_WORKSPACE_WRAPPER`. Its presence is what triggers cargo's workspace-vs-dep distinction; drop-point detects the chain and execs rustc directly without invoking it.
- In drop-point: read `RUSTC_WORKSPACE_WRAPPER` from env. If it's set and `argv[1]` matches its value, we're in the workspace chain. `execvp(argv[2], argv[2..])` (i.e., rustc with its args, skipping the unused workspace wrapper). Otherwise, run the cache logic.

## Tasks

1. **Add `drop-point-skip` binary.** New file under `drop-point/src/bin/skip.rs`. Body: `execvp(argv[1], argv+1)`. ~10 lines. Used only as the marker for `RUSTC_WORKSPACE_WRAPPER`; should never actually execute when drop-point's detection works.
2. **Detection in `drop-point/src/lib.rs::run`.** Before `parse_args` or as part of it: if `env::var_os("RUSTC_WORKSPACE_WRAPPER")` matches `argv[1]`, exec rustc directly with `argv[2..]`. Use `std::os::unix::process::CommandExt::exec` so we don't fork.
3. **Delete `is_third_party` and `DROP_POINT_WORKSPACE_MEMBERS`** (lib.rs:84-127). Replace `plan_third_party_cache` with a version that always caches when `parse_arguments` returns `Ok` (the new chain-based detection has already filtered workspace crates by the time we get here).
4. **Daemon side: stop computing and exporting workspace members.** Remove the `cargo metadata` call and the env var setup. Set `RUSTC_WRAPPER=<drop-point>` and `RUSTC_WORKSPACE_WRAPPER=<drop-point-skip>` for the build environment.
5. **Update README + comments.** Drop the "we filter workspace via env var path prefix" line; replace with the chain-based explanation. Update the comment at lib.rs:115-118.
6. **Tests.** Add an integration test that simulates both chain shapes (workspace and non-workspace) and asserts drop-point caches the second but not the first. Mock or fake the rustc exec by pointing it at `/bin/true` or a stub that prints argv.

## Verify before merging

- **Confirm cargo's chain order.** The contract is `RUSTC_WRAPPER` outermost, `RUSTC_WORKSPACE_WRAPPER` second, rustc innermost. Read the cargo source to confirm (don't trust the PR description alone).
- **Path-deps outside `[workspace.members]`.** A `Cargo.toml` with `foo = { path = "../foo" }` where `../foo` isn't in the workspace member list. Confirm cargo treats it as a workspace-wrapper target. If yes, behavior is correct. If no, decide whether you want those cached or not (current path-prefix logic also gets this case wrong, just in the opposite direction).
- **Clippy interaction.** `cargo clippy` already uses `RUSTC_WORKSPACE_WRAPPER`; confirm we don't clobber clippy's setting if a user runs clippy through abrasive. Fix: if the env var is already set when the daemon launches the build, chain through it instead of overwriting (drop-point's skip binary should exec the previous wrapper, not rustc).

## Done criteria

- `DROP_POINT_WORKSPACE_MEMBERS` is gone from the codebase.
- Daemon no longer calls `cargo metadata` for member discovery.
- `cargo build` on a sample workspace caches third-party crates and skips workspace members on a clean run, verified by log lines.
- Integration test for both chain shapes passes.

## Risks

- **Couples drop-point to cargo's wrapper-chain semantics.** If cargo changes the chain order or composition this breaks. Low probability but it's a hard dependency on cargo internals. Mitigation: integration test that exercises the real cargo invocation, not a mock.
- **Clippy-chain interaction needs the "chain through" fix above.** If we overwrite a user's existing `RUSTC_WORKSPACE_WRAPPER`, we break clippy. The fix is straightforward but easy to forget.
- **The skip binary should never actually execute.** If detection has a bug and we fall through to running it, behavior depends on its body. Make it `execvp(argv[1], argv+1)` so worst-case it just transparently runs rustc and the build still works (no caching, no breakage).
