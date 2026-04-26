use drop_point::rustc_args::{
    ArgDisposition, ArgInfo, ArgParseError, ArgsIter, Argument, FromArg, bsearch,
};
use drop_point::{ArgData, flag, take_arg};
use std::cmp::Ordering;
use std::ffi::OsString;
use std::path::PathBuf;

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

ArgData! {
    FooFlag,
    Foo(OsString),
}

use self::ArgData::*;

#[test]
#[allow(clippy::cognitive_complexity)]
fn test_arginfo_cmp() {
    let info = flag!("-foo", FooFlag);
    assert_eq!(info.cmp_arg("-foo"), Ordering::Equal);
    assert_eq!(info.cmp_arg("bar"), Ordering::Less);
    assert_eq!(info.cmp_arg("-bar"), Ordering::Greater);
    assert_eq!(info.cmp_arg("-qux"), Ordering::Less);
    assert_eq!(info.cmp_arg("-foobar"), Ordering::Less);
    assert_eq!(info.cmp_arg("-foo="), Ordering::Less);
    assert_eq!(info.cmp_arg("-foo=bar"), Ordering::Less);

    let info = take_arg!("-foo", OsString, Separated, Foo);
    assert_eq!(info.cmp_arg("-foo"), Ordering::Equal);
    assert_eq!(info.cmp_arg("bar"), Ordering::Less);
    assert_eq!(info.cmp_arg("-bar"), Ordering::Greater);
    assert_eq!(info.cmp_arg("-qux"), Ordering::Less);
    assert_eq!(info.cmp_arg("-foobar"), Ordering::Less);
    assert_eq!(info.cmp_arg("-foo="), Ordering::Less);
    assert_eq!(info.cmp_arg("-foo=bar"), Ordering::Less);

    let info = take_arg!("-foo", OsString, Concatenated, Foo);
    assert_eq!(info.cmp_arg("-foo"), Ordering::Equal);
    assert_eq!(info.cmp_arg("bar"), Ordering::Less);
    assert_eq!(info.cmp_arg("-bar"), Ordering::Greater);
    assert_eq!(info.cmp_arg("-qux"), Ordering::Less);
    assert_eq!(info.cmp_arg("-foobar"), Ordering::Equal);
    assert_eq!(info.cmp_arg("-foo="), Ordering::Equal);
    assert_eq!(info.cmp_arg("-foo=bar"), Ordering::Equal);

    let info = take_arg!("-foo", OsString, Concatenated(b'='), Foo);
    assert_eq!(info.cmp_arg("-foo"), Ordering::Equal);
    assert_eq!(info.cmp_arg("bar"), Ordering::Less);
    assert_eq!(info.cmp_arg("-bar"), Ordering::Greater);
    assert_eq!(info.cmp_arg("-qux"), Ordering::Less);
    assert_eq!(info.cmp_arg("-foobar"), Ordering::Greater);
    assert_eq!(info.cmp_arg("-foo="), Ordering::Equal);
    assert_eq!(info.cmp_arg("-foo=bar"), Ordering::Equal);

    let info = take_arg!("-foo", OsString, CanBeConcatenated(b'='), Foo);
    assert_eq!(info.cmp_arg("-foo"), Ordering::Equal);
    assert_eq!(info.cmp_arg("bar"), Ordering::Less);
    assert_eq!(info.cmp_arg("-bar"), Ordering::Greater);
    assert_eq!(info.cmp_arg("-qux"), Ordering::Less);
    assert_eq!(info.cmp_arg("-foobar"), Ordering::Greater);
    assert_eq!(info.cmp_arg("-foo="), Ordering::Equal);
    assert_eq!(info.cmp_arg("-foo=bar"), Ordering::Equal);

    let info = take_arg!("-foo", OsString, CanBeSeparated, Foo);
    assert_eq!(info.cmp_arg("-foo"), Ordering::Equal);
    assert_eq!(info.cmp_arg("bar"), Ordering::Less);
    assert_eq!(info.cmp_arg("-bar"), Ordering::Greater);
    assert_eq!(info.cmp_arg("-qux"), Ordering::Less);
    assert_eq!(info.cmp_arg("-foobar"), Ordering::Equal);
    assert_eq!(info.cmp_arg("-foo="), Ordering::Equal);
    assert_eq!(info.cmp_arg("-foo=bar"), Ordering::Equal);

    let info = take_arg!("-foo", OsString, CanBeSeparated(b'='), Foo);
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
    let info = flag!("-foo", FooFlag);
    assert_eq!(
        info.process("-foo", || None).unwrap(),
        arg!(Flag("-foo", FooFlag))
    );

    let info = take_arg!("-foo", OsString, Separated, Foo);
    assert_eq!(
        info.clone().process("-foo", || None).unwrap_err(),
        ArgParseError::UnexpectedEndOfArgs
    );
    assert_eq!(
        info.process("-foo", || Some("bar".into())).unwrap(),
        arg!(WithValue("-foo", Foo("bar"), Separated))
    );

    let info = take_arg!("-foo", OsString, Concatenated, Foo);
    assert_eq!(
        info.clone().process("-foo", || None).unwrap(),
        arg!(WithValue("-foo", Foo(""), Concatenated))
    );
    assert_eq!(
        info.process("-foobar", || None).unwrap(),
        arg!(WithValue("-foo", Foo("bar"), Concatenated))
    );

    let info = take_arg!("-foo", OsString, Concatenated(b'='), Foo);
    assert_eq!(
        info.clone().process("-foo=", || None).unwrap(),
        arg!(WithValue("-foo", Foo(""), Concatenated(b'=')))
    );
    assert_eq!(
        info.process("-foo=bar", || None).unwrap(),
        arg!(WithValue("-foo", Foo("bar"), Concatenated(b'=')))
    );

    let info = take_arg!("-foo", OsString, CanBeSeparated, Foo);
    assert_eq!(
        info.clone().process("-foo", || None).unwrap(),
        arg!(WithValue("-foo", Foo(""), Concatenated))
    );
    assert_eq!(
        info.clone().process("-foobar", || None).unwrap(),
        arg!(WithValue("-foo", Foo("bar"), CanBeSeparated))
    );
    assert_eq!(
        info.process("-foo", || Some("bar".into())).unwrap(),
        arg!(WithValue("-foo", Foo("bar"), CanBeConcatenated))
    );

    let info = take_arg!("-foo", OsString, CanBeSeparated(b'='), Foo);
    assert_eq!(
        info.clone().process("-foo", || None).unwrap_err(),
        ArgParseError::UnexpectedEndOfArgs
    );
    assert_eq!(
        info.clone().process("-foo=", || None).unwrap(),
        arg!(WithValue("-foo", Foo(""), CanBeSeparated(b'=')))
    );
    assert_eq!(
        info.clone().process("-foo=bar", || None).unwrap(),
        arg!(WithValue("-foo", Foo("bar"), CanBeSeparated(b'=')))
    );
    assert_eq!(
        info.process("-foo", || Some("bar".into())).unwrap(),
        arg!(WithValue("-foo", Foo("bar"), CanBeConcatenated(b'=')))
    );
}

