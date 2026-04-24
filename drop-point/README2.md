# drop-point
drop-point is a remote cargo cache greatly inspired by sccache

This is meant to answer these questions:
- "What would sccache look like if it only needed to support cargo / rustc?"
- "What would a remote cache look like if it was married to remote exec?"

### What would sccache look like if it only needed to support cargo / rustc?

sccache is designed to work with a few different compilers so it's a general solution, as a result there are a lot of rust-specific opportunities left on the  table. drop-point is designed for rust, cargo, and rustc.

specifically:
- It supports incremental compilation. For more on why / how see INCREMENTAL.md
- It can cache build.rs and proc macros (imperfectly but good enough for us)
- Cargo-aware, not just rustc-aware. sccache wraps rustc and treats each invocation independently; drop-point has some per-cargo-invocation logic.
- Pipelined caching. rustc emits .rmeta before codegen finishes so downstream crates can start type-checking early. drop-point stores .rmeta and .rlib as separate blobs and serves them independently. A hit on .rmeta returns immediately even if the .rlib isn't ready yet. sccache treats a compile as atomic so it can't do this.


### What would a remote cache look like if it was married to remote exec?

What if you don't get remote caching unless you use remote build execution? in a system like bazel there are three platforms to think about at a given time: 'host', 'exec' and 'target'. host is typically the dev's laptop exec is the build machine and target is where the built code will run. In the default configuration (no target specified) host and target are assumed to be the same.

bazel allows you to graduate. First you run the bazel client and server on the same machine just to get things working. Next you set up remote shared cache. You set up remote build execution last. Problems arise in that middle step, and lots of complexity comes from supporting it. When compilation is distributed across different machines with different setups but the cache is shared it is easy to accidentally leak details of the host system into the built artifacts. you can imagine just how easy it is to avoid this when the only thing you ever ship off the the remote is a build request, and all building happens on the remote in a well defined environment.

so for now for abrasive I am going easy mode. It's all or nothing. if you use remote caching you use remote building.

I think alot of sccache's complexity comes from having to handle arbitrary dev machines with arbitrary rustcs, sysroots, $HOMEs, and envs all writing to the same cache. If I assume every builder is running a known image I can have a digest / hash per image and that to cover any and all environment mismatches.

I'm hoping to skip:

- Toolchain identity stops being a per-invocation hashing problem. No rustc binary fingerprinting, no rustup shim walking, no sysroot re-hashing, no codegen backend detection. The image digest is the toolchain identity.
- Path handling. No `--remap-path-prefix` discovery, no absolute-path heuristics in `-I`/`-L`/`--extern`, no `$HOME`/`$CARGO_HOME` detection, no case-sensitivity handling. The workspace lives where I put it.
- Env denylist. I can use an allow-list instead because I control what env the build starts with.
- Mostly free semi Hermeticity.
- Whole local-mode codepath. No offline fallback, no per-user cache config, no client-side rustc logic.

Might be wrong about some of this. I am taking note of the decision here so I can revisit my thinking. here is some prior art from the bazel world that partly shaped my thinking: [bonanza](https://github.com/buildbarn/bonanza), BuildBuddy's ["remote bazel"](https://www.buildbuddy.io/docs/remote-bazel/) (Maggie Lou's BazelCon 2024 talk [Introducing Remote Bazel](https://www.youtube.com/watch?v=BM2gsH2Ao04) is a good intro). Julio Merino's writeup of [the next generation of bazel builds](https://blogsystem5.substack.com/p/the-next-generation-of-bazel-builds) covers the broader pattern.
