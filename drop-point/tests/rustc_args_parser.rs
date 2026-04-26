use drop_point::rustc_args::{
    ArgData, ArgDisposition, ArgTarget, Argument, ColorMode, ParseOutcome, ParsedArguments,
    parse_arguments,
};
use std::ffi::OsString;
use std::path::PathBuf;

fn _parse_arguments(arguments: &[String]) -> ParseOutcome<ParsedArguments> {
    let arguments = arguments.iter().map(OsString::from).collect::<Vec<_>>();
    parse_arguments(&arguments, ".".as_ref())
}

macro_rules! parses {
    ( $( $s:expr ),* ) => {
        match _parse_arguments(&[ $( $s.to_string(), )* ]) {
            ParseOutcome::Ok(a) => a,
            o => panic!("Got unexpected parse result: {:?}", o),
        }
    }
}

macro_rules! fails {
    ( $( $s:expr ),* ) => {
        match _parse_arguments(&[ $( $s.to_string(), )* ]) {
            ParseOutcome::Ok(_) => panic!("Should not have parsed ok: `{}`", stringify!($( $s, )*)),
            o => o,
        }
    }
}

#[test]
#[allow(clippy::cognitive_complexity)]
fn test_parse_arguments_simple() {
    let h = parses!(
        "--emit",
        "link",
        "foo.rs",
        "--out-dir",
        "out",
        "--crate-name",
        "foo",
        "--crate-type",
        "lib"
    );
    assert_eq!(h.output_dir.to_str(), Some("out"));
    assert!(h.dep_info.is_none());
    assert!(h.externs.is_empty());
    let h = parses!(
        "--emit=link",
        "foo.rs",
        "--out-dir",
        "out",
        "--crate-name=foo",
        "--crate-type=lib"
    );
    assert_eq!(h.output_dir.to_str(), Some("out"));
    assert!(h.dep_info.is_none());
    let h = parses!(
        "--emit",
        "link",
        "foo.rs",
        "--out-dir=out",
        "--crate-name=foo",
        "--crate-type=lib"
    );
    assert_eq!(h.output_dir.to_str(), Some("out"));
    assert_eq!(
        parses!(
            "--emit",
            "link",
            "-C",
            "opt-level=1",
            "foo.rs",
            "--out-dir",
            "out",
            "--crate-name",
            "foo",
            "--crate-type",
            "lib"
        ),
        parses!(
            "--emit=link",
            "-Copt-level=1",
            "foo.rs",
            "--out-dir=out",
            "--crate-name=foo",
            "--crate-type=lib"
        )
    );
    let h = parses!(
        "--emit",
        "link,dep-info",
        "foo.rs",
        "--out-dir",
        "out",
        "--crate-name",
        "my_crate",
        "--crate-type",
        "lib",
        "-C",
        "extra-filename=-abcxyz"
    );
    assert_eq!(h.output_dir.to_str(), Some("out"));
    assert_eq!(h.dep_info.unwrap().to_str().unwrap(), "my_crate-abcxyz.d");
    fails!(
        "--emit",
        "link",
        "--out-dir",
        "out",
        "--crate-name=foo",
        "--crate-type=lib"
    );
    fails!(
        "--emit",
        "link",
        "foo.rs",
        "--crate-name=foo",
        "--crate-type=lib"
    );
    fails!(
        "--emit",
        "asm",
        "foo.rs",
        "--out-dir",
        "out",
        "--crate-name=foo",
        "--crate-type=lib"
    );
    fails!(
        "--emit",
        "asm,link",
        "foo.rs",
        "--out-dir",
        "out",
        "--crate-name=foo",
        "--crate-type=lib"
    );
    fails!(
        "--emit",
        "asm,link,dep-info",
        "foo.rs",
        "--out-dir",
        "out",
        "--crate-name=foo",
        "--crate-type=lib"
    );
    fails!(
        "--emit",
        "link",
        "foo.rs",
        "--out-dir",
        "out",
        "--crate-name=foo"
    );
    fails!(
        "--emit",
        "link",
        "foo.rs",
        "--out-dir",
        "out",
        "--crate-type=lib"
    );
    // From an actual cargo compilation, with some args shortened:
    let h = parses!(
        "--crate-name",
        "foo",
        "src/lib.rs",
        "--crate-type",
        "lib",
        "--emit=dep-info,link",
        "-C",
        "debuginfo=2",
        "-C",
        "metadata=d6ae26f5bcfb7733",
        "-C",
        "extra-filename=-d6ae26f5bcfb7733",
        "--out-dir",
        "/foo/target/debug/deps",
        "-L",
        "dependency=/foo/target/debug/deps",
        "--extern",
        "libc=/foo/target/debug/deps/liblibc-89a24418d48d484a.rlib",
        "--extern",
        "log=/foo/target/debug/deps/liblog-2f7366be74992849.rlib"
    );
    assert_eq!(h.output_dir.to_str(), Some("/foo/target/debug/deps"));
    assert_eq!(h.crate_name, "foo");
    assert_eq!(
        h.dep_info.unwrap().to_str().unwrap(),
        "foo-d6ae26f5bcfb7733.d"
    );
    assert_eq!(
        h.externs,
        vec![
            PathBuf::from("/foo/target/debug/deps/liblibc-89a24418d48d484a.rlib"),
            PathBuf::from("/foo/target/debug/deps/liblog-2f7366be74992849.rlib"),
        ]
    );
}

