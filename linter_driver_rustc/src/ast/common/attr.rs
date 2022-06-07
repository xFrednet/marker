use linter_api::ast::{AttrInput, AttrStyle, Attribute, Path, PathResolution, PathSegment, Symbol};

use crate::ast::{rustc::RustcContext, ToApi};

impl<'ast, 'tcx> ToApi<'ast, 'tcx, &'ast Attribute<'ast>> for rustc_ast::Attribute {
    fn to_api(&self, cx: &'ast RustcContext<'ast, 'tcx>) -> &'ast Attribute<'ast> {
        let (path, input) = match &self.kind {
            rustc_ast::AttrKind::Normal(item, _) => {
                let input = if let Some(symbol) = self.value_str() {
                    AttrInput::Expr(symbol.to_api(cx))
                } else {
                    AttrInput::None
                };
                (item.path.to_api(cx), input)
            },
            rustc_ast::AttrKind::DocComment(_, symbol) => {
                let segments = cx.alloc_slice(1, |_| {
                    PathSegment::new(Symbol::new(rustc_span::sym::doc.as_u32()), PathResolution::Unresolved)
                });
                (
                    Path::new(segments, PathResolution::Unresolved),
                    AttrInput::Expr(symbol.to_api(cx)),
                )
            },
        };

        cx.alloc_with(|| Attribute::new(self.style.to_api(cx), path, input, cx.new_span(self.span)))
    }
}

impl<'ast, 'tcx> ToApi<'ast, 'tcx, AttrStyle> for rustc_ast::AttrStyle {
    fn to_api(&self, cx: &'ast RustcContext<'ast, 'tcx>) -> AttrStyle {
        match self {
            rustc_ast::AttrStyle::Outer => AttrStyle::Outer,
            rustc_ast::AttrStyle::Inner => AttrStyle::Inner,
        }
    }
}
