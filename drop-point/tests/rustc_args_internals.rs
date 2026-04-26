use drop_point::rustc_args::{
    ArgData, ArgDisposition, ArgInfo, ArgParseError, ArgsIter, Argument, FromArg,
};
use drop_point::{flag, take_arg};
use std::cmp::Ordering;
use std::ffi::OsString;
use std::path::PathBuf;

use ArgData::*;

macro_rules! ovec {
    ($($x:expr),* $(,)?) => {
        vec![$(OsString::from($x)),*]
    };
}

macro_rules! arg {
    ($name:ident($x:expr)) => {
        Argument::$name($x.into())
    };

    ($name:ident($x:expr, $v:ident)) => {
        Argument::$name($x.into(), $v)
    };
    ($name:ident($x:expr, $v:ident($y:expr))) => {
        Argument::$name($x.into(), $v($y.into()))
    };
    ($name:ident($x:expr, $v:ident($y:expr), Separated)) => {
        Argument::$name($x, $v($y.into()), ArgDisposition::Separated)
    };
    ($name:ident($x:expr, $v:ident($y:expr), $d:ident)) => {
        Argument::$name($x, $v($y.into()), ArgDisposition::$d(None))
    };
    ($name:ident($x:expr, $v:ident($y:expr), $d:ident($z:expr))) => {
        Argument::$name($x, $v($y.into()), ArgDisposition::$d(Some($z as u8)))
    };

    ($name:ident($x:expr, $v:ident::$w:ident)) => {
        Argument::$name($x.into(), $v::$w)
    };
    ($name:ident($x:expr, $v:ident::$w:ident($y:expr))) => {
        Argument::$name($x.into(), $v::$w($y.into()))
    };
    ($name:ident($x:expr, $v:ident::$w:ident($y:expr), Separated)) => {
        Argument::$name($x, $v::$w($y.into()), ArgDisposition::Separated)
    };
    ($name:ident($x:expr, $v:ident::$w:ident($y:expr), $d:ident)) => {
        Argument::$name($x, $v::$w($y.into()), ArgDisposition::$d(None))
    };
    ($name:ident($x:expr, $v:ident::$w:ident($y:expr), $d:ident($z:expr))) => {
        Argument::$name($x, $v::$w($y.into()), ArgDisposition::$d(Some($z as u8)))
    };
}

// Tests below tag arbitrary `-foo`-style flags with production `ArgData`
// variants like `TooHardFlag` / `NotCompilation` purely as placeholder
// payloads. The variants' production semantics don't apply here; we're only
// exercising the parser machinery (cmp, process, ArgsIter routing).

