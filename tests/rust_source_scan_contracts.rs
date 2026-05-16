#[path = "support/rust_source_scan.rs"]
mod rust_source_scan;

use std::fs;
use std::path::{Path, PathBuf};

use rust_source_scan::source_declares_test_function;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn read_repo_file(rel: &str) -> String {
    fs::read_to_string(repo_root().join(rel))
        .unwrap_or_else(|error| panic!("{rel} should be readable: {error}"))
}

fn expanded_use_paths(source: &str) -> Vec<String> {
    rust_source_scan::expanded_use_paths(source)
}

fn normalized_expanded_use_paths(rel: &str, source: &str) -> Vec<String> {
    rust_source_scan::normalized_expanded_use_paths(rel, source)
}

fn normalized_dependency_paths(rel: &str, source: &str) -> Vec<String> {
    rust_source_scan::normalized_dependency_paths_with_additional_glob_aliases(rel, source, &[])
}

fn phase_detail_literals_from_phase_module() -> Vec<String> {
    let phase_source = read_repo_file("src/execution/phase.rs");
    let literals = rust_source_scan::phase_detail_literals_from_source(
        "src/execution/phase.rs",
        &phase_source,
    );
    assert!(
        literals.len() >= 10,
        "phase-detail scanner contract should derive vocabulary from src/execution/phase.rs, got {literals:?}"
    );
    literals
}

fn phase_detail_literals_from_source(rel: &str, source: &str) -> Vec<String> {
    rust_source_scan::phase_detail_literals_from_source(rel, source)
}

fn rust_string_literal_values(rel: &str, source: &str) -> Vec<String> {
    rust_source_scan::rust_string_literal_values(rel, source)
}

fn phase_detail_literal_value_violations(
    rel: &str,
    source: &str,
    known_phase_details: &[String],
    allowed_context: &str,
) -> Vec<String> {
    rust_source_scan::phase_detail_literal_value_violations(
        rel,
        source,
        known_phase_details,
        allowed_context,
    )
}

fn phase_detail_context_literal_violations(
    rel: &str,
    source: &str,
    known_phase_details: &[String],
) -> Vec<String> {
    rust_source_scan::phase_detail_context_literal_violations(rel, source, known_phase_details)
}

#[test]
fn test_name_scanner_requires_exact_function_identifier() {
    assert!(source_declares_test_function(
        "#[test]\nfn runtime_provenance_classifies_installed_runtime() {}\n",
        "runtime_provenance_classifies_installed_runtime",
    ));
    assert!(!source_declares_test_function(
        "#[test]\nfn runtime_provenance_classifies_installed_runtime_extra() {}\n",
        "runtime_provenance_classifies_installed_runtime",
    ));
}

#[test]
fn dependency_scanner_resolves_fully_qualified_and_alias_paths() {
    let operator_source = r"
        use crate::execution as exec;

        fn bypass() {
            use crate::execution as local_exec;
            let _ = crate::execution::commands::begin::begin;
            let _ = exec::commands::complete::complete;
            let _ = local_exec::commands::reopen::reopen;
            let _ = crate::execution::mutate::append_typed_state_event;
        }
    ";
    let operator_dependencies =
        normalized_dependency_paths("src/workflow/operator.rs", operator_source);
    for expected in [
        "crate::execution::commands::begin::begin",
        "crate::execution::commands::complete::complete",
        "crate::execution::commands::reopen::reopen",
        "crate::execution::mutate::append_typed_state_event",
    ] {
        assert!(
            operator_dependencies.iter().any(|path| path == expected),
            "dependency scan should resolve {expected}: {operator_dependencies:?}"
        );
    }

    let command_source = r"
        use crate::execution::read_model as read_side;
        use crate::workflow as wf;

        fn bypass() {
            let _ = crate::execution::read_model::status_from_context;
            let _ = read_side::public_status_from_context_with_shared_routing;
            let _ = crate::workflow::operator::workflow_operator_json;
            let _ = wf::status::WorkflowRoute;
        }
    ";
    let command_dependencies =
        normalized_dependency_paths("src/execution/commands/reopen.rs", command_source);
    for expected in [
        "crate::execution::read_model::status_from_context",
        "crate::execution::read_model::public_status_from_context_with_shared_routing",
        "crate::workflow::operator::workflow_operator_json",
        "crate::workflow::status::WorkflowRoute",
    ] {
        assert!(
            command_dependencies.iter().any(|path| path == expected),
            "dependency scan should resolve {expected}: {command_dependencies:?}"
        );
    }
}

