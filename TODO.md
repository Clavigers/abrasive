# To Do
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
[SYNC] Opt-in `ignore_env` field in `abrasive.toml` listing env vars (or globs) the client should NOT forward to the remote build
[FARM] Use the authenticated user identity (from the existing auth/login step) as the slot key — no separate fingerprint needed
[CACHE] New crate `abrasive-rustc-wrapper` that the daemon spawns as `RUSTC_WRAPPER`; first version is pure passthrough, just logs the rustc invocation it sees
[CACHE] Wire the daemon's cargo spawn to set `RUSTC_WRAPPER=/path/to/abrasive-rustc-wrapper`
[CACHE] Parse rustc command line in the wrapper: extract `--crate-name`, source root, `--out-dir`, `--extern` deps with `.rmeta` paths, `-C` flags, edition, target, etc.
[CACHE] Compute a content hash per invocation: hash of crate source files (walk crate dir, exclude `target/`), hash of all dep `.rmeta`s, normalized rustc flags (with `-C incremental`, `-C metadata`, output filenames stripped), rustc version
[CACHE] Plumb an `ABRASIVE_ENV_HASH` env var from CLI → BuildRequest → daemon → wrapper; mix it into the per-crate cache key as the env/RUSTFLAGS/rustc-version component
[CACHE] Define exactly what goes into `ABRASIVE_ENV_HASH` on the client (rustc version, RUSTFLAGS, target triple, profile, codegen backend) — and explicitly what does NOT (USER, HOSTNAME, paths, timestamps)
[CACHE] Cache directory layout at `/dev/shm/abrasive-content-cache/<hash>/{rlib,rmeta,d}` shared across all scopes
[CACHE] On cache hit: copy/hardlink cached files into the locations cargo expected (rename to whatever `--out-dir`/filename cargo asked for), skip rustc, exit success
[CACHE] On cache miss with non-incremental rustc: run rustc, then store outputs in the cache after success
[CACHE] On cache miss with incremental rustc: run rustc, do NOT cache (output is nondeterministic)
[CACHE] Atomic cache writes via temp-file-then-rename so concurrent slot builds can't corrupt entries
[CACHE] Use `--remap-path-prefix` to canonicalize embedded paths so cache entries are slot-independent
[CACHE] Add a denylist of crates known to be non-deterministic (anything embedding git rev, build time, hostname) — wrapper skips caching them
[CACHE] Cache build script outputs separately, keyed on (build script source hash, declared env vars, target triple)
[CACHE] Capacity-based LRU eviction for the content cache when it grows beyond a configured size
[CACHE] Optional sccache backend: opt-in flag in abrasive.toml that makes the daemon set `RUSTC_WRAPPER=sccache` + `SCCACHE_DIR=/dev/shm/sccache` + `CARGO_INCREMENTAL=0` instead of using the abrasive wrapper
[PERF] Symlink `~/.cargo/registry` and `~/.cargo/git` to `/dev/shm/cargo-home/` (leave config + bin on disk)
[PERF] Confirm `/tmp` is tmpfs on the remote (`mount | grep '/tmp '`); if not, enable `tmp.mount`
[PERF] Ship `abrasive run` artifacts as zstd-reference-frame deltas against the client's last-received binary. Client keeps the last executable (bytes + hash) in agent memory keyed on scope; daemon uses it as a zstd prefix for the next compression so only the actual bytewise delta pays wire cost. Uses the zstd dep we already have (`Encoder::with_prefix` / `Decoder::with_prefix`) instead of pulling in bidiff/bsdiff. Expected 2-5x smaller payloads than plain zstd for incremental rebuilds, ~30-50 lines of code + one new message exchange ("I have prefix <hash>"). Do when a real user notices `abrasive run` getting slow on a big binary — not urgent while `abrasive build` handles the inner dev loop.
[POLISH] Surface remote environment errors (missing `pkg-config`, missing system libs from build scripts) more clearly in the CLI rather than hiding them in the cargo wall-of-text
[POLISH] Make `--version` / `--help` work outside an abrasive workspace (currently they get filtered by `should_go_remote` and forwarded to cargo even though they're abrasive subcommands)
[POLISH] Make "setup" command that syncs and interactively writes an abrasive.toml file
[POLISH] WebSocket Ping/Pong keepalive: tungstenite doesn't auto-pong on the sync API; long builds may need an explicit ping loop or a read timeout policy to detect dead peers (right now we just silently `continue` past Ping/Pong frames)

[RUN] Support `cargo run` remotely: capture post-`--` args on the client, send build request, stream built binary back, then exec it locally with the captured args. Use an `If-None-Match`-style protocol to avoid re-shipping unchanged binaries: client includes the hash of its last-cached binary in the build request; after building, remote compares hashes and either responds "match, no bytes" (hit) or sends hash+bytes in one shot (miss). Zero extra RTT, zero wasted bandwidth on hits.

## ASAP