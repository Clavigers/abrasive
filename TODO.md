# To Do
## Backlog

[DESIGN] Convert the half slop image on the web page into a fully non slop vector thing

[INFRA] Auto-deploy daemon to Hetzner on push to master (GitHub Actions: rsync daemon/, rebuild, restart systemd service) like netlify
[INFRA] Get a real domain like abrasive-rs or abrasivebuild or something
[INFRA] put abrasive on 1 million package managers

[MARKETING] Make a real demo video
[MARKETING] Make The getting started page real and good
[MARKETING] Make sure none of the links are dead
[MARKETING] Make Docs
[MARKETING] De-Slopify the site
[MARKETING] De-Sussify the Github page for abrasive-cli

[IDEA] run the local cargo and the remote, make them race first to be done wins
[IDEA] make the init / setup command ask you questions in the foreground while the repo is being synced in the background
[IDEA] get the sync stuff to be faster (maybe by creating something more like a merkle tree / or just a 32 bit everything hash that can be calculated quickly and potentially stored somewhere)

[SYNC] Two-tier digest fast path: client sends `ManifestDigest { team, scope, blake3(sorted_manifest) }` first; daemon caches last-accepted digest per `(team, scope)`; on hit reply `ManifestUpToDate` and skip the full manifest exchange entirely
[SYNC] Opt-in `ignore_globs` field in `abrasive.toml` to exclude source files that aren't part of the build (docs, fixtures, tooling) from the sync entirely
[SYNC] Opt-in `ignore_env` field in `abrasive.toml` listing env vars (or globs) the client should NOT forward to the remote build
[FARM] Refactor `workspace_path` to return a routed slot path (`~/{team}_{scope}/slot_N/`) instead of `~/{team}_{scope}/`; start with M=1 and a "always slot 0" router as a no-op refactor
[FARM] Use the authenticated user identity (from the existing auth/login step) as the slot key — no separate fingerprint needed
[FARM] Implement per-`(team, scope)` slot pool in the daemon: `Vec<Slot>` with `current_user: Option<String>`, `last_used: Instant`, hardcoded M=4
[FARM] Routing policy: take the slot you used last time (matching authed user) if it's free, else take any free slot (clobbering whoever was there), else queue. That's it.
[FARM] Add capacity-based LRU eviction when pool is full and a new user arrives with no free slot
[FARM] Print routed slot id in the CLI output so devs can debug "why was my build slow"
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
[TRANSPORT] Convert raw TLS + custom `Header { length }` framing to WebSockets (`tokio-tungstenite` or `tungstenite`). Keep the rustls config and the bincode `Message` payloads as-is — only the framing layer changes. Auth becomes `Authorization: Bearer <token>` on the WS upgrade request, validated by the daemon before accepting the connection. Lets us delete the manual `recv_msg` `read_exact(SIZE)` ceremony and integrates cleanly with the planned GitHub OAuth login flow.
[POLISH] Fix interleaved `[REMOTE]` prefix output: buffer until newline before prefixing each line, so chunks that arrive split mid-line don't render as `[REMOTE]    Compiling[REMOTE]  bytemuck v1.25.0`
[POLISH] Surface remote environment errors (missing `pkg-config`, missing system libs from build scripts) more clearly in the CLI rather than hiding them in the cargo wall-of-text
[POLISH] Make `--version` / `--help` work outside an abrasive workspace (currently they get filtered by `should_go_remote` and forwarded to cargo even though they're abrasive subcommands)
[POLISH] Make "setup" command that syncs and interactively writes an abrasive.toml file

## ASAP

websockets rewrite

github auth

make multiple clones per scope (slots) M=4 add queue and fingerprint