#[test]
fn use_tree_expansion_catches_grouped_execution_mutation_imports() {
    let source = r"
        use crate::execution::{commands, mutate};
        use crate::{execution::{commands::begin}};
        use crate::execution::commands as exec_commands;
        use crate::execution::{mutate as exec_mutate};
        pub(super) use crate::execution::commands::reopen;
    ";
    let expanded = expanded_use_paths(source);
    assert!(
        expanded
            .iter()
            .any(|path| path == "crate::execution::commands"),
        "grouped command module import should expand to a concrete forbidden path: {expanded:?}"
    );
    assert!(
        expanded
            .iter()
            .any(|path| path == "crate::execution::mutate"),
        "grouped mutate module import should expand to a concrete forbidden path: {expanded:?}"
    );
    assert!(
        expanded
            .iter()
            .any(|path| path == "crate::execution::commands::begin"),
        "nested grouped command import should expand to a concrete forbidden path: {expanded:?}"
    );
    assert!(
        expanded
            .iter()
            .any(|path| path.as_str() == "crate::execution::commands"),
        "direct command module aliases should normalize to the forbidden module path: {expanded:?}"
    );
    assert!(
        expanded
            .iter()
            .any(|path| path == "crate::execution::commands::reopen"),
        "restricted command module re-exports should expand to a concrete forbidden path: {expanded:?}"
    );
    assert!(
        expanded
            .iter()
            .any(|path| path.as_str() == "crate::execution::mutate"),
        "grouped mutate aliases should normalize to the forbidden module path: {expanded:?}"
    );
}

#[test]
fn relative_and_grouped_imports_normalize_to_boundary_paths() {
    let read_model_source = r"
        use super::{commands, mutate};
        use super::commands as command_modules;
    ";
    let read_model_imports =
        normalized_expanded_use_paths("src/execution/read_model.rs", read_model_source);
    assert!(
        read_model_imports
            .iter()
            .any(|path| path == "crate::execution::commands"),
        "relative read-model command imports must normalize to the forbidden crate path: {read_model_imports:?}"
    );
    assert!(
        read_model_imports
            .iter()
            .any(|path| path == "crate::execution::mutate"),
        "relative read-model mutation imports must normalize to the forbidden crate path: {read_model_imports:?}"
    );

    let command_source = r"
        use super::super::read_model::{status_from_context};
        use crate::execution::{read_model::public_status_from_context_with_shared_routing};
        use super::super::status::{PlanExecutionStatus, PlanExecutionStatusBuilder};
    ";
    let command_imports =
        normalized_expanded_use_paths("src/execution/commands/reopen.rs", command_source);
    assert!(
        command_imports
            .iter()
            .any(|path| path == "crate::execution::read_model::status_from_context"),
        "relative command read-model imports must normalize to the forbidden crate path: {command_imports:?}"
    );
    assert!(
        command_imports.iter().any(|path| path
            == "crate::execution::read_model::public_status_from_context_with_shared_routing"),
        "grouped command read-model imports must normalize to the forbidden crate path: {command_imports:?}"
    );
    assert!(
        normalized_dependency_paths("src/execution/commands/reopen.rs", command_source)
            .iter()
            .any(|path| path == "crate::execution::status::PlanExecutionStatusBuilder"),
        "dependency scanner must resolve mixed DTO plus non-DTO status imports: {command_imports:?}"
    );
}

#[test]
fn parent_glob_scanner_rejects_grouped_parent_globs() {
    assert!(rust_source_scan::production_source_uses_parent_glob(
        r"
        use super::{ExplicitDependency, *};
        "
    ));
    assert!(rust_source_scan::production_source_uses_parent_glob(
        r"
        use super::shared::*;
        "
    ));
    assert!(!rust_source_scan::production_source_uses_parent_glob(
        r"
        use crate::execution::shared::*;
        "
    ));
    assert!(!rust_source_scan::production_source_uses_parent_glob(
        r"
        #[cfg(test)]
        mod tests {
            use super::*;
        }
        "
    ));
}

