# Abrasive: Remote Cache / Remote Exec for Cargo

## The Bazel Realization

I had a thought the other day that maybe I am making my life harder than it needs to be by committing to this bazel stuff, and I listed out what I like about bazel in theory.

1. Incremental compilation / execution, the DAG thing.
   - 1.a Cross-language dependency graph
2. The dev env idea (manage environment variables and sysroots and whatever else can differ from system to system that makes a project buildable or not buildable, basically what people are using docker for when they are running builds within docker)
3. The task runner / developer onboarding idea — install one tool that installs the right version of everything else in the universe, use this tool to register tasks like "run dev server" or "run linter" or "do the release process" etc. Central management for npm and cargo and uv and a million other things that sort of combine task running and package managing.
   - 3.a The theoretical central list of all these tasks. I can bazel query for them, I can check the BUILD file in the repo root for common tasks, I can automatically run a bunch of runnable targets by matching a pattern for instance `bazel test //...`
4. Remote caching — all agents share one cache including CI! So if you just ran all tests in your local testing the remote running won't run them again in CI, it will just get an instant cache hit.
5. Remote execution — you can use cloud services to rent a very strong machine and make this crazy fast. (Bursty usage means renting is better than buying). Also gives people one fast path to building inside a container. You could make everyone build in a docker container but that would be annoying, and speed beats hygiene so people eventually will just turn local sandboxing off. Having very fast remote builds set up incentivizes everyone to keep the builds working inside the sandbox and only inside the sandbox, which makes reproducibility of builds a little easier.
6. Bit for bit reproducibility
7. It is a build system that I can extend programmatically

## My realization

Forcing all of these goals to come from one tool (bazel) is making my life harder than it needs to be. An alternative approach:

