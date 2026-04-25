// This is the rustc arg parsing library for drop point.
// its basically sccache/src/compiler/args.rs and parts of sccache/src/compiler/rust.rs
// with all the generic machinery removed, also lightly updated for 2026 rust.

use std::cmp::Ordering;
use std::ffi::OsString;
use std::fmt::Debug;
use std::marker::PhantomData;
use std::path::PathBuf;
use std::str;
use thiserror::Error;

pub type ArgParseResult<T> = Result<T, ArgParseError>;

#[derive(Debug, PartialEq, Eq, Error)]
pub enum ArgParseError {
    #[error("Unexpected end of args")]
    UnexpectedEndOfArgs,
    #[error("String {0:?} contained invalid unicode")]
    InvalidUnicode(OsString),
    #[error("Arg-specific parsing failed: {0}")]
    Other(&'static str),
}

/// The byte that joins a flag to its value when they are written as a single
/// argv element.
///
/// It's a `u8` rather than `char` because argv parsing happens byte-wise on
/// `OsString` (which on Unix is just bytes with no encoding guarantee), and
/// CLI delimiters are always ASCII anyway.
///
/// # Examples
///
/// ```
/// // `Some(b'=')` produces `--crate-name=foo`:
/// let d: Option<u8> = Some(b'=');
///
/// // `None` produces no separator at all (e.g. `-Lpath`, `-Iinclude`):
/// let d: Option<u8> = None;
/// ```
pub type Delimiter = Option<u8>;

/// How a value is passed to an argument with a value.
#[derive(PartialEq, Eq, Clone, Debug)]
pub enum ArgDisposition {
    /// As "-arg value"
    Separated,
    /// As "-arg value", but "-arg<delimiter>value" would be valid too
    CanBeConcatenated(Delimiter),
    /// As "-arg<delimiter>value", but "-arg value" would be valid too
    CanBeSeparated(Delimiter),
    /// As "-arg<delimiter>value"
    Concatenated(Delimiter),
}

/// Representation of a parsed argument
/// The type parameter T contains the parsed information for this argument,
/// for use during argument handling (typically an enum to allow switching
/// on the different kinds of argument). `Flag`s may contain a simple
/// variant which influences how to do caching, whereas `WithValue`s could
/// be a struct variant with parsed data from the value.
#[derive(PartialEq, Eq, Clone, Debug)]
pub enum Argument<T> {
    /// Unknown non-flag argument; e.g. "foo"
    Raw(OsString),
    /// Unknown flag argument; e.g. "-foo"
    UnknownFlag(OsString),
    /// Known flag argument; e.g. "-bar"
    Flag(&'static str, T),
    /// Known argument with a value; e.g. "-qux bar", where the way the
    /// value is passed is described by the ArgDisposition type.
    WithValue(&'static str, T, ArgDisposition),
}

/// Target form for collapsing an `Argument`'s `ArgDisposition` to a canonical
/// shape. Used to make `--out-dir=foo` and `--out-dir foo` produce the same
/// bytes when re-serialized for the cache key, regardless of how the user
/// originally wrote them. Only two variants because there are only two ways
/// to write a flag and its value: glued together or as two separate argv
/// elements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NormalizedDisposition {
    Separated,
    Concatenated,
}

impl<T: ArgumentValue> Argument<T> {
    /// For arguments that allow both a concatenated or separated disposition,
    /// normalize a parsed argument to a preferred disposition.
    pub fn normalize(self, disposition: NormalizedDisposition) -> Self {
        match self {
            Argument::WithValue(s, v, ArgDisposition::CanBeConcatenated(d))
            | Argument::WithValue(s, v, ArgDisposition::CanBeSeparated(d)) => Argument::WithValue(
                s,
                v,
                match disposition {
                    NormalizedDisposition::Separated => ArgDisposition::Separated,
                    NormalizedDisposition::Concatenated => ArgDisposition::Concatenated(d),
                },
            ),
            a => a,
        }
    }

    pub fn to_os_string(&self) -> OsString {
        match *self {
            Argument::Raw(ref s) | Argument::UnknownFlag(ref s) => s.clone(),
            Argument::Flag(ref s, _) | Argument::WithValue(ref s, _, _) => s.into(),
        }
    }