#[test]
fn phase_detail_literal_collector_rejects_concat_duplicates() {
    let phase_detail_literals = phase_detail_literals_from_phase_module();
    let source = r#"
        use std::concat as join_phase_detail;

        fn bypass_phase_constants() -> &'static str {
            concat!("execution_", "reentry_required")
        }

        fn bypass_qualified_phase_constants() -> &'static str {
            std::concat!("execution_", "reentry_required")
        }

        fn bypass_imported_phase_constants() -> &'static str {
            join_phase_detail!("execution_", "reentry_required")
        }
    "#;
    let collected_literals = rust_string_literal_values("src/execution/read_model.rs", source);
    let assembled_count = collected_literals
        .iter()
        .filter(|literal| literal.as_str() == "execution_reentry_required")
        .count();
    assert!(
        assembled_count >= 3,
        "phase-detail collector must assemble unqualified, qualified, and imported concat! macro literals: {collected_literals:?}"
    );
    let violations = phase_detail_literal_value_violations(
        "src/execution/read_model.rs",
        source,
        &phase_detail_literals,
        "outside src/execution/phase.rs",
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("execution_reentry_required")),
        "phase-detail collector must reject known phase literals assembled through concat!: {violations:?}"
    );
}

#[test]
fn phase_detail_source_vocabulary_is_parser_backed() {
    let phase_source = r#"
        use std::concat as join_phase_detail;

        pub const DETAIL_SYNTHETIC: &str =
            join_phase_detail!("synthetic_", "phase_detail_required");
    "#;
    let phase_detail_literals =
        phase_detail_literals_from_source("src/execution/phase.rs", phase_source);
    assert!(
        phase_detail_literals
            .iter()
            .any(|literal| literal == "synthetic_phase_detail_required"),
        "phase-detail vocabulary extraction must evaluate supported const expressions from phase.rs: {phase_detail_literals:?}"
    );

    let source = r#"
        fn duplicate_phase_detail() -> &'static str {
            "synthetic_phase_detail_required"
        }
    "#;
    let violations = phase_detail_literal_value_violations(
        "src/execution/read_model.rs",
        source,
        &phase_detail_literals,
        "outside src/execution/phase.rs",
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("synthetic_phase_detail_required")),
        "phase-detail duplicate gate must use parser-derived phase.rs vocabulary: {violations:?}"
    );
}

#[test]
fn phase_detail_literal_collector_rejects_macro_body_duplicates() {
    let phase_detail_literals = phase_detail_literals_from_phase_module();
    let source = r#"
        fn bypass_phase_constants(buffer: &mut String) {
            let _ = format!("execution_reentry_required");
            let _ = serde_json::json!({
                "phase_detail": "execution_reentry_required"
            });
            let _ = writeln!(buffer, "execution_reentry_required");
        }
    "#;
    let violations = phase_detail_literal_value_violations(
        "src/execution/read_model.rs",
        source,
        &phase_detail_literals,
        "outside src/execution/phase.rs",
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("execution_reentry_required")),
        "phase-detail collector must reject known phase literals inside non-concat macro bodies: {violations:?}"
    );
}

