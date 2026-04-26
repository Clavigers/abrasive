//! Tests assert that semantically-equivalent rustc argvs produce the same
//! cache key, and that meaningfully-different ones produce different keys.
//! The point is to lock in the invariants drop-point's cache depends on so
//! that a future change to digest.rs / rustc_args.rs that quietly changes
//! the hash-input set fails here instead of silently invalidating caches.

use super::hash_rustc_args;
use crate::rustc_args::{ParseOutcome, parse_arguments};
use std::ffi::OsString;
use std::path::Path;

fn key(args: &[&str]) -> String {
    let argv: Vec<OsString> = args.iter().map(|s| OsString::from(*s)).collect();
    let parsed = match parse_arguments(&argv, Path::new(".")) {
        ParseOutcome::Ok(p) => p,
        other => panic!("parse_arguments failed: {other:?}\n  argv: {args:?}"),
    };
    let mut hasher = blake3::Hasher::new();
    hash_rustc_args(&parsed, &mut hasher).expect("hash_rustc_args");
    hasher.finalize().to_hex().to_string()
}

/// Smallest valid argv that parse_arguments accepts as cacheable.
const BASE: &[&str] = &[
    "--crate-name",
    "foo",
    "--edition=2021",
    "src/lib.rs",
    "--crate-type",
    "lib",
    "--emit=link,metadata,dep-info",
    "-C",
    "metadata=abc123",
    "-C",
    "extra-filename=-abc123",
    "--out-dir",
    "/tmp/target/debug/deps",
];

fn extend<'a>(extras: &[&'a str]) -> Vec<&'a str> {
    BASE.iter().chain(extras.iter()).copied().collect()
}

// Order-independence ---------------------------------------------------------

#[test]
fn cfg_reorder_is_same() {
    let a = extend(&["--cfg", "feature=\"a\"", "--cfg", "feature=\"b\""]);
    let b = extend(&["--cfg", "feature=\"b\"", "--cfg", "feature=\"a\""]);
    assert_eq!(key(&a), key(&b));
}

#[test]
fn extern_reorder_is_same() {
    let a = extend(&[
        "--extern",
        "x=/tmp/target/debug/deps/libx-h1.rmeta",
        "--extern",
        "y=/tmp/target/debug/deps/liby-h2.rmeta",
    ]);
    let b = extend(&[
        "--extern",
        "y=/tmp/target/debug/deps/liby-h2.rmeta",
        "--extern",
        "x=/tmp/target/debug/deps/libx-h1.rmeta",
    ]);
    assert_eq!(key(&a), key(&b));
}

#[test]
fn check_cfg_reorder_is_same() {
    // --check-cfg is filtered out, so any ordering is equivalent.
    let a = extend(&["--check-cfg", "cfg(a)", "--check-cfg", "cfg(b)"]);
    let b = extend(&["--check-cfg", "cfg(b)", "--check-cfg", "cfg(a)"]);
    assert_eq!(key(&a), key(&b));
}

// Form-invariance (concatenated vs separated) -------------------------------

#[test]
fn cfg_concatenated_and_separated_are_same() {
    let a = extend(&["--cfg=feature=\"x\""]);
    let b = extend(&["--cfg", "feature=\"x\""]);
    assert_eq!(key(&a), key(&b));
}

#[test]
fn emit_concatenated_and_separated_are_same() {
    // Splice in place so the relative position of --emit in argv stays
    // identical between the two forms.
    let mut a = BASE.to_vec();
    let pos = a.iter().position(|s| s.starts_with("--emit=")).unwrap();
    a.splice(pos..=pos, ["--emit", "link,metadata,dep-info"]);
    let b = BASE.to_vec();
    assert_eq!(key(&a), key(&b));
}

// Filtered-arg invariance ---------------------------------------------------

#[test]
fn out_dir_path_does_not_affect_key() {
    let a: Vec<&str> = BASE
        .iter()
        .copied()
        .map(|s| {
            if s == "/tmp/target/debug/deps" {
                "/aaa/x"
            } else {
                s
            }
        })
        .collect();
    let b: Vec<&str> = BASE
        .iter()
        .copied()
        .map(|s| {
            if s == "/tmp/target/debug/deps" {
                "/zzz/y"
            } else {
                s
            }
        })
        .collect();
    assert_eq!(key(&a), key(&b));
}

