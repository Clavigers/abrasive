# To Do

## V0 (blocking launch)

[DROP-POINT] New crate `drop-point` — cargo-aware build-output cache, publishable standalone; library surface is roughly `lookup(action_key) -> Option<ActionEntry>` / `store(action_key, outputs)` where an entry is `[(output_path, blob_hash), ...]` pointing into the shared blob store (see [BLOBS])
[DROP-POINT] Action-cache / blob split enables early cutoff: downstream crates depend on dep rmeta *blob hashes*, not dep source hashes — comment-only edits to a leaf don't invalidate the downstream subtree
[DROP-POINT] New crate `abrasive-rustc-wrapper` — the thing cargo invokes as RUSTC_WRAPPER; parses argv, computes action key, calls drop-point, either materializes hit (hardlink blobs to `--out-dir`) or execs rustc + uploads outputs
[DROP-POINT] Wire the daemon's cargo spawn to set `RUSTC_WRAPPER=/path/to/abrasive-rustc-wrapper`
[DROP-POINT] Parse rustc argv in the wrapper: extract `--crate-name`, source root, `--out-dir`, `--extern` deps with `.rmeta` paths, `-C` flags, edition, target
[DROP-POINT] Compute action key: crate source tree hash (covers proc macros + build.rs correctly), dep `.rmeta` hashes, normalized rustc flags (strip `-C incremental`, `-C metadata`, output filenames), rustc version, env hash (see [ENV]), fleet-snapshot marker
[DROP-POINT] Fleet-snapshot marker (hash of worker image digest or `/var/lib/dpkg/status`) in the key root so builder upgrades partition old/new entries cleanly
[DROP-POINT] Use `--remap-path-prefix` for workspace, `$CARGO_HOME`, and the rustc sysroot so embedded paths are machine-independent
[DROP-POINT] On cache miss, non-incremental: run rustc, upload outputs to blob store, write action-cache entry
[DROP-POINT] On cache miss, incremental: run rustc, do NOT cache (output is nondeterministic)
[DROP-POINT] Cache build-script outputs separately, keyed on (build script source hash, declared env vars, fleet-snapshot marker) — target triple is implicit in the image, no need to key on it separately
[DROP-POINT] Denylist crates known to be non-deterministic (anything embedding git rev, build time, hostname) — wrapper skips caching them
[DROP-POINT] Pipelined rmeta serving: on hit, return `.rmeta` immediately even if `.rlib` is still materializing, matching cargo's own pipelining so downstream type-check doesn't wait on upstream codegen
[DROP-POINT] Source-input identity strategy splits first-party vs third-party. Third-party: rustc argv alone is sufficient. The input source path passed to rustc embeds the registry index hash plus `crate@version` (e.g. `index.crates.io-<hash>/foo-1.2.3/src/lib.rs`), and crates.io is content-addressed and immutable, so identical argv pins to identical bytes. No source hashing or `--emit=dep-info` pass needed for third-party. First-party / workspace crates: still need source hashing (run `rustc --emit=dep-info` like sccache), since workspace crate paths don't carry a version and a crate can `include_str!`/`include_bytes!` arbitrary repo paths outside its own dir.

[SYNC] Source sync writes each received file as a blob in the shared blob store (see [BLOBS]); workspace materialization is a hardlink tree over the blob store
[SYNC] Blobs chmod 444 in the blob store so build scripts can't silently mutate shared state — EACCES surfaces the problem immediately
[SYNC] Workspace teardown is `rm -rf <workspace>` (just unlinks hardlinks); blob refcount drops naturally
[SYNC] Gap-fill during sync checks local + network blob store before asking client for missing blobs — enables cross-user dedup on overlapping source trees
[SYNC] Keep the existing stat-fingerprint probe + speculative-diff send; CAS gap-fill only kicks in when the speculative send left holes

[BLOBS] Shared blake3-addressed blob store consumed by both [DROP-POINT] and [SYNC]; local replica at `~/.abrasive/blobs/<prefix>/<hash>` on each worker
[BLOBS] Network blob service as source of truth across workers; per-worker local replica fetches misses on demand
[BLOBS] Atomic writes via temp-file-then-rename so concurrent builds can't corrupt entries
[BLOBS] Capacity-based LRU eviction when a local replica grows beyond its configured size
[BLOBS] Lazy reaper: weekly job drops local blobs with link count 1; network store uses refcount-aware eviction

[ORCH] Split daemon into orchestrator (routing, quotas, auth, blob-store gateway) and N builder workers
[ORCH] Orchestrator maintains `(team, scope, user, target) → (worker, workspace)` affinity; sticky until the worker dies
[ORCH] Per-scope concurrency quota enforced by orchestrator, decoupled from per-worker slot counts
[ORCH] Worker advertises `(target, toolchain, sysroot)` capabilities; orchestrator filters the pool on target match before applying load/affinity
[ORCH] Bounded-TTL token validation cache in orchestrator (~30s) so dashboard revoke takes effect without a Supabase round-trip per request
[ORCH] Retire the on-same-box Unix-socket agent proxy — the agent becomes a thin client of the orchestrator, not a per-machine daemon

