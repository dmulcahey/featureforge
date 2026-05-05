#[path = "support/rust_source_scan.rs"]
mod rust_source_scan;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use syn::visit::{self, Visit};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn read_repo_file(rel: &str) -> String {
    fs::read_to_string(repo_root().join(rel))
        .unwrap_or_else(|error| panic!("{rel} should be readable: {error}"))
}

fn rust_source_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rust_source_files(root, &mut files);
    files.sort();
    files
}

fn collect_rust_source_files(dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("{} should be readable: {error}", dir.display()))
    {
        let entry = entry.expect("source directory entry should be readable");
        let path = entry.path();
        if path.is_dir() {
            collect_rust_source_files(&path, files);
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("rs") {
            files.push(path);
        }
    }
}

fn repo_relative(path: &Path) -> String {
    path.strip_prefix(repo_root())
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn execution_command_sources() -> Vec<(String, String)> {
    rust_source_files(&repo_root().join("src/execution/commands"))
        .into_iter()
        .filter(|path| repo_relative(path) != "src/execution/commands/common/unit_tests.rs")
        .map(|path| {
            let rel = repo_relative(&path);
            let source = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("{rel} should be readable: {error}"));
            (rel, source)
        })
        .collect()
}

fn read_model_boundary_sources() -> Vec<(String, String)> {
    let mut rels = vec![
        String::from("src/execution/read_model.rs"),
        String::from("src/execution/read_model_support.rs"),
        String::from("src/execution/status.rs"),
    ];
    rels.extend(
        rust_source_files(&repo_root().join("src/execution/read_model"))
            .into_iter()
            .map(|path| repo_relative(&path)),
    );
    rels.sort();
    rels.dedup();
    rels.into_iter()
        .map(|rel| {
            let source = read_repo_file(&rel);
            (rel, source)
        })
        .collect()
}

#[test]
fn completed_task_closure_preemption_predicate_has_single_authoritative_definition() {
    let function_name = "completed_task_closure_preempts_execution_reentry";
    let mut definitions = rust_source_files(&repo_root().join("src/execution"))
        .into_iter()
        .filter_map(|path| {
            let rel = repo_relative(&path);
            let source = read_repo_file(&rel);
            source
                .contains(&format!("fn {function_name}("))
                .then_some(rel)
        })
        .collect::<Vec<_>>();
    definitions.sort();

    assert_eq!(
        definitions,
        vec![String::from("src/execution/repair_target_selection.rs")],
        "completed-task closure preemption is shared routing truth; define it once in repair_target_selection and call that predicate from route surfaces"
    );
    for rel in ["src/execution/next_action.rs", "src/execution/router.rs"] {
        let source = read_repo_file(rel);
        assert!(
            source.contains(&format!("{function_name}(")),
            "{rel} must call the shared completed-task closure preemption predicate"
        );
    }
}

#[test]
fn execution_reentry_close_preemption_targets_specific_task_closure() {
    let router_source = read_repo_file("src/execution/router.rs");
    assert!(
        !router_source.contains(
            "seed.phase_detail == phase::DETAIL_EXECUTION_REENTRY_REQUIRED\n            && status.current_task_closures.is_empty()"
        ),
        "execution-reentry close-current-task preemption must check whether the target task already has a current closure, not whether all task closures are absent"
    );
}

#[test]
fn review_dispatch_cycle_target_honors_public_close_route_before_stale_fallback() {
    let source = read_repo_file("src/execution/closure_dispatch.rs");
    let public_route_index = source
        .find("public_close_current_task_cycle_target(context, status)")
        .expect("closure dispatch target selection must have an explicit public close-current-task route preemption helper");
    let stale_fallback_index = source
        .find("let earliest_stale_boundary_task")
        .expect("closure dispatch target selection should still mention stale fallback targeting");
    assert!(
        public_route_index < stale_fallback_index,
        "close-current-task dispatch refresh must honor the same public route/operator task before consulting pre-reducer stale fallback targets"
    );
}

#[test]
fn execution_reentry_target_honors_earlier_resume_before_later_stale_target() {
    let source = read_repo_file("src/execution/repair_target_selection.rs");
    let resume_preemption_index = source
        .find("resume_step_preempts_later_stale_target(status, authority_inputs.authoritative_stale_target)")
        .expect("execution reentry target selection must explicitly preempt later stale reopen targets with an earlier parked resume step");
    let stale_target_index = source
        .find("if let Some(target) = authority_inputs.authoritative_stale_target")
        .expect(
            "execution reentry target selection should still consult authoritative stale targets",
        );
    assert!(
        resume_preemption_index < stale_target_index,
        "execution reentry routing must not recommend reopening a later stale target while an earlier interrupted resume step is parked"
    );
}

fn expanded_use_paths(source: &str) -> Vec<String> {
    rust_source_scan::expanded_use_paths(source)
}

fn normalized_expanded_use_paths(rel: &str, source: &str) -> Vec<String> {
    rust_source_scan::normalized_expanded_use_paths(rel, source)
}

fn parse_rust_source(rel: &str, source: &str) -> syn::File {
    rust_source_scan::parse_rust_source(rel, source)
}

fn syn_path_to_string(path: &syn::Path) -> String {
    rust_source_scan::syn_path_to_string(path)
}

fn normalize_code_path_for_source(
    rel: &str,
    path: &str,
    aliases: &BTreeMap<String, String>,
) -> String {
    rust_source_scan::normalize_code_path_for_source(rel, path, aliases)
}