#[test]
#[allow(clippy::cognitive_complexity)]
fn test_arginfo_cmp() {
    let info = flag!("-foo", TooHardFlag);
    assert_eq!(info.cmp_arg("-foo"), Ordering::Equal);
    assert_eq!(info.cmp_arg("bar"), Ordering::Less);
    assert_eq!(info.cmp_arg("-bar"), Ordering::Greater);
    assert_eq!(info.cmp_arg("-qux"), Ordering::Less);
    assert_eq!(info.cmp_arg("-foobar"), Ordering::Less);
    assert_eq!(info.cmp_arg("-foo="), Ordering::Less);
    assert_eq!(info.cmp_arg("-foo=bar"), Ordering::Less);

    let info = take_arg!("-foo", OsString, Separated, NotCompilation);
    assert_eq!(info.cmp_arg("-foo"), Ordering::Equal);
    assert_eq!(info.cmp_arg("bar"), Ordering::Less);
    assert_eq!(info.cmp_arg("-bar"), Ordering::Greater);
    assert_eq!(info.cmp_arg("-qux"), Ordering::Less);
    assert_eq!(info.cmp_arg("-foobar"), Ordering::Less);
    assert_eq!(info.cmp_arg("-foo="), Ordering::Less);
    assert_eq!(info.cmp_arg("-foo=bar"), Ordering::Less);

    let info = take_arg!("-foo", OsString, Concatenated, NotCompilation);
    assert_eq!(info.cmp_arg("-foo"), Ordering::Equal);
    assert_eq!(info.cmp_arg("bar"), Ordering::Less);
    assert_eq!(info.cmp_arg("-bar"), Ordering::Greater);
    assert_eq!(info.cmp_arg("-qux"), Ordering::Less);
    assert_eq!(info.cmp_arg("-foobar"), Ordering::Equal);
    assert_eq!(info.cmp_arg("-foo="), Ordering::Equal);
    assert_eq!(info.cmp_arg("-foo=bar"), Ordering::Equal);

    let info = take_arg!("-foo", OsString, Concatenated(b'='), NotCompilation);
    assert_eq!(info.cmp_arg("-foo"), Ordering::Equal);
    assert_eq!(info.cmp_arg("bar"), Ordering::Less);
    assert_eq!(info.cmp_arg("-bar"), Ordering::Greater);
    assert_eq!(info.cmp_arg("-qux"), Ordering::Less);
    assert_eq!(info.cmp_arg("-foobar"), Ordering::Greater);
    assert_eq!(info.cmp_arg("-foo="), Ordering::Equal);
    assert_eq!(info.cmp_arg("-foo=bar"), Ordering::Equal);

    let info = take_arg!("-foo", OsString, CanBeConcatenated(b'='), NotCompilation);
    assert_eq!(info.cmp_arg("-foo"), Ordering::Equal);
    assert_eq!(info.cmp_arg("bar"), Ordering::Less);
    assert_eq!(info.cmp_arg("-bar"), Ordering::Greater);
    assert_eq!(info.cmp_arg("-qux"), Ordering::Less);
    assert_eq!(info.cmp_arg("-foobar"), Ordering::Greater);
    assert_eq!(info.cmp_arg("-foo="), Ordering::Equal);
    assert_eq!(info.cmp_arg("-foo=bar"), Ordering::Equal);

    let info = take_arg!("-foo", OsString, CanBeSeparated, NotCompilation);
    assert_eq!(info.cmp_arg("-foo"), Ordering::Equal);
    assert_eq!(info.cmp_arg("bar"), Ordering::Less);
    assert_eq!(info.cmp_arg("-bar"), Ordering::Greater);
    assert_eq!(info.cmp_arg("-qux"), Ordering::Less);
    assert_eq!(info.cmp_arg("-foobar"), Ordering::Equal);
    assert_eq!(info.cmp_arg("-foo="), Ordering::Equal);
    assert_eq!(info.cmp_arg("-foo=bar"), Ordering::Equal);

    let info = take_arg!("-foo", OsString, CanBeSeparated(b'='), NotCompilation);
    assert_eq!(info.cmp_arg("-foo"), Ordering::Equal);
    assert_eq!(info.cmp_arg("bar"), Ordering::Less);
    assert_eq!(info.cmp_arg("-bar"), Ordering::Greater);
    assert_eq!(info.cmp_arg("-qux"), Ordering::Less);
    assert_eq!(info.cmp_arg("-foobar"), Ordering::Greater);
    assert_eq!(info.cmp_arg("-foo="), Ordering::Equal);
    assert_eq!(info.cmp_arg("-foo=bar"), Ordering::Equal);
}

