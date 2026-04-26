// This is the rustc arg parsing library for drop point.
// its basically sccache/src/compiler/args.rs and parts of sccache/src/compiler/rust.rs
// with all the generic machinery removed, also lightly updated for 2026 rust.

use log::debug;
use std::cmp::Ordering;
use std::collections::HashSet;
use std::ffi::OsString;
use std::fmt::Debug;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::str;
use std::sync::LazyLock;
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
        #[derive(Clone, Debug, PartialEq)]
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
                _ => s.cmp(arg),
            },
            _ => s.cmp(arg),
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
            phantom: PhantomData,
        }
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

/// The state of `--color` options passed to the compiler.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ColorMode {
    Off,
    On,
    #[default]
    Auto,
}

/// Possible results of parsing argv. Generic over `T` so the same outcome
/// shape works for any flag-set parser, not just rustc's.
#[derive(Debug, PartialEq, Eq)]
pub enum ParseOutcome<T> {
    /// Commandline can be handled.
    Ok(T),
    /// Cannot cache this compilation.
    CannotCache(&'static str, Option<String>),
    /// This commandline is not a compile.
    NotCompilation,
}

macro_rules! cannot_cache {
    ($why:expr) => {
        return ParseOutcome::CannotCache($why, None)
    };
    ($why:expr, $extra_info:expr) => {
        return ParseOutcome::CannotCache($why, Some($extra_info))
    };
}

macro_rules! try_or_cannot_cache {
    ($arg:expr, $why:expr) => {{
        match $arg {
            Ok(arg) => arg,
            Err(e) => cannot_cache!($why, e.to_string()),
        }
    }};
}

// =============================================================================
// Concrete rustc argument parsing
// =============================================================================
//
// Everything above is generic over the argument-data type T and could parse
// any flag-style CLI. Below is the rustc-specific layer: the ParsedArguments
// struct (the typed result of parsing a rustc invocation), the ArgData enum
// that names each typed value rustc cares about, the ARGS table that
// describes every flag rustc accepts, and the parse_arguments function that
// walks an argv into a ParsedArguments.

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedArguments {
    /// The full commandline, with all parsed arguments.
    pub(crate) arguments: Vec<Argument<ArgData>>,
    /// The input source file. For third-party crates this path embeds
    /// `<registry>/<crate>@<version>/...`, which is sufficient to identify
    /// the source bytes (crates.io is immutable).
    pub(crate) input: PathBuf,
    /// The location of compiler outputs.
    pub(crate) output_dir: PathBuf,
    /// Paths to extern crates used in the compile. Sorted (cargo doesn't
    /// guarantee --extern ordering and we need deterministic hash inputs).
    pub(crate) externs: Vec<PathBuf>,
    /// The directories searched for rlibs.
    crate_link_paths: Vec<PathBuf>,
    /// Static libraries linked to in the compile.
    pub(crate) staticlibs: Vec<PathBuf>,
    /// The crate name passed to --crate-name.
    pub(crate) crate_name: String,
    /// The crate types that will be generated.
    crate_types: CrateTypes,
    /// If dependency info is being emitted, the name of the dep info file.
    pub(crate) dep_info: Option<PathBuf>,
    /// If `-C profile-use=PATH` was passed, the path to the profile data file.
    /// See https://doc.rust-lang.org/rustc/profile-guided-optimization.html
    pub(crate) profile: Option<PathBuf>,
    /// Set of `--emit` modes requested.
    /// rustc says it emits .rlib for `--emit=metadata`,
    /// see https://github.com/rust-lang/rust/issues/54852
    pub(crate) emit: HashSet<String>,
    /// The value of any `--color` option passed on the commandline.
    color_mode: ColorMode,
    /// Whether `--json` was passed to this invocation.
    has_json: bool,
    /// A `--target` parameter that specifies a path to a JSON file.
    pub(crate) target_json: Option<PathBuf>,
}

/// The selection of crate types for this compilation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrateTypes {
    rlib: bool,
    staticlib: bool,
}