#[test]
fn test_parse_arguments_incremental() {
    parses!(
        "--emit",
        "link",
        "foo.rs",
        "--out-dir",
        "out",
        "--crate-name",
        "foo",
        "--crate-type",
        "lib"
    );
    // Incremental builds are uncacheable: extra outputs we don't track plus
    // nondeterministic session state. Match sccache's punt.
    let r = fails!(
        "--emit",
        "link",
        "foo.rs",
        "--out-dir",
        "out",
        "--crate-name",
        "foo",
        "--crate-type",
        "lib",
        "-C",
        "incremental=/foo"
    );
    assert_eq!(r, ParseOutcome::CannotCache("incremental", None));
}

#[test]
fn test_parse_arguments_dep_info_no_extra_filename() {
    let h = parses!(
        "--crate-name",
        "foo",
        "--crate-type",
        "lib",
        "src/lib.rs",
        "--emit=dep-info,link",
        "--out-dir",
        "/out"
    );
    assert_eq!(h.dep_info, Some("foo.d".into()));
}

#[test]
fn test_parse_arguments_native_libs() {
    parses!(
        "--crate-name",
        "foo",
        "--crate-type",
        "lib,staticlib",
        "--emit",
        "link",
        "-l",
        "bar",
        "foo.rs",
        "--out-dir",
        "out"
    );
    parses!(
        "--crate-name",
        "foo",
        "--crate-type",
        "lib,staticlib",
        "--emit",
        "link",
        "-l",
        "static=bar",
        "foo.rs",
        "--out-dir",
        "out"
    );
    parses!(
        "--crate-name",
        "foo",
        "--crate-type",
        "lib,staticlib",
        "--emit",
        "link",
        "-l",
        "dylib=bar",
        "foo.rs",
        "--out-dir",
        "out"
    );
}

#[test]
fn test_parse_arguments_non_rlib_crate() {
    parses!(
        "--crate-type",
        "rlib",
        "--emit",
        "link",
        "foo.rs",
        "--out-dir",
        "out",
        "--crate-name",
        "foo"
    );
    parses!(
        "--crate-type",
        "lib",
        "--emit",
        "link",
        "foo.rs",
        "--out-dir",
        "out",
        "--crate-name",
        "foo"
    );
    parses!(
        "--crate-type",
        "staticlib",
        "--emit",
        "link",
        "foo.rs",
        "--out-dir",
        "out",
        "--crate-name",
        "foo"
    );
    parses!(
        "--crate-type",
        "rlib,staticlib",
        "--emit",
        "link",
        "foo.rs",
        "--out-dir",
        "out",
        "--crate-name",
        "foo"
    );
    fails!(
        "--crate-type",
        "bin",
        "--emit",
        "link",
        "foo.rs",
        "--out-dir",
        "out",
        "--crate-name",
        "foo"
    );
    fails!(
        "--crate-type",
        "rlib,dylib",
        "--emit",
        "link",
        "foo.rs",
        "--out-dir",
        "out",
        "--crate-name",
        "foo"
    );
}

