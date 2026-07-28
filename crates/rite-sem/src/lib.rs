//! Semantic analysis: resolver, desugaring, modules, and IR.

pub mod desugar;
pub mod ir;
pub mod modules;
pub mod resolve;

pub use desugar::desugar_program;
pub use ir::*;
pub use modules::{resolve_module_path, LoadedModule, ModuleGraph, ModuleLoader};
pub use resolve::{resolve, ResolvedProgram, Resolver};

use rite_core::{Diagnostics, SourceFile, SourceMap};
use rite_syntax::parse_file;
use std::path::{Path, PathBuf};

/// Full front-end pipeline: parse → modules → resolve → desugar → IR.
pub fn compile_to_ir(file: &SourceFile) -> (Option<ProgramIr>, Diagnostics) {
    compile_to_ir_with_roots(file, None, &[])
}

pub fn compile_to_ir_with_roots(
    file: &SourceFile,
    entry_path: Option<&Path>,
    roots: &[PathBuf],
) -> (Option<ProgramIr>, Diagnostics) {
    let mut sources = SourceMap::new();
    // Keep entry text available for diagnostics in loader
    let _ = sources.add_file(&file.name, file.as_str());

    let mut loader = ModuleLoader::new(&mut sources, roots.to_vec());
    let mut ast = match loader.load_entry(file, entry_path) {
        Some(a) => a,
        None => {
            let (ast, diags) = parse_file(file);
            let mut all = loader.diagnostics;
            all.extend(diags.into_vec());
            return (ast.and_then(|_| None), all);
        }
    };
    let (graph, mut load_diags) = loader.into_graph();
    modules::merge_exports_into_entry(&mut ast, &graph, &mut load_diags);

    let (resolved, rdiags) = resolve(&ast, file);
    load_diags.extend(rdiags.into_vec());
    if load_diags.has_errors() {
        return (None, load_diags);
    }
    let mut ir = desugar_program(&resolved);

    // Append module-only top-level statements? Functions already merged.
    // Record module names for runtime
    for name in &graph.load_order {
        ir.modules.push(ModuleIr {
            name: name.clone(),
            statements: vec![],
            exports: graph
                .modules
                .get(name)
                .map(|m| m.exports.keys().cloned().collect())
                .unwrap_or_default(),
        });
    }

    (Some(ir), load_diags)
}

pub fn compile_source(name: &str, text: &str) -> (Option<ProgramIr>, Diagnostics, SourceMap) {
    let mut sources = SourceMap::new();
    let id = sources.add_file(name, text);
    let file = sources.get(id).unwrap().clone();
    let path = PathBuf::from(name);
    let (ir, diags) = if path.exists() {
        compile_to_ir_with_roots(&file, Some(&path), &[])
    } else {
        compile_to_ir(&file)
    };
    (ir, diags, sources)
}

/// Compile a path from disk (enables relative module resolution).
pub fn compile_path(path: &Path) -> (Option<ProgramIr>, Diagnostics, SourceMap) {
    let mut sources = SourceMap::new();
    let id = match sources.add_path(path) {
        Ok(id) => id,
        Err(e) => {
            let mut d = Diagnostics::new();
            d.push(rite_core::Diagnostic::error(
                rite_core::E080_IO,
                format!("cannot read {}: {}", path.display(), e),
            ));
            return (None, d, sources);
        }
    };
    let file = sources.get(id).unwrap().clone();
    let roots = path
        .parent()
        .map(|p| vec![p.to_path_buf()])
        .unwrap_or_default();
    let (ir, diags) = compile_to_ir_with_roots(&file, Some(path), &roots);
    (ir, diags, sources)
}
