use drop_point::{ChainShape, classify};
use std::ffi::{OsStr, OsString};

fn osv(parts: &[&str]) -> Vec<OsString> {
    parts.iter().map(OsString::from).collect()
}

#[test]
fn workspace_chain_matches_env() {
    let argv = osv(&[
        "/usr/local/bin/drop-point",
        "/usr/local/bin/drop-point-skip",
        "/opt/rust/bin/rustc",
        "--crate-name",
        "my_app",
        "src/lib.rs",
    ]);
    let ws = OsStr::new("/usr/local/bin/drop-point-skip");
    let shape = classify(&argv, Some(ws)).unwrap();
    assert_eq!(
        shape,
        ChainShape::Workspace {
            rustc: OsString::from("/opt/rust/bin/rustc"),
            rest: osv(&["--crate-name", "my_app", "src/lib.rs"]),
        }
    );
}

#[test]
fn cache_chain_when_env_unset() {
    let argv = osv(&[
        "/usr/local/bin/drop-point",
        "/opt/rust/bin/rustc",
        "--crate-name",
        "serde",
        "src/lib.rs",
    ]);
    let shape = classify(&argv, None).unwrap();
    assert_eq!(
        shape,
        ChainShape::Cache {
            rustc: OsString::from("/opt/rust/bin/rustc"),
            rest: osv(&["--crate-name", "serde", "src/lib.rs"]),
        }
    );
}

#[test]
fn cache_chain_when_env_set_but_argv1_doesnt_match() {
    // Defensive: env says workspace wrapper is at one path, but cargo invoked
    // us with rustc directly at argv[1] (third-party shape). Trust argv.
    let argv = osv(&[
        "/usr/local/bin/drop-point",
        "/opt/rust/bin/rustc",
        "--crate-name",
        "serde",
    ]);
    let ws = OsStr::new("/usr/local/bin/drop-point-skip");
    let shape = classify(&argv, Some(ws)).unwrap();
    assert_eq!(
        shape,
        ChainShape::Cache {
            rustc: OsString::from("/opt/rust/bin/rustc"),
            rest: osv(&["--crate-name", "serde"]),
        }
    );
}

#[test]
fn no_args_returns_none() {
    let argv = osv(&["/usr/local/bin/drop-point"]);
    assert!(classify(&argv, None).is_none());
}

#[test]
fn workspace_with_no_rustc_returns_none() {
    // argv[1] matches the env var but there's nothing after it. Malformed
    // chain — drop-point should refuse rather than guess.
    let argv = osv(&[
        "/usr/local/bin/drop-point",
        "/usr/local/bin/drop-point-skip",
    ]);
    let ws = OsStr::new("/usr/local/bin/drop-point-skip");
    assert!(classify(&argv, Some(ws)).is_none());
}

#[test]
fn workspace_with_no_rustc_args_yields_empty_rest() {
    // Like above but argv[2] (rustc) is present, just no rustc args. Valid.
    let argv = osv(&[
        "/usr/local/bin/drop-point",
        "/usr/local/bin/drop-point-skip",
        "/opt/rust/bin/rustc",
    ]);
    let ws = OsStr::new("/usr/local/bin/drop-point-skip");
    let shape = classify(&argv, Some(ws)).unwrap();
    assert_eq!(
        shape,
        ChainShape::Workspace {
            rustc: OsString::from("/opt/rust/bin/rustc"),
            rest: vec![],
        }
    );
}