    pub fn flag_str(&self) -> Option<&'static str> {
        match *self {
            Argument::Flag(s, _) | Argument::WithValue(s, _, _) => Some(s),
            _ => None,
        }
    }

    pub fn get_data(&self) -> Option<&T> {
        match *self {
            Argument::Flag(_, ref d) => Some(d),
            Argument::WithValue(_, ref d, _) => Some(d),
            _ => None,
        }
    }

    /// Transforms a parsed argument into an iterator.
    pub fn iter_os_strings(&self) -> Iter<'_, T> {
        Iter {
            arg: self,
            emitted: 0,
        }
    }
}

pub struct Iter<'a, T> {
    arg: &'a Argument<T>,
    emitted: usize,
}

impl<T: ArgumentValue> Iterator for Iter<'_, T> {
    type Item = OsString;

    fn next(&mut self) -> Option<Self::Item> {
        let result = match *self.arg {
            Argument::Raw(ref s) | Argument::UnknownFlag(ref s) => emit_raw(s, self.emitted),
            Argument::Flag(s, _) => emit_flag(s, self.emitted),
            Argument::WithValue(s, ref v, ref d) => emit_with_value(s, v, d, self.emitted),
        };
        if result.is_some() {
            self.emitted += 1;
        }
        result
    }
}

fn emit_raw(s: &OsString, emitted: usize) -> Option<OsString> {
    match emitted {
        0 => Some(s.clone()),
        _ => None,
    }
}

fn emit_flag(s: &'static str, emitted: usize) -> Option<OsString> {
    match emitted {
        0 => Some(s.into()),
        _ => None,
    }
}

fn emit_with_value<T: ArgumentValue>(
    flag: &'static str,
    value: &T,
    disposition: &ArgDisposition,
    emitted: usize,
) -> Option<OsString> {
    match (emitted, disposition) {
        (0, &ArgDisposition::CanBeSeparated(d)) | (0, &ArgDisposition::Concatenated(d)) => {
            Some(emit_concatenated(flag, value, d))
        }
        (0, &ArgDisposition::Separated) | (0, &ArgDisposition::CanBeConcatenated(_)) => {
            Some(flag.into())
        }
        (1, &ArgDisposition::Separated) | (1, &ArgDisposition::CanBeConcatenated(_)) => {
            Some(value.clone().into_arg_os_string())
        }
        _ => None,
    }
}

fn emit_concatenated<T: ArgumentValue>(flag: &str, value: &T, delim: Delimiter) -> OsString {
    let mut s = OsString::from(flag);
    let v = value.clone().into_arg_os_string();
    if let Some(d) = delim
        && !v.is_empty()
    {
        s.push(str::from_utf8(&[d]).expect("delimiter must be ASCII; see ARGS table"));
    }
    s.push(v);
    s
}

/// Generates the `ArgData` enum and an `IntoArg` impl that delegates each
/// variant to its inner value, so adding a new variant is one line in the
/// macro call instead of one new match arm per IntoArg method.
macro_rules! ArgData {
    // Collected all the arms, time to create the match
    { __matchify $var:ident $fn:ident ($( $fnarg:ident )*) ($( $arms:tt )*) } => {
        match $var {
            $( $arms )*
        }
    };
    // Unit variant
    { __matchify $var:ident $fn:ident ($( $fnarg:ident )*) ($( $arms:tt )*) $x:ident, $( $rest:tt )* } => {
        ArgData!{
            __matchify $var $fn ($($fnarg)*)
            ($($arms)* ArgData::$x => ().$fn($( $fnarg )*),)
            $($rest)*
        }
    };
    // Tuple variant
    { __matchify $var:ident $fn:ident ($( $fnarg:ident )*) ($( $arms:tt )*) $x:ident($y:ty), $( $rest:tt )* } => {
        ArgData!{
            __matchify $var $fn ($($fnarg)*)
            ($($arms)* ArgData::$x(inner) => inner.$fn($( $fnarg )*),)
            $($rest)*
        }
    };

    { __impl $( $tok:tt )+ } => {
        impl IntoArg for ArgData {
            fn into_arg_os_string(self) -> OsString {
                ArgData!{ __matchify self into_arg_os_string () () $($tok)+ }
            }
        }
    };

    // PartialEq necessary for tests
    { pub $( $tok:tt )+ } => {
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub enum ArgData {
            $($tok)+
        }
        ArgData!{ __impl $( $tok )+ }
    };
    { $( $tok:tt )+ } => {
        #[derive(Clone, Debug, PartialEq)]
        #[allow(clippy::enum_variant_names)]
        enum ArgData {
            $($tok)+
        }
        ArgData!{ __impl $( $tok )+ }
    };
}

