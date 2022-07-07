use std::{marker::PhantomData, str::FromStr};

use once_cell::sync::{Lazy, OnceCell};

use super::{Path, Span, Symbol};

/// An attribute attached to an expression or item.
///
/// Examples:
/// ```
/// // Outer attributes
/// #[clippy::msrv = "1.23.0"]
/// #[rustfmt::skip]
/// #[allow(unused)]
/// mod example {
///     // Inner attribute
///     #![warn(dead_code)]
/// }
/// ```
///
/// See <https://doc.rust-lang.org/stable/reference/attributes.html>
#[derive(Debug)]
pub struct Attribute<'ast> {
    /// This field indicates the style of the attribute. If it's not an outer
    /// attribute it's automatically an inner attribute
    is_outer: bool,
    path: Path<'ast>,
    input: AttrInput<'ast>,
    span: &'ast dyn Span<'ast>,
}

impl<'ast> Attribute<'ast> {
    /// Returns true, if the attribute is attached to items from the outside, like:
    /// ```
    /// #[allow(dead_code)]
    /// mod item {}
    /// ```
    pub fn is_outer(&self) -> bool {
        self.is_outer
    }

    /// Returns true if this attribute is attached to the item from the inside, like:
    /// ```
    /// mod item {
    ///     #![allow(dead_code)]
    /// }
    /// ```
    pub fn is_inner(&self) -> bool {
        !self.is_outer
    }

    pub fn path(&self) -> &Path<'ast> {
        &self.path
    }

    pub fn input(&self) -> &AttrInput {
        &self.input
    }

    pub fn span(&self) -> &dyn Span<'ast> {
        self.span
    }
}

#[cfg(feature = "driver-api")]
impl<'ast> Attribute<'ast> {
    #[must_use]
    pub fn new(is_outer: bool, path: Path<'ast>, input: AttrInput<'ast>, span: &'ast dyn Span<'ast>) -> Self {
        Self {
            is_outer,
            path,
            input,
            span,
        }
    }
}

/// The input of the attribute.
#[non_exhaustive]
#[derive(Debug)]
pub enum AttrInput<'ast> {
    /// The attribute didn't receive an input. An example could be:
    /// ```
    /// #[rustfmt::skip]
    /// # mod placeholder {}
    /// ```
    None,
    /// The attribute receives an expression as an input. An example could be:
    /// ```
    /// #[clippy::msrv = "1.45.0"]
    /// # mod placeholder {}
    /// ```
    ///
    /// FIXME, this should return an expression and not just a symbol. This requires
    /// the definition of expression nodes. (@xFrednet)
    Expr(Symbol),
    /// The attribute received a token tree as an inout. An example could be:
    /// ```
    /// #[derive(Debug, Clone)]
    /// #[allow(dead_code)]
    /// # struct Placeholder {}
    /// ```
    DelimTokenTree(&'ast MacroTokenStream<'ast>),
}

/// A token tree used for macros and attributes.
///
/// See: <https://doc.rust-lang.org/stable/reference/macros.html>
pub struct MacroTokenStream<'ast> {
    _data: PhantomData<&'ast ()>,
    string_repr: Lazy<String>,
    proc_macro2_repr: OnceCell<Result<proc_macro2::TokenStream, ()>>,
}

impl<'ast> std::fmt::Debug for MacroTokenStream<'ast> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenTree")
            .field("string_repr", &self.string_repr)
            .field("proc_macro2_repr init", &self.proc_macro2_repr.get().is_some())
            .finish()
    }
}

#[cfg(feature = "driver-api")]
impl<'ast> MacroTokenStream<'ast> {
    pub fn new(string_repr: Lazy<String>) -> Self {
        Self {
            _data: PhantomData,
            string_repr,
            proc_macro2_repr: OnceCell::default(),
        }
    }
}

impl<'ast> MacroTokenStream<'ast> {
    pub fn as_str_repr(&self) -> &str {
        &self.string_repr
    }

    /// This function tries to return a `proc_macro2` representation of the TokenSteam.
    ///
    /// Note that this returns a representation from an external crate. The conversion
    /// might not always succeed. The representation of the external type is not part of the
    /// stable API. For the stability of the representation, please refer to the
    /// external crate document.
    pub fn as_proc_macro2_repr(&self) -> &Result<proc_macro2::TokenStream, ()> {
        self.proc_macro2_repr
            .get_or_init(|| proc_macro2::TokenStream::from_str(self.as_str_repr()).map_err(|_| ()))
    }
}