fn with_command_common_aliases<T>(
    rel: &str,
    source: &str,
    scanner: impl FnOnce(&[rust_source_scan::AdditionalGlobAliasSource<'_>]) -> T,
) -> T {
    if rel.starts_with("src/execution/commands/")
        && rel != "src/execution/commands/common.rs"
        && normalized_expanded_use_paths(rel, source)
            .into_iter()
            .any(|path| path == "crate::execution::commands::common::*")
    {
        let common_source = read_repo_file("src/execution/commands/common.rs");
        let additional = [rust_source_scan::AdditionalGlobAliasSource {
            glob_path: "crate::execution::commands::common::*",
            source_rel: "src/execution/commands/common.rs",
            source: &common_source,
        }];
        scanner(&additional)
    } else {
        scanner(&[])
    }
}

fn aliases_for_source(rel: &str, source: &str, syntax: &syn::File) -> BTreeMap<String, String> {
    with_command_common_aliases(rel, source, |additional| {
        rust_source_scan::aliases_for_source(rel, source, syntax, additional)
    })
}

fn normalized_code_paths(rel: &str, source: &str) -> Vec<String> {
    with_command_common_aliases(rel, source, |additional| {
        rust_source_scan::normalized_code_paths_with_additional_glob_aliases(
            rel, source, additional,
        )
    })
}

fn normalized_dependency_paths(rel: &str, source: &str) -> Vec<String> {
    with_command_common_aliases(rel, source, |additional| {
        rust_source_scan::normalized_dependency_paths_with_additional_glob_aliases(
            rel, source, additional,
        )
    })
}

fn import_leaf_name(path: &str) -> &str {
    path.rsplit("::").next().unwrap_or(path)
}

fn allowed_status_dto_names() -> BTreeSet<&'static str> {
    [
        "GateDiagnostic",
        "GateResult",
        "GateState",
        "PlanExecutionStatus",
        "PublicExecutionCommandContext",
        "PublicRecordingContext",
        "PublicRepairTarget",
        "PublicReviewStateTaskClosure",
        "StatusBlockingRecord",
    ]
    .into_iter()
    .collect()
}

fn assert_no_import_path_prefix(
    rel: &str,
    source: &str,
    forbidden_prefixes: &[&str],
    reason: &str,
) {
    let violations = import_path_prefix_violations(rel, source, forbidden_prefixes);
    assert!(
        violations.is_empty(),
        "{rel} {reason}:\n{}",
        violations.join("\n")
    );
}

fn import_path_prefix_violations(
    rel: &str,
    source: &str,
    forbidden_prefixes: &[&str],
) -> Vec<String> {
    let mut violations = Vec::new();
    for path in normalized_dependency_paths(rel, source) {
        for forbidden in forbidden_prefixes {
            if path == *forbidden
                || path.starts_with(&format!("{forbidden}::"))
                || glob_path_covers(&path, forbidden)
            {
                violations.push(format!("forbidden dependency path `{path}`"));
            }
        }
    }
    violations.sort();
    violations.dedup();
    violations
}

fn glob_path_covers(glob_path: &str, target: &str) -> bool {
    rust_source_scan::glob_path_covers(glob_path, target)
}

#[test]
fn workflow_operator_does_not_import_mutation_command_modules() {
    let operator = read_repo_file("src/workflow/operator.rs");
    assert_no_import_path_prefix(
        "src/workflow/operator.rs",
        &operator,
        &["crate::execution::commands", "crate::execution::mutate"],
        "must consume execution query/router DTOs, not mutation command internals",
    );
    for forbidden in [
        "require_public_mutation",
        "persist_authoritative_state",
        "append_typed_state_event",
        "persist_if_dirty",
    ] {
        assert!(
            !operator.contains(forbidden),
            "workflow/operator must consume execution query/router DTOs, not mutation command internals: found `{forbidden}`"
        );
    }
}

#[test]
fn router_does_not_construct_reopen_commands_outside_shared_next_action() {
    let rel = "src/execution/router.rs";
    let router = read_repo_file(rel);
    let paths = normalized_code_paths(rel, &router);
    assert!(
        !paths
            .iter()
            .any(|path| path == "crate::execution::command_eligibility::PublicCommand::Reopen"),
        "router must not synthesize reopen commands locally; stale reentry targets must flow through shared next-action exact command helpers"
    );

    let next_action = read_repo_file("src/execution/next_action.rs");
    let next_action_paths = normalized_code_paths("src/execution/next_action.rs", &next_action);
    assert!(
        next_action_paths
            .iter()
            .any(|path| path == "crate::execution::command_eligibility::PublicCommand::Reopen"),
        "shared next-action must remain the owner of reopen public-command construction"
    );
}

#[test]
fn public_route_decision_rules_have_focused_module_owners() {
    let public_route = read_repo_file("src/execution/public_route_selection.rs");
    assert!(
        public_route.contains("pub(crate) fn shared_next_action_seed_from_runtime_state"),
        "public-route seed projection must live in src/execution/public_route_selection.rs"
    );
    assert!(
        public_route.contains("fn shared_next_action_seed_from_precomputed_decision"),
        "public-route exact command projection must stay in the focused public-route module"
    );

    let repair_target = read_repo_file("src/execution/repair_target_selection.rs");
    assert!(
        repair_target.contains("pub(crate) fn execution_reentry_target"),
        "repair target selection must live in src/execution/repair_target_selection.rs"
    );
    assert!(
        repair_target.contains("pub(crate) fn select_authoritative_stale_reentry_target"),
        "authoritative stale repair-target selection must stay in the focused repair-target module"
    );

    let late_stage = read_repo_file("src/execution/late_stage_route_selection.rs");
    assert!(
        late_stage.contains("pub(crate) fn select_late_stage_public_route"),
        "late-stage public route selection must live in src/execution/late_stage_route_selection.rs"
    );
    assert!(
        late_stage.contains("pub(crate) fn late_stage_decision"),
        "late-stage route command/phase projection must stay in the focused late-stage module"
    );

    let router = read_repo_file("src/execution/router.rs");
    assert!(
        !router.contains("fn shared_next_action_seed_from_precomputed_decision")
            && !router.contains("fn marker_free_started_execution"),
        "router.rs must delegate shared public-route seed projection instead of owning it"
    );

    let next_action = read_repo_file("src/execution/next_action.rs");
    assert!(
        !next_action.contains("pub(crate) fn execution_reentry_target")
            && !next_action.contains("pub(crate) fn select_authoritative_stale_reentry_target"),
        "next_action.rs must delegate repair-target selection to repair_target_selection.rs"
    );
    assert!(
        next_action.contains("select_late_stage_public_route")
            && !next_action.contains("HarnessPhase::FinalReviewPending =>")
            && !next_action.contains("HarnessPhase::QaPending =>")
            && !next_action.contains("HarnessPhase::ReadyForBranchCompletion =>"),
        "next_action.rs must delegate late-stage route ordering to late_stage_route_selection.rs"
    );
}

#[test]
fn blocking_scope_task_projection_has_single_execution_owner() {
    let query = read_repo_file("src/execution/query.rs");
    assert!(
        query.contains("pub(crate) struct ExecutionBlockingProjection")
            && query.contains("pub(crate) fn project_execution_blocking"),
        "blocking scope/task projection must be owned by execution::query"
    );

    let router = read_repo_file("src/execution/router.rs");
    assert!(
        router.contains("project_execution_blocking(ExecutionBlockingProjectionInputs"),
        "router must delegate blocking scope/task derivation to the shared query projection"
    );

    let public_route_projection =
        read_repo_file("src/execution/read_model/public_route_projection.rs");
    assert!(
        public_route_projection
            .contains("project_execution_blocking(ExecutionBlockingProjectionInputs"),
        "read-model public route projection must reuse the shared blocking projection"
    );
    for forbidden in [
        "status.blocking_scope = Some(String::from(\"task\"))",
        "status.blocking_scope = Some(String::from(\"branch\"))",
    ] {
        assert!(
            !public_route_projection.contains(forbidden),
            "read-model public route projection must not locally override blocking scope/task outside the shared projection: found `{forbidden}`"
        );
    }

    let operator = read_repo_file("src/workflow/operator.rs");
    for forbidden in [
        "operator_blocking_scope = Some(String::from(\"task\"))",
        "fn task_blocking_record_task",
        "strip_prefix(\"task-\")",
    ] {
        assert!(
            !operator.contains(forbidden),
            "workflow operator must consume projected blocking scope/task instead of deriving it locally: found `{forbidden}`"
        );
    }
}

#[test]
fn late_stage_phase_mapping_delegates_to_shared_canonical_phase() {
    let late_stage = read_repo_file("src/execution/late_stage_route_selection.rs");
    assert!(
        late_stage.contains(
            "canonical_phase_for_shared_decision(status.harness_phase.as_str(), phase_detail)"
        ),
        "late-stage route selection must delegate phase-detail to phase mapping to the shared canonical helper"
    );
    assert!(
        !late_stage.contains("phase: match phase_detail"),
        "late-stage route selection must not maintain a local phase-detail to phase match table"
    );

    let mut violations = Vec::new();
    for path in rust_source_files(&repo_root().join("src")) {
        let rel = repo_relative(&path);
        if rel == "src/execution/query.rs" {
            continue;
        }
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{rel} should be readable: {error}"));
        if local_phase_detail_to_phase_mapping_spans(&source)
            .into_iter()
            .any(|span| span.contains("PHASE_"))
        {
            violations.push(format!(
                "{rel} contains a local phase_detail match that maps to public phase constants"
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "phase-detail to phase mapping must stay centralized in execution::query: {violations:#?}"
    );
}

fn local_phase_detail_to_phase_mapping_spans(source: &str) -> Vec<&str> {
    let mut spans = Vec::new();
    let mut offset = 0;
    while let Some(start) = source[offset..].find("match phase_detail") {
        let start = offset + start;
        let Some(open_relative) = source[start..].find('{') else {
            break;
        };
        let open = start + open_relative;
        let mut depth = 0_u32;
        let mut end = source.len();
        for (relative, character) in source[open..].char_indices() {
            match character {
                '{' => depth += 1,
                '}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        end = open + relative + character.len_utf8();
                        break;
                    }
                }
                _ => {}
            }
        }
        spans.push(&source[start..end]);
        offset = end;
    }
    spans
}

#[test]
fn execution_reentry_target_construction_has_focused_owner() {
    let allowed_owner = "src/execution/repair_target_selection.rs";
    let mut violations = Vec::new();
    for path in rust_source_files(&repo_root().join("src/execution")) {
        let rel = repo_relative(&path);
        if rel == allowed_owner || rel.ends_with("/unit_tests.rs") {
            continue;
        }
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{rel} should be readable: {error}"));
        let mut byte_offset = 0;
        for (index, line) in source.lines().enumerate() {
            let trimmed = line.trim();
            let constructs_reentry_target = trimmed.contains("ExecutionReentryTarget::new(")
                || trimmed.contains("ExecutionReentryTarget {")
                || trimmed.contains("ExecutionReentryTargetSource::");
            if constructs_reentry_target && !line_is_in_cfg_test_module(&source, byte_offset) {
                violations.push(format!("{rel}:{}: {trimmed}", index + 1));
            }
            byte_offset += line.len() + 1;
        }
    }
    assert!(
        violations.is_empty(),
        "execution reentry target construction and source selection must stay in {allowed_owner}:\n{}",
        violations.join("\n")
    );

    let owner = read_repo_file(allowed_owner);
    assert!(
        owner.contains("pub(crate) fn execution_reentry_target")
            && owner.contains("ExecutionReentryTargetSource::NegativeReviewOrVerificationResult"),
        "{allowed_owner} must remain the focused repair-target selection owner, including negative-result reentry targets"
    );
}

#[test]
fn reopen_and_repair_public_commands_have_shared_next_action_owner() {
    let allowed_owner = "src/execution/next_action.rs";
    let enum_owner = "src/execution/command_eligibility.rs";
    let mut violations = Vec::new();
    for path in rust_source_files(&repo_root().join("src/execution")) {
        let rel = repo_relative(&path);
        if rel == allowed_owner || rel == enum_owner || rel.ends_with("/unit_tests.rs") {
            continue;
        }
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{rel} should be readable: {error}"));
        let mut byte_offset = 0;
        for (index, line) in source.lines().enumerate() {
            let trimmed = line.trim();
            let constructs_reopen = trimmed.contains("PublicCommand::Reopen {")
                && !trimmed.contains("{ .. }")
                && !line_is_in_cfg_test_module(&source, byte_offset);
            let constructs_repair = trimmed.contains("PublicCommand::RepairReviewState {")
                && !trimmed.contains("{ .. }")
                && !line_is_in_cfg_test_module(&source, byte_offset);
            if constructs_reopen || constructs_repair {
                violations.push(format!("{rel}:{}: {trimmed}", index + 1));
            }
            byte_offset += line.len() + 1;
        }
    }
    assert!(
        violations.is_empty(),
        "production reopen/repair public commands must be constructed through the shared next-action helpers:\n{}",
        violations.join("\n")
    );

    let owner = read_repo_file(allowed_owner);
    assert!(
        owner.contains("pub(crate) fn repair_review_state_public_command")
            && owner.contains("pub(crate) fn reopen_public_command"),
        "{allowed_owner} must remain the shared construction owner for repair/reopen public commands"
    );
}

fn line_is_in_cfg_test_module(source: &str, byte_offset: usize) -> bool {
    source[..byte_offset]
        .rfind("#[cfg(test)]")
        .is_some_and(|cfg_offset| source[cfg_offset..byte_offset].contains("mod "))
}

#[test]
fn stale_unreviewed_closure_projection_has_single_owner() {
    let allowed_owner = "src/execution/stale_target_projection.rs";
    let mut violations = Vec::new();
    for path in rust_source_files(&repo_root().join("src/execution")) {
        let rel = repo_relative(&path);
        if rel == allowed_owner {
            continue;
        }
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{rel} should be readable: {error}"));
        for (index, line) in source.lines().enumerate() {
            if line.contains(".stale_unreviewed_closures =") {
                violations.push(format!("{rel}:{}: {}", index + 1, line.trim()));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "`stale_unreviewed_closures` must be projected only by the focused stale-target module:\n{}",
        violations.join("\n")
    );

    let owner = read_repo_file(allowed_owner);
    assert!(
        owner.contains("pub(crate) fn project_stale_unreviewed_closures"),
        "{allowed_owner} must expose the single stale closure projection function"
    );
    assert!(
        owner.contains("pub(crate) fn project_review_state_stale_unreviewed_closures"),
        "{allowed_owner} must expose the review-state stale closure projection function"
    );

    let mut stale_target_reader_violations = Vec::new();
    for path in rust_source_files(&repo_root().join("src/execution")) {
        let rel = repo_relative(&path);
        if rel == allowed_owner {
            continue;
        }
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{rel} should be readable: {error}"));
        for (index, line) in source.lines().enumerate() {
            if line.contains("task_stale_record_ids()") || line.contains("stale_record_ids()") {
                stale_target_reader_violations.push(format!(
                    "{rel}:{}: {}",
                    index + 1,
                    line.trim()
                ));
            }
        }
    }
    assert!(
        stale_target_reader_violations.is_empty(),
        "stale closure record-id selection must stay inside the focused stale-target module:\n{}",
        stale_target_reader_violations.join("\n")
    );

    let query = read_repo_file("src/execution/query.rs");
    assert!(
        query.contains("project_review_state_stale_unreviewed_closures"),
        "review-state query must consume stale_target_projection instead of selecting stale closure ids locally"
    );
    assert!(
        !query.contains("let stale_unreviewed_closures = if"),
        "review-state query must not rebuild stale closure projection control flow locally"
    );
}

#[test]
fn stale_target_projection_uses_preloaded_authority_for_current_task_stale_records() {
    let owner = read_repo_file("src/execution/stale_target_projection.rs");
    assert!(
        owner.contains("event_authority_state: Option<&AuthoritativeTransitionState>"),
        "stale-target projection must thread the reducer/query authoritative-state snapshot through current task stale-target projection"
    );
    assert!(
        owner.contains(
            "Some(state) => stale_current_task_closure_records_from_authoritative_state(context, state)?"
        ),
        "stale-target projection must consume the supplied authoritative-state snapshot instead of reloading transition state from disk"
    );
    assert!(
        owner.contains("None => stale_current_task_closure_records(context)?"),
        "stale-target projection must preserve the no-snapshot fallback for standalone callers"
    );
}

#[test]
fn current_task_closure_status_projection_has_single_owner() {
    let owner = read_repo_file("src/execution/current_closure_projection.rs");
    assert!(
        owner.contains("pub(crate) fn project_current_task_closures"),
        "current task-closure DTO projection must live in the focused current-closure module"
    );

    let read_model = read_repo_file("src/execution/read_model.rs");
    assert!(
        !read_model.contains(".map(|record| PublicReviewStateTaskClosure"),
        "read_model.rs must consume current_closure_projection::project_current_task_closures instead of rebuilding current task-closure DTOs inline"
    );
    let query = read_repo_file("src/execution/query.rs");
    assert!(
        query.contains("project_current_task_closures"),
        "review-state query must consume current_closure_projection::project_current_task_closures"
    );
    assert!(
        !query.contains("ReviewStateTaskClosure {")
            && !query.contains("still_current_task_closure_records(context)?"),
        "review-state query must not rebuild current task-closure DTO projection inline"
    );
}

#[test]
fn read_model_modules_do_not_append_events_or_import_mutations() {
    for (rel, source) in read_model_boundary_sources() {
        assert_no_import_path_prefix(
            &rel,
            &source,
            &["crate::execution::commands", "crate::execution::mutate"],
            "must remain a read-model/status surface and must not import mutation modules",
        );
        for forbidden in [
            "append_typed_state_event",
            "append_state_event",
            "sync_fixture_event_log",
            "persist_if_dirty",
            "record_current_task_closure(",
            "record_current_branch_closure(",
            "record_final_review(",
            "record_release_readiness(",
            "record_browser_qa(",
        ] {
            assert!(
                !source.contains(forbidden),
                "{rel} must remain a read-model/status surface and must not append events or import mutation modules: found `{forbidden}`"
            );
        }
        let writer_violations = read_model_writer_violations(&rel, &source);
        assert!(
            writer_violations.is_empty(),
            "{rel} must remain a read-model/status surface and must not write files or append events directly:\n{}",
            writer_violations.join("\n")
        );
    }
}

#[test]
fn command_modules_do_not_import_read_model_or_workflow_presentation_layers() {
    for (rel, source) in execution_command_sources() {
        assert_no_import_path_prefix(
            &rel,
            &source,
            &[
                "crate::execution::read_model",
                "crate::execution::read_model_support",
                "crate::workflow::operator",
                "crate::workflow::status",
            ],
            "must not depend on read-model/status/workflow presentation layers",
        );
        assert_command_status_imports_are_dto_only(&rel, &source);
        let state_reexport_violations = command_state_reexport_violations(&rel, &source);
        assert!(
            state_reexport_violations.is_empty(),
            "{rel} must not import read-model/status builders through crate::execution::state compatibility re-exports; use the explicit command-facing boundary instead:\n{}",
            state_reexport_violations.join("\n")
        );
    }
}

#[test]
fn command_common_remains_a_facade_over_bounded_domain_modules() {
    let common_source = read_repo_file("src/execution/commands/common.rs");
    assert!(
        common_source.lines().count() <= 180,
        "src/execution/commands/common.rs should stay a small facade over focused command-support modules"
    );
    assert!(
        !common_source.contains("\nfn ")
            && !common_source.contains("\npub fn ")
            && !common_source.contains("\npub(super) fn ")
            && !common_source.contains("\npub(in crate::execution::commands) fn "),
        "src/execution/commands/common.rs should not regain production helper bodies"
    );

    for path in rust_source_files(&repo_root().join("src/execution/commands/common")) {
        let rel = repo_relative(&path);
        if rel.ends_with("/unit_tests.rs") {
            continue;
        }
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{rel} should be readable: {error}"));
        assert!(
            source.lines().count() <= 900,
            "{rel} should remain a focused command-support module instead of becoming the next catch-all"
        );
    }
}

#[test]
fn closure_dispatch_authority_is_not_raw_transition_dispatch_fallback() {
    let closure_dispatch = read_repo_file("src/execution/closure_dispatch.rs");
    let closure_dispatch_mutation = format!(
        "{}\n{}",
        read_repo_file("src/execution/closure_dispatch_mutation.rs"),
        read_repo_file("src/execution/closure_dispatch_mutation/recording.rs")
    );
    assert!(
        closure_dispatch.contains("pub(crate) fn current_review_dispatch_id_candidate")
            && closure_dispatch.contains("pub(crate) fn current_review_dispatch_id_from_lineage")
            && closure_dispatch.contains("current_review_dispatch_id_if_still_current"),
        "closure dispatch must own current public dispatch candidate selection through the current-lineage helper"
    );
    assert!(
        closure_dispatch.contains("validate_expected_dispatch_id")
            && closure_dispatch.contains("ensure_task_dispatch_id_matches")
            && closure_dispatch.contains("ensure_final_review_dispatch_id_matches")
            && closure_dispatch.contains("task_dispatch_reviewed_state_status"),
        "closure dispatch must own current dispatch validation and explicit hidden dispatch-id validation"
    );
    assert!(
        !closure_dispatch.contains("fn record_review_dispatch_strategy_checkpoint(")
            && !closure_dispatch
                .contains("fn record_review_dispatch_strategy_checkpoint_without_claim(")
            && !closure_dispatch.contains("fn ensure_review_dispatch_authoritative_bootstrap(")
            && !closure_dispatch.contains("closure_dispatch_mutation")
            && !closure_dispatch.contains("ReviewDispatchMutationAction"),
        "closure_dispatch.rs must not regain or re-export dispatch mutation or bootstrap authority"
    );
    assert!(
        closure_dispatch_mutation.contains("pub(crate) fn ensure_current_review_dispatch_id")
            && closure_dispatch_mutation
                .contains("pub(crate) fn ensure_review_dispatch_authoritative_bootstrap")
            && closure_dispatch_mutation
                .contains("pub(crate) fn record_review_dispatch_strategy_checkpoint")
            && closure_dispatch_mutation
                .contains("fn record_review_dispatch_strategy_checkpoint_without_claim")
            && closure_dispatch_mutation.contains("ReviewDispatchMutationAction")
            && closure_dispatch_mutation.contains("claim_step_write_authority")
            && closure_dispatch_mutation.contains("persist_if_dirty_with_failpoint_and_command"),
        "closure_dispatch_mutation.rs must own dispatch mutation, bootstrap, write authority, and event persistence"
    );
    assert!(
        closure_dispatch.contains("task_closure_dispatch_lineage_reason_code(reason_code)")
            && !closure_dispatch.contains("\"prior_task_review_dispatch_missing\"")
            && !closure_dispatch.contains("\"prior_task_review_dispatch_stale\""),
        "closure_dispatch.rs must consume dispatch-lineage diagnostics through closure_diagnostics instead of duplicating stale/missing dispatch reason literals"
    );
    let runtime_methods = read_repo_file("src/execution/state/runtime_methods.rs");
    assert!(
        !runtime_methods.contains("pub(crate) fn current_review_dispatch_id_candidate"),
        "runtime_methods must not regain public dispatch candidate selection"
    );
    assert!(
        !runtime_methods.contains("ensure_current_review_dispatch_id_impl")
            && !runtime_methods.contains("fn validate_expected_dispatch_id")
            && !runtime_methods.contains("fn record_review_dispatch_strategy_checkpoint"),
        "runtime_methods must not regain closure dispatch ensure/validation/recording authority"
    );

    let read_model_support = read_repo_file("src/execution/read_model_support.rs");
    for forbidden in [
        "fn current_review_dispatch_id_if_still_current",
        "fn current_review_dispatch_id_from_lineage",
        "shared_current_task_review_dispatch_id",
        "shared_current_final_review_dispatch_id",
    ] {
        assert!(
            !read_model_support.contains(forbidden),
            "read_model_support.rs must not regain dispatch lookup ownership from closure_dispatch: found `{forbidden}`"
        );
    }

    let public_repair_targets = read_repo_file("src/execution/public_repair_targets.rs");
    assert!(
        !public_repair_targets.contains("read_model_support::current_review_dispatch_id"),
        "public repair targets must consume closure_dispatch dispatch authority, not read_model_support"
    );

    assert!(
        !repo_root()
            .join("src/execution/commands/common/dispatch_lineage.rs")
            .exists(),
        "command-local dispatch_lineage helpers must not be recreated outside closure_dispatch"
    );

    let command_local_currentness_tokens = [
        "strategy_review_dispatch_lineage",
        "ExistingTaskDispatchReviewedStateStatus",
        "current_review_dispatch_id_from_lineage",
        "shared_current_task_review_dispatch_id",
        "shared_current_final_review_dispatch_id",
        "task_review_dispatch_id(task)",
    ];
    for (rel, source) in execution_command_sources() {
        if rel == "src/execution/commands/common/operator_outputs.rs" {
            continue;
        }
        for forbidden in command_local_currentness_tokens {
            assert!(
                !source.contains(forbidden),
                "{rel} must not reimplement dispatch currentness checks locally; consume closure_dispatch authority instead: found `{forbidden}`"
            );
        }
    }

    for rel in [
        "src/execution/reducer.rs",
        "src/execution/public_repair_targets.rs",
    ] {
        let source = read_repo_file(rel);
        assert!(
            !source.contains("task_review_dispatch_id(task)"),
            "{rel} must not use raw transition task_review_dispatch_id(task) as public dispatch authority"
        );
    }
}

#[test]
fn closure_receipt_diagnostics_stay_out_of_public_route_selection() {
    let closure_diagnostics = read_repo_file("src/execution/closure_diagnostics.rs");
    assert!(
        closure_diagnostics.contains("TASK_BOUNDARY_PROJECTION_DIAGNOSTIC_REASON_CODES")
            && closure_diagnostics.contains("task_boundary_projection_diagnostic_reason_code"),
        "closure diagnostics must centralize receipt/projection diagnostic classification"
    );
    let diagnostic_codes_start = closure_diagnostics
        .find("pub(crate) const TASK_BOUNDARY_PROJECTION_DIAGNOSTIC_REASON_CODES")
        .expect("closure_diagnostics.rs should keep task-boundary diagnostic code vocabulary");
    let diagnostic_codes_end = closure_diagnostics[diagnostic_codes_start..]
        .find("pub(crate) fn task_boundary_projection_diagnostic_reason_code")
        .map(|offset| diagnostic_codes_start + offset)
        .expect("closure_diagnostics.rs should keep diagnostic code predicate after vocabulary");
    let diagnostic_codes = &closure_diagnostics[diagnostic_codes_start..diagnostic_codes_end];
    assert!(
        diagnostic_codes.contains("\"prior_task_review_dispatch_missing\"")
            && diagnostic_codes.contains("\"prior_task_review_dispatch_stale\""),
        "stale/missing task-review dispatch lineage must remain diagnostic-only vocabulary"
    );
    let public_codes_start = closure_diagnostics
        .find("const PUBLIC_TASK_BOUNDARY_REASON_CODES")
        .expect("closure_diagnostics.rs should keep public task-boundary reason vocabulary");
    let public_codes_end = closure_diagnostics[public_codes_start..]
        .find("#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]")
        .map(|offset| public_codes_start + offset)
        .expect("closure_diagnostics.rs should keep public task-boundary state after vocabulary");
    let public_codes = &closure_diagnostics[public_codes_start..public_codes_end];
    assert!(
        !public_codes.contains("\"prior_task_review_dispatch_missing\"")
            && !public_codes.contains("\"prior_task_review_dispatch_stale\""),
        "stale/missing task-review dispatch lineage must not become public blocking reason vocabulary"
    );
    let recording_blocker_start = closure_diagnostics
        .find("fn task_closure_recording_blocking_reason_code")
        .expect("closure_diagnostics.rs should keep task-closure recording blocker predicate");
    let recording_blocker_end = closure_diagnostics[recording_blocker_start..]
        .find("pub(crate) fn task_closure_dispatch_lineage_reason_code")
        .map(|offset| recording_blocker_start + offset)
        .expect(
            "closure_diagnostics.rs should keep dispatch-lineage predicate after blocker predicate",
        );
    let recording_blocker = &closure_diagnostics[recording_blocker_start..recording_blocker_end];
    assert!(
        !recording_blocker.contains("\"prior_task_review_dispatch_missing\"")
            && !recording_blocker.contains("\"prior_task_review_dispatch_stale\""),
        "task-closure recording blockers must not consume stale/missing dispatch diagnostics as blockers"
    );
    assert!(
        closure_diagnostics.contains("push_task_closure_pending_verification_reason_codes_for_run")
            && closure_diagnostics.contains("parse_artifact_document")
            && closure_diagnostics.contains("authoritative_unit_review_receipt_path")
            && closure_diagnostics.contains("authoritative_task_verification_receipt_path")
            && closure_diagnostics.contains("task_closure_recording_diagnostic_reason_codes")
            && closure_diagnostics.contains("task_closure_recording_status_reason_codes"),
        "closure diagnostics must own task-boundary receipt parsing and diagnostic classification"
    );
    assert!(
        closure_diagnostics.contains("pub(crate) fn public_task_boundary_decision")
            && closure_diagnostics.contains("apply_task_boundary_projection_diagnostics")
            && closure_diagnostics.contains("merge_status_projection_diagnostics")
            && closure_diagnostics.contains("merge_task_boundary_projection_diagnostics"),
        "closure diagnostics must own public diagnostic field projection and merge helpers"
    );

    let read_model_support = read_repo_file("src/execution/read_model_support.rs");
    assert!(
        read_model_support.contains("diagnostic_reason_codes")
            && read_model_support.contains("blocking_reason_codes"),
        "task closure prerequisites must keep blockers separate from receipt/projection diagnostics"
    );
    for forbidden in [
        "parse_artifact_document",
        "fn authoritative_unit_review_receipt_path",
        "fn authoritative_task_verification_receipt_path",
        "fn task_closure_recording_diagnostic_reason_codes",
        "fn task_closure_recording_reason_code",
    ] {
        assert!(
            !read_model_support.contains(forbidden),
            "read_model_support.rs must not regain receipt parsing/path construction owned by closure_diagnostics: found `{forbidden}`"
        );
    }

    for rel in [
        "src/execution/read_model/public_route_projection.rs",
        "src/execution/router.rs",
        "src/execution/next_action.rs",
        "src/workflow/operator.rs",
    ] {
        let source = read_repo_file(rel);
        for forbidden in [
            "parse_artifact_document",
            "authoritative_unit_review_receipt_path",
            "authoritative_task_verification_receipt_path",
        ] {
            assert!(
                !source.contains(forbidden),
                "{rel} must not parse receipt/projection artifacts while selecting public routes: found `{forbidden}`"
            );
        }
    }
    let public_route_projection =
        read_repo_file("src/execution/read_model/public_route_projection.rs");
    assert!(
        public_route_projection.contains("apply_task_boundary_projection_diagnostics(status)")
            && !public_route_projection
                .contains("public_task_boundary_decision(status).diagnostic_reason_codes"),
        "public route projection must delegate task-boundary diagnostic projection to closure_diagnostics"
    );
    let router = read_repo_file("src/execution/router.rs");
    let public_blocking_start = router
        .find("fn public_route_blocking_reason_codes")
        .expect("router.rs should keep public_route_blocking_reason_codes");
    let public_blocking_end = router[public_blocking_start..]
        .find("fn prior_task_closure_progress_edge_required")
        .map(|offset| public_blocking_start + offset)
        .expect("router.rs should keep prior_task_closure_progress_edge_required after public blocking projection");
    let public_blocking = &router[public_blocking_start..public_blocking_end];
    assert!(
        public_blocking.contains("!task_boundary_projection_diagnostic_reason_code(reason_code)")
            && !public_blocking.contains("\"prior_task_review_dispatch_missing\"")
            && !public_blocking.contains("\"prior_task_review_dispatch_stale\""),
        "public route blocking reasons must filter diagnostic reason codes through closure_diagnostics instead of naming stale dispatch lineage as blockers"
    );
    let mutation_guards = read_repo_file("src/execution/commands/common/mutation_guards.rs");
    let begin_failure_start = mutation_guards
        .find(
            "pub(in crate::execution::commands) fn begin_failure_class_from_blocking_reason_codes",
        )
        .expect("mutation_guards.rs should keep begin failure-class projection");
    let begin_failure_end = mutation_guards[begin_failure_start..]
        .find("pub(in crate::execution::commands) fn begin_failure_class_from_status")
        .map(|offset| begin_failure_start + offset)
        .expect("mutation_guards.rs should keep begin_failure_class_from_status after failure-class projection");
    let begin_failure_classification = &mutation_guards[begin_failure_start..begin_failure_end];
    assert!(
        !begin_failure_classification.contains("\"prior_task_review_dispatch_missing\"")
            && !begin_failure_classification.contains("\"prior_task_review_dispatch_stale\""),
        "public mutation guard failure classification must not consume stale/missing dispatch diagnostics as blockers"
    );
    let workflow_operator = read_repo_file("src/workflow/operator.rs");
    assert!(
        workflow_operator.contains("merge_status_projection_diagnostics")
            && !workflow_operator.contains("for reason_code in &status.projection_diagnostics"),
        "workflow operator must use closure_diagnostics to merge public projection diagnostics"
    );
}

#[derive(Clone, Copy)]
struct FocusedRuntimeModuleLineCap {
    rel: &'static str,
    max_lines: usize,
    boundary: &'static str,
}

const FOCUSED_RUNTIME_MODULE_LINE_CAPS: &[FocusedRuntimeModuleLineCap] = &[
    FocusedRuntimeModuleLineCap {
        rel: "src/execution/closure_dispatch.rs",
        max_lines: 720,
        boundary: "current closure dispatch authority",
    },
    FocusedRuntimeModuleLineCap {
        rel: "src/execution/closure_dispatch_mutation.rs",
        max_lines: 160,
        boundary: "review-dispatch mutation and bootstrap",
    },
    FocusedRuntimeModuleLineCap {
        rel: "src/execution/closure_diagnostics.rs",
        max_lines: 520,
        boundary: "artifact/projection diagnostic reason classification",
    },
    FocusedRuntimeModuleLineCap {
        rel: "src/execution/current_closure_projection.rs",
        max_lines: 450,
        boundary: "current task-closure DTO and reason projection",
    },
    FocusedRuntimeModuleLineCap {
        rel: "src/execution/stale_target_projection.rs",
        max_lines: 850,
        boundary: "stale target and stale closure projection",
    },
    FocusedRuntimeModuleLineCap {
        rel: "src/execution/repair_target_selection.rs",
        max_lines: 450,
        boundary: "execution reentry and repair target selection",
    },
    FocusedRuntimeModuleLineCap {
        rel: "src/execution/late_stage_route_selection.rs",
        max_lines: 350,
        boundary: "late-stage public route selection",
    },
    FocusedRuntimeModuleLineCap {
        rel: "src/execution/public_route_selection.rs",
        max_lines: 400,
        boundary: "public next-action route seed projection",
    },
    FocusedRuntimeModuleLineCap {
        rel: "src/execution/read_model/late_stage.rs",
        max_lines: 400,
        boundary: "read-model late-stage precedence projection",
    },
    FocusedRuntimeModuleLineCap {
        rel: "src/execution/read_model/public_route_projection.rs",
        max_lines: 700,
        boundary: "read-model public route DTO projection",
    },
    FocusedRuntimeModuleLineCap {
        rel: "src/execution/read_model/review_state.rs",
        max_lines: 260,
        boundary: "read-model review-state authority projection",
    },
    FocusedRuntimeModuleLineCap {
        rel: "src/execution/read_model/task_state.rs",
        max_lines: 500,
        boundary: "read-model task-boundary and exact-command projection",
    },
];

fn source_line_count(source: &str) -> usize {
    source.lines().count()
}

#[test]
fn focused_runtime_modules_have_line_caps() {
    let boundary_doc =
        read_repo_file("docs/featureforge/reference/execution-runtime-module-boundaries.md");
    for cap in FOCUSED_RUNTIME_MODULE_LINE_CAPS {
        let source = read_repo_file(cap.rel);
        let line_count = source_line_count(&source);
        assert!(
            line_count <= cap.max_lines,
            "{} has {line_count} lines, above the focused-module cap of {} for {}",
            cap.rel,
            cap.max_lines,
            cap.boundary
        );
        assert!(
            boundary_doc.contains(&format!("| `{}` | {}", cap.rel, cap.max_lines)),
            "execution runtime module boundary doc must record the focused-module cap for {}",
            cap.rel
        );
    }
}

const REDUCED_RUNTIME_FACADE_LINE_CAPS: &[FocusedRuntimeModuleLineCap] = &[
    FocusedRuntimeModuleLineCap {
        rel: "src/execution/state.rs",
        max_lines: 350,
        boundary: "compatibility facade over execution state/read APIs",
    },
    FocusedRuntimeModuleLineCap {
        rel: "src/execution/mutate.rs",
        max_lines: 80,
        boundary: "compatibility facade over public mutation command modules",
    },
];

#[test]
fn reduced_runtime_facades_have_line_caps() {
    let boundary_doc =
        read_repo_file("docs/featureforge/reference/execution-runtime-module-boundaries.md");
    for cap in REDUCED_RUNTIME_FACADE_LINE_CAPS {
        let source = read_repo_file(cap.rel);
        let line_count = source_line_count(&source);
        assert!(
            line_count <= cap.max_lines,
            "{} has {line_count} lines, above the reduced-facade cap of {} for {}",
            cap.rel,
            cap.max_lines,
            cap.boundary
        );
        assert!(
            boundary_doc.contains(&format!("| `{}` | {}", cap.rel, cap.max_lines)),
            "execution runtime module boundary doc must record the reduced-facade cap for {}",
            cap.rel
        );
    }
}

#[derive(Clone, Copy)]
struct LargeRuntimeModuleBoundary {
    rel: &'static str,
    status: &'static str,
}

const LARGE_RUNTIME_MODULE_LINE_THRESHOLD: usize = 2_000;

const LARGE_RUNTIME_MODULE_BOUNDARIES: &[LargeRuntimeModuleBoundary] = &[
    LargeRuntimeModuleBoundary {
        rel: "src/execution/transitions.rs",
        status: "documented exception",
    },
    LargeRuntimeModuleBoundary {
        rel: "src/execution/read_model.rs",
        status: "scheduled follow-up",
    },
    LargeRuntimeModuleBoundary {
        rel: "src/execution/event_log.rs",
        status: "documented exception",
    },
    LargeRuntimeModuleBoundary {
        rel: "src/execution/review_state.rs",
        status: "scheduled follow-up",
    },
    LargeRuntimeModuleBoundary {
        rel: "src/execution/context.rs",
        status: "documented exception",
    },
    LargeRuntimeModuleBoundary {
        rel: "src/execution/next_action.rs",
        status: "scheduled follow-up",
    },
    LargeRuntimeModuleBoundary {
        rel: "src/execution/authority.rs",
        status: "documented exception",
    },
    LargeRuntimeModuleBoundary {
        rel: "src/execution/current_truth.rs",
        status: "scheduled follow-up",
    },
    LargeRuntimeModuleBoundary {
        rel: "src/execution/projection_renderer.rs",
        status: "documented exception",
    },
    LargeRuntimeModuleBoundary {
        rel: "src/execution/router.rs",
        status: "scheduled follow-up",
    },
];

fn markdown_section_for_heading<'a>(doc: &'a str, heading: &str) -> Option<&'a str> {
    let start = doc.find(heading)?;
    let rest = &doc[start..];
    let section_end = rest
        .get(heading.len()..)
        .and_then(|after_heading| after_heading.find("\n### "))
        .map(|relative_end| heading.len() + relative_end)
        .unwrap_or(rest.len());
    Some(&rest[..section_end])
}

#[test]
fn large_runtime_modules_have_documented_exception_or_followup() {
    let execution_root = repo_root().join("src/execution");
    let actual_large_modules = rust_source_files(&execution_root)
        .into_iter()
        .filter(|path| path.parent() == Some(execution_root.as_path()))
        .filter_map(|path| {
            let rel = repo_relative(&path);
            let source = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("{rel} should be readable: {error}"));
            (source_line_count(&source) > LARGE_RUNTIME_MODULE_LINE_THRESHOLD).then_some(rel)
        })
        .collect::<BTreeSet<_>>();
    let expected_large_modules = LARGE_RUNTIME_MODULE_BOUNDARIES
        .iter()
        .map(|boundary| boundary.rel.to_owned())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        actual_large_modules, expected_large_modules,
        "top-level execution modules above {LARGE_RUNTIME_MODULE_LINE_THRESHOLD} lines must be either documented exceptions or scheduled follow-ups",
    );

    let boundary_doc =
        read_repo_file("docs/featureforge/reference/execution-runtime-module-boundaries.md");
    for boundary in LARGE_RUNTIME_MODULE_BOUNDARIES {
        let heading = format!("### `{}`", boundary.rel);
        let section = markdown_section_for_heading(&boundary_doc, &heading).unwrap_or_else(|| {
            panic!(
                "execution runtime module boundary doc must have a section for {}",
                boundary.rel
            )
        });
        assert!(
            section.contains(&format!("- Status: {}", boundary.status)),
            "{} must be marked as `{}` in the boundary doc",
            boundary.rel,
            boundary.status
        );
        match boundary.status {
            "documented exception" => assert!(
                section.contains("- Why exception:"),
                "{} documented exception must explain why the large module is acceptable",
                boundary.rel
            ),
            "scheduled follow-up" => assert!(
                section.contains("- Follow-up:"),
                "{} scheduled follow-up must name the next extraction direction",
                boundary.rel
            ),
            other => panic!("unsupported large-module boundary status `{other}`"),
        }
        assert!(
            section.contains("- Boundary guard:"),
            "{} must document the active boundary guard that prevents drift",
            boundary.rel
        );
    }
}