#[test]
fn test_bsearch() {
    let data = vec![
        ("bar", 1),
        ("foo", 2),
        ("fuga", 3),
        ("hoge", 4),
        ("plop", 5),
        ("qux", 6),
        ("zorglub", 7),
    ];
    for item in &data {
        assert_eq!(bsearch(item.0, &data, |i, k| i.0.cmp(k)), Some(item));
    }

    let data = &data[..6];
    for item in data {
        assert_eq!(bsearch(item.0, data, |i, k| i.0.cmp(k)), Some(item));
    }

    let data = vec![
        ("a", 1),
        ("ab", 2),
        ("abc", 3),
        ("abd", 4),
        ("abe", 5),
        ("abef", 6),
        ("abefg", 7),
    ];
    for item in &data {
        assert_eq!(
            bsearch(item.0, &data, |i, k| if k.starts_with(i.0) {
                Ordering::Equal
            } else {
                i.0.cmp(k)
            }),
            Some(item)
        );
    }

    let data = &data[..6];
    for item in data {
        assert_eq!(
            bsearch(item.0, data, |i, k| if k.starts_with(i.0) {
                Ordering::Equal
            } else {
                i.0.cmp(k)
            }),
            Some(item)
        );
    }
}

#[test]
fn test_argsiter() {
    ArgData! {
        Bar,
        Foo(OsString),
        Fuga,
        Hoge(PathBuf),
        Plop,
        Qux(OsString),
        Zorglub,
    }

    // Need to explicitly refer to enum because `use` doesn't work if it's in a module
    // https://internals.rust-lang.org/t/pre-rfc-support-use-enum-for-function-local-enums/3853/13
    static ARGS: [ArgInfo<ArgData>; 7] = [
        flag!("-bar", ArgData::Bar),
        take_arg!("-foo", OsString, Separated, ArgData::Foo),
        flag!("-fuga", ArgData::Fuga),
        take_arg!("-hoge", PathBuf, Concatenated, ArgData::Hoge),
        flag!("-plop", ArgData::Plop),
        take_arg!("-qux", OsString, CanBeSeparated(b'='), ArgData::Qux),
        flag!("-zorglub", ArgData::Zorglub),
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
        arg!(WithValue("-foo", ArgData::Foo("value"), Separated)),
        arg!(WithValue("-hoge", ArgData::Hoge(""), Concatenated)),
        arg!(Raw("value")),
        arg!(WithValue("-hoge", ArgData::Hoge("=value"), Concatenated)),
        arg!(WithValue("-hoge", ArgData::Hoge("value"), Concatenated)),
        arg!(Flag("-zorglub", ArgData::Zorglub)),
        arg!(WithValue(
            "-qux",
            ArgData::Qux("value"),
            CanBeConcatenated(b'=')
        )),
        arg!(Flag("-plop", ArgData::Plop)),
        arg!(UnknownFlag("-quxbar")),
        arg!(WithValue(
            "-qux",
            ArgData::Qux("value"),
            CanBeSeparated(b'=')
        )),
    ];
    assert_eq!(actual, expected);
}

