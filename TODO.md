# To Do
## Backlog

[DESIGN] Convert the half slop image on the web page into a fully non slop vector thing

[INFRA] Get a real domain like abrasive-rs or abrasivebuild or something
[INFRA] put abrasive on 1 million package managers
[INFRA] Stand up reverse proxy (nginx/caddy) in front of the daemon on a real domain, terminate TLS there with Let's Encrypt — prerequisite for OAuth redirect URIs working
[INFRA] After proxy lands: rip rustls + cert handling out of the daemon, have it accept plain TCP on loopback; delete certs/server.crt + server.key + the bundled cert in cli/tls.rs
[INFRA] After proxy lands: switch CLI to public-CA trust (webpki-roots or system roots) instead of the bundled self-signed cert; can collapse cli/src/tls.rs to a single tungstenite::connect call

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
[POLISH] Fix interleaved `[REMOTE]` prefix output: buffer until newline before prefixing each line, so chunks that arrive split mid-line don't render as `[REMOTE]    Compiling[REMOTE]  bytemuck v1.25.0`
[POLISH] Surface remote environment errors (missing `pkg-config`, missing system libs from build scripts) more clearly in the CLI rather than hiding them in the cargo wall-of-text
[POLISH] Make `--version` / `--help` work outside an abrasive workspace (currently they get filtered by `should_go_remote` and forwarded to cargo even though they're abrasive subcommands)
[POLISH] Make "setup" command that syncs and interactively writes an abrasive.toml file
[POLISH] WebSocket Ping/Pong keepalive: tungstenite doesn't auto-pong on the sync API; long builds may need an explicit ping loop or a read timeout policy to detect dead peers (right now we just silently `continue` past Ping/Pong frames)

[RUN] Support `cargo run` remotely: capture post-`--` args on the client, send build request, stream built binary back, then exec it locally with the captured args. Use an `If-None-Match`-style protocol to avoid re-shipping unchanged binaries: client includes the hash of its last-cached binary in the build request; after building, remote compares hashes and either responds "match, no bytes" (hit) or sends hash+bytes in one shot (miss). Zero extra RTT, zero wasted bandwidth on hits.

## ASAP