Mostly use one language (Rust). When I do use other languages just use their build systems and almost always avoid the need for a cross-language dependency graph. Maybe if I need it I can use mise tasks which are a DAG as well (https://mise.jdx.dev/tasks/architecture.html).

**mise handles:**
- 1.a (kinda)
- 2. completely
- 3. completely
- 3.a kinda (I'm gonna say not really for now)

**cargo natively handles:**
- 1. completely
- 7. and I could fill in some gaps with python scripts and mise tasks

So if I could make a remote execution / remote caching service that is just for cargo and it reuses all the incremental compilation logic from cargo... the only problem I would have left is bit for bit reproducibility.

For my own sanity, I would be ok with a post hoc check. Instead of forcing the logic to be reproducible by making the build system not be able to express certain things, I can asynchronously check if non-determinism has entered by rerunning the build while fuzzing the environment (using something like reprotest).

So this has convinced me that remote exec/cache for Rust is a thing worth making / investigating solutions.

Claude said it thinks I should do this as a cargo plugin (which I think is what cargo-remote does) and it also thinks I should implement it with the Remote Build Execution Protocol same as bazel, pants, buck etc. I think claude might be pattern matching a little too hard / cargo culting there, but I want to investigate the approach for myself.

The main prior art I have found myself so far (just by googling) is this:
- https://github.com/sgeisler/cargo-remote
- https://github.com/liamaharon/crunch

I do not think any of these sensibly articulate the all important (Host | Exec | Target) relationship though which makes them feel sort of flabby to me.

I word vommitted my idea for an approach to this problem to claude and asked for it to do its own prior art survey, here is what it gave me:

[AI WRITTEN START]
## Prior Art: Remote Caching / Remote Exec for Cargo/Rust

### sccache (Mozilla)
https://github.com/mozilla/sccache

A compiler wrapper that intercepts individual rustc invocations (via `RUSTC_WRAPPER`), hashes the inputs, and caches the outputs. Supports remote storage backends: S3, GCS, Azure, Redis, Memcached, GitHub Actions cache, WebDAV. Also has a distributed compilation mode (sccache-dist) that farms compile jobs to remote workers with bubblewrap sandboxing. Key limitations: cannot cache crates that invoke the linker (bin, dylib, cdylib, proc-macro targets), cannot cache incremental compilation, and operates at the individual rustc invocation level rather than the build-graph level. It's the de facto standard for Rust build caching today but it's bolted on from outside rather than working with cargo's own incrementality.

### The approach we're considering

All builds happen on a shared remote server. Every developer's code is rsynced to the server and cargo runs there. This gives us a single machine with a single toolchain, single paths, single environment — no cross-machine consistency problems.

The naive version of this is a shared `target/` directory, but that breaks under concurrent builds (cargo is not designed for multiple writers to the same `target/`). The Bazel solution is a content-addressed artifact store where actions have immutable inputs and outputs keyed by content hash. We want the same thing, but working *with* cargo instead of replacing it.

**Architecture:**

1. **Content-addressed artifact store** (shared, immutable, append-only) lives on the server. Key = hash of (source files + dependency artifacts + rustc version + flags + relevant env vars). Value = the compiled artifact. Multiple versions of the same crate coexist, keyed by different inputs.

2. **Per-user `target/` directory** (mutable, ephemeral). Before cargo runs, abrasive populates it from the store — pulling in cached artifacts for any crate whose inputs haven't changed. After cargo runs, new artifacts are harvested into the store.

3. **Mtime forging** is the critical glue. Cargo decides whether to rebuild a crate by comparing source file mtimes against artifact mtimes (plus fingerprints stored in `target/.fingerprint/`). When we pull a cached artifact from the store into a user's `target/`, cargo didn't compile it — it has no reason to trust it. We forge the artifact's mtime to match what cargo's fingerprinting expects (derived deterministically from content hashes: same content → same forged mtime). Cargo sees: source mtime < artifact mtime, fingerprint matches → skip rebuild. Without mtime forging, cargo would see unfamiliar timestamps and rebuild everything, defeating the cache entirely.

4. **No concurrent write problem** because each user has their own `target/`. No redundant compilation because the store is shared. Cargo's native DAG, dependency resolution, and incremental compilation do all the real work — abrasive is a thin layer managing the store and making the per-user `target/` state consistent.

This sidesteps the REAPI problem (decomposing cargo's build into hermetic actions it wasn't designed to express) and the sccache problem (wrapping rustc from outside, throwing away cargo's own incremental compilation). It also sidesteps the problems the cargo team is struggling with on issue #5931 (build scripts invalidating everything downstream via fresh mtimes) because same content → same forged mtime → no spurious invalidation.

Hermeticity is not enforced at authoring time but checked post hoc via environment fuzzing (see reproducibility verifier below).

[AI WRITTEN END]

I think this could work.

## Build Reproducibility Verifier

For bit for bit reproducibility I investigated reprotest and I think it is partially worth stealing and partially not. I word vomited my ideas for a new tool based on reprotest and had claude write a ticket for the idea, the following text is all claude but the concept is from me (mimihyp + reprotest - deb stuff)

[AI WRITTEN START]

### Build Reproducibility Verifier with Environment Fuzzing and Variation Shrinking

**Context:** We want to detect accidental hermeticity violations in our builds — cases when build outputs depend on environment details they shouldn't (hostname, timestamps, paths, etc.).

Each environment axis is a generator. Starting set of axes (drawn from reprotest's variation list):

- `build_path` — generates directory paths
- `kernel` — generates kernel version strings (via uname spoofing)
- `aslr` — generates on/off
- `num_cpus` — generates integers
- `time` — generates faketime offsets
- `user_group` — generates username/group pairs
- `fileordering` — generates directory iteration orders (via disorderfs)
- `domain_host` — generates hostnames
- `home` — generates home directory paths
- `locales` — generates locale strings
- `exec_path` — generates `$PATH` orderings
- `timezone` — generates from the timezone set
- `umask` — generates umask values
- `environment` — generates arbitrary env var noise

The only check is byte-identity of the artifact glob. The actionable output comes from shrinking, not from diffing.

### Shrinking

Standard PBT shrinking over the full generated environment. Find the minimal delta from the default environment that still causes output divergence, including interaction effects between axes.

See https://github.com/DRMacIver/minithesis for a reference implementation.

### Usage modes

- **Background/async:** Run continuously or on a schedule against the current tree. Non-blocking to developers. Flag violations when found.
- **Release gate:** Run as a CI check before tagged releases. Blocking. Must pass (no variation causes output divergence) for the release to proceed.

### Non-goals

- Detecting nondeterminism within a fixed environment (e.g. parallel build races). That's a related but different problem.
- Producing human-readable diffs of binary artifacts.
- Debian/distro packaging support.
[AI WRITTEN END]

### rust code scanning 

on a similar note, I have a few ideas for scans you could run on a rust project to throw off some warnings about reproducibility, for instance in the env! macro in rust grabs Environment variables at build time to configure the build of course these need to be in our dag for cache invalidation and if the build needs env vars set then those need to be in the vm running the remote build so we could scan the code for env! calls and warn there, we could also scan the build code for ANY io or randomness. I think this big source of nondeterminism that we will have to shrug and say skill issue to is racey build code. 

[AI WRITTEN END]

HIGH QUALITY REFERENCES ON THIS TOPIC

https://mmapped.blog/posts/17-scaling-rust-builds-with-bazel
https://matklad.github.io/2021/09/04/fast-rust-builds.html 
https://brokenco.de/2025/08/25/sccache-is-pretty-okay.html 