#[test]
fn test_arginfo_process() {
    let info = flag!("-foo", TooHardFlag);
    assert_eq!(
        info.process("-foo", || None).unwrap(),
        arg!(Flag("-foo", TooHardFlag))
    );

    let info = take_arg!("-foo", OsString, Separated, NotCompilation);
    assert_eq!(
        info.clone().process("-foo", || None).unwrap_err(),
        ArgParseError::UnexpectedEndOfArgs
    );
    assert_eq!(
        info.process("-foo", || Some("bar".into())).unwrap(),
        arg!(WithValue("-foo", NotCompilation("bar"), Separated))
    );

    let info = take_arg!("-foo", OsString, Concatenated, NotCompilation);
    assert_eq!(
        info.clone().process("-foo", || None).unwrap(),
        arg!(WithValue("-foo", NotCompilation(""), Concatenated))
    );
    assert_eq!(
        info.process("-foobar", || None).unwrap(),
        arg!(WithValue("-foo", NotCompilation("bar"), Concatenated))
    );

    let info = take_arg!("-foo", OsString, Concatenated(b'='), NotCompilation);
    assert_eq!(
        info.clone().process("-foo=", || None).unwrap(),
        arg!(WithValue("-foo", NotCompilation(""), Concatenated(b'=')))
    );
    assert_eq!(
        info.process("-foo=bar", || None).unwrap(),
        arg!(WithValue("-foo", NotCompilation("bar"), Concatenated(b'=')))
    );

    let info = take_arg!("-foo", OsString, CanBeSeparated, NotCompilation);
    assert_eq!(
        info.clone().process("-foo", || None).unwrap(),
        arg!(WithValue("-foo", NotCompilation(""), Concatenated))
    );
    assert_eq!(
        info.clone().process("-foobar", || None).unwrap(),
        arg!(WithValue("-foo", NotCompilation("bar"), CanBeSeparated))
    );
    assert_eq!(
        info.process("-foo", || Some("bar".into())).unwrap(),
        arg!(WithValue("-foo", NotCompilation("bar"), CanBeConcatenated))
    );

    let info = take_arg!("-foo", OsString, CanBeSeparated(b'='), NotCompilation);
    assert_eq!(
        info.clone().process("-foo", || None).unwrap_err(),
        ArgParseError::UnexpectedEndOfArgs
    );
    assert_eq!(
        info.clone().process("-foo=", || None).unwrap(),
        arg!(WithValue("-foo", NotCompilation(""), CanBeSeparated(b'=')))
    );
    assert_eq!(
        info.clone().process("-foo=bar", || None).unwrap(),
        arg!(WithValue("-foo", NotCompilation("bar"), CanBeSeparated(b'=')))
    );
    assert_eq!(
        info.process("-foo", || Some("bar".into())).unwrap(),
        arg!(WithValue("-foo", NotCompilation("bar"), CanBeConcatenated(b'=')))
    );
}

#[test]
fn test_argsiter() {
    static ARGS: [ArgInfo; 7] = [
        flag!("-bar", ArgData::TooHardFlag),
        take_arg!("-foo", OsString, Separated, ArgData::NotCompilation),
        flag!("-fuga", ArgData::NotCompilationFlag),
        take_arg!("-hoge", PathBuf, Concatenated, ArgData::TooHardPath),
        flag!("-plop", ArgData::TooHardFlag),
        take_arg!("-qux", OsString, CanBeSeparated(b'='), ArgData::PassThrough),
        flag!("-zorglub", ArgData::NotCompilationFlag),
    ];

    let args = [
        "-nomatch",
        "-foo",
        "value",
        "-hoge",
        "value",
        "-hoge=value",
        "-hogevalue",
        "-zorglub",
        "-qux",
        "value",
        "-plop",
        "-quxbar",
        "-qux=value",
    ];
    let actual: Vec<_> = ArgsIter::new(args.iter().map(OsString::from), &ARGS[..])
        .map(|r| r.unwrap())
        .collect();
    let expected = vec![
        arg!(UnknownFlag("-nomatch")),
        arg!(WithValue("-foo", ArgData::NotCompilation("value"), Separated)),
        arg!(WithValue("-hoge", ArgData::TooHardPath(""), Concatenated)),
        arg!(Raw("value")),
        arg!(WithValue("-hoge", ArgData::TooHardPath("=value"), Concatenated)),
        arg!(WithValue("-hoge", ArgData::TooHardPath("value"), Concatenated)),
        arg!(Flag("-zorglub", ArgData::NotCompilationFlag)),
        arg!(WithValue(
            "-qux",
            ArgData::PassThrough("value"),
            CanBeConcatenated(b'=')
        )),
        arg!(Flag("-plop", ArgData::TooHardFlag)),
        arg!(UnknownFlag("-quxbar")),
        arg!(WithValue(
            "-qux",
            ArgData::PassThrough("value"),
            CanBeSeparated(b'=')
        )),
    ];
    assert_eq!(actual, expected);
}