#[test]
fn task9_import_direction_boundary_matrix_covers_required_edges() {
    let operator = read_repo_file("src/workflow/operator.rs");
    assert_no_import_path_prefix(
        "src/workflow/operator.rs",
        &operator,
        &["crate::execution::commands", "crate::execution::mutate"],
        "must not depend on command or mutation internals",
    );

    let mut read_side_sources = read_model_boundary_sources();
    read_side_sources.push((
        String::from("src/execution/query.rs"),
        read_repo_file("src/execution/query.rs"),
    ));
    for (rel, source) in read_side_sources {
        assert_no_import_path_prefix(
            &rel,
            &source,
            &["crate::execution::commands", "crate::execution::mutate"],
            "must not depend on mutation command modules",
        );
    }

    for (rel, source) in execution_command_sources() {
        assert_no_import_path_prefix(
            &rel,
            &source,
            &[
                "crate::execution::read_model",
                "crate::execution::read_model_support",
                "crate::workflow::operator",
                "crate::workflow::status",
            ],
            "must not import presentation or read-model modules",
        );
        assert_command_status_imports_are_dto_only(&rel, &source);
    }
}

fn assert_command_status_imports_are_dto_only(rel: &str, source: &str) {
    let violations = status_import_violations(rel, source);
    assert!(
        violations.is_empty(),
        "{rel} may import explicit DTO types from crate::execution::status, but every status import must exactly match the DTO allowlist:\n{}",
        violations.join("\n")
    );
}

