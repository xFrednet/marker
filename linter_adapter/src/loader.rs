use std::panic::RefUnwindSafe;

use libloading::Library;

use linter_api::ast::item::{ExternCrateItem, ModItem, UseDeclItem};
use linter_api::context::AstContext;
use linter_api::interface::{LintPassDeclaration, LintPassRegistry, PanicInfo};
use linter_api::LintPass;

#[derive(Default)]
pub struct ExternalLintCrateRegistry<'ast> {
    lint_passes: Vec<Box<dyn LintPass<'ast>>>,
    invalid_lint_passes: Vec<Box<dyn LintPass<'ast>>>,
    _libs: Vec<Library>,
}

impl<'a> ExternalLintCrateRegistry<'a> {
    /// # Errors
    /// This can return errors if the library couldn't be found or if the
    /// required symbols weren't provided.
    pub fn load_external_lib(&mut self, lib_path: &str) -> Result<(), LoadingError> {
        let lib = unsafe { Library::new(lib_path) }.map_err(|_| LoadingError::FileNotFound)?;

        let decl = unsafe {
            lib.get::<*mut LintPassDeclaration>(b"__lint_pass_declaration\0")
                .map_err(|_| LoadingError::MissingLintDeclaration)?
                .read()
        };

        if decl.linter_api_version != linter_api::LINTER_API_VERSION || decl.rustc_version != linter_api::RUSTC_VERSION
        {
            return Err(LoadingError::IncompatibleVersion);
        }

        unsafe {
            (decl.register)(self);
        }

        self._libs.push(lib);

        Ok(())
    }

    /// # Panics
    ///
    /// Panics if a lint in the environment couln't be loaded.
    pub fn new_from_env() -> Self {
        let mut new_self = Self::default();

        if let Ok(lint_crates_lst) = std::env::var("LINTER_LINT_CRATES") {
            for lint_crate in lint_crates_lst.split(';') {
                if let Err(err) = new_self.load_external_lib(lint_crate) {
                    panic!("Unable to load `{lint_crate}`, reason: {err:?}");
                }
            }
        }

        new_self
    }

    fn for_each_lint_pass<T: PanicInfo<'a> + Copy + RefUnwindSafe>(&mut self, call: impl Fn(&mut dyn LintPass, T) + RefUnwindSafe, node: T) {
        let mut invalid = vec![];
        // self.lint_passes.retain_mut(|lint_pass|)
        for index in 0..self.lint_passes.len() {
            let catch = std::panic::catch_unwind(|| {
                let mut lint_pass = self.lint_passes[index].as_mut();
                call(lint_pass, node);
            });
            if catch.is_err() {
                invalid.push(index);
            }
        }
        for index in invalid {
            self.invalid_lint_passes.push(self.lint_passes.remove(index))
        }
    }
}

impl<'ast> LintPassRegistry<'ast> for ExternalLintCrateRegistry<'ast> {
    fn register(&mut self, _name: &str, init: Box<dyn LintPass<'ast>>) {
        self.lint_passes.push(init);
    }
}

impl<'ast> LintPass<'ast> for ExternalLintCrateRegistry<'ast> {
    fn registered_lints(&self) -> Vec<&'static linter_api::lint::Lint> {
        let mut all_lints = vec![];
        self.lint_passes
            .iter()
            .for_each(|pass| all_lints.append(&mut pass.registered_lints()));
        all_lints
    }

    fn check_item(&mut self, cx: &'ast AstContext<'ast>, item: linter_api::ast::item::ItemType<'ast>) {
        for lint_pass in self.lint_passes.iter_mut() {
            lint_pass.check_item(cx, item);
        }
    }

    fn check_mod(&mut self, cx: &'ast AstContext<'ast>, mod_item: &'ast dyn ModItem<'ast>) {
        for lint_pass in self.lint_passes.iter_mut() {
            lint_pass.check_mod(cx, mod_item);
        }
    }
    fn check_extern_crate(&mut self, cx: &'ast AstContext<'ast>, extern_crate_item: &'ast dyn ExternCrateItem<'ast>) {
        for lint_pass in self.lint_passes.iter_mut() {
            lint_pass.check_extern_crate(cx, extern_crate_item);
        }
    }
    fn check_use_decl(&mut self, cx: &'ast AstContext<'ast>, use_item: &'ast dyn UseDeclItem<'ast>) {
        for lint_pass in self.lint_passes.iter_mut() {
            lint_pass.check_use_decl(cx, use_item);
        }
    }
}

#[derive(Debug)]
pub enum LoadingError {
    FileNotFound,
    IncompatibleVersion,
    MissingLintDeclaration,
}