#[test]
fn extern_path_does_not_affect_key_when_basename_matches() {
    let a = extend(&["--extern", "x=/buildA/target/debug/deps/libx-h1.rmeta"]);
    let b = extend(&["--extern", "x=/buildB/target/debug/deps/libx-h1.rmeta"]);
    assert_eq!(key(&a), key(&b));
}

#[test]
fn dash_l_path_does_not_affect_key() {
    let a = extend(&["-L", "dependency=/aaa/target/debug/deps"]);
    let b = extend(&["-L", "dependency=/zzz/target/debug/deps"]);
    assert_eq!(key(&a), key(&b));
}

#[test]
fn diagnostic_width_does_not_affect_key() {
    let a = extend(&["--diagnostic-width", "80"]);
    let b = extend(&["--diagnostic-width", "120"]);
    assert_eq!(key(&a), key(&b));
}

// Negative tests (changes that MUST change the key) -------------------------

#[test]
fn extern_basename_change_does_change_key() {
    let a = extend(&["--extern", "x=/tmp/target/debug/deps/libx-h1.rmeta"]);
    let b = extend(&["--extern", "x=/tmp/target/debug/deps/libx-h2.rmeta"]);
    assert_ne!(key(&a), key(&b));
}

#[test]
fn cfg_value_change_does_change_key() {
    let a = extend(&["--cfg", "feature=\"a\""]);
    let b = extend(&["--cfg", "feature=\"b\""]);
    assert_ne!(key(&a), key(&b));
}

#[test]
fn crate_name_change_does_change_key() {
    let a = BASE.to_vec();
    let mut b = BASE.to_vec();
    let pos = b.iter().position(|s| *s == "foo").unwrap();
    b[pos] = "bar";
    assert_ne!(key(&a), key(&b));
}

#[test]
fn metadata_hash_change_does_change_key() {
    let a = BASE.to_vec();
    let b: Vec<&str> = BASE
        .iter()
        .copied()
        .map(|s| {
            if s == "metadata=abc123" {
                "metadata=xyz789"
            } else {
                s
            }
        })
        .collect();
    assert_ne!(key(&a), key(&b));
}

// Parser-level normalizations -----------------------------------------------

#[test]
fn color_setting_does_not_affect_key() {
    // Parser drops --color from the args vec entirely; client always asks
    // for --color=always and the host strips colors if needed.
    let a = extend(&["--color", "always"]);
    let b = extend(&["--color", "never"]);
    assert_eq!(key(&a), key(&b));
}

#[test]
fn leading_rustc_is_discounted() {
    // When the wrapper is invoked via clippy-driver, argv[0] is literally
    // "rustc". The parser silently drops it.
    let mut a = vec!["rustc"];
    a.extend_from_slice(BASE);
    let b = BASE.to_vec();
    assert_eq!(key(&a), key(&b));
}

#[test]
fn crate_type_lib_equals_rlib() {
    // ArgCrateTypes treats "lib" and "rlib" as the same flag, and
    // re-emits the canonical form ("rlib"), so both inputs hash identically.
    let a: Vec<&str> = BASE
        .iter()
        .copied()
        .map(|s| if s == "lib" { "rlib" } else { s })
        .collect();
    let b = BASE.to_vec();
    assert_eq!(key(&a), key(&b));
}

#[test]
fn crate_type_value_set_is_order_independent() {
    // ArgCrateTypes::into_arg_os_string sorts the type list before emitting,
    // so any permutation hashes the same.
    let a: Vec<&str> = BASE
        .iter()
        .copied()
        .map(|s| if s == "lib" { "lib,staticlib" } else { s })
        .collect();
    let b: Vec<&str> = BASE
        .iter()
        .copied()
        .map(|s| if s == "lib" { "staticlib,lib" } else { s })
        .collect();
    let c: Vec<&str> = BASE
        .iter()
        .copied()
        .map(|s| if s == "lib" { "rlib,staticlib" } else { s })
        .collect();
    let k = key(&a);
    assert_eq!(k, key(&b));
    assert_eq!(k, key(&c));
}

#[test]
fn edition_change_does_change_key() {
    let a = BASE.to_vec();
    let b: Vec<&str> = BASE
        .iter()
        .copied()
        .map(|s| {
            if s == "--edition=2021" {
                "--edition=2018"
            } else {
                s
            }
        })
        .collect();
    assert_ne!(key(&a), key(&b));
}
