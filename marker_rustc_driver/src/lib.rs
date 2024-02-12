#![doc = include_str!("../README.md")]
#![feature(rustc_private)]
#![feature(let_chains)]
#![feature(lint_reasons)]
#![feature(iter_collect_into)]
#![feature(non_exhaustive_omitted_patterns_lint)]
#![feature(once_cell_try)]
#![warn(rustc::internal)]
#![warn(clippy::pedantic)]
#![warn(non_exhaustive_omitted_patterns)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::wildcard_imports)]
#![allow(clippy::needless_lifetimes, reason = "lifetimes will be required to fix ICEs")]
#![allow(clippy::needless_collect, reason = "false positives for `alloc_slice`")]
#![allow(clippy::too_many_lines, reason = "long functions are unavoidable for matches")]
#![allow(rustdoc::private_intra_doc_links)]

extern crate rustc_ast;
extern crate rustc_data_structures;
extern crate rustc_driver;
extern crate rustc_errors;
extern crate rustc_hash;
extern crate rustc_hir;
extern crate rustc_hir_analysis;
extern crate rustc_interface;
extern crate rustc_lint;
extern crate rustc_lint_defs;
extern crate rustc_middle;
extern crate rustc_session;
extern crate rustc_span;
extern crate rustc_target;

pub mod context;
pub mod conversion;
pub mod lint_pass;

use std::env;
use std::ops::Deref;

use camino::Utf8Path;
use marker_adapter::{LintCrateInfo, LINT_CRATES_ENV};
use marker_error::Context;

use crate::conversion::rustc::RustcConverter;

// region replace rust toolchain dev
const RUSTC_TOOLCHAIN_VERSION: &str = "nightly-2024-02-08";
// endregion replace rust toolchain dev

pub const MARKER_SYSROOT_ENV: &str = "MARKER_SYSROOT";

struct DefaultCallbacks {
    env_vars: Vec<&'static str>,
}
impl rustc_driver::Callbacks for DefaultCallbacks {
    fn config(&mut self, config: &mut rustc_interface::Config) {
        let env_vars = std::mem::take(&mut self.env_vars);
        config.parse_sess_created = Some(Box::new(move |sess| {
            register_tracked_env(sess, &env_vars);
        }));
    }
}

struct MarkerCallback {
    env_vars: Vec<&'static str>,
    lint_crates: Vec<LintCrateInfo>,
}

impl rustc_driver::Callbacks for MarkerCallback {
    fn config(&mut self, config: &mut rustc_interface::Config) {
        let env_vars = std::mem::take(&mut self.env_vars);
        let lint_crates = self.lint_crates.clone();
        config.parse_sess_created = Some(Box::new(move |sess| {
            register_tracked_env(sess, &env_vars);
            register_tracked_files(sess, &lint_crates);
        }));

        // Clippy explicitly calls any previous `register_lints` functions. This
        // will not be done here to keep it simple and to ensure that only known
        // code is executed.
        assert!(config.register_lints.is_none());
        let lint_crates = std::mem::take(&mut self.lint_crates);

        config.register_lints = Some(Box::new(move |_sess, lint_store| {
            // It looks like it can happen, that the `config` function is called
            // with a different thread than the actual lint pass later, how interesting.
            // This will not make sure that the adapter is always initiated.
            if let Err(err) = lint_pass::RustcLintPass::init_adapter(&lint_crates) {
                err.print();
                // FIXME: we need to figure out the way to run the initialization code
                // earlier where we can cleanly report an error and exit the process.
                std::process::exit(1);
            }
            // Register lints from lint crates. This is required to have rustc track
            // the lint level correctly.
            let lints: Vec<_> = lint_pass::RustcLintPass::marker_lints()
                .into_iter()
                .map(RustcConverter::static_to_lint)
                .collect();

            lint_store.register_lints(&lints);

            lint_store.register_late_pass(|_| Box::new(lint_pass::RustcLintPass));
        }));
    }
}