/// `--emit` modes that the parser will accept as cacheable. Anything outside
/// this set causes `parse_arguments` to return `CannotCache` so the wrapper
/// runs rustc directly and bypasses the cache layer.
static ALLOWED_EMIT: LazyLock<HashSet<&'static str>> =
    LazyLock::new(|| ["link", "metadata", "dep-info"].iter().copied().collect());

macro_rules! make_os_string {
    ($( $v:expr ),*) => {{
        let mut s = OsString::new();
        $(
            s.push($v);
        )*
        s
    }};
}

#[derive(Clone, Debug, PartialEq)]
struct ArgCrateTypes {
    rlib: bool,
    staticlib: bool,
    others: HashSet<String>,
}

impl FromArg for ArgCrateTypes {
    fn process(arg: OsString) -> ArgParseResult<Self> {
        let arg = String::process(arg)?;
        let mut crate_types = ArgCrateTypes {
            rlib: false,
            staticlib: false,
            others: HashSet::new(),
        };
        for ty in arg.split(',') {
            match ty {
                // It is assumed that "lib" always refers to "rlib", which
                // is true right now but may not be in the future
                "lib" | "rlib" => crate_types.rlib = true,
                "staticlib" => crate_types.staticlib = true,
                other => {
                    crate_types.others.insert(other.to_owned());
                }
            }
        }
        Ok(crate_types)
    }
}

impl IntoArg for ArgCrateTypes {
    fn into_arg_os_string(self) -> OsString {
        let ArgCrateTypes {
            rlib,
            staticlib,
            others,
        } = self;
        let mut types: Vec<_> = others
            .iter()
            .map(String::as_str)
            .chain(if rlib { Some("rlib") } else { None })
            .chain(if staticlib { Some("staticlib") } else { None })
            .collect();
        types.sort_unstable();
        let types_string = types.join(",");
        types_string.into()
    }
}

#[derive(Clone, Debug, PartialEq)]
struct ArgLinkLibrary {
    kind: String,
    name: String,
}

impl FromArg for ArgLinkLibrary {
    fn process(arg: OsString) -> ArgParseResult<Self> {
        let (kind, name) = match split_os_string_arg(arg, "=")? {
            (kind, Some(name)) => (kind, name),
            // If no kind is specified, the default is dylib.
            (name, None) => ("dylib".to_owned(), name),
        };
        Ok(ArgLinkLibrary { kind, name })
    }
}

impl IntoArg for ArgLinkLibrary {
    fn into_arg_os_string(self) -> OsString {
        let ArgLinkLibrary { kind, name } = self;
        make_os_string!(kind, "=", name)
    }
}

#[derive(Clone, Debug, PartialEq)]
struct ArgLinkPath {
    kind: String,
    path: PathBuf,
}

impl FromArg for ArgLinkPath {
    fn process(arg: OsString) -> ArgParseResult<Self> {
        let (kind, path) = match split_os_string_arg(arg, "=")? {
            (kind, Some(path)) => (kind, path),
            // If no kind is specified, the path is used to search for all kinds
            (path, None) => ("all".to_owned(), path),
        };
        Ok(ArgLinkPath {
            kind,
            path: path.into(),
        })
    }
}

impl IntoArg for ArgLinkPath {
    fn into_arg_os_string(self) -> OsString {
        let ArgLinkPath { kind, path } = self;
        make_os_string!(kind, "=", path)
    }
}

#[derive(Clone, Debug, PartialEq)]
struct ArgCodegen {
    opt: String,
    value: Option<String>,
}

impl FromArg for ArgCodegen {
    fn process(arg: OsString) -> ArgParseResult<Self> {
        let (opt, value) = split_os_string_arg(arg, "=")?;
        Ok(ArgCodegen { opt, value })
    }
}