#[test]
fn test_argument_into_iter() {
    let raw: Argument = arg!(Raw("value"));
    let unknown: Argument = arg!(UnknownFlag("-foo"));
    assert_eq!(raw.iter_os_strings().collect::<Vec<_>>(), ovec!["value"]);
    assert_eq!(unknown.iter_os_strings().collect::<Vec<_>>(), ovec!["-foo"]);
    assert_eq!(
        arg!(Flag("-foo", TooHardFlag))
            .iter_os_strings()
            .collect::<Vec<_>>(),
        ovec!["-foo"]
    );

    let arg = arg!(WithValue("-foo", NotCompilation("bar"), Concatenated));
    assert_eq!(arg.iter_os_strings().collect::<Vec<_>>(), ovec!["-foobar"]);

    let arg = arg!(WithValue("-foo", NotCompilation("bar"), Concatenated(b'=')));
    assert_eq!(arg.iter_os_strings().collect::<Vec<_>>(), ovec!["-foo=bar"]);

    let arg = arg!(WithValue("-foo", NotCompilation("bar"), CanBeSeparated));
    assert_eq!(arg.iter_os_strings().collect::<Vec<_>>(), ovec!["-foobar"]);

    let arg = arg!(WithValue("-foo", NotCompilation("bar"), CanBeSeparated(b'=')));
    assert_eq!(arg.iter_os_strings().collect::<Vec<_>>(), ovec!["-foo=bar"]);

    let arg = arg!(WithValue("-foo", NotCompilation("bar"), CanBeConcatenated));
    assert_eq!(
        arg.iter_os_strings().collect::<Vec<_>>(),
        ovec!["-foo", "bar"]
    );

    let arg = arg!(WithValue("-foo", NotCompilation("bar"), CanBeConcatenated(b'=')));
    assert_eq!(
        arg.iter_os_strings().collect::<Vec<_>>(),
        ovec!["-foo", "bar"]
    );

    let arg = arg!(WithValue("-foo", NotCompilation("bar"), Separated));
    assert_eq!(
        arg.iter_os_strings().collect::<Vec<_>>(),
        ovec!["-foo", "bar"]
    );
}

#[test]
fn test_arginfo_process_take_concat_arg_delim_doesnt_crash() {
    let _ = take_arg!("-foo", OsString, Concatenated(b'='), NotCompilation)
        .process("-foo", || None);
}

#[cfg(debug_assertions)]
mod assert_tests {
    use super::*;

    #[test]
    #[should_panic]
    fn test_arginfo_process_flag() {
        flag!("-foo", TooHardFlag).process("-bar", || None).unwrap();
    }

    #[test]
    #[should_panic]
    fn test_arginfo_process_take_arg() {
        take_arg!("-foo", OsString, Separated, NotCompilation)
            .process("-bar", || None)
            .unwrap();
    }

    #[test]
    #[should_panic]
    fn test_arginfo_process_take_concat_arg() {
        take_arg!("-foo", OsString, Concatenated, NotCompilation)
            .process("-bar", || None)
            .unwrap();
    }

    #[test]
    #[should_panic]
    fn test_arginfo_process_take_concat_arg_delim() {
        take_arg!("-foo", OsString, Concatenated(b'='), NotCompilation)
            .process("-bar", || None)
            .unwrap();
    }

    #[test]
    #[should_panic]
    fn test_arginfo_process_take_maybe_concat_arg() {
        take_arg!("-foo", OsString, CanBeSeparated, NotCompilation)
            .process("-bar", || None)
            .unwrap();
    }

    #[test]
    #[should_panic]
    fn test_arginfo_process_take_maybe_concat_arg_delim() {
        take_arg!("-foo", OsString, CanBeSeparated(b'='), NotCompilation)
            .process("-bar", || None)
            .unwrap();
    }

    #[test]
    #[should_panic]
    fn test_args_iter_unsorted() {
        static ARGS: [ArgInfo; 2] = [flag!("-foo", TooHardFlag), flag!("-bar", TooHardFlag)];
        ArgsIter::new(Vec::<OsString>::new().into_iter(), &ARGS[..]);
    }

    #[test]
    #[should_panic]
    fn test_args_iter_unsorted_2() {
        static ARGS: [ArgInfo; 2] = [flag!("-foo", TooHardFlag), flag!("-foo", TooHardFlag)];
        ArgsIter::new(Vec::<OsString>::new().into_iter(), &ARGS[..]);
    }

    #[test]
    fn test_args_iter_no_conflict() {
        static ARGS: [ArgInfo; 2] = [flag!("-foo", TooHardFlag), flag!("-fooz", TooHardFlag)];
        ArgsIter::new(Vec::<OsString>::new().into_iter(), &ARGS[..]);
    }
}