[CROSS] Per-target worker images: linux-x86_64-gnu, linux-aarch64-gnu, linux-x86_64-musl, wasm32 (macOS when viable)
[CROSS] Worker image recipes follow cross-rs conventions (target sysroots + linkers baked into the image)
[CROSS] Ultra-thin containers (Maelstrom-inspired): minimal rootfs, one-build-per-invocation lifecycle where possible
[CROSS] Orchestrator's "no worker available for target T" response doubles as "target not on the free tier"

[ENV] Explicit allow-list of env vars the client forwards AND that participate in the CAS key (defaults: `RUSTFLAGS`, `CARGO_*`, `RUSTC_*`, target/profile flags); deny `USER`, `HOSTNAME`, absolute paths, timestamps
[ENV] `abrasive.toml` can extend/override the allow-list per workspace
[ENV] Anything not in the allow-list never reaches the build env — closes the silent-nondeterminism hole and makes the CAS key honest
[ENV] Plumb an `ABRASIVE_ENV_HASH` env var from CLI → BuildRequest → daemon → wrapper; mix it into the per-crate cache key

[METRICS] Per-user build counter (for the free-tier cap, e.g. 10k/mo)
[METRICS] Per-org disk footprint in the blob store (sum of referenced blob bytes)
[METRICS] Build history table: `{build_id, user, org, scope, target, started_at, finished_at, exit_code, bytes_downloaded_from_blobs, cache_hit_rate}`
[METRICS] In-progress builds endpoint from orchestrator (for a dashboard view)
[METRICS] Surface usage to the dashboard; reuse the existing Supabase schema patterns

## V1 (post-launch)

[SYNC] Promote source sync from a flat `Vec<FileEntry { path, hash }>` manifest to a proper Merkle tree: each directory becomes a content-addressed blob (sorted `(name, kind, child_hash)` list), workspace identity = root directory blob hash. Bonanza-style. Wins: cross-workspace dedup at directory granularity (two users with identical `target/` subtrees → one blob set globally), constant-size top-level state, snapshot identity for replay/debug. Keep the existing stat-fingerprint + speculative-send as the fast path; Merkle structure is the precise/dedup path. Skip the Bonanza protobuf/reference machinery — bincode-encoded directory blobs over blake3 is enough.

## Backlog

[DESIGN] Convert the half slop image on the web page into a fully non slop vector thing

[INFRA] Get a real domain like abrasive-rs or abrasivebuild or something

[DIST] Per-target release tarballs with cargo-dist-compatible naming (`abrasive-<version>-<target-triple>.tar.{gz,xz}`) produced by the worker-cluster cross-build — Linux x86_64, Linux aarch64, maybe musl, macOS (once Mac worker lands)
[DIST] Generate `dist-manifest.json` alongside tarballs so `cargo binstall abrasive` works out of the box
[DIST] Generate + publish Homebrew tap formula on release (push formula to tap repo)
[DIST] Generate shell installer (`curl | sh`) that detects OS/arch and pulls the right target tarball
[DIST] Upload release artifacts to GitHub Release on tag push — the hosting layer Homebrew, binstall, and the shell installer all point at
[DIST] Execution strategy: literally `cp` cargo-dist's source for the pieces above into abrasive and only change what's needed to wire them to our artifact/release flow — don't reimplement. Gets the accumulated edge-case handling for free. Apache/MIT, so copying is fine; preserve copyright headers. Deliberately out of scope for now: MSI, Authenticode, Apple notarization, npm wrapper, .deb/.rpm — add only when a user asks

[MARKETING] Make a real demo video
[MARKETING] Make The getting started page real and good
[MARKETING] Make sure none of the links are dead
[MARKETING] Make Docs
[MARKETING] De-Slopify the site
[MARKETING] De-Sussify the Github page for abrasive-cli
[MARKETING] make cargo the main install path for the cli

[IDEA] run the local cargo and the remote, make them race first to be done wins
[IDEA] make the init / setup command ask you questions in the foreground while the repo is being synced in the background

[SYNC] Opt-in `ignore_globs` field in `abrasive.toml` to exclude source files that aren't part of the build (docs, fixtures, tooling) from the sync entirely

[PERF] Ship `abrasive run` artifacts as zstd-reference-frame deltas against the client's last-received binary. Client keeps the last executable (bytes + hash) in agent memory keyed on scope; daemon uses it as a zstd prefix for the next compression so only the actual bytewise delta pays wire cost. Uses the zstd dep we already have (`Encoder::with_prefix` / `Decoder::with_prefix`) instead of pulling in bidiff/bsdiff. Expected 2-5x smaller payloads than plain zstd for incremental rebuilds, ~30-50 lines of code + one new message exchange ("I have prefix <hash>"). Do when a real user notices `abrasive run` getting slow on a big binary — not urgent while `abrasive build` handles the inner dev loop.

[POLISH] Make "setup" command that syncs and interactively writes an abrasive.toml file

[RUN] Support `cargo run` remotely: capture post-`--` args on the client, send build request, stream built binary back, then exec it locally with the captured args. Use an `If-None-Match`-style protocol to avoid re-shipping unchanged binaries: client includes the hash of its last-cached binary in the build request; after building, remote compares hashes and either responds "match, no bytes" (hit) or sends hash+bytes in one shot (miss). Zero extra RTT, zero wasted bandwidth on hits.

## ASAP