/// The value associated with a parsed argument.
pub trait ArgumentValue: IntoArg + Clone + Debug {}

impl<T: IntoArg + Clone + Debug> ArgumentValue for T {}

pub trait FromArg: Sized {
    fn process(arg: OsString) -> ArgParseResult<Self>;
}

pub trait IntoArg: Sized {
    fn into_arg_os_string(self) -> OsString;
}

impl FromArg for OsString {
    fn process(arg: OsString) -> ArgParseResult<Self> {
        Ok(arg)
    }
}
impl FromArg for PathBuf {
    fn process(arg: OsString) -> ArgParseResult<Self> {
        Ok(arg.into())
    }
}
impl FromArg for String {
    fn process(arg: OsString) -> ArgParseResult<Self> {
        arg.into_string().map_err(ArgParseError::InvalidUnicode)
    }
}

impl IntoArg for OsString {
    fn into_arg_os_string(self) -> OsString {
        self
    }
}
impl IntoArg for PathBuf {
    fn into_arg_os_string(self) -> OsString {
        self.into()
    }
}
impl IntoArg for String {
    fn into_arg_os_string(self) -> OsString {
        self.into()
    }
}
impl IntoArg for () {
    fn into_arg_os_string(self) -> OsString {
        OsString::new()
    }
}

pub fn split_os_string_arg(val: OsString, split: &str) -> ArgParseResult<(String, Option<String>)> {
    let val = val.into_string().map_err(ArgParseError::InvalidUnicode)?;
    let mut split_it = val.splitn(2, split);
    let s1 = split_it.next().expect("splitn with no values");
    let maybe_s2 = split_it.next();
    Ok((s1.to_owned(), maybe_s2.map(|s| s.to_owned())))
}

/// The description of how an argument may be parsed
#[derive(PartialEq, Eq, Clone, Debug)]
#[allow(unpredictable_function_pointer_comparisons)]
pub enum ArgInfo<T> {
    /// An simple flag argument, of the form "-foo"
    Flag(&'static str, T),
    /// An argument with a value; e.g. "-qux bar", where the way the
    /// value is passed is described by the ArgDisposition type.
    TakeArg(
        &'static str,
        fn(OsString) -> ArgParseResult<T>,
        ArgDisposition,
    ),
}

impl<T: ArgumentValue> ArgInfo<T> {
    /// Transform an argument description into a parsed Argument, given a
    /// string. For arguments with a value, where the value is separate, the
    /// `get_next_arg` function returns the next argument, in raw `OsString`
    /// form.
    fn process(
        self,
        arg: &str,
        get_next_arg: impl FnOnce() -> Option<OsString>,
    ) -> ArgParseResult<Argument<T>> {
        match self {
            ArgInfo::Flag(s, variant) => process_flag(s, arg, variant),
            ArgInfo::TakeArg(s, create, disposition) => {
                process_take_arg(s, arg, create, disposition, get_next_arg)
            }
        }
    }

    /// Returns whether the given string matches the argument description, and
    /// if not, how it differs.
    fn cmp(&self, arg: &str) -> Ordering {
        let s = self.flag_str();
        match self {
            ArgInfo::TakeArg(
                _,
                _,
                ArgDisposition::CanBeSeparated(d)
                | ArgDisposition::Concatenated(d)
                | ArgDisposition::CanBeConcatenated(d),
            ) if arg.starts_with(s) => match d {
                None => Ordering::Equal,
                Some(d) if arg.len() > s.len() => arg.as_bytes()[s.len()].cmp(d),
                _ => s.cmp(&arg),
            },
            _ => s.cmp(&arg),
        }
    }

    fn flag_str(&self) -> &'static str {
        match self {
            &ArgInfo::Flag(s, _) | &ArgInfo::TakeArg(s, _, _) => s,
        }
    }
}