#[test]
fn test_argument_into_iter() {
    let raw: Argument<ArgData> = arg!(Raw("value"));
    let unknown: Argument<ArgData> = arg!(UnknownFlag("-foo"));
    assert_eq!(raw.iter_os_strings().collect::<Vec<_>>(), ovec!["value"]);
    assert_eq!(unknown.iter_os_strings().collect::<Vec<_>>(), ovec!["-foo"]);
    assert_eq!(
        arg!(Flag("-foo", FooFlag))
            .iter_os_strings()
            .collect::<Vec<_>>(),
        ovec!["-foo"]
    );

    let arg = arg!(WithValue("-foo", Foo("bar"), Concatenated));
    assert_eq!(arg.iter_os_strings().collect::<Vec<_>>(), ovec!["-foobar"]);

    let arg = arg!(WithValue("-foo", Foo("bar"), Concatenated(b'=')));
    assert_eq!(arg.iter_os_strings().collect::<Vec<_>>(), ovec!["-foo=bar"]);

    let arg = arg!(WithValue("-foo", Foo("bar"), CanBeSeparated));
    assert_eq!(arg.iter_os_strings().collect::<Vec<_>>(), ovec!["-foobar"]);

    let arg = arg!(WithValue("-foo", Foo("bar"), CanBeSeparated(b'=')));
    assert_eq!(arg.iter_os_strings().collect::<Vec<_>>(), ovec!["-foo=bar"]);

    let arg = arg!(WithValue("-foo", Foo("bar"), CanBeConcatenated));
    assert_eq!(
        arg.iter_os_strings().collect::<Vec<_>>(),
        ovec!["-foo", "bar"]
    );

    let arg = arg!(WithValue("-foo", Foo("bar"), CanBeConcatenated(b'=')));
    assert_eq!(
        arg.iter_os_strings().collect::<Vec<_>>(),
        ovec!["-foo", "bar"]
    );

    let arg = arg!(WithValue("-foo", Foo("bar"), Separated));
    assert_eq!(
        arg.iter_os_strings().collect::<Vec<_>>(),
        ovec!["-foo", "bar"]
    );
}

#[test]
fn test_arginfo_process_take_concat_arg_delim_doesnt_crash() {
    let _ = take_arg!("-foo", OsString, Concatenated(b'='), Foo).process("-foo", || None);
}

#[cfg(debug_assertions)]
mod assert_tests {
    use super::*;

    #[test]
    #[should_panic]
    fn test_arginfo_process_flag() {
        flag!("-foo", FooFlag).process("-bar", || None).unwrap();
    }

    #[test]
    #[should_panic]
    fn test_arginfo_process_take_arg() {
        take_arg!("-foo", OsString, Separated, Foo)
            .process("-bar", || None)
            .unwrap();
    }

    #[test]
    #[should_panic]
    fn test_arginfo_process_take_concat_arg() {
        take_arg!("-foo", OsString, Concatenated, Foo)
            .process("-bar", || None)
            .unwrap();
    }

    #[test]
    #[should_panic]
    fn test_arginfo_process_take_concat_arg_delim() {
        take_arg!("-foo", OsString, Concatenated(b'='), Foo)
            .process("-bar", || None)
            .unwrap();
    }

    #[test]
    #[should_panic]
    fn test_arginfo_process_take_maybe_concat_arg() {
        take_arg!("-foo", OsString, CanBeSeparated, Foo)
            .process("-bar", || None)
            .unwrap();
    }

    #[test]
    #[should_panic]
    fn test_arginfo_process_take_maybe_concat_arg_delim() {
        take_arg!("-foo", OsString, CanBeSeparated(b'='), Foo)
            .process("-bar", || None)
            .unwrap();
    }

    #[test]
    #[should_panic]
    fn test_args_iter_unsorted() {
        static ARGS: [ArgInfo<ArgData>; 2] = [flag!("-foo", FooFlag), flag!("-bar", FooFlag)];
        ArgsIter::new(Vec::<OsString>::new().into_iter(), &ARGS[..]);
    }

    #[test]
    #[should_panic]
    fn test_args_iter_unsorted_2() {
        static ARGS: [ArgInfo<ArgData>; 2] = [flag!("-foo", FooFlag), flag!("-foo", FooFlag)];
        ArgsIter::new(Vec::<OsString>::new().into_iter(), &ARGS[..]);
    }

    #[test]
    fn test_args_iter_no_conflict() {
        static ARGS: [ArgInfo<ArgData>; 2] = [flag!("-foo", FooFlag), flag!("-fooz", FooFlag)];
        ArgsIter::new(Vec::<OsString>::new().into_iter(), &ARGS[..]);
    }
}