fn status_import_violations(rel: &str, source: &str) -> Vec<String> {
    let allowed_status_dtos = allowed_status_dto_names();
    let mut violations = Vec::new();

    for path in normalized_dependency_paths(rel, source) {
        if path != "crate::execution::status" && !path.starts_with("crate::execution::status::") {
            continue;
        }
        let imported = import_leaf_name(&path);
        if !allowed_status_dtos.contains(imported) {
            violations.push(path);
        }
    }
    violations
}

fn state_reexported_read_or_status_items() -> BTreeSet<String> {
    let allowed_status_dtos = allowed_status_dto_names();
    normalized_expanded_use_paths(
        "src/execution/state.rs",
        &read_repo_file("src/execution/state.rs"),
    )
    .into_iter()
    .filter_map(|path| {
        let imported = import_leaf_name(&path);
        if path.starts_with("crate::execution::read_model::")
            || path.starts_with("crate::execution::read_model_support::")
            || (path.starts_with("crate::execution::status::")
                && !allowed_status_dtos.contains(imported))
        {
            Some(imported.to_owned())
        } else {
            None
        }
    })
    .collect()
}

fn command_state_reexport_violations(rel: &str, source: &str) -> Vec<String> {
    let forbidden_state_reexports = state_reexported_read_or_status_items();
    let mut violations = Vec::new();
    for path in normalized_dependency_paths(rel, source) {
        if path == "crate::execution::state::*" {
            violations.push(format!(
                "{rel}: forbidden state re-export dependency `{path}`"
            ));
            continue;
        }
        let Some(imported) = path.strip_prefix("crate::execution::state::") else {
            continue;
        };
        let imported = import_leaf_name(imported);
        if forbidden_state_reexports.contains(imported) {
            violations.push(format!(
                "{rel}: forbidden state re-export dependency `{path}`"
            ));
        }
    }
    violations.sort();
    violations.dedup();
    violations
}

