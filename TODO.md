# To Do

## V0 (blocking launch)

[CAS] New crate `abrasive-rustc-wrapper` that the daemon spawns as `RUSTC_WRAPPER`; first version is pure passthrough, just logs the rustc invocation it sees
[CAS] Wire the daemon's cargo spawn to set `RUSTC_WRAPPER=/path/to/abrasive-rustc-wrapper`
[CAS] Parse rustc command line in the wrapper: extract `--crate-name`, source root, `--out-dir`, `--extern` deps with `.rmeta` paths, `-C` flags, edition, target, etc.
[CAS] Compute a content hash per invocation: hash of crate source tree (covers proc macros + build.rs correctly), hash of all dep `.rmeta`s, normalized rustc flags (with `-C incremental`, `-C metadata`, output filenames stripped), rustc version, env hash (see [ENV])
[CAS] Source CAS: sync writes blobs to `~/.abrasive/cas/<prefix>/<hash>` (chmod 444); workspaces are hardlink farms over the CAS; teardown is `rm -rf <workspace>`; CAS blobs reaped when link count drops to 1
[CAS] Unified blob store — source files, rlibs, rmetas, build-script outputs all live in the same CAS keyed by blake3 hash; add a small kind tag only if needed for eviction policy
[CAS] Network CAS service: local per-worker hardlink cache + shared network CAS as source of truth; worker fetches misses on demand
[CAS] Gap-fill step checks CAS before asking client for missing blobs — this is the cross-user dedup payoff
[CAS] Include a fleet-snapshot marker (hash of `/var/lib/dpkg/status` or worker image digest) in the CAS key root so `apt upgrade` on the builder partitions old/new entries cleanly
[CAS] On cache hit: hardlink cached rlib/rmeta into the location cargo expected (rename to match `--out-dir`/filename cargo asked for), skip rustc
[CAS] On cache miss with non-incremental rustc: run rustc, upload outputs to CAS after success
[CAS] On cache miss with incremental rustc: run rustc, do NOT cache (output is nondeterministic)
[CAS] Atomic cache writes via temp-file-then-rename so concurrent slot builds can't corrupt entries
[CAS] Use `--remap-path-prefix` for workspace, `$CARGO_HOME`, and the rustc sysroot so embedded paths are machine-independent
[CAS] Cache build script outputs separately, keyed on (build script source hash, declared env vars, target triple, fleet-snapshot marker)
[CAS] Denylist of crates known to be non-deterministic (anything embedding git rev, build time, hostname) — wrapper skips caching them
[CAS] Capacity-based LRU eviction when the CAS grows beyond a configured size
[CAS] Lazy reaper: weekly job that drops CAS blobs with link count 1

[ORCH] Split daemon into orchestrator (routing, quotas, auth, CAS gateway) and N builder workers
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
[METRICS] Per-org disk footprint in CAS (sum of referenced blob bytes)
[METRICS] Build history table: `{build_id, user, org, scope, target, started_at, finished_at, exit_code, bytes_downloaded_from_cas, cache_hit_rate}`
[METRICS] In-progress builds endpoint from orchestrator (for a dashboard view)
[METRICS] Surface usage to the dashboard; reuse the existing Supabase schema patterns

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
[IDEA] get the sync stuff to be faster (maybe by creating something more like a merkle tree / or just a 32 bit everything hash that can be calculated quickly and potentially stored somewhere)

[SYNC] Opt-in `ignore_globs` field in `abrasive.toml` to exclude source files that aren't part of the build (docs, fixtures, tooling) from the sync entirely

[PERF] Ship `abrasive run` artifacts as zstd-reference-frame deltas against the client's last-received binary. Client keeps the last executable (bytes + hash) in agent memory keyed on scope; daemon uses it as a zstd prefix for the next compression so only the actual bytewise delta pays wire cost. Uses the zstd dep we already have (`Encoder::with_prefix` / `Decoder::with_prefix`) instead of pulling in bidiff/bsdiff. Expected 2-5x smaller payloads than plain zstd for incremental rebuilds, ~30-50 lines of code + one new message exchange ("I have prefix <hash>"). Do when a real user notices `abrasive run` getting slow on a big binary — not urgent while `abrasive build` handles the inner dev loop.

[POLISH] Make "setup" command that syncs and interactively writes an abrasive.toml file

[RUN] Support `cargo run` remotely: capture post-`--` args on the client, send build request, stream built binary back, then exec it locally with the captured args. Use an `If-None-Match`-style protocol to avoid re-shipping unchanged binaries: client includes the hash of its last-cached binary in the build request; after building, remote compares hashes and either responds "match, no bytes" (hit) or sends hash+bytes in one shot (miss). Zero extra RTT, zero wasted bandwidth on hits.

## ASAP
