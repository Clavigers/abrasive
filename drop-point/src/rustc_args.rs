// This is the rustc arg parsing library for drop point.
// its basically sccache/src/compiler/args.rs and parts of sccache/src/compiler/rust.rs
// with all the generic machinery removed, also lightly updated for 2026 rust.

use std::cmp::Ordering;
use std::ffi::OsString;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::str;
use thiserror::Error;

pub type ArgParseResult<T> = Result<T, ArgParseError>;
pub type ArgToStringResult = Result<String, ArgToStringError>;
pub type PathTransformerFn<'a> = &'a mut dyn FnMut(&Path) -> Option<String>;

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
            Argument::Raw(ref s) | Argument::UnknownFlag(ref s) => match self.emitted {
                0 => Some(s.clone()),
                _ => None,
            },
            Argument::Flag(s, _) => match self.emitted {
                0 => Some(s.into()),
                _ => None,
            },
            Argument::WithValue(s, ref v, ref d) => match (self.emitted, d) {
                (0, &ArgDisposition::CanBeSeparated(d)) | (0, &ArgDisposition::Concatenated(d)) => {
                    let mut s = OsString::from(s);
                    let v = v.clone().into_arg_os_string();
                    if let Some(d) = d {
                        if !v.is_empty() {
                            s.push(OsString::from(
                                str::from_utf8(&[d]).expect("delimiter should be ascii"),
                            ));
                        }
                    }
                    s.push(v);
                    Some(s)
                }
                (0, &ArgDisposition::Separated) | (0, &ArgDisposition::CanBeConcatenated(_)) => {
                    Some(s.into())
                }
                (1, &ArgDisposition::Separated) | (1, &ArgDisposition::CanBeConcatenated(_)) => {
                    Some(v.clone().into_arg_os_string())
                }
                _ => None,
            },
        };
        if result.is_some() {
            self.emitted += 1;
        }
        result
    }
}