fn forbidden_projection_writer_paths() -> BTreeSet<&'static str> {
    [
        "crate::execution::authority::write_authoritative_unit_review_receipt_artifact",
        "crate::execution::projection_renderer::ProjectionWriteMode",
        "crate::execution::projection_renderer::materialize_late_stage_projection_artifacts",
        "crate::execution::projection_renderer::write_execution_projection_read_models",
        "crate::execution::projection_renderer::write_project_artifact",
        "crate::execution::projection_renderer::write_project_artifact_at_path",
        "crate::execution::transitions::materialize_authoritative_transition_state_projection",
    ]
    .into_iter()
    .collect()
}

fn forbidden_projection_writer_globs() -> BTreeSet<&'static str> {
    [
        "crate::execution::authority::*",
        "crate::execution::projection_renderer::*",
        "crate::execution::transitions::*",
    ]
    .into_iter()
    .collect()
}

fn projection_writer_dependency_violations(rel: &str, source: &str) -> Vec<String> {
    let forbidden_paths = forbidden_projection_writer_paths();
    let forbidden_globs = forbidden_projection_writer_globs();
    let mut violations = Vec::new();

    for path in normalized_dependency_paths(rel, source) {
        if forbidden_globs.contains(path.as_str()) {
            violations.push(format!(
                "{rel}: forbidden projection writer glob dependency `{path}`"
            ));
            continue;
        }
        if forbidden_paths
            .iter()
            .any(|forbidden| glob_path_covers(&path, forbidden))
        {
            violations.push(format!(
                "{rel}: forbidden projection writer parent-glob dependency `{path}`"
            ));
            continue;
        }
        if forbidden_paths
            .iter()
            .any(|forbidden| path == *forbidden || path.starts_with(&format!("{forbidden}::")))
        {
            violations.push(format!(
                "{rel}: forbidden projection writer dependency `{path}`"
            ));
        }
    }

    violations.sort();
    violations.dedup();
    violations
}

#[test]
fn non_materialization_command_modules_do_not_write_projection_read_models() {
    for (rel, source) in execution_command_sources() {
        if rel.ends_with("materialize_projections.rs") {
            continue;
        }

        let projection_writer_violations = projection_writer_dependency_violations(&rel, &source);
        assert!(
            projection_writer_violations.is_empty(),
            "{rel} must not write projection/read-model artifacts directly; only materialize-projections may call projection writer helpers:\n{}",
            projection_writer_violations.join("\n")
        );
        let direct_write_violations = command_writer_violations(&rel, &source);
        assert!(
            direct_write_violations.is_empty(),
            "{rel} must not contain unreviewed generic writers; projection/read-model aliases must not bypass materialize-projections:\n{}",
            direct_write_violations.join("\n")
        );
    }

    let transitions = read_repo_file("src/execution/transitions.rs");
    assert!(
        !transitions.contains("write_atomic_file(&self.state_path"),
        "transition persistence must remain event-only; state.json projection writes belong behind materialize-projections"
    );
}

type WriterCall = rust_source_scan::RustWriterCall;

fn expr_target_name(expr: &syn::Expr) -> Option<String> {
    match expr {
        syn::Expr::Reference(reference) => expr_target_name(&reference.expr),
        syn::Expr::Path(path) => Some(syn_path_to_string(&path.path)),
        _ => None,
    }
}

fn writer_target_arg_name(
    callee: &str,
    args: &syn::punctuated::Punctuated<syn::Expr, syn::Token![,]>,
) -> Option<String> {
    let lower = callee.to_ascii_lowercase();
    let target_index = if matches!(
        lower.as_str(),
        "fs::copy"
            | "std::fs::copy"
            | "fs::rename"
            | "std::fs::rename"
            | "fs::hard_link"
            | "std::fs::hard_link"
    ) {
        1
    } else {
        0
    };
    args.iter().nth(target_index).and_then(expr_target_name)
}

fn is_generic_file_writer_call(callee: &str) -> bool {
    let lower = callee.to_ascii_lowercase();
    let leaf = lower.rsplit("::").next().unwrap_or(lower.as_str());
    matches!(
        lower.as_str(),
        "fs::write"
            | "std::fs::write"
            | "fs::copy"
            | "std::fs::copy"
            | "fs::rename"
            | "std::fs::rename"
            | "fs::hard_link"
            | "std::fs::hard_link"
            | "file::create"
            | "std::fs::file::create"
            | "file::options"
            | "std::fs::file::options"
            | "openoptions::new"
            | "std::fs::openoptions::new"
    ) || matches!(
        leaf,
        "write_atomic" | "write_atomic_file" | "write_all" | "write"
    )
}

fn writer_calls(rel: &str, source: &str) -> Vec<WriterCall> {
    with_command_common_aliases(rel, source, |additional| {
        rust_source_scan::writer_call_hits(
            rel,
            source,
            additional,
            is_generic_file_writer_call,
            writer_target_arg_name,
        )
    })
}

fn writer_violations(
    rel: &str,
    source: &str,
    allowed: impl Fn(&WriterCall) -> bool,
) -> Vec<String> {
    writer_calls(rel, source)
        .into_iter()
        .filter(|call| !allowed(call))
        .map(|call| {
            format!(
                "{rel}:{} calls generic writer `{}` for target {:?} outside an explicit writer exception",
                call.function, call.callee, call.target_arg
            )
        })
        .collect()
}

fn command_writer_violations(rel: &str, source: &str) -> Vec<String> {
    writer_violations(rel, source, |call| is_allowed_command_writer(rel, call))
}

fn read_model_writer_violations(rel: &str, source: &str) -> Vec<String> {
    writer_violations(rel, source, |call| is_allowed_read_model_writer(rel, call))
}

fn is_allowed_read_model_writer(rel: &str, call: &WriterCall) -> bool {
    matches!(
        (
            rel,
            call.function.as_str(),
            call.callee.as_str(),
            call.target_arg.as_deref()
        ),
        (
            "src/execution/status.rs",
            "write_plan_execution_schema",
            "std::fs::write",
            None
        )
    )
}

fn is_allowed_command_writer(rel: &str, call: &WriterCall) -> bool {
    matches!(
        (
            rel,
            call.function.as_str(),
            call.callee.as_str(),
            call.target_arg.as_deref()
        ),
        (
            "src/execution/commands/common/path_persistence.rs",
            "restore_plan_and_evidence",
            "fs::write" | "std::fs::write",
            Some("plan_path" | "evidence_path")
        ) | (
            "src/execution/commands/common/path_persistence.rs",
            "write_atomic",
            "write_atomic_file" | "crate::paths::write_atomic",
            Some("path")
        ) | (
            "src/execution/commands/advance_late_stage.rs",
            "record_qa",
            "write_atomic_file" | "crate::paths::write_atomic",
            Some("authoritative_test_plan_path" | "authoritative_qa_path")
        )
    )
}