fn register_tracked_env(sess: &mut rustc_session::parse::ParseSess, vars: &[&'static str]) {
    use rustc_span::Symbol;
    let env = sess.env_depinfo.get_mut();

    for key in vars {
        env.insert((
            Symbol::intern(key),
            Some(Symbol::intern(&std::env::var(key).unwrap_or_default())),
        ));
    }
}

fn register_tracked_files(sess: &mut rustc_session::parse::ParseSess, lint_crates: &[LintCrateInfo]) {
    use rustc_span::Symbol;

    let files = sess.file_depinfo.get_mut();

    // Cargo sets the current directory, to the folder containing the `Cargo.toml`
    // file of the compiled crate. Therefore, we can use the relative path.
    let cargo_file = Utf8Path::new("./Cargo.toml");
    if cargo_file.exists() {
        files.insert(Symbol::intern("./Cargo.toml"));
    }

    // At this point, the lint crate paths should be absolute. Therefore, we
    // can just throw it in the `files` set.
    for lint_crate in lint_crates {
        files.insert(Symbol::intern(lint_crate.path.as_str()));
    }

    // Track the driver executable in debug builds
    #[cfg(debug_assertions)]
    match env::current_exe().as_ref().map(|path| path.to_str()) {
        Ok(Some(current_exe)) => {
            files.insert(Symbol::intern(current_exe));
        },
        Ok(None) => {
            // The path is not valid UTF-8. We can simply ignore this case
        },
        Err(e) => eprintln!("getting the path of the current executable failed {e:#?}"),
    }
}

/// If a command-line option matches `find_arg`, then apply the predicate `pred` on its value. If
/// `true`, then return it. The parameter is assumed to be either `--arg=value` or `--arg value`.
fn arg_value<'a, T: Deref<Target = str>>(
    args: &'a [T],
    find_arg: &str,
    pred: impl Fn(&str) -> bool,
) -> Option<&'a str> {
    let mut args = args.iter().map(Deref::deref);
    while let Some(arg) = args.next() {
        let mut arg = arg.splitn(2, '=');
        if arg.next() != Some(find_arg) {
            continue;
        }

        match arg.next().or_else(|| args.next()) {
            Some(v) if pred(v) => return Some(v),
            _ => {},
        }
    }
    None
}

fn display_help() {
    println!(
        "\
Marker's rustc driver to run your lint crates.

Usage:
    marker_rustc_driver [options] [--] [<opts>...]

Common options:
    -h, --help               Print this message
        --rustc              Pass all arguments to rustc
    -V, --version            Print version information and exit
        --toolchain          Print the required toolchain and API version

---

This message belongs to a specific marker driver, if possible you should avoid
interfacing with the driver directly and use `cargo marker` instead.
"
    );
}

