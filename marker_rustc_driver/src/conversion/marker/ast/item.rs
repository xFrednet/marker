use marker_api::{
    ast::{
        self, AdtKind, AssocItemKind, Body, CommonItemData, CommonPatData, ConstItem, EnumItem, EnumVariant,
        ExternBlockItem, ExternCrateItem, ExternItemKind, FnItem, FnParam, IdentPat, ImplItem, ItemField, ItemKind,
        ModItem, PatKind, StaticItem, StructItem, TraitItem, TyAliasItem, UnionItem, UnstableItem, UseItem, UseKind,
        Visibility,
    },
    common::{Abi, Constness, Mutability, Safety, Syncness},
    prelude::*,
    CtorBlocker,
};
use rustc_hir as hir;

use crate::conversion::marker::MarkerConverterInner;

impl<'ast, 'tcx> MarkerConverterInner<'ast, 'tcx> {
    #[must_use]
    pub fn to_items(&self, items: &[hir::ItemId]) -> &'ast [ItemKind<'ast>] {
        let items: Vec<_> = items
            .iter()
            .map(|rid| self.rustc_cx.hir().item(*rid))
            .filter_map(|rustc_item| self.to_item(rustc_item))
            .collect();
        self.alloc_slice(items)
    }

    pub fn to_item_from_id(&self, item: hir::ItemId) -> Option<ItemKind<'ast>> {
        let item = self.rustc_cx.hir().item(item);
        self.to_item(item)
    }

    #[must_use]
    pub fn to_item(&self, rustc_item: &'tcx hir::Item<'tcx>) -> Option<ItemKind<'ast>> {
        let id = self.to_item_id(rustc_item.owner_id);
        // During normal conversion, this'll never be hit. However, if the user
        // requests an item from an ID it might be, that the child has already
        // been converted. This is not the case for items in the main crate,
        // since all of them have been converted, but external crates could
        // run into this issue. If performance becomes a problem, we can try
        // benchmarking, a flag to disable this during initial translation.
        if let Some(item) = self.items.borrow().get(&id) {
            return Some(*item);
        }

        if self.is_compiler_generated(rustc_item.span) {
            return None;
        }

        let ident = self.to_ident(rustc_item.ident);
        let data = CommonItemData::builder()
            .id(id)
            .span(self.to_span_id(rustc_item.span))
            .vis(self.to_visibility(rustc_item.owner_id.def_id, rustc_item.vis_span))
            .ident(ident)
            .build();
        let item =
            match &rustc_item.kind {
                hir::ItemKind::ExternCrate(original_name) => ItemKind::ExternCrate(self.alloc({
                    ExternCrateItem::new(data, self.to_symbol_id(original_name.unwrap_or(rustc_item.ident.name)))
                })),
                hir::ItemKind::Use(path, use_kind) => {
                    let use_kind = match use_kind {
                        hir::UseKind::Single => UseKind::Single,
                        hir::UseKind::Glob => UseKind::Glob,
                        hir::UseKind::ListStem => return None,
                    };
                    ItemKind::Use(self.alloc(UseItem::new(data, self.to_path(path), use_kind)))
                },
                hir::ItemKind::Static(rustc_ty, rustc_mut, rustc_body_id) => ItemKind::Static(self.alloc({
                    StaticItem::new(
                        data,
                        self.to_mutability(*rustc_mut),
                        Some(self.to_body_id(*rustc_body_id)),
                        self.to_syn_ty(rustc_ty),
                    )
                })),
                hir::ItemKind::Const(rustc_ty, _generics, rustc_body_id) => ItemKind::Const(self.alloc(
                    ConstItem::new(data, self.to_syn_ty(rustc_ty), Some(self.to_body_id(*rustc_body_id))),
                )),
                hir::ItemKind::Fn(fn_sig, generics, body_id) => {
                    #[cfg(debug_assertions)]
                    #[allow(clippy::manual_assert)]
                    if rustc_item.ident.name.as_str() == "rustc_driver_please_ice_on_this" {
                        panic!("this is your captain talking, we are about to ICE");
                    }

                    ItemKind::Fn(self.alloc(self.to_fn_item(
                        data,
                        generics,
                        fn_sig,
                        false,
                        hir::TraitFn::Provided(*body_id),
                    )))
                },
                hir::ItemKind::Mod(rustc_mod) => ItemKind::Mod(
                    self.alloc(
                        ModItem::builder()
                            .data(data)
                            .items(self.to_items(rustc_mod.item_ids))
                            .build(),
                    ),
                ),
                hir::ItemKind::ForeignMod { abi, items } => ItemKind::ExternBlock(self.alloc({
                    let abi = self.to_abi(*abi);
                    ExternBlockItem::new(data, abi, self.to_external_items(items, abi))
                })),
                hir::ItemKind::Macro(_, _) | hir::ItemKind::GlobalAsm(_) => return None,
                hir::ItemKind::TyAlias(rustc_ty, rustc_generics) => ItemKind::TyAlias(self.alloc({
                    TyAliasItem::new(
                        data,
                        self.to_syn_generic_params(rustc_generics),
                        &[],
                        Some(self.to_syn_ty(rustc_ty)),
                    )
                })),
                hir::ItemKind::OpaqueTy(_) => ItemKind::Unstable(self.alloc(UnstableItem::new(
                    data,
                    Some(self.to_symbol_id(rustc_span::sym::type_alias_impl_trait)),
                ))),
                hir::ItemKind::Enum(enum_def, generics) => {
                    let variants = self.alloc_slice(enum_def.variants.iter().map(|variant| {
                        EnumVariant::new(
                            self.to_variant_id(variant.def_id),
                            self.to_symbol_id(variant.ident.name),
                            self.to_span_id(variant.span),
                            self.to_adt_kind(&variant.data),
                            variant.disr_expr.map(|anon| self.to_const_expr(anon)),
                        )
                    }));
                    self.variants
                        .borrow_mut()
                        .extend(variants.iter().map(|var| (var.id(), var)));
                    ItemKind::Enum(self.alloc(EnumItem::new(data, self.to_syn_generic_params(generics), variants)))
                },
                hir::ItemKind::Struct(var_data, generics) => ItemKind::Struct(self.alloc(StructItem::new(
                    data,
                    self.to_syn_generic_params(generics),
                    self.to_adt_kind(var_data),
                ))),
                hir::ItemKind::Union(var_data, generics) => ItemKind::Union(self.alloc({
                    UnionItem::new(
                        data,
                        self.to_syn_generic_params(generics),
                        self.to_adt_kind(var_data).fields(),
                    )
                })),
                hir::ItemKind::Trait(_is_auto, unsafety, generics, bounds, items) => ItemKind::Trait(self.alloc({
                    TraitItem::new(
                        data,
                        matches!(unsafety, hir::Unsafety::Unsafe),
                        self.to_syn_generic_params(generics),
                        self.to_syn_ty_param_bound(bounds),
                        self.to_assoc_items(items),
                    )
                })),
                hir::ItemKind::TraitAlias(_, _) => ItemKind::Unstable(self.alloc(UnstableItem::new(
                    data,
                    Some(self.to_symbol_id(rustc_span::sym::trait_alias)),
                ))),
                hir::ItemKind::Impl(imp) => ItemKind::Impl(self.alloc({
                    ImplItem::new(
                        data,
                        matches!(imp.unsafety, hir::Unsafety::Unsafe),
                        matches!(imp.polarity, rustc_ast::ImplPolarity::Positive),
                        imp.of_trait.as_ref().map(|trait_ref| self.to_trait_ref(trait_ref)),
                        self.to_syn_generic_params(imp.generics),
                        self.to_syn_ty(imp.self_ty),
                        self.to_assoc_items_from_impl(imp.items),
                    )
                })),
            };

        self.items.borrow_mut().insert(id, item);
        Some(item)
    }

    fn is_compiler_generated(&self, span: rustc_span::Span) -> bool {
        let ctxt = span.ctxt();

        !ctxt.is_root() && matches!(ctxt.outer_expn_data().kind, rustc_span::ExpnKind::AstPass(_))
    }

    fn to_visibility(&self, owner_id: hir::def_id::LocalDefId, vis_span: rustc_span::Span) -> Visibility<'ast> {
        let span = (!vis_span.is_empty()).then(|| self.to_span_id(vis_span));

        Visibility::builder()
            .span(span)
            .sem(self.to_sem_visibility(owner_id, span.is_some()))
            .build()
    }

    fn to_fn_item(
        &self,
        data: CommonItemData<'ast>,
        generics: &hir::Generics<'tcx>,
        fn_sig: &hir::FnSig<'tcx>,
        is_extern: bool,
        body_info: hir::TraitFn<'_>,
    ) -> FnItem<'ast> {
        let api_body = match &body_info {
            hir::TraitFn::Provided(id) => Some(self.to_body_id(*id)),
            hir::TraitFn::Required(_) => None,
        };
        let params = self.to_fn_params(fn_sig.decl, body_info);
        let header = fn_sig.header;
        let return_ty = if let hir::FnRetTy::Return(rust_ty) = fn_sig.decl.output {
            // Unwrap `impl Future<Output = <ty>>` for async
            if let hir::IsAsync::Async(_) = header.asyncness
                && let hir::TyKind::OpaqueDef(item_id, _bounds, _) = rust_ty.kind
                && let item = self.rustc_cx.hir().item(item_id)
                && let hir::ItemKind::OpaqueTy(opty) = &item.kind
                && let [output_bound] = opty.bounds
                && let hir::GenericBound::LangItemTrait(_lang_item, _span, _hir_id, rustc_args) = output_bound
                && let [output_bound] = rustc_args.bindings
            {
                Some(self.to_syn_ty(output_bound.ty()))
            } else {
                Some(self.to_syn_ty(rust_ty))
            }
        } else {
            None
        };

        FnItem::new(
            data,
            self.to_syn_generic_params(generics),
            self.to_constness(header.constness),
            self.to_syncness(header.asyncness),
            self.to_safety(header.unsafety),
            is_extern,
            fn_sig.decl.implicit_self.has_implicit_self(),
            self.to_abi(header.abi),
            params,
            return_ty,
            api_body,
        )
    }

    fn to_fn_params(&self, decl: &hir::FnDecl<'tcx>, body_info: hir::TraitFn<'_>) -> &'ast [FnParam<'ast>] {
        match body_info {
            hir::TraitFn::Required(idents) => {
                self.alloc_slice(idents.iter().zip(decl.inputs.iter()).map(|(ident, ty)| {
                    FnParam::new(
                        self.to_span_id(ident.span.to(ty.span)),
                        PatKind::Ident(self.alloc(IdentPat::new(
                            CommonPatData::new(self.to_span_id(ident.span)),
                            self.to_symbol_id(ident.name),
                            self.to_var_id(hir::HirId::INVALID),
                            Mutability::Unmut,
                            false,
                            None,
                        ))),
                        self.to_syn_ty(ty),
                    )
                }))
            },
            hir::TraitFn::Provided(body_id) => {
                let body = self.rustc_cx.hir().body(body_id);
                self.with_body(body_id, || {
                    self.alloc_slice(body.params.iter().zip(decl.inputs.iter()).map(|(param, ty)| {
                        FnParam::new(self.to_span_id(param.span), self.to_pat(param.pat), self.to_syn_ty(ty))
                    }))
                })
            },
        }
    }

    fn to_adt_kind(&self, var_data: &'tcx hir::VariantData) -> AdtKind<'ast> {
        match var_data {
            hir::VariantData::Struct(fields, _recovered) => AdtKind::Field(self.to_fields(fields).into()),
            hir::VariantData::Tuple(fields, ..) => AdtKind::Tuple(self.to_fields(fields).into()),
            hir::VariantData::Unit(..) => AdtKind::Unit,
        }
    }

    fn to_fields(&self, fields: &'tcx [hir::FieldDef]) -> &'ast [ItemField<'ast>] {
        let fields = self.alloc_slice(fields.iter().map(|field| {
            ItemField::new(
                self.to_field_id(field.hir_id),
                self.to_visibility(field.def_id, field.vis_span),
                self.to_symbol_id(field.ident.name),
                self.to_syn_ty(field.ty),
                self.to_span_id(field.span),
            )
        }));

        self.fields
            .borrow_mut()
            .extend(fields.iter().map(|field| (field.id(), field)));

        fields
    }

    fn to_external_items(&self, items: &'tcx [hir::ForeignItemRef], abi: Abi) -> &'ast [ExternItemKind<'ast>] {
        self.alloc_slice(items.iter().map(|item| self.to_external_item(item, abi)))
    }

    fn to_external_item(&self, rustc_item: &'tcx hir::ForeignItemRef, abi: Abi) -> ExternItemKind<'ast> {
        let id = self.to_item_id(rustc_item.id.owner_id);
        if let Some(item) = self.items.borrow().get(&id) {
            #[expect(non_exhaustive_omitted_patterns)]
            return match item {
                ItemKind::Static(data) => ExternItemKind::Static(data, CtorBlocker::new()),
                ItemKind::Fn(data) => ExternItemKind::Fn(data, CtorBlocker::new()),
                _ => unreachable!("only static and `Static` and `Fn` items can be found a foreign item id"),
            };
        }

        let foreign_item = self.rustc_cx.hir().foreign_item(rustc_item.id);
        let data = CommonItemData::builder()
            .id(id)
            .span(self.to_span_id(rustc_item.span))
            .vis(self.to_visibility(foreign_item.owner_id.def_id, foreign_item.vis_span))
            .ident(self.to_ident(rustc_item.ident))
            .build();
        let item = match &foreign_item.kind {
            hir::ForeignItemKind::Fn(decl, idents, generics) => {
                let return_ty = if let hir::FnRetTy::Return(rust_ty) = decl.output {
                    Some(self.to_syn_ty(rust_ty))
                } else {
                    None
                };
                ExternItemKind::Fn(
                    self.alloc(FnItem::new(
                        data,
                        self.to_syn_generic_params(generics),
                        Constness::NotConst,
                        Syncness::Sync,
                        Safety::Safe,
                        true,
                        decl.implicit_self.has_implicit_self(),
                        abi,
                        self.to_fn_params(decl, hir::TraitFn::Required(idents)),
                        return_ty,
                        None,
                    )),
                    CtorBlocker::new(),
                )
            },
            hir::ForeignItemKind::Static(ty, rustc_mut) => ExternItemKind::Static(
                self.alloc(StaticItem::new(
                    data,
                    self.to_mutability(*rustc_mut),
                    None,
                    self.to_syn_ty(ty),
                )),
                CtorBlocker::new(),
            ),
            hir::ForeignItemKind::Type => {
                todo!("foreign type are currently sadly not supported. See rust-marker/marker#182")
            },
        };

        self.items.borrow_mut().insert(id, item.as_item());
        item
    }

    fn to_assoc_items(&self, items: &[hir::TraitItemRef]) -> &'ast [AssocItemKind<'ast>] {
        self.alloc_slice(items.iter().map(|item| self.to_assoc_item(item)))
    }

    fn to_assoc_item(&self, rustc_item: &hir::TraitItemRef) -> AssocItemKind<'ast> {
        let id = self.to_item_id(rustc_item.id.owner_id);
        if let Some(item) = self.items.borrow().get(&id) {
            #[expect(non_exhaustive_omitted_patterns)]
            return match item {
                ItemKind::TyAlias(item) => AssocItemKind::TyAlias(item, CtorBlocker::new()),
                ItemKind::Const(item) => AssocItemKind::Const(item, CtorBlocker::new()),
                ItemKind::Fn(item) => AssocItemKind::Fn(item, CtorBlocker::new()),
                _ => unreachable!("only static and `TyAlias`, `Const` and `Fn` items can be found as an assoc item"),
            };
        }

        let trait_item = self.rustc_cx.hir().trait_item(rustc_item.id);
        let data = CommonItemData::builder()
            .id(id)
            .span(self.to_span_id(rustc_item.span))
            .vis(
                Visibility::builder()
                    .sem(sem::Visibility::builder().kind(sem::VisibilityKind::DefaultPub).build())
                    .build(),
            )
            .ident(self.to_ident(rustc_item.ident))
            .build();

        let item = match &trait_item.kind {
            hir::TraitItemKind::Const(ty, body_id) => AssocItemKind::Const(
                self.alloc(ConstItem::new(
                    data,
                    self.to_syn_ty(ty),
                    body_id.map(|id| self.to_body_id(id)),
                )),
                CtorBlocker::new(),
            ),
            hir::TraitItemKind::Fn(fn_sig, trait_fn) => AssocItemKind::Fn(
                self.alloc(self.to_fn_item(data, trait_item.generics, fn_sig, false, *trait_fn)),
                CtorBlocker::new(),
            ),
            hir::TraitItemKind::Type(bounds, ty) => AssocItemKind::TyAlias(
                self.alloc({
                    TyAliasItem::new(
                        data,
                        self.to_syn_generic_params(trait_item.generics),
                        self.to_syn_ty_param_bound(bounds),
                        ty.map(|ty| self.to_syn_ty(ty)),
                    )
                }),
                CtorBlocker::new(),
            ),
        };

        self.items.borrow_mut().insert(id, item.as_item());
        item
    }

    fn to_assoc_items_from_impl(&self, items: &[hir::ImplItemRef]) -> &'ast [AssocItemKind<'ast>] {
        self.alloc_slice(items.iter().map(|item| self.to_assoc_item_from_impl(item)))
    }

    fn to_assoc_item_from_impl(&self, rustc_item: &hir::ImplItemRef) -> AssocItemKind<'ast> {
        let id = self.to_item_id(rustc_item.id.owner_id);
        if let Some(item) = self.items.borrow().get(&id) {
            #[expect(non_exhaustive_omitted_patterns)]
            return match item {
                ItemKind::TyAlias(item) => AssocItemKind::TyAlias(item, CtorBlocker::new()),
                ItemKind::Const(item) => AssocItemKind::Const(item, CtorBlocker::new()),
                ItemKind::Fn(item) => AssocItemKind::Fn(item, CtorBlocker::new()),
                _ => unreachable!("only static and `TyAlias`, `Const` and `Fn` items can be found by an impl ref item"),
            };
        }

        let impl_item = self.rustc_cx.hir().impl_item(rustc_item.id);
        let data = CommonItemData::builder()
            .id(id)
            .span(self.to_span_id(rustc_item.span))
            .vis(self.to_visibility(rustc_item.id.owner_id.def_id, impl_item.vis_span))
            .ident(self.to_ident(rustc_item.ident))
            .build();

        let item = match &impl_item.kind {
            hir::ImplItemKind::Const(ty, body_id) => AssocItemKind::Const(
                self.alloc(ConstItem::new(
                    data,
                    self.to_syn_ty(ty),
                    Some(self.to_body_id(*body_id)),
                )),
                CtorBlocker::new(),
            ),
            hir::ImplItemKind::Fn(fn_sig, body_id) => AssocItemKind::Fn(
                self.alloc(self.to_fn_item(
                    data,
                    impl_item.generics,
                    fn_sig,
                    false,
                    hir::TraitFn::Provided(*body_id),
                )),
                CtorBlocker::new(),
            ),
            hir::ImplItemKind::Type(ty) => AssocItemKind::TyAlias(
                self.alloc({
                    TyAliasItem::new(
                        data,
                        self.to_syn_generic_params(impl_item.generics),
                        &[],
                        Some(self.to_syn_ty(ty)),
                    )
                }),
                CtorBlocker::new(),
            ),
        };

        self.items.borrow_mut().insert(id, item.as_item());
        item
    }

    pub fn to_body(&self, body: &hir::Body<'tcx>) -> &'ast Body<'ast> {
        // Caching check first
        let id = self.to_body_id(body.id());
        if let Some(&body) = self.bodies.borrow().get(&id) {
            return body;
        }

        // Yield expressions are currently unstable
        if let Some(hir::CoroutineKind::Coroutine) = body.coroutine_kind {
            return self.alloc(Body::new(
                self.to_item_id(self.rustc_cx.hir().body_owner_def_id(body.id())),
                ast::ExprKind::Unstable(self.alloc(ast::UnstableExpr::new(
                    ast::CommonExprData::new(self.to_expr_id(body.value.hir_id), self.to_span_id(body.value.span)),
                    ast::ExprPrecedence::Unstable(0),
                ))),
            ));
        }

        self.with_body(body.id(), || {
            let owner = self.to_item_id(self.rustc_cx.hir().body_owner_def_id(body.id()));
            let api_body = self.alloc(Body::new(owner, self.to_expr(body.value)));
            self.bodies.borrow_mut().insert(id, api_body);
            api_body
        })
    }
}