#[test]
fn phase_detail_string_literals_are_centralized() {
    let phase_detail_literals = phase_detail_literals_from_phase_module();
    let allowed_src_files = [
        "src/execution/phase.rs",
        // Task 11 explicitly permits the workflow precedence table to mirror
        // public phase-detail vocabulary while asserting precedence rows.
        "src/workflow/late_stage_precedence.rs",
    ];
    let allowed_test_files = [
        "tests/contracts_execution_runtime_boundaries.rs",
        "tests/execution_query.rs",
        "tests/liveness_model_checker.rs",
        "tests/plan_execution.rs",
        "tests/plan_execution_final_review.rs",
        "tests/public_replay_churn.rs",
        "tests/runtime_behavior_golden.rs",
        "tests/runtime_module_boundaries.rs",
        "tests/workflow_entry_shell_smoke.rs",
        "tests/workflow_runtime.rs",
        "tests/workflow_runtime_final_review.rs",
        "tests/workflow_shell_smoke.rs",
    ];
    let mut violations = Vec::new();

    for path in rust_source_files(&repo_root().join("src")) {
        let rel = repo_relative(&path);
        if allowed_src_files.contains(&rel.as_str()) {
            continue;
        }
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{rel} should be readable: {error}"));
        violations.extend(phase_detail_literal_value_violations(
            &rel,
            &source,
            &phase_detail_literals,
            "outside the explicit production allowlist",
        ));
        violations.extend(phase_detail_context_literal_violations(
            &rel,
            &source,
            &phase_detail_literals,
        ));
    }

    for path in rust_source_files(&repo_root().join("tests")) {
        let rel = repo_relative(&path);
        if allowed_test_files.contains(&rel.as_str()) || rel.starts_with("tests/internal_") {
            continue;
        }
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{rel} should be readable: {error}"));
        violations.extend(phase_detail_literal_value_violations(
            &rel,
            &source,
            &phase_detail_literals,
            "outside the explicit test allowlist",
        ));
        violations.extend(phase_detail_context_literal_violations(
            &rel,
            &source,
            &phase_detail_literals,
        ));
    }

    assert!(
        violations.is_empty(),
        "phase-detail string literals must be sourced from src/execution/phase.rs or an explicit test allowlist:\n{}",
        violations.join("\n")
    );
}

fn phase_detail_literals_from_phase_module() -> Vec<String> {
    let phase_source = read_repo_file("src/execution/phase.rs");
    let literals = phase_detail_literals_from_source("src/execution/phase.rs", &phase_source);
    assert!(
        literals.len() >= 10,
        "phase-detail boundary test should derive the public phase-detail vocabulary from src/execution/phase.rs, got {literals:?}"
    );
    literals
}

fn phase_detail_literals_from_source(rel: &str, source: &str) -> Vec<String> {
    let syntax = parse_rust_source(rel, source);
    let aliases = aliases_for_source(rel, source, &syntax);
    let mut literals = syntax
        .items
        .iter()
        .filter_map(|item| {
            let syn::Item::Const(item_const) = item else {
                return None;
            };
            item_const
                .ident
                .to_string()
                .starts_with("DETAIL_")
                .then(|| phase_detail_const_expr_value(rel, &aliases, &item_const.expr))
                .flatten()
        })
        .collect::<Vec<_>>();
    literals.sort();
    literals.dedup();
    literals
}

fn phase_detail_const_expr_value(
    rel: &str,
    aliases: &BTreeMap<String, String>,
    expr: &syn::Expr,
) -> Option<String> {
    match expr {
        syn::Expr::Lit(literal) => {
            if let syn::Lit::Str(literal) = &literal.lit {
                Some(literal.value())
            } else {
                None
            }
        }
        syn::Expr::Macro(macro_expr) => {
            let raw_macro_path = syn_path_to_string(&macro_expr.mac.path);
            let normalized_macro_path =
                normalize_code_path_for_source(rel, &raw_macro_path, aliases);
            if normalized_macro_path.rsplit("::").next() != Some("concat") {
                return None;
            }
            macro_expr
                .mac
                .parse_body_with(
                    syn::punctuated::Punctuated::<syn::LitStr, syn::Token![,]>::parse_terminated,
                )
                .ok()
                .map(|parts| parts.iter().map(syn::LitStr::value).collect())
        }
        syn::Expr::Paren(paren) => phase_detail_const_expr_value(rel, aliases, &paren.expr),
        syn::Expr::Group(group) => phase_detail_const_expr_value(rel, aliases, &group.expr),
        _ => None,
    }
}

struct StringLiteralValueCollector<'a> {
    rel: &'a str,
    aliases: &'a BTreeMap<String, String>,
    values: Vec<String>,
}

impl<'ast> Visit<'ast> for StringLiteralValueCollector<'_> {
    fn visit_lit_str(&mut self, literal: &'ast syn::LitStr) {
        self.values.push(literal.value());
    }

    fn visit_macro(&mut self, macro_call: &'ast syn::Macro) {
        let raw_macro_path = syn_path_to_string(&macro_call.path);
        let normalized_macro_path =
            normalize_code_path_for_source(self.rel, &raw_macro_path, self.aliases);
        if normalized_macro_path.rsplit("::").next() == Some("concat")
            && let Ok(parts) = macro_call.parse_body_with(
                syn::punctuated::Punctuated::<syn::LitStr, syn::Token![,]>::parse_terminated,
            )
        {
            self.values
                .push(parts.iter().map(syn::LitStr::value).collect());
        }
        collect_string_literal_values_from_tokens(macro_call.tokens.clone(), &mut self.values);
        visit::visit_macro(self, macro_call);
    }
}

fn collect_string_literal_values_from_tokens(
    tokens: proc_macro2::TokenStream,
    values: &mut Vec<String>,
) {
    for token in tokens {
        match token {
            proc_macro2::TokenTree::Group(group) => {
                collect_string_literal_values_from_tokens(group.stream(), values);
            }
            proc_macro2::TokenTree::Literal(literal) => {
                if let Ok(literal) = syn::parse_str::<syn::LitStr>(&literal.to_string()) {
                    values.push(literal.value());
                }
            }
            proc_macro2::TokenTree::Ident(_) | proc_macro2::TokenTree::Punct(_) => {}
        }
    }
}

fn rust_string_literal_values(rel: &str, source: &str) -> Vec<String> {
    let syntax = parse_rust_source(rel, source);
    let aliases = aliases_for_source(rel, source, &syntax);
    let mut collector = StringLiteralValueCollector {
        rel,
        aliases: &aliases,
        values: Vec::new(),
    };
    collector.visit_file(&syntax);
    collector.values
}

fn phase_detail_literal_value_violations(
    rel: &str,
    source: &str,
    known_phase_details: &[String],
    allowed_context: &str,
) -> Vec<String> {
    let known_phase_details = known_phase_details
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let mut violations = rust_string_literal_values(rel, source)
        .into_iter()
        .filter(|literal| known_phase_details.contains(literal.as_str()))
        .map(|literal| {
            format!("{rel} duplicates phase-detail literal `{literal}` {allowed_context}")
        })
        .collect::<Vec<_>>();
    violations.sort();
    violations.dedup();
    violations
}

fn is_phase_detail_shaped_literal(literal: &str) -> bool {
    let Some(suffix) = literal.rsplit('_').next() else {
        return false;
    };
    literal.contains('_')
        && literal
            .chars()
            .all(|character| character.is_ascii_lowercase() || character == '_')
        && matches!(
            suffix,
            "bug" | "gate" | "pending" | "progress" | "ready" | "reconcile" | "required"
        )
}

fn collect_string_values_from_expr(
    rel: &str,
    aliases: &BTreeMap<String, String>,
    expr: &syn::Expr,
    values: &mut Vec<String>,
) {
    let mut collector = StringLiteralValueCollector {
        rel,
        aliases,
        values: Vec::new(),
    };
    collector.visit_expr(expr);
    values.extend(collector.values);
}

fn collect_string_values_from_pat(pat: &syn::Pat, values: &mut Vec<String>) {
    match pat {
        syn::Pat::Lit(lit) => {
            if let syn::Lit::Str(literal) = &lit.lit {
                values.push(literal.value());
            }
        }
        syn::Pat::Or(or_pat) => {
            for case in &or_pat.cases {
                collect_string_values_from_pat(case, values);
            }
        }
        syn::Pat::Reference(reference) => collect_string_values_from_pat(&reference.pat, values),
        syn::Pat::Tuple(tuple) => {
            for elem in &tuple.elems {
                collect_string_values_from_pat(elem, values);
            }
        }
        syn::Pat::TupleStruct(tuple) => {
            for elem in &tuple.elems {
                collect_string_values_from_pat(elem, values);
            }
        }
        syn::Pat::Struct(struct_pat) => {
            for field in &struct_pat.fields {
                collect_string_values_from_pat(&field.pat, values);
            }
        }
        syn::Pat::Slice(slice) => {
            for elem in &slice.elems {
                collect_string_values_from_pat(elem, values);
            }
        }
        syn::Pat::Paren(paren) => collect_string_values_from_pat(&paren.pat, values),
        syn::Pat::Type(typed) => collect_string_values_from_pat(&typed.pat, values),
        syn::Pat::Const(_)
        | syn::Pat::Ident(_)
        | syn::Pat::Macro(_)
        | syn::Pat::Path(_)
        | syn::Pat::Range(_)
        | syn::Pat::Rest(_)
        | syn::Pat::Verbatim(_)
        | syn::Pat::Wild(_) => {}
        _ => {}
    }
}

fn attrs_include_test_only_cfg(attrs: &[syn::Attribute]) -> bool {
    rust_source_scan::attrs_include_test_only_cfg(attrs)
}

fn path_mentions_phase_detail(path: &syn::Path) -> bool {
    path.segments
        .iter()
        .any(|segment| segment.ident == "phase_detail")
}

fn expr_mentions_phase_detail(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::Field(field) => {
            matches!(&field.member, syn::Member::Named(ident) if ident == "phase_detail")
                || expr_mentions_phase_detail(&field.base)
        }
        syn::Expr::Path(path) => path_mentions_phase_detail(&path.path),
        syn::Expr::Reference(reference) => expr_mentions_phase_detail(&reference.expr),
        syn::Expr::Paren(paren) => expr_mentions_phase_detail(&paren.expr),
        syn::Expr::Tuple(tuple) => tuple.elems.iter().any(expr_mentions_phase_detail),
        syn::Expr::MethodCall(method_call) => expr_mentions_phase_detail(&method_call.receiver),
        syn::Expr::Binary(binary) => {
            expr_mentions_phase_detail(&binary.left) || expr_mentions_phase_detail(&binary.right)
        }
        _ => false,
    }
}

fn pat_mentions_phase_detail(pat: &syn::Pat) -> bool {
    match pat {
        syn::Pat::Ident(ident) => ident.ident == "phase_detail",
        syn::Pat::Reference(reference) => pat_mentions_phase_detail(&reference.pat),
        syn::Pat::Tuple(tuple) => tuple.elems.iter().any(pat_mentions_phase_detail),
        syn::Pat::TupleStruct(tuple) => tuple.elems.iter().any(pat_mentions_phase_detail),
        syn::Pat::Struct(struct_pat) => struct_pat.fields.iter().any(|field| {
            matches!(&field.member, syn::Member::Named(ident) if ident == "phase_detail")
                || pat_mentions_phase_detail(&field.pat)
        }),
        _ => false,
    }
}

