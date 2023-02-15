use marker_api::ast::generic::{
    BindingGenericArg, GenericArgKind, GenericArgs, GenericParamKind, GenericParams, Lifetime, LifetimeClause,
    LifetimeKind, LifetimeParam, TraitBound, TyClause, TyParam, TyParamBound, WhereClauseKind,
};
use rustc_hir as hir;

use super::MarkerConverterInner;

impl<'ast, 'tcx> MarkerConverterInner<'ast, 'tcx> {
    #[must_use]
    pub fn to_lifetime(&self, rust_lt: &hir::Lifetime) -> Option<Lifetime<'ast>> {
        let kind = match rust_lt.res {
            hir::LifetimeName::Param(_) if rust_lt.is_anonymous() => return None,
            hir::LifetimeName::Param(local_id) => {
                LifetimeKind::Label(self.to_symbol_id(rust_lt.ident.name), self.to_generic_id(local_id))
            },
            hir::LifetimeName::ImplicitObjectLifetimeDefault => return None,
            hir::LifetimeName::Infer => LifetimeKind::Infer,
            hir::LifetimeName::Static => LifetimeKind::Static,
            hir::LifetimeName::Error => unreachable!("would have triggered a rustc error"),
        };

        Some(Lifetime::new(Some(self.to_span_id(rust_lt.ident.span)), kind))
    }

    pub fn to_generic_args_from_path(&self, rust_path: &rustc_hir::Path<'tcx>) -> GenericArgs<'ast> {
        self.to_generic_args(rust_path.segments.last().and_then(|s| s.args))
    }

    #[must_use]
    pub fn to_generic_args(&self, rustc_args: Option<&hir::GenericArgs<'tcx>>) -> GenericArgs<'ast> {
        let Some(rustc_args) = rustc_args else {
            return GenericArgs::new(&[]);
        };

        let mut args: Vec<_> = rustc_args
            .args
            .iter()
            .filter(|rustc_arg| !rustc_arg.is_synthetic())
            .filter_map(|rustc_arg| match rustc_arg {
                rustc_hir::GenericArg::Lifetime(rust_lt) => self
                    .to_lifetime(rust_lt)
                    .map(|lifetime| GenericArgKind::Lifetime(self.alloc(|| lifetime))),
                rustc_hir::GenericArg::Type(r_ty) => Some(GenericArgKind::Ty(self.alloc(|| self.to_ty(*r_ty)))),
                rustc_hir::GenericArg::Const(_) => todo!(),
                rustc_hir::GenericArg::Infer(_) => todo!(),
            })
            .collect();
        args.extend(rustc_args.bindings.iter().map(|binding| match &binding.kind {
            rustc_hir::TypeBindingKind::Equality { term } => match term {
                rustc_hir::Term::Ty(rustc_ty) => GenericArgKind::Binding(self.alloc(|| {
                    BindingGenericArg::new(
                        Some(self.to_span_id(binding.span)),
                        self.to_symbol_id(binding.ident.name),
                        self.to_ty(*rustc_ty),
                    )
                })),
                rustc_hir::Term::Const(_) => todo!(),
            },
            rustc_hir::TypeBindingKind::Constraint { .. } => todo!(),
        }));
        GenericArgs::new(self.alloc_slice_iter(args.drain(..)))
    }

    pub fn to_generic_params(&self, rustc_generics: &hir::Generics<'tcx>) -> GenericParams<'ast> {
        let clauses: Vec<_> = rustc_generics
            .predicates
            .iter()
            .filter_map(|predicate| {
                match predicate {
                    hir::WherePredicate::BoundPredicate(ty_bound) => {
                        // FIXME Add span to API clause:
                        // let span = to_api_span_id(ty_bound.span);
                        let params =
                            GenericParams::new(self.to_generic_param_kinds(ty_bound.bound_generic_params), &[]);
                        let ty = self.to_ty(ty_bound.bounded_ty);
                        Some(WhereClauseKind::Ty(self.alloc(|| {
                            TyClause::new(Some(params), ty, self.to_ty_param_bound(predicate.bounds()))
                        })))
                    },
                    hir::WherePredicate::RegionPredicate(lifetime_bound) => {
                        self.to_lifetime(lifetime_bound.lifetime).map(|lifetime| {
                            WhereClauseKind::Lifetime(self.alloc(|| {
                                let bounds: Vec<_> = lifetime_bound
                                    .bounds
                                    .iter()
                                    .filter_map(|bound| match bound {
                                        hir::GenericBound::Outlives(lifetime) => self.to_lifetime(lifetime),
                                        _ => unreachable!("lifetimes can only be bound by lifetimes"),
                                    })
                                    .collect();
                                let bounds = if bounds.is_empty() {
                                    self.alloc_slice_iter(bounds.into_iter())
                                } else {
                                    &[]
                                };
                                LifetimeClause::new(lifetime, bounds)
                            }))
                        })
                    },
                    hir::WherePredicate::EqPredicate(_) => {
                        unreachable!("the documentation states, that this is unsupported")
                    },
                }
            })
            .collect();
        let clauses = self.alloc_slice_iter(clauses.into_iter());

        GenericParams::new(self.to_generic_param_kinds(rustc_generics.params), clauses)
    }

    fn to_generic_param_kinds(&self, params: &[hir::GenericParam<'tcx>]) -> &'ast [GenericParamKind<'ast>] {
        if params.is_empty() {
            return &[];
        }

        let params: Vec<_> = params
            .iter()
            .filter_map(|rustc_param| {
                let name = match rustc_param.name {
                    hir::ParamName::Plain(ident) => self.to_symbol_id(ident.name),
                    _ => return None,
                };
                let def_id = self.rustc_cx.hir().local_def_id(rustc_param.hir_id);
                let id = self.to_generic_id(def_id.to_def_id());
                let span = self.to_span_id(rustc_param.span);
                match rustc_param.kind {
                    hir::GenericParamKind::Lifetime {
                        kind: hir::LifetimeParamKind::Explicit,
                    } => Some(GenericParamKind::Lifetime(
                        self.alloc(|| LifetimeParam::new(id, name, Some(span))),
                    )),
                    hir::GenericParamKind::Type { synthetic: false, .. } => {
                        Some(GenericParamKind::Ty(self.alloc(|| TyParam::new(Some(span), name, id))))
                    },
                    _ => None,
                }
            })
            .collect();

        self.alloc_slice_iter(params.into_iter())
    }

    #[must_use]
    pub fn to_ty_param_bound(&self, bounds: &[hir::GenericBound<'tcx>]) -> &'ast [TyParamBound<'ast>] {
        if bounds.is_empty() {
            return &[];
        }

        let bounds: Vec<_> = bounds
            .iter()
            .filter_map(|bound| match bound {
                hir::GenericBound::Trait(trait_ref, modifier) => Some(TyParamBound::TraitBound(self.alloc(|| {
                    TraitBound::new(
                        !matches!(modifier, hir::TraitBoundModifier::None),
                        self.to_trait_ref(&trait_ref.trait_ref),
                        self.to_span_id(bound.span()),
                    )
                }))),
                hir::GenericBound::LangItemTrait(_, _, _, _) => todo!(),
                hir::GenericBound::Outlives(rust_lt) => self
                    .to_lifetime(rust_lt)
                    .map(|api_lt| TyParamBound::Lifetime(self.alloc(|| api_lt))),
            })
            .collect();

        self.alloc_slice_iter(bounds.into_iter())
    }

    pub fn to_ty_param_bound_from_hir(
        &self,
        rust_bounds: &[rustc_hir::PolyTraitRef<'tcx>],
        rust_lt: &rustc_hir::Lifetime,
    ) -> &'ast [TyParamBound<'ast>] {
        let traits = rust_bounds.iter().map(|rust_trait_ref| {
            TyParamBound::TraitBound(self.storage.alloc(|| {
                TraitBound::new(
                    false,
                    self.to_trait_ref(&rust_trait_ref.trait_ref),
                    self.to_span_id(rust_trait_ref.span),
                )
            }))
        });

        if let Some(lt) = self.to_lifetime(rust_lt) {
            // alloc_slice_iter requires a const size, which is not possible otherwise
            let mut bounds: Vec<_> = traits.collect();
            bounds.push(TyParamBound::Lifetime(self.alloc(move || lt)));
            self.alloc_slice_iter(bounds.drain(..))
        } else {
            self.alloc_slice_iter(traits)
        }
    }
}
