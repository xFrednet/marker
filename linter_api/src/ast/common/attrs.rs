use std::marker::PhantomData;

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
    style: AttrStyle,
    path: Path<'ast>,
    input: AttrInput<'ast>,
    span: &'ast dyn Span<'ast>,
}

impl<'ast> Attribute<'ast> {
    pub fn style(&self) -> AttrStyle {
        self.style
    }

    pub fn path(&self) -> &Path<'ast> {
        &self.path
    }

    pub fn input(&self) -> AttrInput {
        self.input
    }

    pub fn span(&self) -> &dyn Span<'ast> {
        self.span
    }
}

#[cfg(feature = "driver-api")]
impl<'ast> Attribute<'ast> {
    #[must_use]
    pub fn new(style: AttrStyle, path: Path<'ast>, input: AttrInput<'ast>, span: &'ast dyn Span<'ast>) -> Self {
        Self {
            style,
            path,
            input,
            span,
        }
    }
}

/// The location of the attribute relative to the attached item oder expression.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AttrStyle {
    /// Inner attribute, attached to an item from the inside, like:
    /// ```
    /// mod item {
    ///     #![allow(dead_code)]
    /// }
    /// ```
    Inner,
    /// Outer attributes attached to items from the outside, like:
    /// ```
    /// #[allow(dead_code)]
    /// mod item {}
    /// ```
    Outer,
}

/// The input of the attribute.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AttrInput<'ast> {
    /// The attribute didn't receive an input. An example could be:
    /// ```
    /// #[rustfmt::skip]
    /// ```
    None,
    /// The attribute receives an expression as an input. An example could be:
    /// ```
    /// #[clippy::msrv = "1.45.0"]
    /// ```
    ///
    /// FIXME, this should return an expression and not just a symbol. This requires
    /// the definition of expression nodes. (@xFrednet)
    Expr(Symbol),
    /// The attribute received a token tree as an inout. An example could be:
    /// ```
    /// #[derive(Debug, Clone)]
    /// #[allow(dead_code)]
    /// ```
    DelimTokenTree(&'ast TokenTree<'ast>),
}

/// A token tree used for macros and attributes.
///
/// See: <https://doc.rust-lang.org/stable/reference/macros.html>
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TokenTree<'ast> {
    _data: PhantomData<&'ast ()>,
}