#[allow(clippy::missing_errors_doc)]
pub fn try_main(args: impl Iterator<Item = String>) -> Result<(), MainError> {
    // Note: This driver has two different kinds of "arguments".
    // 1. Normal arguments, passed directly to the binary. (Collected below)
    // 2. Arguments provided to `cargo-marker` which are forwarded to this process as environment
    //    values. These are usually driver-independent and handled by the adapter.
    let mut orig_args: Vec<String> = args.collect();

    // Rustc will by default use the location of the rustc library as the system
    // root. Custom drivers, like Clippy, rustdoc, and Miri have options to override
    // this directory. This seems to be required for cross compiling and cases where
    // the driver is invoked directly.
    //
    // I haven't found a definite explanation which circumstances require it, but
    // it's probably best to support the same behavior. This code is an adaptation
    // of Clippy's solution, with the deviation, that we add the `--sysroot` directly
    // and have a `MARKER_` prefix for the `SYSROOT` environment value. This is similar
    // to Miri's `MIRI_SYSROOT` variable.
    //
    // The `ui_test` crate used by `marker_uitest` seems to also fiddle around with
    // the system root, if custom dependencies are defined.
    if arg_value(&orig_args, "--sysroot", |_| true).is_none()
        && let Ok(env_sys_root) = std::env::var(MARKER_SYSROOT_ENV)
    {
        orig_args.extend(vec!["--sysroot".into(), env_sys_root]);
    }

    // make "marker_rustc_driver --rustc" work like a subcommand that passes
    // all args to "rustc" for example `marker_rustc_driver --rustc --version`
    // will print the rustc version that is used
    if let Some(pos) = orig_args.iter().position(|arg| arg == "--rustc") {
        orig_args.remove(pos);
        orig_args[0] = "rustc".to_string();

        rustc_driver::RunCompiler::new(&orig_args, &mut DefaultCallbacks { env_vars: vec![] }).run()?;

        return Ok(());
    }

    if orig_args.iter().any(|a| a == "--version" || a == "-V") {
        let version_info = rustc_tools_util::get_version_info!();
        println!("{version_info}");
        return Ok(());
    }

    if orig_args.iter().any(|a| a == "--toolchain") {
        println!("toolchain: {RUSTC_TOOLCHAIN_VERSION}");
        println!("driver: {}", env!("CARGO_PKG_VERSION"));
        println!("marker-api: {}", marker_api::MARKER_API_VERSION);

        return Ok(());
    }

    // Setting RUSTC_WRAPPER causes Cargo to pass 'rustc' as the first argument.
    // We're invoking the compiler programmatically, so we'll ignore this.
    let wrapper_mode = orig_args.get(1).map(Utf8Path::new).and_then(Utf8Path::file_stem) == Some("rustc");

    if wrapper_mode {
        // we still want to be able to invoke rustc normally
        orig_args.remove(1);
    }

    // The `--rustc` argument has already been handled, therefore we can just
    // check for this argument without caring about the position.
    if !wrapper_mode && orig_args.iter().any(|a| a == "--help" || a == "-h") {
        display_help();
        return Ok(());
    }

    // We enable Marker if one of the following conditions is met
    // - IF Marker is run on the main crate, not on deps (`!cap_lints_allow`) THEN
    //    - IF `--no-deps` is not set (`!no_deps`) OR
    //    - IF `--no-deps` is set and Marker is run on the specified primary package
    let cap_lints_allow = arg_value(&orig_args, "--cap-lints", |val| val == "allow").is_some()
        && arg_value(&orig_args, "--force-warn", |_| true).is_none();
    let no_deps = orig_args.iter().any(|arg| arg == "--no-deps");
    let in_primary_package = env::var("CARGO_PRIMARY_PACKAGE").is_ok();

    let enable_marker = !cap_lints_allow && (!no_deps || in_primary_package);
    let env_vars = vec![LINT_CRATES_ENV, MARKER_SYSROOT_ENV];
    if !enable_marker {
        rustc_driver::RunCompiler::new(&orig_args, &mut DefaultCallbacks { env_vars }).run()?;
        return Ok(());
    }

    let lint_crates = LintCrateInfo::list_from_env()
        .context(|| "Error while determining the lint crates to load")?
        .unwrap_or_default();

    let additional_args = [
        // Make it possible to use `#[allow(marker::{lint_name})]` without
        // having to add `#![feature(register_tool)]` and `#![register_tool(marker)]`.
        "-Zcrate-attr=feature(register_tool)",
        "-Zcrate-attr=register_tool(marker)",
        // We need to provide a marker cfg flag to allow conditional compilation,
        // we add a simple `marker` config for the common use case, but also provide
        // `marker=crate_name` for more complex uses
        "--cfg=marker",
    ]
    .into_iter()
    .map(str::to_string)
    .chain(
        lint_crates
            .iter()
            .map(|krate| format!("--cfg=marker=\"{}\"", krate.name)),
    );

    orig_args.extend(additional_args);

    let mut callback = MarkerCallback { env_vars, lint_crates };
    rustc_driver::RunCompiler::new(&orig_args, &mut callback).run()?;

    Ok(())
}

pub enum MainError {
    Custom(marker_error::Error),
    Rustc(rustc_span::ErrorGuaranteed),
}

impl From<marker_error::Error> for MainError {
    fn from(err: marker_error::Error) -> Self {
        Self::Custom(err)
    }
}

impl From<rustc_span::ErrorGuaranteed> for MainError {
    fn from(err: rustc_span::ErrorGuaranteed) -> Self {
        Self::Rustc(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arg_value() {
        let args = &["--bar=bar", "--foobar", "123", "--foo"];

        assert_eq!(arg_value(&[] as &[&str], "--foobar", |_| true), None);
        assert_eq!(arg_value(args, "--bar", |_| false), None);
        assert_eq!(arg_value(args, "--bar", |_| true), Some("bar"));
        assert_eq!(arg_value(args, "--bar", |p| p == "bar"), Some("bar"));
        assert_eq!(arg_value(args, "--bar", |p| p == "foo"), None);
        assert_eq!(arg_value(args, "--foobar", |p| p == "foo"), None);
        assert_eq!(arg_value(args, "--foobar", |p| p == "123"), Some("123"));
        assert_eq!(arg_value(args, "--foo", |_| true), None);
    }
}
