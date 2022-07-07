use linter_api::ast::{AttrInput, Attribute, Path, PathResolution, PathSegment, Symbol};

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

        cx.alloc_with(|| Attribute::new(self.style == rustc_ast::AttrStyle::Outer, path, input, cx.new_span(self.span)))
    }
}