fn process_take_arg<T: ArgumentValue>(
    s: &'static str,
    arg: &str,
    create: fn(OsString) -> ArgParseResult<T>,
    disposition: ArgDisposition,
    get_next_arg: impl FnOnce() -> Option<OsString>,
) -> ArgParseResult<Argument<T>> {
    match disposition {
        ArgDisposition::Separated => process_separated(s, arg, create, get_next_arg),
        ArgDisposition::Concatenated(d) => process_concatenated(s, arg, create, d),
        ArgDisposition::CanBeSeparated(d) | ArgDisposition::CanBeConcatenated(d) => {
            process_either(s, arg, create, d, get_next_arg)
        }
    }
}

fn process_flag<T>(s: &'static str, arg: &str, variant: T) -> ArgParseResult<Argument<T>> {
    debug_assert_eq!(s, arg);
    Ok(Argument::Flag(s, variant))
}

fn process_separated<T>(
    s: &'static str,
    arg: &str,
    create: fn(OsString) -> ArgParseResult<T>,
    get_next_arg: impl FnOnce() -> Option<OsString>,
) -> ArgParseResult<Argument<T>> {
    debug_assert_eq!(s, arg);
    let next = get_next_arg().ok_or(ArgParseError::UnexpectedEndOfArgs)?;
    Ok(Argument::WithValue(
        s,
        create(next)?,
        ArgDisposition::Separated,
    ))
}

fn process_concatenated<T>(
    s: &'static str,
    arg: &str,
    create: fn(OsString) -> ArgParseResult<T>,
    d: Delimiter,
) -> ArgParseResult<Argument<T>> {
    let mut len = s.len();
    debug_assert_eq!(&arg[..len], s);
    if let Some(d) = d
        && arg.as_bytes().get(len) == Some(&d)
    {
        len += 1;
    }
    Ok(Argument::WithValue(
        s,
        create(arg[len..].into())?,
        ArgDisposition::Concatenated(d),
    ))
}

fn process_either<T: ArgumentValue>(
    s: &'static str,
    arg: &str,
    create: fn(OsString) -> ArgParseResult<T>,
    d: Delimiter,
    get_next_arg: impl FnOnce() -> Option<OsString>,
) -> ArgParseResult<Argument<T>> {
    let derived = if arg == s {
        ArgInfo::TakeArg(s, create, ArgDisposition::Separated)
    } else {
        ArgInfo::TakeArg(s, create, ArgDisposition::Concatenated(d))
    };
    match derived.process(arg, get_next_arg) {
        Err(ArgParseError::UnexpectedEndOfArgs) if d.is_none() => Ok(Argument::WithValue(
            s,
            create("".into())?,
            ArgDisposition::Concatenated(d),
        )),
        Ok(Argument::WithValue(s, v, ArgDisposition::Concatenated(d))) => {
            Ok(Argument::WithValue(s, v, ArgDisposition::CanBeSeparated(d)))
        }
        Ok(Argument::WithValue(s, v, ArgDisposition::Separated)) => Ok(Argument::WithValue(
            s,
            v,
            ArgDisposition::CanBeConcatenated(d),
        )),
        a => a,
    }
}

// todo revisit the design above. I think some of these should be impls on something

/// Binary search for a `key` in a sorted array of items, given a comparison
/// function. Tweaked to handle prefix matching, where multiple items in the
/// array might match but the last match is the one actually matching.
fn bsearch<K, T, F>(key: K, items: &[T], cmp: F) -> Option<&T>
where
    F: Fn(&T, &K) -> Ordering,
{
    let mut slice = items;
    while !slice.is_empty() {
        let middle = slice.len() / 2;
        match cmp(&slice[middle], &key) {
            Ordering::Equal => {
                return bsearch(key, &slice[middle + 1..], cmp).or(Some(&slice[middle]));
            }
            Ordering::Greater => slice = &slice[..middle],
            Ordering::Less => slice = &slice[middle + 1..],
        }
    }
    None
}

/// Trait for generically searching over a "set" of `ArgInfo`s.
pub trait SearchableArgInfo<T> {
    fn search(&self, key: &str) -> Option<&ArgInfo<T>>;

    #[cfg(debug_assertions)]
    fn check(&self) -> bool;
}