#[test]
fn test_parse_arguments_color() {
    let h = parses!(
        "--emit",
        "link",
        "foo.rs",
        "--out-dir",
        "out",
        "--crate-name",
        "foo",
        "--crate-type",
        "lib"
    );
    assert_eq!(h.color_mode, ColorMode::Auto);
    let h = parses!(
        "--emit",
        "link",
        "foo.rs",
        "--out-dir",
        "out",
        "--crate-name",
        "foo",
        "--crate-type",
        "lib",
        "--color=always"
    );
    assert_eq!(h.color_mode, ColorMode::On);
    let h = parses!(
        "--emit",
        "link",
        "foo.rs",
        "--out-dir",
        "out",
        "--crate-name",
        "foo",
        "--crate-type",
        "lib",
        "--color=never"
    );
    assert_eq!(h.color_mode, ColorMode::Off);
    let h = parses!(
        "--emit",
        "link",
        "foo.rs",
        "--out-dir",
        "out",
        "--crate-name",
        "foo",
        "--crate-type",
        "lib",
        "--color=auto"
    );
    assert_eq!(h.color_mode, ColorMode::Auto);
}

#[test]
fn test_parse_arguments_multiple_inputs() {
    fails!(
        "huh.rs",
        "--emit",
        "link",
        "foo.rs",
        "--out-dir",
        "out",
        "--crate-name",
        "foo",
        "--crate-type",
        "lib"
    );

    // Having `rustc` as the first argument is indicative of clippy
    parses!(
        "rustc",
        "--emit",
        "link",
        "foo.rs",
        "--out-dir",
        "out",
        "--crate-name",
        "foo",
        "--crate-type",
        "lib"
    );
}

#[test]
fn test_parse_remap_path_prefix() {
    let h = parses!(
        "--crate-name",
        "foo",
        "--crate-type",
        "lib",
        "./src/lib.rs",
        "--emit=dep-info,link",
        "--out-dir",
        "/out",
        "--remap-path-prefix",
        "/home/test=~",
        "--remap-path-prefix",
        "/root=~"
    );
    assert!(h.arguments.contains(&Argument::WithValue(
        "--remap-path-prefix",
        ArgData::PassThrough(OsString::from("/home/test=~")),
        ArgDisposition::Separated
    )));
    assert!(h.arguments.contains(&Argument::WithValue(
        "--remap-path-prefix",
        ArgData::PassThrough(OsString::from("/root=~")),
        ArgDisposition::Separated
    )));
}

#[test]
fn test_parse_target() {
    // Parse a --target argument that is a string (not a path to a .json file).
    let h = parses!(
        "--crate-name",
        "foo",
        "--crate-type",
        "lib",
        "./src/lib.rs",
        "--emit=dep-info,link",
        "--out-dir",
        "/out",
        "--target",
        "string"
    );
    assert!(h.arguments.contains(&Argument::WithValue(
        "--target",
        ArgData::Target(ArgTarget::Name("string".to_owned())),
        ArgDisposition::Separated
    )));
    assert!(h.target_json.is_none());

    // Parse a --target argument that is a path.
    let h = parses!(
        "--crate-name",
        "foo",
        "--crate-type",
        "lib",
        "./src/lib.rs",
        "--emit=dep-info,link",
        "--out-dir",
        "/out",
        "--target",
        "/path/to/target.json"
    );
    assert!(h.arguments.contains(&Argument::WithValue(
        "--target",
        ArgData::Target(ArgTarget::Path(PathBuf::from("/path/to/target.json"))),
        ArgDisposition::Separated
    )));
    assert_eq!(h.target_json, Some(PathBuf::from("/path/to/target.json")));
}