struct PhaseDetailContextLiteralCollector<'a> {
    rel: &'a str,
    aliases: &'a BTreeMap<String, String>,
    values: Vec<String>,
}

impl PhaseDetailContextLiteralCollector<'_> {
    fn collect_expr(&mut self, expr: &syn::Expr) {
        collect_string_values_from_expr(self.rel, self.aliases, expr, &mut self.values);
    }
}

impl<'ast> Visit<'ast> for PhaseDetailContextLiteralCollector<'_> {
    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        if attrs_include_test_only_cfg(&node.attrs) {
            return;
        }
        visit::visit_item_mod(self, node);
    }

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        if attrs_include_test_only_cfg(&node.attrs) {
            return;
        }
        visit::visit_item_fn(self, node);
    }

    fn visit_expr_assign(&mut self, node: &'ast syn::ExprAssign) {
        if expr_mentions_phase_detail(&node.left) {
            self.collect_expr(&node.right);
        }
        visit::visit_expr_assign(self, node);
    }

    fn visit_expr_binary(&mut self, node: &'ast syn::ExprBinary) {
        if expr_mentions_phase_detail(&node.left) {
            self.collect_expr(&node.right);
        }
        if expr_mentions_phase_detail(&node.right) {
            self.collect_expr(&node.left);
        }
        visit::visit_expr_binary(self, node);
    }

    fn visit_expr_match(&mut self, node: &'ast syn::ExprMatch) {
        if expr_mentions_phase_detail(&node.expr) {
            for arm in &node.arms {
                collect_string_values_from_pat(&arm.pat, &mut self.values);
                if let Some((_if_token, guard)) = &arm.guard {
                    self.collect_expr(guard);
                }
            }
        }
        visit::visit_expr_match(self, node);
    }

    fn visit_field_value(&mut self, node: &'ast syn::FieldValue) {
        if matches!(&node.member, syn::Member::Named(ident) if ident == "phase_detail") {
            self.collect_expr(&node.expr);
        }
        visit::visit_field_value(self, node);
    }

    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = node.func.as_ref()
            && path_mentions_phase_detail(&path.path)
        {
            for arg in &node.args {
                self.collect_expr(arg);
            }
        }
        visit::visit_expr_call(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        if node.method == "phase_detail" {
            for arg in &node.args {
                self.collect_expr(arg);
            }
        }
        visit::visit_expr_method_call(self, node);
    }

    fn visit_local(&mut self, node: &'ast syn::Local) {
        if pat_mentions_phase_detail(&node.pat)
            && let Some(init) = &node.init
        {
            self.collect_expr(&init.expr);
        }
        visit::visit_local(self, node);
    }
}

fn phase_detail_context_literal_violations(
    rel: &str,
    source: &str,
    known_phase_details: &[String],
) -> Vec<String> {
    let known_phase_details = known_phase_details
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let syntax = parse_rust_source(rel, source);
    let aliases = aliases_for_source(rel, source, &syntax);
    let mut collector = PhaseDetailContextLiteralCollector {
        rel,
        aliases: &aliases,
        values: Vec::new(),
    };
    collector.visit_file(&syntax);
    collector
        .values
        .into_iter()
        .filter_map(|literal| {
            if literal == "phase_detail" {
                return None;
            }
            (is_phase_detail_shaped_literal(&literal)
                && !known_phase_details.contains(literal.as_str()))
            .then(|| {
                format!(
                    "{rel} uses unregistered phase-detail-shaped literal `{literal}` in a phase_detail context"
                )
            })
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
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
        use super::super::read_model::{derive_public_phase_detail};
        use crate::execution::{read_model::derive_public_next_action};
        use super::super::status::{PlanExecutionStatus, PlanExecutionStatusBuilder};
    ";
    let command_imports =
        normalized_expanded_use_paths("src/execution/commands/reopen.rs", command_source);
    assert!(
        command_imports
            .iter()
            .any(|path| path == "crate::execution::read_model::derive_public_phase_detail"),
        "relative command read-model imports must normalize to the forbidden crate path: {command_imports:?}"
    );
    assert!(
        command_imports
            .iter()
            .any(|path| path == "crate::execution::read_model::derive_public_next_action"),
        "grouped command read-model imports must normalize to the forbidden crate path: {command_imports:?}"
    );
    assert!(
        status_import_violations("src/execution/commands/reopen.rs", command_source)
            .iter()
            .any(|path| path == "crate::execution::status::PlanExecutionStatusBuilder"),
        "relative status imports must still reject non-DTO builders while allowing DTOs"
    );
}

#[test]
fn dependency_scan_catches_fully_qualified_boundary_paths() {
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
    assert!(
        operator_dependencies
            .iter()
            .any(|path| path == "crate::execution::commands::begin::begin"),
        "operator dependency scan must catch fully qualified command-module references: {operator_dependencies:?}"
    );
    assert!(
        operator_dependencies
            .iter()
            .any(|path| path == "crate::execution::commands::complete::complete"),
        "operator dependency scan must catch command-module references through module aliases: {operator_dependencies:?}"
    );
    assert!(
        operator_dependencies
            .iter()
            .any(|path| path == "crate::execution::commands::reopen::reopen"),
        "operator dependency scan must catch command-module references through local aliases: {operator_dependencies:?}"
    );
    assert!(
        operator_dependencies
            .iter()
            .any(|path| path == "crate::execution::mutate::append_typed_state_event"),
        "operator dependency scan must catch fully qualified mutation references: {operator_dependencies:?}"
    );

    let command_source = r"
        use crate::execution::read_model as read_side;
        use crate::workflow as wf;

        fn bypass() {
            let _ = crate::execution::read_model::derive_public_phase_detail;
            let _ = read_side::derive_public_next_action;
            let _ = crate::workflow::operator::workflow_operator_json;
            let _ = wf::status::WorkflowRoute;
            let _ = crate::execution::status::PlanExecutionStatusBuilder;
        }
    ";
    let command_dependencies =
        normalized_dependency_paths("src/execution/commands/reopen.rs", command_source);
    assert!(
        command_dependencies
            .iter()
            .any(|path| path == "crate::execution::read_model::derive_public_phase_detail"),
        "command dependency scan must catch fully qualified read-model references: {command_dependencies:?}"
    );
    assert!(
        command_dependencies
            .iter()
            .any(|path| path == "crate::execution::read_model::derive_public_next_action"),
        "command dependency scan must catch read-model references through module aliases: {command_dependencies:?}"
    );
    assert!(
        command_dependencies
            .iter()
            .any(|path| path == "crate::workflow::operator::workflow_operator_json"),
        "command dependency scan must catch fully qualified workflow presentation references: {command_dependencies:?}"
    );
    assert!(
        command_dependencies
            .iter()
            .any(|path| path == "crate::workflow::status::WorkflowRoute"),
        "command dependency scan must catch workflow presentation references through module aliases: {command_dependencies:?}"
    );
    assert!(
        status_import_violations("src/execution/commands/reopen.rs", command_source)
            .iter()
            .any(|path| path == "crate::execution::status::PlanExecutionStatusBuilder"),
        "status DTO gate must reject fully qualified status builders"
    );
}

#[test]
fn parent_glob_imports_are_boundary_violations() {
    let operator_source = r"
        use crate::execution::*;

        fn bypass() {
            let _ = commands::begin::begin;
        }
    ";
    let violations = import_path_prefix_violations(
        "src/workflow/operator.rs",
        operator_source,
        &["crate::execution::commands", "crate::execution::mutate"],
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("crate::execution::*")),
        "operator boundary gate must reject parent execution globs that can import command modules: {violations:?}"
    );

    let read_model_source = r"
        use crate::execution::*;

        fn bypass() {
            let _ = mutate::append_typed_state_event;
        }
    ";
    let violations = import_path_prefix_violations(
        "src/execution/read_model.rs",
        read_model_source,
        &["crate::execution::commands", "crate::execution::mutate"],
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("crate::execution::*")),
        "read-model boundary gate must reject parent execution globs that can import mutation modules: {violations:?}"
    );

    let command_source = r"
        use crate::execution::*;
        use crate::workflow::*;

        fn bypass() {
            let _ = read_model::derive_public_phase_detail;
            let _ = operator::workflow_operator_json;
        }
    ";
    let violations = import_path_prefix_violations(
        "src/execution/commands/reopen.rs",
        command_source,
        &[
            "crate::execution::read_model",
            "crate::execution::read_model_support",
            "crate::workflow::operator",
            "crate::workflow::status",
        ],
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("crate::execution::*"))
            && violations
                .iter()
                .any(|violation| violation.contains("crate::workflow::*")),
        "command boundary gate must reject parent globs that can import read-model or workflow presentation modules: {violations:?}"
    );
}

#[test]
fn state_reexport_gate_rejects_read_model_builder_imports_in_commands() {
    let command_source = r"
        use crate::execution::state::{
            PlanExecutionStatus,
            branch_closure_record_matches_plan_exemption,
            load_execution_read_scope,
        };

        fn bypass() {
            let _ = crate::execution::state::load_execution_read_scope_for_mutation;
        }
    ";
    let violations =
        command_state_reexport_violations("src/execution/commands/reopen.rs", command_source);
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("branch_closure_record_matches_plan_exemption")),
        "state re-export gate must reject read-model helpers hidden behind state imports: {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("load_execution_read_scope")),
        "state re-export gate must reject read-scope loaders hidden behind state imports: {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("load_execution_read_scope_for_mutation")),
        "state re-export gate must reject direct qualified read-scope loaders hidden behind state imports: {violations:?}"
    );
    assert!(
        violations.iter().all(|violation| !violation.contains(
            "direct state re-export reference `crate::execution::state::load_execution_read_scope`"
        )),
        "direct qualified scanning must not match a shorter re-export name inside a longer one: {violations:?}"
    );
    assert!(
        violations
            .iter()
            .all(|violation| !violation.contains("PlanExecutionStatus")),
        "status DTO imports through the compatibility state surface should remain allowed: {violations:?}"
    );

    let glob_source = r"
        use crate::execution::state::*;

        fn bypass() {
            let _ = load_execution_read_scope;
        }
    ";
    let violations =
        command_state_reexport_violations("src/execution/commands/reopen.rs", glob_source);
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("crate::execution::state::*")),
        "state re-export gate must reject state globs that can hide read-model/status-builder imports: {violations:?}"
    );
}

#[test]
fn read_model_writer_gate_rejects_direct_event_log_appends() {
    let source = r##"
        use std::fs::OpenOptions;
        use std::io::Write;

        fn append_event(events_path: &std::path::Path) {
            let open_event_log = OpenOptions::new;
            let mut events = OpenOptions::new()
                .append(true)
                .create(true)
                .open(events_path)
                .expect("event log should open");
            let mut alias_events = open_event_log()
                .append(true)
                .create(true)
                .open(events_path)
                .expect("event log should open through alias");
            let append_event_bytes = Write::write_all;
            events
                .write_all(br#"{"command":"bypass"}"#)
                .expect("event log should append");
            append_event_bytes(&mut alias_events, br#"{"command":"alias-bypass"}"#)
                .expect("event log should append through alias");
        }
    "##;
    let violations = read_model_writer_violations("src/execution/read_model.rs", source);
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("std::fs::OpenOptions::new")),
        "read-model writer gate must reject direct event-log OpenOptions append paths: {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("write_all")),
        "read-model writer gate must reject direct event-log write calls: {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("std::io::Write::write_all")),
        "read-model writer gate must reject event-log write calls hidden behind local function aliases: {violations:?}"
    );
}