impl IntoArg for ArgCodegen {
    fn into_arg_os_string(self) -> OsString {
        let ArgCodegen { opt, value } = self;
        if let Some(value) = value {
            make_os_string!(opt, "=", value)
        } else {
            make_os_string!(opt)
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct ArgUnstable {
    opt: String,
    value: Option<String>,
}

impl FromArg for ArgUnstable {
    fn process(arg: OsString) -> ArgParseResult<Self> {
        let (opt, value) = split_os_string_arg(arg, "=")?;
        Ok(ArgUnstable { opt, value })
    }
}

impl IntoArg for ArgUnstable {
    fn into_arg_os_string(self) -> OsString {
        let ArgUnstable { opt, value } = self;
        if let Some(value) = value {
            make_os_string!(opt, "=", value)
        } else {
            make_os_string!(opt)
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct ArgExtern {
    name: String,
    path: PathBuf,
}

impl FromArg for ArgExtern {
    fn process(arg: OsString) -> ArgParseResult<Self> {
        if let (name, Some(path)) = split_os_string_arg(arg, "=")? {
            Ok(ArgExtern {
                name,
                path: path.into(),
            })
        } else {
            Err(ArgParseError::Other("no path for extern"))
        }
    }
}

impl IntoArg for ArgExtern {
    fn into_arg_os_string(self) -> OsString {
        let ArgExtern { name, path } = self;
        make_os_string!(name, "=", path)
    }
}

#[derive(Clone, Debug, PartialEq)]
enum ArgTarget {
    Name(String),
    Path(PathBuf),
    Unsure(OsString),
}

impl FromArg for ArgTarget {
    fn process(arg: OsString) -> ArgParseResult<Self> {
        // Is it obviously a json file path?
        if Path::new(&arg)
            .extension()
            .map(|ext| ext == "json")
            .unwrap_or(false)
        {
            return Ok(ArgTarget::Path(arg.into()));
        }
        // Time for clever detection - if we append .json (even if it's clearly
        // a directory, i.e. resulting in /my/dir/.json), does the path exist?
        let mut path = arg.clone();
        path.push(".json");
        if Path::new(&path).is_file() {
            // Unfortunately, we're now not sure what will happen without having
            // a list of all the built-in targets handy, as they don't get .json
            // auto-added for target json discovery
            return Ok(ArgTarget::Unsure(arg));
        }
        // The file doesn't exist so it can't be a path, safe to assume it's a name
        Ok(ArgTarget::Name(
            arg.into_string().map_err(ArgParseError::InvalidUnicode)?,
        ))
    }
}

impl IntoArg for ArgTarget {
    fn into_arg_os_string(self) -> OsString {
        match self {
            ArgTarget::Name(s) => s.into(),
            ArgTarget::Path(p) => p.into(),
            ArgTarget::Unsure(s) => s,
        }
    }
}

ArgData! { pub
    TooHardFlag,
    TooHardPath(PathBuf),
    NotCompilationFlag,
    NotCompilation(OsString),
    LinkLibrary(ArgLinkLibrary),
    LinkPath(ArgLinkPath),
    Emit(String),
    Extern(ArgExtern),
    Color(String),
    Json(String),
    CrateName(String),
    CrateType(ArgCrateTypes),
    OutDir(PathBuf),
    CodeGen(ArgCodegen),
    PassThrough(OsString),
    Target(ArgTarget),
    Unstable(ArgUnstable),
}

use self::ArgData::*;

// Taken from rustc's `rustc_optgroups()`:
// https://github.com/rust-lang/rust/blob/597d9e43be882cdbd218e58c4f7efb2fa3da7540/compiler/rustc_session/src/config.rs#L1764
static ARGS: &[ArgInfo<ArgData>] = &[
    flag!("-", TooHardFlag),
    take_arg!("--allow", OsString, CanBeSeparated(b'='), PassThrough),
    take_arg!("--cap-lints", OsString, CanBeSeparated(b'='), PassThrough),
    take_arg!("--cfg", OsString, CanBeSeparated(b'='), PassThrough),
    take_arg!("--check-cfg", OsString, CanBeSeparated(b'='), PassThrough),
    take_arg!("--codegen", ArgCodegen, CanBeSeparated(b'='), CodeGen),
    take_arg!("--color", String, CanBeSeparated(b'='), Color),
    take_arg!("--crate-name", String, CanBeSeparated(b'='), CrateName),
    take_arg!(
        "--crate-type",
        ArgCrateTypes,
        CanBeSeparated(b'='),
        CrateType
    ),
    take_arg!("--deny", OsString, CanBeSeparated(b'='), PassThrough),
    take_arg!(
        "--diagnostic-width",
        OsString,
        CanBeSeparated(b'='),
        PassThrough
    ),
    take_arg!("--edition", OsString, CanBeSeparated(b'='), PassThrough),
    take_arg!("--emit", String, CanBeSeparated(b'='), Emit),
    take_arg!("--env-set", OsString, CanBeSeparated(b'='), PassThrough),
    take_arg!(
        "--error-format",
        OsString,
        CanBeSeparated(b'='),
        PassThrough
    ),
    take_arg!("--explain", OsString, CanBeSeparated(b'='), NotCompilation),
    take_arg!("--extern", ArgExtern, CanBeSeparated(b'='), Extern),
    take_arg!("--forbid", OsString, CanBeSeparated(b'='), PassThrough),
    take_arg!("--force-warn", OsString, CanBeSeparated(b'='), PassThrough),
    flag!("--help", NotCompilationFlag),
    take_arg!("--json", String, CanBeSeparated(b'='), Json),
    take_arg!("--out-dir", PathBuf, CanBeSeparated(b'='), OutDir),
    take_arg!("--pretty", OsString, CanBeSeparated(b'='), NotCompilation),
    take_arg!("--print", OsString, CanBeSeparated(b'='), NotCompilation),
    take_arg!(
        "--remap-path-prefix",
        OsString,
        CanBeSeparated(b'='),
        PassThrough
    ),
    take_arg!(
        "--remap-path-scope",
        OsString,
        CanBeSeparated(b'='),
        PassThrough
    ),
    take_arg!("--sysroot", PathBuf, CanBeSeparated(b'='), TooHardPath),
    take_arg!("--target", ArgTarget, CanBeSeparated(b'='), Target),
    take_arg!("--unpretty", OsString, CanBeSeparated(b'='), NotCompilation),
    flag!("--version", NotCompilationFlag),
    take_arg!("--warn", OsString, CanBeSeparated(b'='), PassThrough),
    take_arg!("-A", OsString, CanBeSeparated, PassThrough),
    take_arg!("-C", ArgCodegen, CanBeSeparated, CodeGen),
    take_arg!("-D", OsString, CanBeSeparated, PassThrough),
    take_arg!("-F", OsString, CanBeSeparated, PassThrough),
    take_arg!("-L", ArgLinkPath, CanBeSeparated, LinkPath),
    flag!("-V", NotCompilationFlag),
    take_arg!("-W", OsString, CanBeSeparated, PassThrough),
    take_arg!("-Z", ArgUnstable, CanBeSeparated, Unstable),
    take_arg!("-l", ArgLinkLibrary, CanBeSeparated, LinkLibrary),
    take_arg!("-o", PathBuf, CanBeSeparated, TooHardPath),
];

/// Parse `arguments` as rustc command-line arguments, determine if
/// we can cache the result of compilation. This is only intended to
/// cover a subset of rustc invocations, primarily focused on those
/// that will occur when cargo invokes rustc.
///
/// Caveats:
/// * We don't support compilation from stdin.
/// * We require --emit.
/// * We only support `link` and `dep-info` in --emit (and don't support *just* 'dep-info')
/// * We require `--out-dir`.
/// * We don't support `-o file`.
pub fn parse_arguments(arguments: &[OsString], cwd: &Path) -> ParseOutcome<ParsedArguments> {
    let mut args = vec![];

    let mut emit: Option<HashSet<String>> = None;
    let mut input = None;
    let mut output_dir = None;
    let mut crate_name = None;
    let mut crate_types = CrateTypes {
        rlib: false,
        staticlib: false,
    };
    let mut extra_filename = None;
    let mut externs = vec![];
    let mut crate_link_paths = vec![];
    let mut static_lib_names = vec![];
    let mut static_link_paths: Vec<PathBuf> = vec![];
    let mut color_mode = ColorMode::Auto;
    let mut has_json = false;
    let mut profile = None;
    let mut target_json = None;

    for (idx, arg) in ArgsIter::new(arguments.iter().cloned(), ARGS).enumerate() {
        let arg = try_or_cannot_cache!(arg, "argument parse");
        match arg.get_data() {
            Some(TooHardFlag) | Some(TooHardPath(_)) => {
                cannot_cache!(arg.flag_str().expect("Can't be Argument::Raw/UnknownFlag",))
            }
            Some(NotCompilationFlag) | Some(NotCompilation(_)) => {
                return ParseOutcome::NotCompilation;
            }
            Some(LinkLibrary(ArgLinkLibrary { kind, name })) => {
                if kind == "static" {
                    static_lib_names.push(name.to_owned());
                }
            }
            Some(LinkPath(ArgLinkPath { kind, path })) => {
                // "crate" is not typically necessary as cargo will normally
                // emit explicit --extern arguments
                if kind == "crate" || kind == "dependency" || kind == "all" {
                    crate_link_paths.push(cwd.join(path));
                }
                if kind == "native" || kind == "all" {
                    static_link_paths.push(cwd.join(path));
                }
            }
            Some(Emit(value)) => {
                if emit.is_some() {
                    // We don't support passing --emit more than once.
                    cannot_cache!("more than one --emit");
                }
                emit = Some(value.split(',').map(str::to_owned).collect());
            }
            Some(CrateType(ArgCrateTypes {
                rlib,
                staticlib,
                others,
            })) => {
                // We can't cache non-rlib/staticlib crates, because rustc invokes the
                // system linker to link them, and we don't know about all the linker inputs.
                if !others.is_empty() {
                    let others: Vec<&str> = others.iter().map(String::as_str).collect();
                    let others_string = others.join(",");
                    cannot_cache!("crate-type", others_string)
                }
                crate_types.rlib |= rlib;
                crate_types.staticlib |= staticlib;
            }
            Some(CrateName(value)) => crate_name = Some(value.clone()),
            Some(OutDir(value)) => output_dir = Some(value.clone()),
            Some(Extern(ArgExtern { path, .. })) => externs.push(path.clone()),
            Some(CodeGen(ArgCodegen { opt, value })) => match (opt.as_ref(), value) {
                ("extra-filename", Some(value)) => extra_filename = Some(value.to_owned()),
                ("extra-filename", None) => cannot_cache!("extra-filename"),
                ("profile-use", Some(v)) => profile = Some(v.clone()),
                // Incremental compilation produces extra outputs we don't
                // track and the session-state files it writes are not
                // deterministic across runs. Letting rustc do its incremental
                // thing locally is also likely faster than a remote hit.
                // Punt for now; same call sccache makes.
                ("incremental", _) => cannot_cache!("incremental"),
                (_, _) => (),
            },
            Some(Unstable(_)) => (),
            Some(Color(value)) => {
                // We'll just assume the last specified value wins.
                color_mode = match value.as_ref() {
                    "always" => ColorMode::On,
                    "never" => ColorMode::Off,
                    _ => ColorMode::Auto,
                };
            }
            Some(Json(_)) => {
                has_json = true;
            }
            Some(PassThrough(_)) => (),
            Some(Target(target)) => match target {
                ArgTarget::Path(json_path) => target_json = Some(json_path.to_owned()),
                ArgTarget::Unsure(_) => cannot_cache!("target unsure"),
                ArgTarget::Name(_) => (),
            },
            None => match arg {
                Argument::Raw(ref val) => {
                    if idx == 0
                        && let Some(value) = val.to_str()
                        && value == "rustc"
                    {
                        // If the first argument is rustc, it's likely called via clippy-driver,
                        // so it's not actually an input file, which means we should discount it.
                        continue;
                    }
                    if input.is_some() {
                        // Can't cache compilations with multiple inputs.
                        cannot_cache!(
                            "multiple input files",
                            format!("prev = {input:?}, next = {arg:?}")
                        );
                    }
                    input = Some(val.clone());
                }
                Argument::UnknownFlag(_) => {}
                _ => unreachable!(),
            },
        }
        // We'll drop --color arguments, we're going to pass --color=always and the client will
        // strip colors if necessary.
        match arg.get_data() {
            Some(Color(_)) => {}
            _ => args.push(arg.normalize(NormalizedDisposition::Separated)),
        }
    }

    // Unwrap required values.
    macro_rules! req {
        ($x:ident) => {
            let $x = if let Some($x) = $x {
                $x
            } else {
                debug!("Can't cache compilation, missing `{}`", stringify!($x));
                cannot_cache!(concat!("missing ", stringify!($x)));
            };
        };
    }
    req!(input);
    req!(output_dir);
    req!(emit);
    req!(crate_name);
    // We won't cache invocations that are not producing
    // binary output.
    if !emit.is_empty() && !emit.contains("link") && !emit.contains("metadata") {
        return ParseOutcome::NotCompilation;
    }
    // If it's not an rlib and not a staticlib then crate-type wasn't passed,
    // so it will usually be inferred as a binary, though the `#![crate_type`
    // annotation may dictate otherwise - either way, we don't know what to do.
    if let CrateTypes {
        rlib: false,
        staticlib: false,
    } = crate_types
    {
        cannot_cache!("crate-type", "No crate-type passed".to_owned())
    }
    // We won't cache invocations that are outputting anything but
    // linker output and dep-info.
    if emit.iter().any(|e| !ALLOWED_EMIT.contains(e.as_str())) {
        cannot_cache!("unsupported --emit");
    }

    // Figure out the dep-info filename, if emitting dep-info.
    let dep_info = if emit.contains("dep-info") {
        let mut dep_info = crate_name.clone();
        if let Some(extra_filename) = extra_filename {
            dep_info.push_str(&extra_filename[..]);
        }
        dep_info.push_str(".d");
        Some(dep_info)
    } else {
        None
    };

    // Ignore profile if `link` is not in emit which means we are running `cargo check`.
    let profile = if emit.contains("link") { profile } else { None };

    // Locate all static libs specified on the commandline.
    let staticlibs = static_lib_names
        .into_iter()
        .filter_map(|name| {
            for path in &static_link_paths {
                for filename in [
                    format!("lib{}.a", name),
                    format!("{}.lib", name),
                    format!("{}.a", name),
                ] {
                    let lib_path = path.join(filename);
                    if lib_path.exists() {
                        return Some(lib_path);
                    }
                }
            }
            // rustc will just error if there's a missing static library, so don't worry about
            // it too much.
            None
        })
        .collect();
    // Cargo doesn't deterministically order --externs, and we need the hash inputs in a
    // deterministic order.
    externs.sort();
    ParseOutcome::Ok(ParsedArguments {
        arguments: args,
        input: PathBuf::from(input),
        output_dir,
        crate_types,
        externs,
        crate_link_paths,
        staticlibs,
        crate_name,
        dep_info: dep_info.map(|s| s.into()),
        profile: profile.map(|s| s.into()),
        emit,
        color_mode,
        has_json,
        target_json,
    })
}

#[cfg(test)]
#[path = "rustc_args_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "rustc_args_parser_tests.rs"]
mod parser_tests;
