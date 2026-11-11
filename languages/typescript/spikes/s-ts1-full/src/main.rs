use std::sync::Arc;

use tsrs_compiler::{Context, ProgramOptions};
use tsrs_core::{CompilerOptions, P, tspath};
use tsrs_vfs::{FS, bundled, cachedvfs, osvfs};

fn main() {
    let cwd = std::env::args().nth(1).expect("usage: s-ts1-full <dir>");
    let file = format!("{cwd}/a.ts");
    let fs: Arc<dyn FS> = Arc::new(bundled::wrap_fs(cachedvfs::from(osvfs::fs())));
    let config = tsrs_tsoptions::new_parsed_command_line(
        P::new(CompilerOptions::default()),
        vec![file],
        Vec::new(),
        tspath::ComparePathsOptions {
            current_directory: cwd.clone(),
            use_case_sensitive_file_names: fs.use_case_sensitive_file_names(),
        },
    );
    let host = tsrs_compiler::new_compiler_host(&cwd, fs, &bundled::default_lib_path(), None, None, None);
    let program = tsrs_compiler::new_program(ProgramOptions::new(P::new(config), host));
    let ctx = Context::default();
    println!("files: {}", program.get_source_files().len());
    println!("global diagnostics: {}", program.get_global_diagnostics(&ctx).len());
}