#[test]
fn generic_writer_gate_rejects_projection_alias_writes() {
    let source = r#"
        use std::fs as filesystem;
        use std::fs::write as persist;
        use std::io::Write;

        const CONST_WRITE: fn(&std::path::Path, &str) -> std::io::Result<()> = std::fs::write;
        static STATIC_WRITE: fn(&std::path::Path, &str) -> std::io::Result<()> = std::fs::write;

        struct WriterAlias;
        impl WriterAlias {
            const CREATE: fn(&std::path::Path) -> std::io::Result<std::fs::File> =
                std::fs::File::create;
        }

        fn bypass_projection_materialization_boundary(state_dir: &std::path::Path) {
            let dest = state_dir.join("state.json");
            let temp = state_dir.join("state.tmp");
            let alias_dest = state_dir.join("state-alias.json");
            let create_dest = state_dir.join("state-create.json");
            fs::write(&dest, "{}").expect("projection bypass");
            filesystem::write(&dest, "{}").expect("projection bypass");
            persist(&dest, "{}").expect("projection bypass");
            CONST_WRITE(&alias_dest, "{}").expect("projection const alias bypass");
            STATIC_WRITE(&alias_dest, "{}").expect("projection static alias bypass");
            let _ = WriterAlias::CREATE(&create_dest).expect("projection associated const alias bypass");
            let alias_write = std::fs::write;
            alias_write(&alias_dest, "{}").expect("projection alias bypass");
            let alias_create = std::fs::File::create;
            let _ = alias_create(&create_dest).expect("projection alias bypass");
            let alias_open_options = std::fs::OpenOptions::new;
            let _ = alias_open_options()
                .write(true)
                .create(true)
                .open(&alias_dest)
                .expect("projection alias bypass");
            filesystem::copy(&temp, &dest).expect("projection bypass");
            filesystem::rename(&temp, &dest).expect("projection bypass");
            filesystem::hard_link(&temp, &dest).expect("projection bypass");
            let mut projected_state = std::fs::File::options()
                .write(true)
                .create(true)
                .open(&dest)
                .expect("projection bypass");
            projected_state.write(b"{}").expect("projection bypass");
            {
                use std::fs as local_filesystem;
                local_filesystem::write(&dest, "{}").expect("projection bypass");
            }
        }
    "#;
    let violations = command_writer_violations("src/execution/commands/reopen.rs", source);
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("fs::write")),
        "generic writer gate must reject projection writes hidden behind neutral path aliases: {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("std::fs::write")),
        "generic writer gate must reject projection writes through fs module/function aliases: {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("std::fs::write")
                && violation.contains("alias_dest")),
        "generic writer gate must reject std::fs::write hidden behind local function aliases: {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("std::fs::File::create")
                && violation.contains("create_dest")),
        "generic writer gate must reject File::create hidden behind local function aliases: {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("std::fs::OpenOptions::new")),
        "generic writer gate must reject OpenOptions::new hidden behind local function aliases: {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("alias CONST_WRITE")),
        "generic writer gate must reject module-level const writer aliases directly: {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("alias STATIC_WRITE")),
        "generic writer gate must reject module-level static writer aliases directly: {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("alias CREATE")),
        "generic writer gate must reject associated const writer aliases directly: {violations:?}"
    );
    for mutator in ["std::fs::copy", "std::fs::rename", "std::fs::hard_link"] {
        assert!(
            violations
                .iter()
                .any(|violation| violation.contains(mutator) && violation.contains("dest")),
            "generic writer gate must reject projection writes through filesystem mutator `{mutator}` and report its destination target: {violations:?}"
        );
    }
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("std::fs::File::options")),
        "generic writer gate must reject File::options write chains: {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("write")),
        "generic writer gate must reject Write::write calls after file opens: {violations:?}"
    );

    let record_qa_projection_source = r#"
        use super::common::*;

        fn record_qa() {
            let state_path = std::path::PathBuf::from("state.json");
            let _ = write_atomic_file(&state_path, "{}");
        }
    "#;
    let violations = command_writer_violations(
        "src/execution/commands/advance_late_stage.rs",
        record_qa_projection_source,
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("state_path")),
        "record_qa writer exception must stay limited to named authoritative artifact targets: {violations:?}"
    );

    let common_glob_writer_alias_source = r#"
        use super::common::*;

        fn bypass_common_glob_writer_alias(state_dir: &std::path::Path) {
            let state_path = state_dir.join("state.json");
            let alias_state_path = state_dir.join("state-alias.json");
            let _ = write_atomic_file(&state_path, "{}");
            let alias_write_atomic = write_atomic_file;
            let _ = alias_write_atomic(&alias_state_path, "{}");
        }
    "#;
    let violations = command_writer_violations(
        "src/execution/commands/reopen.rs",
        common_glob_writer_alias_source,
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("crate::paths::write_atomic")),
        "generic writer gate must reject writer aliases imported through the command common::* facade: {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("alias_state_path")),
        "generic writer gate must reject command common::* writer aliases hidden behind local function bindings: {violations:?}"
    );
}

#[test]
fn projection_writer_gate_rejects_aliased_projection_helpers() {
    let source = r#"
        use crate::execution::authority::write_authoritative_unit_review_receipt_artifact as write_receipt;
        use crate::execution::projection_renderer::{
            ProjectionWriteMode as Mode,
            write_execution_projection_read_models as write_models,
        };
        use crate::execution::projection_renderer as renderer;
        use crate::execution::transitions::materialize_authoritative_transition_state_projection as materialize_state;

        fn bypass_projection_materialization_boundary() {
            let _ = write_models();
            let _ = Mode::StateDirOnly;
            let _ = renderer::write_project_artifact_at_path();
            let _ = materialize_state();
            let _ = write_receipt();
        }
    "#;
    let violations =
        projection_writer_dependency_violations("src/execution/commands/reopen.rs", source);
    for forbidden in [
        "crate::execution::authority::write_authoritative_unit_review_receipt_artifact",
        "crate::execution::projection_renderer::ProjectionWriteMode",
        "crate::execution::projection_renderer::write_execution_projection_read_models",
        "crate::execution::projection_renderer::write_project_artifact_at_path",
        "crate::execution::transitions::materialize_authoritative_transition_state_projection",
    ] {
        assert!(
            violations
                .iter()
                .any(|violation| violation.contains(forbidden)),
            "projection writer gate must reject aliased dependency `{forbidden}`: {violations:?}"
        );
    }

    let glob_source = r#"
        use crate::execution::projection_renderer::*;

        fn bypass_projection_materialization_boundary() {
            let _ = write_execution_projection_read_models;
        }
    "#;
    let violations =
        projection_writer_dependency_violations("src/execution/commands/reopen.rs", glob_source);
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("crate::execution::projection_renderer::*")),
        "projection writer gate must reject projection renderer glob imports that could hide writer helpers: {violations:?}"
    );

    let parent_glob_source = r#"
        use crate::execution::*;

        fn bypass_projection_materialization_boundary() {
            let _ = projection_renderer::write_project_artifact_at_path;
        }
    "#;
    let violations = projection_writer_dependency_violations(
        "src/execution/commands/reopen.rs",
        parent_glob_source,
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("crate::execution::*")),
        "projection writer gate must reject parent execution globs that can import projection writer helpers: {violations:?}"
    );
}

#[test]
fn macro_body_scanning_rejects_boundary_bypasses() {
    let operator_source = r#"
        macro_rules! bypass_operator_boundary {
            () => {{
                crate::execution::commands::begin::begin();
            }};
        }
    "#;
    let violations = import_path_prefix_violations(
        "src/workflow/operator.rs",
        operator_source,
        &["crate::execution::commands", "crate::execution::mutate"],
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("crate::execution::commands::begin::begin")),
        "operator boundary gate must reject command module references hidden in macro bodies: {violations:?}"
    );

    let read_model_source = r#"
        macro_rules! bypass_read_model_boundary {
            ($event_log:expr, $event:expr) => {{
                crate::execution::mutate::append_typed_state_event($event_log, $event);
                std::fs::OpenOptions::new()
                    .append(true)
                    .open($event_log)
                    .expect("event log should open");
            }};
        }
    "#;
    let import_violations = import_path_prefix_violations(
        "src/execution/read_model.rs",
        read_model_source,
        &["crate::execution::commands", "crate::execution::mutate"],
    );
    assert!(
        import_violations
            .iter()
            .any(|violation| violation
                .contains("crate::execution::mutate::append_typed_state_event")),
        "read-model boundary gate must reject mutation references hidden in macro bodies: {import_violations:?}"
    );
    let writer_violations =
        read_model_writer_violations("src/execution/read_model.rs", read_model_source);
    assert!(
        writer_violations
            .iter()
            .any(|violation| violation.contains("std::fs::OpenOptions::new")),
        "read-model writer gate must reject event-log writers hidden in macro bodies: {writer_violations:?}"
    );

    let command_source = r#"
        macro_rules! bypass_projection_boundary {
            ($path:expr) => {{
                crate::execution::projection_renderer::write_execution_projection_read_models();
                std::fs::write($path, "{}").expect("projection write");
            }};
        }
    "#;
    let projection_violations =
        projection_writer_dependency_violations("src/execution/commands/reopen.rs", command_source);
    assert!(
        projection_violations
            .iter()
            .any(|violation| violation.contains(
                "crate::execution::projection_renderer::write_execution_projection_read_models"
            )),
        "projection writer gate must reject projection helpers hidden in macro bodies: {projection_violations:?}"
    );
    let writer_violations =
        command_writer_violations("src/execution/commands/reopen.rs", command_source);
    assert!(
        writer_violations
            .iter()
            .any(|violation| violation.contains("std::fs::write")),
        "command writer gate must reject generic writers hidden in macro bodies: {writer_violations:?}"
    );
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
    "##;
    let violations = phase_detail_context_literal_violations(
        "src/execution/read_model.rs",
        source,
        &phase_detail_literals,
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("new_phase_detail_required")),
        "phase-detail context scan must reject unregistered phase-shaped literals: {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("raw_phase_detail_required")),
        "phase-detail context scan must reject raw unregistered phase-shaped literals: {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("production_phase_detail_required")),
        "phase-detail context scan must not skip production-only cfg(not(test)) literals: {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("comparison_phase_detail_required")),
        "phase-detail context scan must reject unregistered literals in phase_detail comparisons: {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("match_phase_detail_required")),
        "phase-detail context scan must reject unregistered literals in phase_detail match arms: {violations:?}"
    );
    assert!(
        violations
            .iter()
            .all(|violation| !violation.contains("execution_reentry_required")),
        "phase-detail context scan must allow literals registered in phase.rs: {violations:?}"
    );
    assert!(
        violations
            .iter()
            .all(|violation| !violation.contains("phase_detail={}")),
        "phase-detail context scan must not treat diagnostic format strings as phase details: {violations:?}"
    );
}

#[test]
fn status_dto_gate_rejects_mixed_allowed_and_unlisted_imports() {
    let mixed_status_import = r"
        use crate::execution::status::{
            PlanExecutionStatus,
            PlanExecutionStatusBuilder,
        };
    ";
    assert!(
        status_import_violations("src/execution/commands/reopen.rs", mixed_status_import)
            .iter()
            .any(|path| path == "crate::execution::status::PlanExecutionStatusBuilder"),
        "status import gate must reject mixed DTO plus non-DTO imports"
    );
}