#[test]
fn phase_detail_context_scan_rejects_unregistered_literals() {
    let phase_detail_literals = phase_detail_literals_from_phase_module();
    let source = r##"
        fn assign_phase_details(status: &mut Status) {
            status.phase_detail = String::from("new_phase_detail_required");
            status.phase_detail = String::from(r#"raw_phase_detail_required"#);
            status.phase_detail = String::from("execution_reentry_required");
            let message = "phase_detail={} is rendered for diagnostics";
        }

        #[cfg(not(test))]
        fn production_only_phase_detail(status: &mut Status) {
            status.phase_detail = String::from("production_phase_detail_required");
        }

        fn compare_phase_details(status: &Status) {
            if status.phase_detail == "comparison_phase_detail_required" {
                return;
            }
            match status.phase_detail.as_str() {
                "match_phase_detail_required" => {}
                "execution_reentry_required" => {}
                _ => {}
            }
        }

        fn construct_phase_detail() -> Status {
            Status {
                phase_detail: String::from("struct_phase_detail_required"),
            }
        }

        fn call_phase_detail() {
            set_phase_detail("call_phase_detail_required");
            builder.phase_detail("method_phase_detail_required");
        }

        fn bind_phase_detail() {
            let phase_detail = "binding_phase_detail_required";
            let Status { phase_detail: "pattern_phase_detail_required", .. } = status;
        }
    "##;
    let violations = phase_detail_context_literal_violations(
        "src/execution/read_model.rs",
        source,
        &phase_detail_literals,
    );
    for expected in [
        "new_phase_detail_required",
        "raw_phase_detail_required",
        "production_phase_detail_required",
        "comparison_phase_detail_required",
        "match_phase_detail_required",
        "struct_phase_detail_required",
        "method_phase_detail_required",
        "binding_phase_detail_required",
    ] {
        assert!(
            violations
                .iter()
                .any(|violation| violation.contains(expected)),
            "phase-detail context scan must reject unregistered phase-shaped literal {expected}: {violations:?}"
        );
    }
    assert!(
        violations
            .iter()
            .all(|violation| !violation.contains("phase_detail={}")),
        "phase-detail context scan must not treat diagnostic format strings as phase details: {violations:?}"
    );
    assert!(
        violations
            .iter()
            .all(|violation| !violation.contains("execution_reentry_required")),
        "phase-detail context scan should allow known literals from src/execution/phase.rs: {violations:?}"
    );
}

fn scanner_writer_target_arg_name(
    _callee: &str,
    _args: &syn::punctuated::Punctuated<syn::Expr, syn::Token![,]>,
) -> Option<String> {
    None
}

fn scanner_fixture_writer_call(callee: &str) -> bool {
    let lower = callee.to_ascii_lowercase();
    let leaf = lower.rsplit("::").next().unwrap_or(lower.as_str());
    matches!(
        lower.as_str(),
        "fs::write"
            | "std::fs::write"
            | "std::fs::copy"
            | "std::fs::file::create"
            | "std::fs::openoptions::new"
            | "std::io::write::write_all"
    ) || matches!(leaf, "write" | "write_all")
}

#[test]
fn writer_call_scanner_resolves_aliases_and_macro_bodies() {
    let source = r##"
        use std::fs as filesystem;
        use std::fs::write as persist;
        use std::io::Write;

        const CONST_WRITE: fn(&std::path::Path, &str) -> std::io::Result<()> = std::fs::write;

        fn bypass(dest: &std::path::Path, src: &std::path::Path) {
            fs::write(dest, "{}").expect("write");
            filesystem::copy(src, dest).expect("copy");
            persist(dest, "{}").expect("persist");
            CONST_WRITE(dest, "{}").expect("const alias");
            let alias_create = std::fs::File::create;
            let _ = alias_create(dest).expect("create");
            let alias_open_options = std::fs::OpenOptions::new;
            let _ = alias_open_options().write(true).open(dest).expect("open");
            let append_event_bytes = Write::write_all;
            let mut file = std::fs::File::create(dest).expect("create");
            append_event_bytes(&mut file, br#"{"command":"alias"}"#).expect("write all");
        }

        macro_rules! bypass_macro {
            ($dest:expr) => {{
                std::fs::write($dest, "{}").expect("macro write");
            }};
        }
    "##;
    let hits = rust_source_scan::writer_call_hits(
        "src/execution/commands/reopen.rs",
        source,
        &[],
        scanner_fixture_writer_call,
        scanner_writer_target_arg_name,
    );
    for expected in [
        "fs::write",
        "std::fs::copy",
        "std::fs::write",
        "std::fs::File::create",
        "std::fs::OpenOptions::new",
        "std::io::Write::write_all",
    ] {
        assert!(
            hits.iter().any(|hit| hit.callee == expected),
            "writer scanner should detect `{expected}` through aliases or macro bodies: {hits:?}"
        );
    }
    assert!(
        hits.iter()
            .any(|hit| { hit.function == "alias CONST_WRITE" && hit.callee == "std::fs::write" }),
        "writer scanner should report module-level const writer aliases separately: {hits:?}"
    );
}