/// Search over a sorted array of `ArgInfo` items.
impl<T: ArgumentValue> SearchableArgInfo<T> for &'static [ArgInfo<T>] {
    fn search(&self, key: &str) -> Option<&ArgInfo<T>> {
        bsearch(key, self, |i, k| i.cmp(k))
    }

    #[cfg(debug_assertions)]
    fn check(&self) -> bool {
        self.windows(2).all(|w| {
            let a = w[0].flag_str();
            let b = w[1].flag_str();
            assert!(a < b, "{} can't precede {}", a, b);
            true
        })
    }
}

/// An `Iterator` for parsed arguments.
pub struct ArgsIter<I, T, S>
where
    I: Iterator<Item = OsString>,
    S: SearchableArgInfo<T>,
{
    arguments: I,
    arg_info: S,
    seen_double_dashes: Option<bool>,
    phantom: PhantomData<T>,
}

impl<I, T, S> ArgsIter<I, T, S>
where
    I: Iterator<Item = OsString>,
    T: ArgumentValue,
    S: SearchableArgInfo<T>,
{
    /// Create an `Iterator` for parsed arguments, given an iterator of raw
    /// `OsString` arguments, and argument descriptions.
    pub fn new(arguments: I, arg_info: S) -> Self {
        #[cfg(debug_assertions)]
        debug_assert!(arg_info.check());
        ArgsIter {
            arguments,
            arg_info,
            seen_double_dashes: None,
            phantom: PhantomData,
        }
    }

    pub fn with_double_dashes(mut self) -> Self {
        self.seen_double_dashes = Some(false);
        self
    }

    fn in_post_double_dash_mode(&mut self, arg: &OsString) -> bool {
        let Some(seen) = self.seen_double_dashes.as_mut() else {
            return false;
        };
        if !*seen && arg == "--" {
            *seen = true;
        }
        *seen
    }

    fn classify_arg(&mut self, arg: OsString) -> ArgParseResult<Argument<T>> {
        let s = arg.to_string_lossy();
        let arguments = &mut self.arguments;
        match self.arg_info.search(&s) {
            Some(i) => i.clone().process(&s, || arguments.next()),
            None if s.starts_with('-') => Ok(Argument::UnknownFlag(arg.clone())),
            None => Ok(Argument::Raw(arg.clone())),
        }
    }
}

impl<I, T, S> Iterator for ArgsIter<I, T, S>
where
    I: Iterator<Item = OsString>,
    T: ArgumentValue,
    S: SearchableArgInfo<T>,
{
    type Item = ArgParseResult<Argument<T>>;

    fn next(&mut self) -> Option<Self::Item> {
        let arg = self.arguments.next()?;
        if self.in_post_double_dash_mode(&arg) {
            return Some(Ok(Argument::Raw(arg)));
        }
        Some(self.classify_arg(arg))
    }
}

/// Helper macro used to define `ArgInfo::Flag`s.
/// Variant is an enum variant, e.g. `enum ArgType { Variant }`.
///
/// ```ignore
/// flag!("-foo", Variant)
/// ```
macro_rules! flag {
    ($s:expr, $variant:expr) => {
        ArgInfo::Flag($s, $variant)
    };
}

/// Helper macro used to define `ArgInfo::TakeArg`s.
/// Variant is an enum variant, e.g. `enum ArgType { Variant(OsString) }`.
///
/// ```ignore
/// take_arg!("-foo", OsString, Separated, Variant)
/// take_arg!("-foo", OsString, Concatenated, Variant)
/// take_arg!("-foo", OsString, Concatenated(b'='), Variant)
/// ```
macro_rules! take_arg {
    ($s:expr, $vtype:ident, Separated, $variant:expr) => {
        ArgInfo::TakeArg(
            $s,
            |arg: OsString| $vtype::process(arg).map($variant),
            ArgDisposition::Separated,
        )
    };
    ($s:expr, $vtype:ident, $d:ident, $variant:expr) => {
        ArgInfo::TakeArg(
            $s,
            |arg: OsString| $vtype::process(arg).map($variant),
            ArgDisposition::$d(None),
        )
    };
    ($s:expr, $vtype:ident, $d:ident($x:expr), $variant:expr) => {
        ArgInfo::TakeArg(
            $s,
            |arg: OsString| $vtype::process(arg).map($variant),
            ArgDisposition::$d(Some($x)),
        )
    };
}

#[cfg(test)]
mod tests;
