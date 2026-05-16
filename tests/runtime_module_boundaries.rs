#[path = "support/rust_source_scan.rs"]
mod rust_source_scan;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use featureforge::execution::command_eligibility::public_mutation_command_tokens;
use featureforge::execution::follow_up::public_follow_up_tokens;
use featureforge::execution::harness::HarnessPhase;
use featureforge::execution::public_repair_target_reasons::{
    PERSISTED_REVIEW_STATE_REPAIR_FOLLOW_UP_REASON_PREFIX, PublicRepairTargetReason,
    persisted_review_state_repair_follow_up_reason,
};
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

fn production_execution_rust_source_files() -> Vec<PathBuf> {
    rust_source_files(&repo_root().join("src/execution"))
        .into_iter()
        .filter(|path| {
            let rel = repo_relative(path);
            !focused_explicit_import_test_module(&rel)
        })
        .collect()
}

#[test]
fn production_execution_rust_source_files_exclude_test_only_modules() {
    let production_rels = production_execution_rust_source_files()
        .into_iter()
        .map(|path| repo_relative(&path))
        .collect::<BTreeSet<_>>();
    assert!(
        production_rels.contains("src/execution/commands/advance_late_stage.rs"),
        "production execution source discovery should include command submodules"
    );
    for test_only_rel in [
        "src/execution/command_eligibility_hidden_flag_tests.rs",
        "src/execution/route_plan/unit_tests.rs",
        "src/execution/commands/common/unit_tests.rs",
        "src/execution/route_plan/next_action_choice/tests.rs",
        "src/execution/read_model/execution_command_route_target_tests/exact_route_tests.rs",
    ] {
        assert!(
            !production_rels.contains(test_only_rel),
            "production execution source discovery must exclude test-only module {test_only_rel}"
        );
    }
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
        String::from("src/execution/runtime_truth.rs"),
        String::from("src/execution/status.rs"),
        String::from("src/execution/status_support.rs"),
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

fn route_plan_boundary_sources() -> Vec<(String, String)> {
    let mut rels = vec![String::from("src/execution/route_plan.rs")];
    rels.extend(
        rust_source_files(&repo_root().join("src/execution/route_plan"))
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

fn route_plan_production_boundary_sources() -> Vec<(String, String)> {
    route_plan_boundary_sources()
        .into_iter()
        .filter(|(rel, _)| !focused_explicit_import_test_module(rel))
        .collect()
}

fn route_plan_next_action_choice_sources() -> Vec<(String, String)> {
    let mut rels = vec![String::from(
        "src/execution/route_plan/next_action_choice.rs",
    )];
    rels.extend(
        rust_source_files(&repo_root().join("src/execution/route_plan/next_action_choice"))
            .into_iter()
            .map(|path| repo_relative(&path))
            .filter(|rel| rel != "src/execution/route_plan/next_action_choice/tests.rs"),
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

fn route_plan_next_action_choice_dependency_paths() -> Vec<String> {
    route_plan_next_action_choice_sources()
        .into_iter()
        .flat_map(|(rel, source)| normalized_dependency_paths(&rel, &source))
        .collect()
}

fn route_plan_next_action_choice_code_paths() -> Vec<String> {
    route_plan_next_action_choice_sources()
        .into_iter()
        .flat_map(|(rel, source)| normalized_code_paths(&rel, &source))
        .collect()
}

fn repair_route_decision_sources() -> Vec<(String, String)> {
    let mut rels = vec![String::from("src/execution/repair_route_decision.rs")];
    rels.extend(
        rust_source_files(&repo_root().join("src/execution/repair_route_decision"))
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

fn repair_route_decision_dependency_paths() -> Vec<String> {
    repair_route_decision_sources()
        .into_iter()
        .flat_map(|(rel, source)| normalized_dependency_paths(&rel, &source))
        .collect()
}

fn repair_route_decision_code_paths() -> Vec<String> {
    repair_route_decision_sources()
        .into_iter()
        .flat_map(|(rel, source)| normalized_code_paths(&rel, &source))
        .collect()
}

fn repair_route_decision_struct_names() -> BTreeSet<String> {
    repair_route_decision_sources()
        .into_iter()
        .flat_map(|(rel, source)| source_struct_names(&rel, &source))
        .collect()
}

fn baseline_bridge_predicate_source_module_prefixes() -> [&'static str; 3] {
    [
        "crate::execution::current_truth::",
        "crate::execution::repair_target_selection::",
        "crate::execution::state::",
    ]
}

fn baseline_bridge_predicate_source_names() -> BTreeSet<String> {
    let prefixes = baseline_bridge_predicate_source_module_prefixes();
    let mut names = BTreeSet::new();
    for (rel, source) in repair_route_decision_sources() {
        names.extend(
            normalized_code_paths(&rel, &source)
                .into_iter()
                .filter(|path| prefixes.iter().any(|prefix| path.starts_with(prefix)))
                .map(|path| import_leaf_name(&path).to_owned())
                .filter(|name| {
                    name.bytes()
                        .next()
                        .is_some_and(|byte| byte.is_ascii_lowercase())
                }),
        );
    }
    assert!(
        !names.is_empty(),
        "baseline-bridge owner modules must expose behavior-derived predicate source names"
    );
    names
}

fn is_baseline_bridge_predicate_source_path(
    path: &str,
    predicate_source_names: &BTreeSet<String>,
) -> bool {
    baseline_bridge_predicate_source_module_prefixes()
        .iter()
        .any(|prefix| path.starts_with(prefix))
        && predicate_source_names.contains(import_leaf_name(path))
}

#[test]
fn completed_task_closure_preemption_predicate_has_single_authoritative_definition() {
    let owner_rel = "src/execution/repair_target_selection.rs";
    let owner_source = read_repo_file(owner_rel);
    assert!(
        owner_source.contains("pub(crate) fn completed_task_closure_preempts_execution_reentry("),
        "{owner_rel} must own completed-task close-preemption as a named shared predicate"
    );
    let route_plan = read_repo_file("src/execution/route_plan.rs");
    assert!(
        function_calls_leaf(
            "src/execution/route_plan.rs",
            &route_plan,
            "route_planning_authority_for_status",
            "completed_task_closure_preempts_execution_reentry",
        ),
        "route_plan.rs must route completed-task close-preemption through the shared repair_target_selection predicate"
    );
    let next_action_route = read_repo_file("src/execution/route_plan/next_action_route.rs");
    assert!(
        !next_action_route.contains("NextActionAuthorityInputs")
            && normalized_dependency_paths(
                "src/execution/route_plan/next_action_route.rs",
                &next_action_route,
            )
            .iter()
            .all(|path| !path.starts_with("crate::execution::repair_target_selection::")),
        "route-plan finalization should consume precomputed route facts for completed-task close-preemption instead of carrying raw route authority inputs"
    );
}

#[test]
fn current_task_closure_branch_route_predicate_has_single_owner() {
    let owner_rel = "src/execution/status_support.rs";
    let owner_source = read_repo_file(owner_rel);
    let owner_function = "current_task_closure_branch_route_facts_from_status";
    assert!(
        owner_source.contains(&format!("pub(crate) fn {owner_function}(")),
        "{owner_rel} must own task-closure branch-route fact derivation"
    );
    let route_plan = read_repo_file("src/execution/route_plan.rs");
    let authority_input_calls = rust_source_scan::normalized_call_paths_in_function(
        "src/execution/route_plan.rs",
        &route_plan,
        "next_action_authority_inputs_for_route_plan",
    );
    assert!(
        authority_input_calls.iter().any(|path| {
            path == "crate::execution::status_support::current_task_closure_branch_route_facts_from_status"
        }),
        "route planning must derive current-task closure branch-route facts once before route selection"
    );

    for (rel, expected_dependencies) in [
        (
            "src/execution/repair_target_selection.rs",
            vec![
                "crate::execution::state::CurrentTaskClosureBranchRouteFacts",
                "crate::execution::state::current_task_closure_branch_route_facts_from_status",
            ],
        ),
        (
            "src/execution/route_plan.rs",
            vec![
                "crate::execution::status_support::current_task_closure_branch_route_facts_from_status",
            ],
        ),
        (
            "src/execution/route_plan/next_action_choice/execution_routes.rs",
            vec!["crate::execution::state::CurrentTaskClosureBranchRouteFacts"],
        ),
    ] {
        let source = read_repo_file(rel);
        let dependency_paths = normalized_dependency_paths(rel, &source);
        for expected_dependency in expected_dependencies {
            assert!(
                dependency_paths
                    .iter()
                    .any(|path| path == expected_dependency),
                "{rel} must consume the shared branch-route fact owner instead of inspecting branch/current-task closure fields directly. Expected dependency `{expected_dependency}`; dependencies: {dependency_paths:?}"
            );
        }
    }
    for rel in [
        "src/execution/route_plan/next_action_choice/execution_routes.rs",
        "src/execution/route_plan/next_action_choice/late_stage_public_routes.rs",
        "src/execution/route_plan/next_action_choice/late_stage_routes.rs",
        "src/execution/route_plan/next_action_choice/late_stage_repair_routes.rs",
        "src/execution/route_plan/next_action_choice/execution_ordering.rs",
    ] {
        let source = read_repo_file(rel);
        let dependency_paths = normalized_dependency_paths(rel, &source);
        let fallback_fact_calls = rust_source_scan::normalized_call_paths(rel, &source, &[])
            .into_iter()
            .filter(|path| {
                import_leaf_name(path) == "current_task_closure_branch_route_facts_or_derive"
            })
            .collect::<Vec<_>>();
        assert!(
            !dependency_paths.iter().any(|path| {
                import_leaf_name(path) == "current_task_closure_branch_route_facts_from_status"
            }),
            "{rel} must consume CurrentTaskClosureBranchRouteFacts passed through route inputs instead of deriving branch-route facts from status locally. Dependencies: {dependency_paths:?}"
        );
        assert!(
            fallback_fact_calls.is_empty(),
            "{rel} must consume required precomputed CurrentTaskClosureBranchRouteFacts instead of calling the fallback derive-or-use accessor. Calls: {fallback_fact_calls:?}"
        );
    }
    for rel in [
        "src/execution/repair_target_selection.rs",
        "src/execution/route_plan.rs",
        "src/execution/route_plan/next_action_choice/execution_routes.rs",
        "src/execution/route_plan/next_action_choice/execution_ordering.rs",
        "src/execution/route_plan/next_action_choice/late_stage_repair_routes.rs",
        "src/execution/route_plan/next_action_choice/late_stage_routes.rs",
        "src/execution/route_plan/next_action_choice/late_stage_public_routes.rs",
    ] {
        let source = read_repo_file(rel);
        let forbidden_direct_calls = rust_source_scan::normalized_call_paths(rel, &source, &[])
            .into_iter()
            .filter(|path| {
                import_leaf_name(path) == "current_task_closure_set_is_non_branch_contributing"
            })
            .collect::<Vec<_>>();
        let forbidden_field_reads =
            branch_route_status_field_recomputation_violations(rel, &source, owner_function);
        assert!(
            forbidden_direct_calls.is_empty() && forbidden_field_reads.is_empty(),
            "{rel} must consume CurrentTaskClosureBranchRouteFacts methods instead of locally rederiving branch/current-task closure state. Direct calls: {forbidden_direct_calls:?}; field reads: {forbidden_field_reads:?}"
        );
    }
}

#[test]
fn execution_reentry_close_preemption_targets_specific_task_closure() {
    let router_source = read_repo_file("src/execution/router.rs");
    let router_paths = normalized_code_paths("src/execution/router.rs", &router_source);
    assert!(
        router_paths
            .iter()
            .any(|path| path == "crate::execution::route_plan::plan_runtime_route")
            && !router_paths
                .iter()
                .any(|path| path.starts_with("crate::execution::repair_target_selection::"))
            && !router_source.contains("current_task_closures"),
        "router.rs must delegate execution-reentry close-current-task preemption to the route-plan owner instead of deriving task-closure state locally"
    );
}

#[test]
fn current_task_closure_route_target_selection_has_single_owner() {
    let owner_rel = "src/execution/current_task_closure_selection.rs";
    let owner = read_repo_file(owner_rel);
    assert!(
        owner.contains("pub(crate) fn current_task_closure_route_target(")
            && owner.contains("pub(crate) fn current_task_closure_route_target_for_task(")
            && owner.contains("pub(crate) fn preferred_current_task_closure_route_target("),
        "{owner_rel} must own deterministic current-task-closure route target selection"
    );

    for rel in [
        "src/execution/route_plan.rs",
        "src/execution/status_assembly/blocking_records.rs",
        "src/execution/invariants.rs",
    ] {
        let source = read_repo_file(rel);
        let paths = normalized_dependency_paths(rel, &source);
        assert!(
            paths
                .iter()
                .any(|path| path.starts_with("crate::execution::current_task_closure_selection::")),
            "{rel} must consume current_task_closure_selection instead of selecting current closures locally"
        );
    }

    let offenders = rust_source_files(&repo_root().join("src"))
        .into_iter()
        .map(|path| (repo_relative(&path), path))
        .filter(|(rel, _)| rel != owner_rel)
        .filter_map(|(rel, path)| {
            let source = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("{rel} should be readable: {error}"));
            source
                .contains("current_task_closures.first()")
                .then_some(rel)
        })
        .collect::<Vec<_>>();
    assert!(
        offenders.is_empty(),
        "production code must not use positional current_task_closures.first() outside {owner_rel}: {offenders:?}"
    );
}

#[test]
fn execution_template_bindability_policy_lives_in_command_eligibility() {
    let owner_rel = "src/execution/command_eligibility/execution_target.rs";
    let owner = read_repo_file(owner_rel);
    let owner_visibilities = source_production_function_visibilities(owner_rel, &owner);
    let owner_dependencies = normalized_dependency_paths(owner_rel, &owner);
    assert!(
        matches!(
            owner_visibilities
                .get("execution_template_inputs_are_bindable")
                .map(String::as_str),
            Some("pub(crate)" | "pub(in crate)")
        ) && owner_dependencies
            .iter()
            .any(|path| path == "crate::execution::public_command_types::PublicCommandTemplate")
            && owner_dependencies
                .iter()
                .any(|path| path
                    == "crate::execution::public_command_types::PublicCommandTemplateInput")
            && owner_dependencies.iter().any(|path| path
                == "crate::execution::public_command_types::PublicCommandInputBindingKind")
            && owner_dependencies
                .iter()
                .any(|path| path.ends_with("command_kind::PublicCommandKind")),
        "{owner_rel} must document and expose the execution-template bindability boundary API and own template/input binding DTO policy; visibilities: {owner_visibilities:?}; dependencies: {owner_dependencies:?}"
    );

    let exact_route_rel = "src/execution/status_assembly/exact_route_template.rs";
    let exact_route = read_repo_file(exact_route_rel);
    let exact_route_dependencies = normalized_dependency_paths(exact_route_rel, &exact_route);
    let exact_route_calls =
        rust_source_scan::normalized_call_paths(exact_route_rel, &exact_route, &[]);
    let forbidden_policy_dependencies = exact_route_dependencies
        .iter()
        .filter(|path| {
            path.ends_with("PublicCommandInputBindingKind")
                || path.ends_with("PublicCommandTemplateInput")
        })
        .collect::<Vec<_>>();
    assert!(
        exact_route_dependencies
            .iter()
            .any(|path| path.ends_with("execution_template_inputs_are_bindable"))
            && exact_route_calls
                .iter()
                .any(|path| path.ends_with("execution_template_inputs_are_bindable"))
            && forbidden_policy_dependencies.is_empty(),
        "{exact_route_rel} must delegate template bindability to command_eligibility instead of importing input-binding policy DTOs; dependencies: {exact_route_dependencies:?}; calls: {exact_route_calls:?}; forbidden policy dependencies: {forbidden_policy_dependencies:?}"
    );

    let status_assembly = read_repo_file("src/execution/status_assembly.rs");
    assert!(
        !status_assembly.contains("exact_route_complete_template"),
        "complete-template verification policy must not live in status assembly"
    );
}

#[test]
fn review_dispatch_cycle_target_honors_public_close_route_before_stale_fallback() {
    let source = read_repo_file("src/execution/closure_dispatch.rs");
    let source_paths = normalized_dependency_paths("src/execution/closure_dispatch.rs", &source);
    assert!(
        !source.contains("pre_reducer_earliest_unresolved_stale_task"),
        "closure dispatch must not use bootstrap-only stale target selection after public route/status projection exists"
    );
    assert!(
        source_paths
            .iter()
            .any(|path| path.starts_with("crate::execution::stale_target_selection::"))
            && !source.contains("command_kind == \"reopen\""),
        "closure dispatch must delegate stale-boundary target ordering to stale_target_selection instead of deriving repair-target ordering locally"
    );
}

#[test]
fn stale_target_selection_has_single_shared_owner() {
    let owner_rel = "src/execution/stale_target_selection.rs";
    let owner = read_repo_file(owner_rel);
    let owner_functions = source_function_names(owner_rel, &owner)
        .into_iter()
        .filter(|name| name.starts_with("select_"))
        .collect::<Vec<_>>();
    assert!(
        owner_functions.len() >= 4,
        "{owner_rel} should remain the focused owner for stale-target selection helpers"
    );
    let owner_paths = normalized_dependency_paths(owner_rel, &owner);
    assert!(
        owner_paths
            .iter()
            .any(|path| path.starts_with("crate::execution::stale_target_projection::"))
            && owner_paths
                .iter()
                .any(|path| path.starts_with("crate::execution::task_scope_key::")),
        "{owner_rel} should derive stale-target ordering from stale-target projection facts and the shared task-scope parser"
    );

    let projection = read_repo_file("src/execution/stale_target_projection.rs");
    let projection_paths =
        normalized_dependency_paths("src/execution/stale_target_projection.rs", &projection);
    assert!(
        projection_paths
            .iter()
            .any(|path| path.starts_with("crate::execution::stale_target_selection::")),
        "stale target projection should consume the shared stale-target selector instead of owning local task-ordering logic"
    );
    let next_action_sources = route_plan_next_action_choice_sources();
    let next_action_paths = route_plan_next_action_choice_dependency_paths();
    assert!(
        next_action_paths
            .iter()
            .any(|path| path.starts_with("crate::execution::stale_target_selection::"))
            && next_action_sources.iter().all(|(rel, source)| {
                source_function_names(rel, source)
                    .iter()
                    .all(|name| !name.starts_with("stale_target"))
            }),
        "route_plan/next_action_choice modules must delegate stale-boundary tie-breaking to stale_target_selection instead of owning local stale-target helper functions"
    );

    let repair = read_repo_file("src/execution/repair_target_selection.rs");
    let repair_paths =
        normalized_dependency_paths("src/execution/repair_target_selection.rs", &repair);
    assert!(
        repair_paths
            .iter()
            .any(|path| path.starts_with("crate::execution::stale_target_selection::")),
        "repair target selection should delegate actionable stale-reentry ordering to the shared stale-target selector"
    );

    let repair_routes = read_repo_file("src/execution/repair_route_decision.rs");
    let repair_route_paths =
        normalized_dependency_paths("src/execution/repair_route_decision.rs", &repair_routes);
    assert!(
        repair_route_paths
            .iter()
            .any(|path| path.starts_with("crate::execution::stale_target_selection::")),
        "repair route decisions should use the shared stale-target selector boundary"
    );
    assert!(
        !repair_route_paths
            .iter()
            .any(|path| path == "crate::execution::task_scope_key::task_scope_key_task_number"),
        "repair route decisions should delegate stale and task-number tie-breakers to src/execution/stale_target_selection.rs"
    );

    let review_state = read_repo_file("src/execution/review_state.rs");
    let review_state_paths =
        normalized_dependency_paths("src/execution/review_state.rs", &review_state);
    assert!(
        review_state_paths
            .iter()
            .any(|path| path.starts_with("crate::execution::stale_target_selection::")),
        "review-state repair analysis should delegate branch stale source-task selection to src/execution/stale_target_selection.rs"
    );
}

#[test]
fn projected_public_status_stale_task_selection_is_owned_by_stale_target_selection() {
    let owner_rel = "src/execution/stale_target_selection.rs";
    let owner = read_repo_file(owner_rel);
    assert!(
        owner.contains("fn projected_earliest_stale_task_candidate_from_status("),
        "{owner_rel} should own projected public-status stale task ordering"
    );

    let query = read_repo_file("src/execution/query.rs");
    let route_semantics = read_repo_file("src/execution/route_plan/route_semantics.rs");
    assert!(
        !query.contains("fn projected_earliest_stale_task_candidate_from_status(")
            && normalized_dependency_paths(
                "src/execution/route_plan/route_semantics.rs",
                &route_semantics
            )
                .iter()
                .any(|path| path
                    == "crate::execution::stale_target_selection::projected_earliest_stale_task_candidate_from_status"),
        "route-plan route semantics must consume projected stale-target ordering from stale_target_selection instead of owning a duplicate selector in query/read-model code"
    );

    let status_support = read_repo_file("src/execution/status_support.rs");
    assert_no_import_path_prefix(
        "src/execution/status_support.rs",
        &status_support,
        &["crate::execution::query"],
        "status_support must not depend on query/read-model helpers for stale-target selection",
    );
    assert!(
        normalized_dependency_paths("src/execution/status_support.rs", &status_support)
            .iter()
            .any(|path| path
                == "crate::execution::stale_target_selection::projected_earliest_stale_task_candidate_from_status"),
        "status_support should consume projected stale-target ordering from stale_target_selection"
    );
}

#[test]
fn task_scope_key_parsing_has_single_owner() {
    let owner_rel = "src/execution/task_scope_key.rs";
    let owner = read_repo_file(owner_rel);
    let owner_functions = source_function_names(owner_rel, &owner);
    assert!(
        owner_functions.contains("task_scope_key_task_number")
            && owner_functions.contains("task_scope_key_for_task"),
        "{owner_rel} must own the named task-scope parsing and formatting boundary functions"
    );

    let mut violations = Vec::new();
    for path in rust_source_files(&repo_root().join("src/execution")) {
        let rel = repo_relative(&path);
        if rel == owner_rel {
            continue;
        }
        let source = read_repo_file(&rel);
        violations.extend(task_scope_key_local_prefix_parse_violations(&rel, &source));
        violations.extend(task_scope_key_numeric_fallback_violations(&rel, &source));
        violations.extend(task_scope_key_payload_fallback_violations(&rel, &source));
    }
    violations.sort();
    assert!(
        violations.is_empty(),
        "task-scope key parsing must delegate to src/execution/task_scope_key.rs:\n{}",
        violations.join("\n")
    );
}

#[test]
fn task_scope_key_numeric_fallback_scanner_covers_task_closure_payload_helpers() {
    let fixture = r#"
        fn task_number_from_task_closure_record(record_key: &str) -> Option<u32> {
            record_key.parse::<u32>().ok()
        }
    "#;
    let violations = task_scope_key_numeric_fallback_violations("fixture.rs", fixture);
    assert!(
        violations.iter().any(|violation| {
            violation.contains("task_number_from_task_closure_record")
                && violation.contains("record_key.parse::<u32>()")
        }),
        "task-scope numeric fallback scanner should reject raw numeric record-key parsing in task-closure payload helpers, got {violations:?}"
    );
}

#[test]
fn public_repair_target_projection_avoids_mutation_helpers() {
    let rel = "src/execution/public_repair_targets.rs";
    let source = read_repo_file(rel);
    let paths = normalized_dependency_paths(rel, &source);
    assert!(
        !paths
            .iter()
            .any(|path| path.starts_with("crate::execution::recording"))
            && !paths
                .iter()
                .any(|path| path.starts_with("crate::execution::commands")),
        "public repair-target projection must not import mutation command or recording helpers: {paths:?}"
    );
    assert!(
        paths
            .iter()
            .any(|path| path.starts_with("crate::execution::current_task_closure_cleanup::")),
        "public repair-target projection should consume read-only current task-closure cleanup helpers"
    );
    assert!(
        !source.contains("load_authoritative_transition_state")
            && !source.contains("active_worktree_lease_release_preview(")
            && !source.contains("claim_step_write_authority")
            && !source.contains("persist_if_dirty")
            && !source.contains("release_active_worktree_leases_with_locked_index"),
        "public repair-target projection must stay read-only and must not reload transition state or claim mutation authority"
    );

    let authority_rel = "src/execution/authority.rs";
    let authority = read_repo_file(authority_rel);
    let preview_from_authority_paths = rust_source_scan::normalized_call_paths_in_function(
        authority_rel,
        &authority,
        "active_worktree_lease_release_preview_from_authority",
    );
    let forbidden_authority_paths = [
        "load_mutable_harness_state_from_event_authority",
        "load_reduced_authoritative_state",
        "crate::execution::migration::ensure_event_log_migrated_from_legacy_state_with_route_parity",
        "crate::execution::event_log::append_typed_state_event_for_state_path",
        "persist_mutable_harness_state",
    ];
    assert!(
        !preview_from_authority_paths.iter().any(|path| {
            forbidden_authority_paths
                .iter()
                .any(|forbidden| path == forbidden || path.ends_with(&format!("::{forbidden}")))
        }),
        "authoritative public repair-target preview must derive from the preloaded state payload without migration or persistence"
    );

    let recording = read_repo_file("src/execution/recording.rs");
    let recording_paths = normalized_dependency_paths("src/execution/recording.rs", &recording);
    assert!(
        recording_paths
            .iter()
            .any(|path| { path.starts_with("crate::execution::current_task_closure_cleanup::") }),
        "recording mutation helpers should reuse the shared read-only current task-closure cleanup decisions"
    );
}

fn task_scope_key_local_prefix_parse_violations(rel: &str, source: &str) -> Vec<String> {
    let syntax = parse_rust_source(rel, source);
    let mut visitor = TaskScopeKeyBoundaryVisitor::new(rel);
    visitor.visit_file(&syntax);
    visitor.local_prefix_parse_violations
}

fn task_scope_key_numeric_fallback_violations(rel: &str, source: &str) -> Vec<String> {
    let syntax = parse_rust_source(rel, source);
    let mut visitor = TaskScopeKeyBoundaryVisitor::new(rel);
    visitor.visit_file(&syntax);
    visitor.numeric_fallback_violations
}

fn task_scope_key_payload_fallback_violations(rel: &str, source: &str) -> Vec<String> {
    let syntax = parse_rust_source(rel, source);
    let mut visitor = TaskScopeKeyBoundaryVisitor::new(rel);
    visitor.visit_file(&syntax);
    visitor.payload_fallback_violations
}

const TASK_SCOPE_KEY_LOCAL_PREFIX_METHODS: &[&str] = &[
    "strip_prefix",
    "starts_with",
    "trim_start_matches",
    "split_once",
    "rsplit_once",
    "split",
];

const TASK_SCOPE_KEY_NUMERIC_FALLBACK_RECEIVERS: &[&str] =
    &["record_key", "scope_key", "entry_key"];

struct TaskScopeKeyBoundaryVisitor<'a> {
    rel: &'a str,
    current_function: Vec<String>,
    local_prefix_parse_violations: Vec<String>,
    numeric_fallback_violations: Vec<String>,
    payload_fallback_violations: Vec<String>,
}

impl<'a> TaskScopeKeyBoundaryVisitor<'a> {
    fn new(rel: &'a str) -> Self {
        Self {
            rel,
            current_function: Vec::new(),
            local_prefix_parse_violations: Vec::new(),
            numeric_fallback_violations: Vec::new(),
            payload_fallback_violations: Vec::new(),
        }
    }

    fn current_function_name(&self) -> &str {
        self.current_function
            .last()
            .map_or("<module>", String::as_str)
    }

    fn visit_function_block(&mut self, name: String, block: &syn::Block) {
        self.current_function.push(name);
        self.visit_block(block);
        self.current_function.pop();
    }
}

impl<'ast> Visit<'ast> for TaskScopeKeyBoundaryVisitor<'_> {
    fn visit_item_fn(&mut self, item_fn: &'ast syn::ItemFn) {
        if item_attrs_mark_test_only(&item_fn.attrs) {
            return;
        }
        self.visit_function_block(item_fn.sig.ident.to_string(), &item_fn.block);
    }

    fn visit_impl_item_fn(&mut self, item_fn: &'ast syn::ImplItemFn) {
        if item_attrs_mark_test_only(&item_fn.attrs) {
            return;
        }
        self.visit_function_block(item_fn.sig.ident.to_string(), &item_fn.block);
    }

    fn visit_item_mod(&mut self, item_mod: &'ast syn::ItemMod) {
        if item_attrs_mark_test_only(&item_mod.attrs) {
            return;
        }
        visit::visit_item_mod(self, item_mod);
    }

    fn visit_item_const(&mut self, item_const: &'ast syn::ItemConst) {
        let name = item_const.ident.to_string();
        if matches!(name.as_str(), "TASK_SCOPE_KEY_PREFIX" | "TASK_PREFIX")
            || string_literal_expr_value(&item_const.expr).as_deref() == Some("task-")
        {
            self.local_prefix_parse_violations.push(format!(
                "{}:{} defines local task-scope prefix constant `{name}`",
                self.rel,
                item_const.ident.span().start().line
            ));
        }
        visit::visit_item_const(self, item_const);
    }

    fn visit_item_static(&mut self, item_static: &'ast syn::ItemStatic) {
        let name = item_static.ident.to_string();
        if matches!(name.as_str(), "TASK_SCOPE_KEY_PREFIX" | "TASK_PREFIX")
            || string_literal_expr_value(&item_static.expr).as_deref() == Some("task-")
        {
            self.local_prefix_parse_violations.push(format!(
                "{}:{} defines local task-scope prefix static `{name}`",
                self.rel,
                item_static.ident.span().start().line
            ));
        }
        visit::visit_item_static(self, item_static);
    }

    fn visit_expr_method_call(&mut self, method_call: &'ast syn::ExprMethodCall) {
        let method = method_call.method.to_string();
        if TASK_SCOPE_KEY_LOCAL_PREFIX_METHODS.contains(&method.as_str())
            && method_call
                .args
                .iter()
                .any(|arg| string_literal_expr_value(arg).as_deref() == Some("task-"))
        {
            self.local_prefix_parse_violations.push(format!(
                "{}:{} function `{}` performs local task-scope prefix parsing with `{method}(\"task-\")`",
                self.rel,
                method_call.method.span().start().line,
                self.current_function_name()
            ));
        }
        if method == "len"
            && string_literal_expr_value(&method_call.receiver).as_deref() == Some("task-")
        {
            self.local_prefix_parse_violations.push(format!(
                "{}:{} function `{}` computes a local task-scope prefix length",
                self.rel,
                method_call.method.span().start().line,
                self.current_function_name()
            ));
        }
        if method == "parse"
            && parse_turbofish_contains_u32(method_call)
            && expr_path_ident(&method_call.receiver)
                .as_deref()
                .is_some_and(|receiver| {
                    TASK_SCOPE_KEY_NUMERIC_FALLBACK_RECEIVERS.contains(&receiver)
                })
        {
            let receiver = expr_path_ident(&method_call.receiver).unwrap_or_default();
            self.numeric_fallback_violations.push(format!(
                "{}:{} function `{}` accepts a raw numeric task-scope key from `{receiver}.parse::<u32>()`",
                self.rel,
                method_call.method.span().start().line,
                self.current_function_name()
            ));
        }
        if self.current_function_name().contains("task_closure_record")
            && method == "get"
            && method_call.args.iter().any(|arg| {
                matches!(
                    string_literal_expr_value(arg).as_deref(),
                    Some("task" | "task_number")
                )
            })
        {
            self.payload_fallback_violations.push(format!(
                "{}:{} function `{}` reads task payload fields while deriving task-scope keys",
                self.rel,
                method_call.method.span().start().line,
                self.current_function_name()
            ));
        }
        visit::visit_expr_method_call(self, method_call);
    }
}

fn parse_turbofish_contains_u32(method_call: &syn::ExprMethodCall) -> bool {
    method_call.turbofish.as_ref().is_some_and(|args| {
        args.args.iter().any(|arg| {
            matches!(
                arg,
                syn::GenericArgument::Type(syn::Type::Path(path))
                    if syn_path_to_string(&path.path) == "u32"
            )
        })
    })
}

#[test]
fn review_route_decision_tokens_are_centralized() {
    let token_owner = read_repo_file("src/execution/review_route_tokens.rs");
    let tokens = review_route_decision_tokens(&token_owner)
        .into_iter()
        .collect::<BTreeSet<_>>();
    assert!(
        !tokens.is_empty(),
        "src/execution/review_route_tokens.rs should define the review/follow-up route tokens scanned by this boundary"
    );
    let mut violations = Vec::new();
    for rel in review_route_decision_token_scan_sources() {
        let source = read_repo_file(&rel);
        violations.extend(
            rust_production_string_literal_values(&rel, &source)
                .into_iter()
                .filter(|literal| tokens.contains(literal.as_str()))
                .map(|literal| {
                    format!(
                        "{rel} duplicates review/follow-up route token literal `{literal}` outside src/execution/review_route_tokens.rs"
                    )
                }),
        );
    }
    violations.sort();
    violations.dedup();
    assert!(
        violations.is_empty(),
        "review-state and follow-up route decision tokens must be sourced from src/execution/review_route_tokens.rs:\n{}",
        violations.join("\n")
    );

    let observability_owner = read_repo_file("src/execution/observability.rs");
    let observability_tokens = observability_route_tokens(&observability_owner)
        .into_iter()
        .collect::<BTreeSet<_>>();
    assert!(
        !observability_tokens.is_empty(),
        "src/execution/observability.rs should define observability route tokens scanned by this boundary"
    );
    let mut observability_violations = Vec::new();
    for rel in review_route_decision_token_scan_sources()
        .into_iter()
        .filter(|rel| rel != "src/execution/observability.rs")
    {
        let source = read_repo_file(&rel);
        observability_violations.extend(
            rust_production_string_literal_values(&rel, &source)
                .into_iter()
                .filter(|literal| observability_tokens.contains(literal.as_str()))
                .map(|literal| {
                    format!(
                        "{rel} duplicates observability route token literal `{literal}` outside src/execution/observability.rs"
                    )
                }),
        );
    }
    observability_violations.sort();
    observability_violations.dedup();
    assert!(
        observability_violations.is_empty(),
        "observability route decision tokens must be sourced from src/execution/observability.rs:\n{}",
        observability_violations.join("\n")
    );
}

fn review_route_decision_tokens(source: &str) -> Vec<String> {
    source_string_const_values(
        "src/execution/review_route_tokens.rs",
        source,
        &[
            "REVIEW_STATE_STALE_UNREVIEWED",
            "REVIEW_STATE_MISSING_CURRENT_CLOSURE",
            "FOLLOW_UP_REPAIR_REVIEW_STATE",
            "FOLLOW_UP_ADVANCE_LATE_STAGE",
            "FOLLOW_UP_EXECUTION_REENTRY",
            "REASON_NEGATIVE_RESULT_REQUIRES_EXECUTION_REENTRY",
        ],
    )
}

fn observability_route_tokens(source: &str) -> Vec<String> {
    source_string_const_values(
        "src/execution/observability.rs",
        source,
        &[
            "REASON_CODE_STALE_PROVENANCE",
            "REASON_CODE_BLOCKED_ON_PLAN_REVISION",
        ],
    )
}

fn review_route_decision_token_scan_sources() -> Vec<String> {
    let mut rels = Vec::new();
    for root in ["src/execution", "src/workflow"] {
        rels.extend(
            rust_source_files(&repo_root().join(root))
                .into_iter()
                .map(|path| repo_relative(&path))
                .filter(|rel| rel != "src/execution/review_route_tokens.rs"),
        );
    }
    rels.sort();
    rels
}

fn normalized_expanded_use_paths(rel: &str, source: &str) -> Vec<String> {
    rust_source_scan::normalized_expanded_use_paths(rel, source)
}

fn parse_rust_source(rel: &str, source: &str) -> syn::File {
    rust_source_scan::parse_rust_source(rel, source)
}

fn const_path_leafs(rel: &str, source: &str, const_name: &str) -> BTreeSet<String> {
    let syntax = parse_rust_source(rel, source);
    let mut leafs = BTreeSet::new();
    for item in syntax.items {
        let syn::Item::Const(item_const) = item else {
            continue;
        };
        if item_const.ident != const_name {
            continue;
        }
        let mut visitor = PathLeafCollector { leafs: &mut leafs };
        visitor.visit_expr(&item_const.expr);
        return leafs;
    }
    panic!("{rel} should define const `{const_name}`");
}

struct PathLeafCollector<'a> {
    leafs: &'a mut BTreeSet<String>,
}

impl<'ast> Visit<'ast> for PathLeafCollector<'_> {
    fn visit_path(&mut self, path: &'ast syn::Path) {
        if let Some(segment) = path.segments.last() {
            self.leafs.insert(segment.ident.to_string());
        }
        visit::visit_path(self, path);
    }
}

fn source_function_names(rel: &str, source: &str) -> BTreeSet<String> {
    rust_source_scan::function_spans(rel, source)
        .into_iter()
        .map(|span| span.name)
        .collect()
}

fn source_module_names(rel: &str, source: &str) -> BTreeSet<String> {
    syn::parse_file(source)
        .unwrap_or_else(|error| panic!("{rel} should parse as Rust: {error}"))
        .items
        .into_iter()
        .filter_map(|item| match item {
            syn::Item::Mod(module) => Some(module.ident.to_string()),
            _ => None,
        })
        .collect()
}

fn source_struct_names(rel: &str, source: &str) -> BTreeSet<String> {
    syn::parse_file(source)
        .unwrap_or_else(|error| panic!("{rel} should parse as Rust: {error}"))
        .items
        .into_iter()
        .filter_map(|item| match item {
            syn::Item::Struct(item_struct) => Some(item_struct.ident.to_string()),
            _ => None,
        })
        .collect()
}

fn source_struct_field_names(rel: &str, source: &str, struct_name: &str) -> BTreeSet<String> {
    for item in syn::parse_file(source)
        .unwrap_or_else(|error| panic!("{rel} should parse as Rust: {error}"))
        .items
    {
        let syn::Item::Struct(item_struct) = item else {
            continue;
        };
        if item_struct.ident != struct_name {
            continue;
        }
        let syn::Fields::Named(fields) = item_struct.fields else {
            return BTreeSet::new();
        };
        return fields
            .named
            .into_iter()
            .filter_map(|field| field.ident.map(|ident| ident.to_string()))
            .collect();
    }
    panic!("{rel} should define struct `{struct_name}`");
}

fn source_enum_names(rel: &str, source: &str) -> BTreeSet<String> {
    syn::parse_file(source)
        .unwrap_or_else(|error| panic!("{rel} should parse as Rust: {error}"))
        .items
        .into_iter()
        .filter_map(|item| match item {
            syn::Item::Enum(item_enum) => Some(item_enum.ident.to_string()),
            _ => None,
        })
        .collect()
}

fn source_type_alias_names(rel: &str, source: &str) -> BTreeSet<String> {
    syn::parse_file(source)
        .unwrap_or_else(|error| panic!("{rel} should parse as Rust: {error}"))
        .items
        .into_iter()
        .filter_map(|item| match item {
            syn::Item::Type(item_type) => Some(item_type.ident.to_string()),
            _ => None,
        })
        .collect()
}

fn source_string_const_values(rel: &str, source: &str, const_names: &[&str]) -> Vec<String> {
    let wanted = const_names.iter().copied().collect::<BTreeSet<_>>();
    let mut values = syn::parse_file(source)
        .unwrap_or_else(|error| panic!("{rel} should parse as Rust: {error}"))
        .items
        .into_iter()
        .filter_map(|item| {
            let syn::Item::Const(item_const) = item else {
                return None;
            };
            let name = item_const.ident.to_string();
            wanted
                .contains(name.as_str())
                .then(|| string_literal_expr_value(&item_const.expr))
                .flatten()
        })
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    assert_eq!(
        values.len(),
        const_names.len(),
        "{rel} should expose string literal values for all requested constants: {const_names:?}"
    );
    values
}

fn source_production_function_visibilities(rel: &str, source: &str) -> BTreeMap<String, String> {
    syn::parse_file(source)
        .unwrap_or_else(|error| panic!("{rel} should parse as Rust: {error}"))
        .items
        .into_iter()
        .filter_map(|item| match item {
            syn::Item::Fn(item_fn) if !item_attrs_mark_test_only(&item_fn.attrs) => Some((
                item_fn.sig.ident.to_string(),
                source_visibility_label(&item_fn.vis),
            )),
            _ => None,
        })
        .collect()
}

fn source_visibility_label(visibility: &syn::Visibility) -> String {
    match visibility {
        syn::Visibility::Inherited => String::from("private"),
        syn::Visibility::Public(_) => String::from("pub"),
        syn::Visibility::Restricted(restricted) => {
            format!("pub(in {})", syn_path_to_string(&restricted.path))
        }
    }
}

fn function_calls_path(rel: &str, source: &str, function_name: &str, called_path: &str) -> bool {
    rust_source_scan::normalized_call_paths_in_function(rel, source, function_name)
        .into_iter()
        .any(|path| path == called_path)
}

fn function_body_source_contains(
    rel: &str,
    source: &str,
    function_name: &str,
    needle: &str,
) -> bool {
    let span = rust_source_scan::function_spans(rel, source)
        .into_iter()
        .find(|span| span.name == function_name)
        .unwrap_or_else(|| panic!("{rel} should define function `{function_name}`"));
    source
        .lines()
        .skip(span.start_line.saturating_sub(1))
        .take(span.end_line.saturating_sub(span.start_line) + 1)
        .any(|line| line.contains(needle))
}

fn function_calls_leaf(rel: &str, source: &str, function_name: &str, leaf: &str) -> bool {
    rust_source_scan::normalized_call_paths_in_function(rel, source, function_name)
        .into_iter()
        .any(|path| import_leaf_name(&path) == leaf)
}

fn source_constructs_struct_literal(rel: &str, source: &str, struct_name: &str) -> bool {
    let syntax = parse_rust_source(rel, source);
    let mut visitor = StructLiteralVisitor {
        struct_name,
        found: false,
    };
    visitor.visit_file(&syntax);
    visitor.found
}

struct StructLiteralVisitor<'a> {
    struct_name: &'a str,
    found: bool,
}

impl<'ast> Visit<'ast> for StructLiteralVisitor<'_> {
    fn visit_expr_struct(&mut self, expr_struct: &'ast syn::ExprStruct) {
        if expr_struct
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == self.struct_name)
        {
            self.found = true;
        }
        visit::visit_expr_struct(self, expr_struct);
    }
}

fn call_path_leaf_violations(sources: &[(String, String)], leaf: &str) -> Vec<String> {
    sources
        .iter()
        .flat_map(|(rel, source)| {
            rust_source_scan::normalized_call_path_hits(rel, source, &[])
                .into_iter()
                .filter(move |hit| hit.path.rsplit("::").next() == Some(leaf))
                .map(move |hit| format!("{rel}:{} calls `{}`", hit.line, hit.path))
        })
        .collect()
}

fn direct_recommended_command_some_violations(rel: &str, source: &str) -> Vec<String> {
    let syntax = parse_rust_source(rel, source);
    let mut visitor = DirectRecommendedCommandSomeVisitor {
        rel,
        violations: Vec::new(),
    };
    visitor.visit_file(&syntax);
    visitor.violations
}

struct DirectRecommendedCommandSomeVisitor<'a> {
    rel: &'a str,
    violations: Vec<String>,
}

impl<'ast> Visit<'ast> for DirectRecommendedCommandSomeVisitor<'_> {
    fn visit_field_value(&mut self, field: &'ast syn::FieldValue) {
        if member_name(&field.member).as_deref() == Some("recommended_command")
            && expr_is_some_constructor(&field.expr)
        {
            self.violations.push(format!(
                "{} initializes `recommended_command` directly with `Some(...)`",
                self.rel
            ));
        }
        visit::visit_field_value(self, field);
    }

    fn visit_expr_assign(&mut self, assignment: &'ast syn::ExprAssign) {
        if expr_field_member_name(&assignment.left).as_deref() == Some("recommended_command")
            && expr_is_some_constructor(&assignment.right)
        {
            self.violations.push(format!(
                "{} assigns `recommended_command` directly from `Some(...)`",
                self.rel
            ));
        }
        visit::visit_expr_assign(self, assignment);
    }
}

fn expr_field_member_name(expr: &syn::Expr) -> Option<String> {
    match expr {
        syn::Expr::Field(field) => member_name(&field.member),
        syn::Expr::Reference(reference) => expr_field_member_name(&reference.expr),
        syn::Expr::Group(group) => expr_field_member_name(&group.expr),
        syn::Expr::Paren(paren) => expr_field_member_name(&paren.expr),
        _ => None,
    }
}

fn expr_is_some_constructor(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::Call(call) => call
            .path_last_segment_ident()
            .is_some_and(|ident| ident == "Some"),
        syn::Expr::Reference(reference) => expr_is_some_constructor(&reference.expr),
        syn::Expr::Group(group) => expr_is_some_constructor(&group.expr),
        syn::Expr::Paren(paren) => expr_is_some_constructor(&paren.expr),
        _ => false,
    }
}

fn late_stage_mode_variant_patterns() -> [&'static str; 7] {
    [
        "PublicAdvanceLateStageMode::Basic",
        "PublicAdvanceLateStageMode::ReleaseReadiness",
        "PublicAdvanceLateStageMode::FinalReviewDispatch",
        "PublicAdvanceLateStageMode::Qa",
        "PublicAdvanceLateStageMode::FinalReview",
        "PublicAdvanceLateStageMode::FinishReview",
        "PublicAdvanceLateStageMode::FinishCompletion",
    ]
}

fn syn_path_to_string(path: &syn::Path) -> String {
    rust_source_scan::syn_path_to_string(path)
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

fn status_projection_route_selection_symbol_names() -> BTreeSet<String> {
    let mut names = source_function_names(
        "src/execution/route_plan/constructors.rs",
        &read_repo_file("src/execution/route_plan/constructors.rs"),
    );
    names.extend(source_function_names(
        "src/execution/route_plan/stale_repair_target.rs",
        &read_repo_file("src/execution/route_plan/stale_repair_target.rs"),
    ));
    assert!(
        !names.is_empty(),
        "status-projection boundary guard must derive route constructor and stale-selector names from non-empty owner modules"
    );
    names
}

fn assert_status_projection_has_no_route_selection_symbols(source: &str) {
    let rel = "src/execution/route_plan/status_projection.rs";
    let forbidden_names = status_projection_route_selection_symbol_names();
    let forbidden_refs = forbidden_names
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let forbidden_imports = normalized_dependency_paths(rel, source)
        .into_iter()
        .filter(|path| forbidden_names.contains(import_leaf_name(path)))
        .map(|path| format!("{rel} imports route-selection symbol `{path}`"))
        .collect::<Vec<_>>();
    let forbidden_calls =
        rust_source_scan::forbidden_call_violations(rel, source, &forbidden_refs, &[]);
    let forbidden_local_helpers = source_function_names(rel, source)
        .into_iter()
        .filter(|name| forbidden_names.contains(name))
        .map(|name| format!("{rel} defines local route-selection helper `{name}`"))
        .collect::<Vec<_>>();
    let mut violations = forbidden_imports;
    violations.extend(forbidden_calls);
    violations.extend(forbidden_local_helpers);
    violations.sort();
    violations.dedup();
    assert!(
        violations.is_empty(),
        "{rel} must remain route-neutral and must not import, call, or locally redefine route constructors or stale-target selectors:\n{}",
        violations.join("\n")
    );
}

fn assert_route_plan_status_projection_remains_route_neutral(source: &str) {
    assert_status_projection_has_no_route_selection_symbols(source);
    let rel = "src/execution/route_plan/status_projection.rs";
    assert_no_import_path_prefix(
        rel,
        source,
        &["crate::execution::repair_route_decision"],
        "must not import route-authority reconstruction helpers",
    );
    let dependency_paths = normalized_dependency_paths(rel, source);
    let code_paths = normalized_code_paths(rel, source);
    let call_paths = rust_source_scan::normalized_call_paths(rel, source, &[]);
    let status_mutations = status_field_mutations_in_module(rel, source);
    let route_decision_mutations =
        field_mutations_on_base_ident_in_module(rel, source, "route_decision");
    let constructs_repair_target =
        source_constructs_struct_literal(rel, source, "PublicRepairTarget");
    assert!(
        (!route_decision_mutations.is_empty()
            && route_decision_mutations
                .iter()
                .all(|field| field != "phase" && field != "phase_detail"))
            && dependency_paths
                .iter()
                .any(|path| path.starts_with("crate::execution::public_repair_targets::"))
            && call_paths
                .iter()
                .any(|path| path.ends_with("public_repair_targets_for_route_decision"))
            && !constructs_repair_target
            && !code_paths
                .iter()
                .any(|path| path.ends_with("PublicRepairTargetReason"))
            && !code_paths
                .iter()
                .any(|path| path.ends_with("RoutePlanningFacts"))
            && !status_mutations.contains("blocking_scope")
            && !status_mutations.contains("blocking_task"),
        "route_plan/status_projection.rs must remain route-neutral: it may enrich selected-route follow-up diagnostics and install centralized public repair targets, but must not construct repair-target literals, select stale targets, construct routes, or mutate route-control blockers"
    );
}

fn assert_public_route_projection_installs_router_finalized_status(source: &str) {
    let rel = "src/execution/read_model/public_route_projection.rs";
    let call_paths = rust_source_scan::normalized_call_paths(rel, source, &[]);
    let code_paths = normalized_code_paths(rel, source);
    assert!(
        call_paths
            .iter()
            .any(|path| path.ends_with("project_final_runtime_routing_projection"))
            && !call_paths
                .iter()
                .any(|path| path.ends_with("apply_common_route_status_projection"))
            && !call_paths
                .iter()
                .any(|path| path.ends_with("apply_route_status_projection_diagnostics"))
            && !code_paths
                .iter()
                .any(|path| path.ends_with("RouteStatusProjectionInput"))
            && !source_constructs_struct_literal(rel, source, "RouteDecision")
            && !code_paths
                .iter()
                .any(|path| path.ends_with("DocumentReleasePending"))
            && !code_paths
                .iter()
                .any(|path| path.ends_with("FinalReviewPending"))
            && !code_paths.iter().any(|path| path.ends_with("QaPending"))
            && !code_paths
                .iter()
                .any(|path| path.ends_with("ReadyForBranchCompletion")),
        "read-model public route projection must install router-finalized status projection instead of rebuilding route/status projection locally"
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
        &[
            "crate::execution::commands",
            "crate::execution::mutate",
            "crate::execution::route_plan::constructors",
            "crate::execution::route_plan::decision_support",
            "crate::execution::route_plan::next_action_choice",
            "crate::execution::route_plan::planning_facts",
            "crate::execution::route_plan::route_facts",
            "crate::execution::route_plan::status_application",
            "crate::execution::route_plan::status_projection",
        ],
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
fn event_log_migration_stays_below_routing_boundary() {
    let rel = "src/execution/event_log.rs";
    let event_log = read_repo_file(rel);
    assert_no_import_path_prefix(
        rel,
        &event_log,
        &["crate::execution::router", "crate::execution::route_plan"],
        "event-log persistence, replay, and migration must not import route decision modules",
    );
    for forbidden in ["RouteDecision", "PublicRouteDecision"] {
        assert!(
            !event_log.contains(forbidden),
            "event_log.rs must not compute or serialize route decisions directly: found `{forbidden}`"
        );
    }

    let migration = read_repo_file("src/execution/migration.rs");
    assert!(
        migration.contains("event_log::legacy_migration_parity_candidate_for_state_path")
            && migration.contains("route_decision_from_runtime_state_with_authority"),
        "route parity for legacy migrations must live in the higher-level migration adapter, not event_log.rs"
    );
}

#[test]
fn legacy_event_log_migration_is_only_called_by_migration_adapter() {
    let allowed = BTreeSet::from([
        String::from("src/execution/event_log.rs"),
        String::from("src/execution/migration.rs"),
    ]);
    let forbidden = "crate::execution::event_log::ensure_event_log_migrated_from_legacy_state";
    let mut violations = Vec::new();
    for path in rust_source_files(&repo_root().join("src/execution")) {
        let rel = repo_relative(&path);
        if allowed.contains(&rel) {
            continue;
        }
        let source = read_repo_file(&rel);
        let paths = normalized_code_paths(&rel, &source);
        if paths.iter().any(|path| path == forbidden) {
            violations.push(rel);
        }
    }
    assert!(
        violations.is_empty(),
        "production code must route legacy state migration through src/execution/migration.rs so route parity runs before non-migration events are appended:\n{}",
        violations.join("\n")
    );
}

#[test]
fn repair_reentry_route_semantics_use_shared_decision_object() {
    let shared = read_repo_file("src/execution/repair_route_decision.rs");
    let stale_projection = read_repo_file("src/execution/stale_target_projection.rs");
    let shared_structs = source_struct_names("src/execution/repair_route_decision.rs", &shared);
    let shared_enums = source_enum_names("src/execution/repair_route_decision.rs", &shared);
    let shared_paths =
        normalized_dependency_paths("src/execution/repair_route_decision.rs", &shared);
    let repair_route_decision_paths = repair_route_decision_dependency_paths();
    let repair_route_decision_structs = repair_route_decision_struct_names();
    let stale_projection_functions = source_function_names(
        "src/execution/stale_target_projection.rs",
        &stale_projection,
    );
    assert!(
        shared_structs.contains("RepairFollowUpDecision")
            && shared_enums.contains("RepairBlockerKind")
            && shared_paths
                .iter()
                .any(|path| path.starts_with("crate::execution::current_truth::"))
            && shared_paths
                .iter()
                .any(|path| path.starts_with("crate::execution::follow_up::")),
        "repair/reentry route semantics must stay centralized in repair_route_decision.rs; the shared decision object names are the boundary types consumed across route surfaces"
    );
    assert!(
        repair_route_decision_structs.contains("TaskClosureBaselineBridgeRouteDecision")
            && repair_route_decision_paths
                .iter()
                .any(|path| path.starts_with("crate::execution::repair_target_selection::"))
            && repair_route_decision_paths
                .iter()
                .any(|path| path.starts_with("crate::execution::stale_target_projection::")),
        "task-closure baseline bridge task/readiness decisions must live under repair_route_decision while consuming repair-target and stale-target owners; the decision object is the shared boundary type"
    );
    assert!(
        stale_projection_functions
            .iter()
            .filter(|function| function.contains("bridge"))
            .count()
            >= 3,
        "stale-target task-closure bridge eligibility must have one focused implementation family in stale_target_projection.rs"
    );

    let status_assembly = read_repo_file("src/execution/status_assembly.rs");
    let status_assembly_paths =
        normalized_code_paths("src/execution/status_assembly.rs", &status_assembly);
    assert!(
        status_assembly_paths
            .iter()
            .any(|path| path.starts_with("crate::execution::repair_route_decision::")),
        "status assembly must consume the shared repair follow-up decision and authority inputs instead of recomputing execution reentry"
    );
    let public_projection = read_repo_file("src/execution/read_model/public_route_projection.rs");
    let public_projection_paths = normalized_dependency_paths(
        "src/execution/read_model/public_route_projection.rs",
        &public_projection,
    );
    assert!(
        public_projection.contains("status_projection")
            && !public_projection.contains("execution_reentry_target_source")
            && !public_projection_paths
                .iter()
                .any(|path| path.starts_with("crate::execution::repair_route_decision::"))
            && !public_projection_paths
                .iter()
                .any(|path| path.starts_with("crate::execution::repair_target_selection::")),
        "public route projection must install router-finalized status instead of copying or selecting execution reentry targets"
    );
    let route_plan = read_repo_file("src/execution/route_plan.rs");
    let route_plan_next_action = read_repo_file("src/execution/route_plan/next_action_route.rs");
    let route_plan_paths = normalized_code_paths("src/execution/route_plan.rs", &route_plan);
    let route_plan_next_action_paths = normalized_dependency_paths(
        "src/execution/route_plan/next_action_route.rs",
        &route_plan_next_action,
    );
    assert!(
        route_plan_paths
            .iter()
            .chain(route_plan_next_action_paths.iter())
            .any(|path| path.starts_with("crate::execution::repair_route_decision::")),
        "route_plan must consume shared stale-target authority, task-closure bridge helpers, and own route-plan reentry target-source selection"
    );
    let review_state = read_repo_file("src/execution/review_state.rs");
    let review_state_paths = normalized_code_paths("src/execution/review_state.rs", &review_state);
    assert!(
        review_state_paths
            .iter()
            .any(|path| path.starts_with("crate::execution::repair_route_decision::"))
            && review_state_paths.iter().any(|path| {
                path.starts_with("crate::execution::route_plan::")
                    || path == "crate::execution::route_plan"
            }),
        "repair-review-state must consume shared repair target classification, follow-up selection, and router close-current-task route semantics"
    );
    for (rel, source) in [
        (
            "src/execution/router.rs",
            read_repo_file("src/execution/router.rs"),
        ),
        (
            "src/execution/route_plan.rs",
            read_repo_file("src/execution/route_plan.rs"),
        ),
    ] {
        let local_bridge_helpers = rust_source_scan::function_spans(rel, &source)
            .into_iter()
            .filter(|function| {
                function.name.contains("bridge")
                    && (function.name.contains("stale")
                        || function.name.contains("closure")
                        || function.name.contains("dispatch"))
            })
            .collect::<Vec<_>>();
        assert!(
            local_bridge_helpers.is_empty(),
            "{rel} must not define local task-closure bridge selectors; consume shared repair decision modules instead: {local_bridge_helpers:?}"
        );
    }

    let repair_target_selection = read_repo_file("src/execution/repair_target_selection.rs");
    let repair_target_selection_paths = normalized_code_paths(
        "src/execution/repair_target_selection.rs",
        &repair_target_selection,
    );
    assert!(
        repair_target_selection_paths
            .iter()
            .any(|path| path.starts_with("crate::execution::stale_target_projection::"))
            && repair_route_decision_code_paths()
                .iter()
                .any(|path| path.starts_with("crate::execution::stale_target_projection::")),
        "repair target selection and repair-route decisions must consume the shared stale-target bridge eligibility helper"
    );

    for rel in [
        "src/execution/read_model.rs",
        "src/execution/read_model/public_route_projection.rs",
        "src/execution/review_state.rs",
    ] {
        let source = read_repo_file(rel);
        assert_no_import_path_prefix(
            rel,
            &source,
            &["crate::execution::repair_target_selection"],
            "must not select repair/reentry targets directly; use the shared route-owned authority path",
        );
        assert!(
            !source.contains("NextActionAuthorityInputs::default()"),
            "{rel} must not classify repair/reentry routes with default authority inputs"
        );
    }
}

#[test]
fn public_route_command_construction_stays_under_route_plan_owner() {
    for rel in [
        "src/execution/router.rs",
        "src/execution/route_plan.rs",
        "src/execution/next_action.rs",
    ] {
        let source = read_repo_file(rel);
        let paths = normalized_code_paths(rel, &source);
        assert!(
            !paths
                .iter()
                .any(|path| path == "crate::execution::command_eligibility::PublicCommand::Reopen"),
            "{rel} must not synthesize reopen commands locally; route_plan/public_commands.rs owns public command binding"
        );
    }

    let owner = read_repo_file("src/execution/route_plan/public_commands.rs");
    let owner_paths = normalized_code_paths("src/execution/route_plan/public_commands.rs", &owner);
    assert!(
        owner_paths
            .iter()
            .any(|path| path == "crate::execution::command_eligibility::PublicCommand::Reopen"),
        "route_plan/public_commands.rs must own reopen public-command construction"
    );
}

#[test]
fn public_route_decision_rules_have_focused_module_owners() {
    let retired_route_module_path = repo_root().join("src/execution/public_route_selection.rs");
    let execution_mod = read_repo_file("src/execution/mod.rs");
    assert!(
        !retired_route_module_path.exists() && !execution_mod.contains("public_route_selection"),
        "the retired public_route_selection marker must stay deleted; route_plan owns production route ordering, route projection, and public command binding"
    );
    let next_action = read_repo_file("src/execution/next_action.rs");
    let next_action_paths = normalized_code_paths("src/execution/next_action.rs", &next_action);
    let next_action_functions = source_function_names("src/execution/next_action.rs", &next_action);
    assert!(
        next_action_functions.is_empty()
            && next_action_paths
                .iter()
                .any(|path| path.starts_with("crate::execution::route_plan::next_action_choice::"))
            && !next_action_paths
                .iter()
                .any(|path| path.starts_with("crate::execution::command_eligibility::")),
        "next_action.rs must stay a display/type facade; ordered route choice and public command binding belong to route_plan"
    );
    let route_plan_next_action_choice =
        read_repo_file("src/execution/route_plan/next_action_choice.rs");
    let route_plan_next_action_choice_modules = source_module_names(
        "src/execution/route_plan/next_action_choice.rs",
        &route_plan_next_action_choice,
    );
    let route_plan_next_action_choice_visibilities = source_production_function_visibilities(
        "src/execution/route_plan/next_action_choice.rs",
        &route_plan_next_action_choice,
    );
    assert!(
        route_plan_next_action_choice_visibilities
            .iter()
            .filter(|(_, visibility)| visibility.as_str() != "private")
            .all(|(_, visibility)| visibility == "pub(in crate::execution::route_plan)")
            && route_plan_next_action_choice_visibilities
                .values()
                .any(|visibility| visibility == "pub(in crate::execution::route_plan)")
            && !route_plan_next_action_choice.contains("PublicCommand"),
        "route_plan/next_action_choice.rs must keep its decision engine route-plan-local and avoid public-command binding. Modules: {route_plan_next_action_choice_modules:?}"
    );
    let mut direct_next_action_dependencies = Vec::new();
    for path in rust_source_files(&repo_root().join("src/execution")) {
        let rel = repo_relative(&path);
        if rel == "src/execution/next_action.rs"
            || rel.starts_with("src/execution/route_plan")
            || rel.ends_with("/unit_tests.rs")
        {
            continue;
        }
        let source = read_repo_file(&rel);
        let paths = normalized_code_paths(&rel, &source);
        direct_next_action_dependencies.extend(
            paths
                .into_iter()
                .filter(|path| path.starts_with("crate::execution::route_plan::next_action_choice"))
                .map(|path| {
                    format!("{rel} depends on route-plan-local next-action choice path `{path}`")
                }),
        );
    }
    assert!(
        direct_next_action_dependencies.is_empty(),
        "production modules outside route_plan must not depend on route-plan-local next-action choice helpers; consume route_plan-owned route/command projection instead: {direct_next_action_dependencies:#?}"
    );
    let route_plan_next_action = read_repo_file("src/execution/route_plan/next_action_route.rs");
    let route_plan_next_action_paths = normalized_code_paths(
        "src/execution/route_plan/next_action_route.rs",
        &route_plan_next_action,
    );
    assert!(
        route_plan_next_action_paths.iter().any(
            |path| path.starts_with("crate::execution::route_plan::next_action_finalization::")
        ) && route_plan_next_action_paths
            .iter()
            .any(|path| path.ends_with("::next_action::NextActionDecision"))
            && !route_plan_next_action_paths
                .iter()
                .any(|path| path == "crate::execution::command_eligibility::PublicCommand"),
        "route_plan/next_action_route.rs must own shared next-action decision finalization through the route-plan finalization boundary without binding public commands inline. Dependencies: {route_plan_next_action_paths:?}"
    );
    let next_action_finalization =
        read_repo_file("src/execution/route_plan/next_action_finalization.rs");
    let next_action_finalization_paths = normalized_code_paths(
        "src/execution/route_plan/next_action_finalization.rs",
        &next_action_finalization,
    );
    assert!(
        next_action_finalization_paths
            .iter()
            .any(|path| path == "crate::execution::next_action::NextActionDecision")
            && next_action_finalization_paths
                .iter()
                .any(|path| path == "crate::execution::command_eligibility::PublicCommand"),
        "route_plan next-action routing must delegate route-field finalization and public-command binding to a focused finalization boundary"
    );
    assert!(
        !route_plan_next_action_paths
            .iter()
            .any(|path| path.starts_with("crate::execution::repair_route_decision::"))
            && !route_plan_next_action.contains("runtime_state.gate_snapshot")
            && !route_plan_next_action.contains("runtime_state.route_repair_target_candidates")
            && !route_plan_next_action.contains("status.blocking_records")
            && !route_plan_next_action.contains("route_authority_inputs"),
        "route-plan next-action finalization must consume precomputed RoutePlanningFacts instead of rebuilding route-authority helpers from RuntimeState/status"
    );
    assert!(
        route_plan_next_action_paths
            .iter()
            .any(|path| path.starts_with("crate::execution::current_truth::")),
        "route-plan next-action finalization must use the current-truth boundary in the route projection branch"
    );

    let repair_target = read_repo_file("src/execution/repair_target_selection.rs");
    assert!(
        repair_target.contains("ExecutionReentryTarget::new("),
        "repair target selection must live in src/execution/repair_target_selection.rs"
    );

    let late_stage_paths = route_plan_next_action_choice_code_paths();
    assert!(
        late_stage_paths
            .iter()
            .any(|path| path.ends_with("route_semantics::canonical_phase_for_shared_decision")),
        "late-stage route command/phase projection must delegate phase-detail mapping to route_plan"
    );

    let router = read_repo_file("src/execution/router.rs");
    let router_paths = normalized_code_paths("src/execution/router.rs", &router);
    assert!(
        !router_paths
            .iter()
            .any(|path| path.starts_with("crate::execution::route_plan::next_action_route")),
        "router.rs must delegate shared public-route finalization through the route_plan API instead of importing next-action route internals"
    );

    let next_action_choice = read_repo_file("src/execution/route_plan/next_action_choice.rs");
    let late_stage_harness_phase_patterns = [
        "HarnessPhase::FinalReviewPending",
        "HarnessPhase::QaPending",
        "HarnessPhase::ReadyForBranchCompletion",
    ];
    let next_action_choice_child_sources =
        rust_source_files(&repo_root().join("src/execution/route_plan/next_action_choice"))
            .into_iter()
            .filter(|path| {
                repo_relative(path) != "src/execution/route_plan/next_action_choice/tests.rs"
            })
            .map(|path| {
                let rel = repo_relative(&path);
                read_repo_file(&rel)
            })
            .collect::<Vec<_>>();
    assert!(
        late_stage_harness_phase_patterns
            .iter()
            .all(|pattern| next_action_choice_child_sources
                .iter()
                .any(|source| source.contains(pattern)))
            && late_stage_harness_phase_patterns
                .iter()
                .all(|pattern| !next_action_choice.contains(pattern)),
        "route_plan/next_action_choice.rs must delegate late-stage route ordering to child modules without pinning the child module names. Modules: {route_plan_next_action_choice_modules:?}"
    );
}

#[test]
fn task_closure_baseline_bridge_predicates_stay_in_shared_route_owner() {
    let repair_route_sources = repair_route_decision_sources();
    let repair_route_structs = repair_route_decision_struct_names();
    let repair_route_paths = repair_route_decision_code_paths();
    assert!(
        repair_route_structs.contains("TaskClosureBaselineBridgeRouteDecision")
            && repair_route_paths
                .iter()
                .any(|path| { path.starts_with("crate::execution::repair_target_selection::") })
            && repair_route_paths
                .iter()
                .any(|path| { path.starts_with("crate::execution::stale_target_projection::") }),
        "baseline bridge route facts must be owned under repair_route_decision while consuming repair-target and stale-target owners; the decision object is the shared boundary type"
    );
    assert!(
        repair_route_sources
            .iter()
            .any(|(_, source)| source.contains("ResumeStalePrecedence::from_inputs"))
            && repair_route_sources
                .iter()
                .all(|(_, source)| !source.contains("authority_inputs.earliest_stale_task()"))
            && repair_route_sources
                .iter()
                .all(|(_, source)| !source
                    .contains("projected_earliest_stale_task_from_status(status)")),
        "baseline bridge route facts must consume ResumeStalePrecedence instead of deriving stale precedence from authority inputs or projected public-status fields"
    );

    assert!(
        repair_route_paths
            .iter()
            .any(|path| path.starts_with("crate::execution::repair_target_selection::"))
            && repair_route_paths
                .iter()
                .any(|path| path.starts_with("crate::execution::stale_target_projection::"))
            && repair_route_paths
                .iter()
                .any(|path| path.starts_with("crate::execution::state::")),
        "baseline-bridge predicates should stay under repair_route_decision and consume shared predicate owners"
    );

    let route_plan_next_action = read_repo_file("src/execution/route_plan/next_action_route.rs");
    let route_plan_next_action_paths = normalized_dependency_paths(
        "src/execution/route_plan/next_action_route.rs",
        &route_plan_next_action,
    );
    let route_plan_planning_facts = read_repo_file("src/execution/route_plan/planning_facts.rs");
    let route_plan_finalization_facts =
        read_repo_file("src/execution/route_plan/finalization_facts.rs");
    let route_plan_finalization_fact_paths = normalized_dependency_paths(
        "src/execution/route_plan/finalization_facts.rs",
        &route_plan_finalization_facts,
    );
    let route_plan = read_repo_file("src/execution/route_plan.rs");
    let route_plan_paths = normalized_dependency_paths("src/execution/route_plan.rs", &route_plan);
    assert!(
        route_plan_finalization_fact_paths
            .iter()
            .any(|path| path.starts_with("crate::execution::repair_route_decision::"))
            && route_plan_finalization_facts.contains("ExecutionReentryTaskClosureBridgeFacts")
            && route_plan_planning_facts.contains("ExecutionReentryTaskClosureBridgeFacts")
            && route_plan_paths
                .iter()
                .any(|path| path.starts_with("crate::execution::repair_route_decision::"))
            && !route_plan_next_action_paths
                .iter()
                .any(|path| path.starts_with("crate::execution::repair_route_decision::"))
            && !route_plan_next_action.contains("runtime_state.route_repair_target_candidates")
            && !route_plan_next_action.contains("runtime_state.gate_snapshot")
            && !route_plan_next_action.contains("route_authority_inputs"),
        "route-plan must precompute baseline-bridge route facts before finalization; next-action finalization must consume RoutePlanningFacts instead of rebuilding helper inputs"
    );
    assert!(
        normalized_code_paths("src/execution/route_plan.rs", &route_plan)
            .iter()
            .any(|path| path.starts_with("crate::execution::repair_route_decision::")),
        "route_plan.rs must own persisted close-current-task bridge route selection"
    );
    let next_action = read_repo_file("src/execution/next_action.rs");
    let route_plan_next_action_choice_sources = route_plan_next_action_choice_sources();
    let next_action_dependency_paths =
        normalized_dependency_paths("src/execution/next_action.rs", &next_action);
    assert!(
        !next_action_dependency_paths
            .iter()
            .any(|path| path.starts_with("crate::execution::repair_route_decision::")),
        "next_action.rs facade must not own persisted close-current-task bridge route selection"
    );
    assert!(
        !next_action.contains("status.public_repair_targets")
            && route_plan_next_action_choice_sources
                .iter()
                .any(|(_, source)| source
                    .contains("authority_inputs.route_repair_target_candidates")),
        "route_plan next-action choice must consume route-authority repair targets and next_action.rs must not use projected public repair targets"
    );
    let next_action_paths = route_plan_next_action_choice_code_paths();
    assert!(
        next_action_paths
            .iter()
            .any(|path| path.starts_with("crate::execution::repair_route_decision::")),
        "route_plan/next_action_choice modules must consume shared baseline-bridge route facts from repair_route_decision"
    );
    let predicate_source_names = baseline_bridge_predicate_source_names();
    let (_, next_action_choice_entrypoint_source) = route_plan_next_action_choice_sources
        .iter()
        .find(|(rel, _)| rel.ends_with("/next_action_choice.rs"))
        .unwrap_or_else(|| {
            panic!(
                "route-plan next-action choice sources should include the module entrypoint: {route_plan_next_action_choice_sources:#?}"
            )
        });
    let mut predicate_boundary_sources =
        vec![(String::from("src/execution/next_action.rs"), next_action)];
    predicate_boundary_sources.push((
        String::from("src/execution/route_plan/next_action_choice.rs"),
        next_action_choice_entrypoint_source.clone(),
    ));
    predicate_boundary_sources.push((
        String::from("src/execution/route_plan/next_action_route.rs"),
        route_plan_next_action,
    ));
    for (rel, source) in predicate_boundary_sources {
        let paths = normalized_code_paths(&rel, &source);
        let direct_predicate_imports = paths
            .iter()
            .filter(|path| is_baseline_bridge_predicate_source_path(path, &predicate_source_names))
            .collect::<Vec<_>>();
        assert!(
            direct_predicate_imports.is_empty(),
            "{rel} must not import baseline-bridge predicate sources directly; consume repair_route_decision baseline_bridge facts:\n{}",
            direct_predicate_imports
                .iter()
                .map(|path| path.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        );
        let predicate_source_name_refs = predicate_source_names
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        let direct_predicate_calls = rust_source_scan::forbidden_call_violations(
            &rel,
            &source,
            &predicate_source_name_refs,
            &[],
        );
        assert!(
            direct_predicate_calls.is_empty(),
            "{rel} must not call baseline-bridge predicate sources directly; consume repair_route_decision baseline_bridge facts:\n{}",
            direct_predicate_calls.join("\n")
        );
    }
}

#[test]
fn status_assembly_exact_execution_route_validation_uses_finalized_projection_only() {
    let rel = "src/execution/status_assembly/exact_route.rs";
    let exact_route = read_repo_file(rel);
    assert_no_import_path_prefix(
        rel,
        &exact_route,
        &[
            "crate::execution::route_plan",
            "crate::execution::router",
            "crate::execution::next_action",
            "crate::execution::repair_route_decision",
            "crate::execution::repair_target_selection",
            "crate::execution::stale_target_selection",
            "crate::execution::transitions",
        ],
        "must validate finalized route projection fields without depending on route-candidate or event-authority selectors",
    );
    let exact_route_status_reads = field_reads_on_base_ident_in_module(rel, &exact_route, "status");
    let exact_route_dependencies = normalized_dependency_paths(rel, &exact_route);
    let exact_route_call_paths = rust_source_scan::normalized_call_paths(rel, &exact_route, &[]);
    assert!(
        [
            "execution_command_context",
            "recommended_public_command_argv",
            "recommended_public_command_template",
            "recommended_public_command",
        ]
        .iter()
        .all(|field| exact_route_status_reads.contains_key(*field))
            && exact_route_dependencies.iter().any(|path| path
                .starts_with("crate::execution::status_assembly::exact_route_surfaces::"))
            && exact_route_dependencies.iter().any(|path| path
                .starts_with("crate::execution::status_assembly::exact_route_template::"))
            && exact_route_call_paths
                .iter()
                .any(|path| path.ends_with("execution_mutation_name_from_public_argv"))
            && exact_route_call_paths
                .iter()
                .any(|path| path.ends_with("from_execution_mutation_name")),
        "exact execution-command validation must consume finalized public route projection fields and typed execution command surfaces instead of recomputing route candidates; status reads: {exact_route_status_reads:?}; dependencies: {exact_route_dependencies:?}; calls: {exact_route_call_paths:?}"
    );
    let task_state = read_repo_file("src/execution/status_assembly/task_state.rs");
    assert_no_import_path_prefix(
        "src/execution/status_assembly/task_state.rs",
        &task_state,
        &[
            "crate::execution::route_plan",
            "crate::execution::router",
            "crate::execution::next_action",
            "crate::execution::repair_route_decision",
            "crate::execution::repair_target_selection",
            "crate::execution::stale_target_selection",
            "crate::execution::transitions",
        ],
        "must not keep a production fallback that recomputes an exact execution route target",
    );
    let route_target_forbidden_modules = [
        (
            "src/execution/status_assembly.rs",
            read_repo_file("src/execution/status_assembly.rs"),
        ),
        (
            "src/execution/status_assembly/task_state.rs",
            task_state.clone(),
        ),
    ];
    for (rel, source) in route_target_forbidden_modules {
        assert_no_import_path_prefix(
            rel,
            &source,
            &[
                "crate::execution::route_plan::execution_targets",
                "super::execution_targets",
            ],
            "status assembly must not consume or duplicate route-target choice helpers owned by route_plan",
        );
    }
    let route_target_owner_rel = "src/execution/route_plan/execution_targets.rs";
    let route_target_owner = read_repo_file(route_target_owner_rel);
    let route_target_structs = source_struct_names(route_target_owner_rel, &route_target_owner);
    let route_target_paths =
        normalized_dependency_paths(route_target_owner_rel, &route_target_owner);
    let route_plan_paths = normalized_dependency_paths(
        "src/execution/route_plan.rs",
        &read_repo_file("src/execution/route_plan.rs"),
    );
    assert!(
        route_target_structs.contains("ExecutionCommandRouteTarget")
            && route_target_paths
                .iter()
                .any(|path| path == "crate::execution::command_eligibility::PublicCommandKind")
            && route_target_paths
                .iter()
                .any(|path| path == "crate::execution::status::PlanExecutionStatus")
            && route_target_paths
                .iter()
                .any(|path| path == "crate::execution::status::PublicRepairTarget")
            && route_plan_paths
                .iter()
                .any(|path| path
                    .ends_with("execution_targets::resolve_execution_command_route_target")),
        "route_plan/execution_targets.rs must own the execution-command route-target DTO and route_plan.rs must consume that owner; structs: {route_target_structs:?}; owner deps: {route_target_paths:?}; route-plan deps: {route_plan_paths:?}"
    );

    let public_commands = read_repo_file("src/execution/route_plan/public_commands.rs");
    let production_public_command_paths = normalized_dependency_paths(
        "src/execution/route_plan/public_commands.rs",
        &public_commands,
    );
    assert!(
        !production_public_command_paths
            .iter()
            .any(|path| path == "crate::execution::state::ExecutionContext")
            && !production_public_command_paths
                .iter()
                .any(|path| path.starts_with("crate::execution::route_plan::next_action_choice")),
        "route_plan/public_commands.rs production code must not depend on status context or route-candidate computation; any compatibility route recompute helpers must be test-quarantined. Production dependencies: {production_public_command_paths:?}"
    );
}

#[test]
fn workflow_routing_queries_thread_exact_execution_validation_to_projection() {
    let rel = "src/execution/query.rs";
    let query = read_repo_file(rel);
    let query_paths = normalized_code_paths(rel, &query);
    assert!(
        query.contains("require_exact_execution_command: bool")
            && !query.contains("_require_exact_execution_command"),
        "execution query routing must keep exact-route validation as a live parameter"
    );
    for required_path in [
        "crate::execution::router::project_final_runtime_routing_projection",
        "crate::execution::status_assembly::require_public_execution_command_route_target",
    ] {
        assert!(
            query_paths.iter().any(|path| path == required_path),
            "execution query routing must delegate exact-route projection/validation through `{required_path}`"
        );
    }
}

#[test]
fn unit_review_proof_authority_distinguishes_active_contract_from_plain_receipts() {
    let unit_review_truth = read_repo_file("src/execution/state/unit_review_truth.rs");
    assert!(
        unit_review_truth.contains("enum UnitReviewProofAuthority")
            && unit_review_truth.contains("ActiveContractSerialRuntimeOwned")
            && unit_review_truth.contains("PlainReceiptDiagnosticOnly")
            && unit_review_truth.contains("plain_unit_review_receipts_diagnostic_only"),
        "unit-review truth must explicitly classify active-contract serial proof authority separately from diagnostic plain receipt artifacts"
    );

    let worktree_lease_truth = read_repo_file("src/execution/state/worktree_lease_truth.rs");
    let worktree_lease_paths = normalized_dependency_paths(
        "src/execution/state/worktree_lease_truth.rs",
        &worktree_lease_truth,
    );
    assert!(
        worktree_lease_paths
            .iter()
            .any(|path| path.ends_with("UnitReviewProofAuthority"))
            && worktree_lease_paths
                .iter()
                .any(|path| path.ends_with("classify_unit_review_proof_authority"))
            && worktree_lease_paths
                .iter()
                .any(|path| path.ends_with("warn_plain_unit_review_receipts_diagnostic_only")),
        "worktree lease gating must consume the unit-review proof-authority boundary instead of duplicating receipt authority decisions"
    );
    let state_parent = read_repo_file("src/execution/state.rs");
    assert!(
        !source_struct_names("src/execution/state.rs", &state_parent)
            .iter()
            .any(|name| name.contains("WorktreeLease"))
            && !state_parent.contains("serde::{Deserialize"),
        "state.rs should remain a facade and must not own worktree-lease DTO parsing"
    );
}

#[test]
fn workflow_plan_candidate_routing_has_single_decision_helper() {
    let rel = "src/workflow/status.rs";
    let source = read_repo_file(rel);
    assert_no_import_path_prefix(
        rel,
        &source,
        &[
            "crate::execution::commands",
            "crate::execution::route_plan::constructors",
            "crate::execution::route_plan::decision_support",
            "crate::execution::route_plan::next_action_choice",
            "crate::execution::route_plan::route_facts",
            "crate::execution::route_plan::status_projection",
            "crate::execution::router",
        ],
        "workflow status plan-candidate routing must stay above execution routing/mutation modules",
    );
    for required in [
        "stale_source_spec_linkage",
        "analyze_full_contract(",
        "workflow_contract_state(",
        "workflow_reason_codes(",
        "workflow_diagnostics(",
        "evaluate_plan_fidelity_gate(",
    ] {
        assert!(
            source.contains(required),
            "workflow status must keep plan-candidate contract analysis and fidelity gating in one workflow-owned routing surface: missing `{required}`"
        );
    }
    assert!(
        source.matches("evaluate_plan_fidelity_gate(").count() <= 3,
        "workflow status should not spread plan-fidelity routing across many local decision sites"
    );
}

#[test]
fn late_stage_public_command_callers_use_shared_phase_detail_resolver() {
    let owner = read_repo_file("src/execution/command_eligibility/late_stage.rs");
    assert!(
        owner.contains("pub enum PublicAdvanceLateStageMode")
            && owner.contains("PublicCommand::AdvanceLateStage"),
        "late-stage public mode and command construction should be owned by command_eligibility::late_stage"
    );
    let late_stage_paths = route_plan_next_action_choice_code_paths();
    assert!(
        late_stage_paths
            .iter()
            .any(|path| path.ends_with("route_semantics::canonical_phase_for_shared_decision")),
        "late-stage route selection should delegate phase mapping to route_plan and leave public command binding to command_eligibility"
    );

    for rel in [
        "src/execution/route_plan/constructors.rs",
        "src/execution/route_plan/follow_up.rs",
        "src/execution/route_plan/next_action_finalization.rs",
        "src/execution/commands/common/operator_outputs.rs",
        "src/execution/state/runtime_methods.rs",
    ] {
        let source = read_repo_file(rel);
        for forbidden in [
            "PublicAdvanceLateStageMode::Basic",
            "PublicAdvanceLateStageMode::ReleaseReadiness",
            "PublicAdvanceLateStageMode::FinalReviewDispatch",
            "PublicAdvanceLateStageMode::Qa",
            "PublicAdvanceLateStageMode::FinalReview",
            "PublicAdvanceLateStageMode::FinishReview",
            "PublicAdvanceLateStageMode::FinishCompletion",
        ] {
            assert!(
                !source.contains(forbidden),
                "{rel} must not reconstruct late-stage public command modes locally via `{forbidden}`"
            );
        }
    }

    let mut violations = Vec::new();
    for path in rust_source_files(&repo_root().join("src")) {
        let rel = repo_relative(&path);
        if rel == "src/execution/command_eligibility.rs"
            || rel == "src/execution/command_eligibility/late_stage.rs"
            || rel.ends_with("/unit_tests.rs")
        {
            continue;
        }
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{rel} should be readable: {error}"));
        for path in normalized_code_paths(&rel, &source) {
            if !late_stage_mode_variant_patterns()
                .iter()
                .any(|variant| path.ends_with(variant))
            {
                continue;
            }
            violations.push(format!(
                "{rel} references late-stage mode variant `{path}` outside the command eligibility owner"
            ));
        }
    }
    assert!(
        violations.is_empty(),
        "late-stage mode variant construction must stay in command_eligibility::late_stage, except command parsing/rendering functions: {violations:#?}"
    );
}

#[test]
fn blocking_scope_task_projection_has_single_execution_owner() {
    let query = read_repo_file("src/execution/query.rs");
    let route_semantics = read_repo_file("src/execution/route_plan/route_semantics.rs");
    let query_functions = source_function_names("src/execution/query.rs", &query);
    let route_semantics_functions = source_function_names(
        "src/execution/route_plan/route_semantics.rs",
        &route_semantics,
    );
    let route_semantics_structs = source_struct_names(
        "src/execution/route_plan/route_semantics.rs",
        &route_semantics,
    );
    let route_semantics_visibilities = source_production_function_visibilities(
        "src/execution/route_plan/route_semantics.rs",
        &route_semantics,
    );
    assert!(
        route_semantics_structs.contains("ExecutionBlockingProjection")
            && route_semantics_functions.contains("project_execution_blocking")
            && route_semantics_functions.contains("external_wait_state_for_phase_detail")
            && matches!(
                route_semantics_visibilities
                    .get("project_execution_blocking")
                    .map(String::as_str),
                Some("pub(crate)" | "pub(in crate)")
            )
            && matches!(
                route_semantics_visibilities
                    .get("external_wait_state_for_phase_detail")
                    .map(String::as_str),
                Some("pub(crate)" | "pub(in crate)")
            )
            && !query_functions.contains("project_execution_blocking")
            && !query_functions.contains("external_wait_state_for_phase_detail"),
        "blocking scope/task and external-wait projection must expose one route-plan-owned projection boundary"
    );
    let mut route_semantic_call_violations = Vec::new();
    for path in rust_source_files(&repo_root().join("src")) {
        let rel = repo_relative(&path);
        if rel.starts_with("src/execution/route_plan/") || rel == "src/execution/route_plan.rs" {
            continue;
        }
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{rel} should be readable: {error}"));
        for forbidden in [
            "project_execution_blocking(",
            "blocking_scope_for_phase_detail(",
            "external_wait_state_for_phase_detail(",
            "canonical_phase_for_shared_decision(",
        ] {
            if source.contains(forbidden) {
                route_semantic_call_violations.push(format!("{rel} calls `{forbidden}`"));
            }
        }
    }
    assert!(
        route_semantic_call_violations.is_empty(),
        "route semantic helpers must stay inside route_plan; callers should consume finalized RouteDecision/status projections instead:\n{}",
        route_semantic_call_violations.join("\n")
    );

    let router = read_repo_file("src/execution/router.rs");
    let router_visibilities =
        source_production_function_visibilities("src/execution/router.rs", &router);
    assert_no_import_path_prefix(
        "src/execution/router.rs",
        &router,
        &["crate::execution::route_plan::status_projection"],
        "must not import route-plan status-projection internals after route selection",
    );
    assert!(
        matches!(
            router_visibilities
                .get("project_final_runtime_routing_projection")
                .map(String::as_str),
            Some("pub(crate)" | "pub(in crate)")
        ),
        "router must expose one final route/blocker projection boundary for read-model and operator callers"
    );
    assert!(
        !router.contains("fn route_status_blocking_projection"),
        "router must not recompute blocking scope/task or external wait state after route_plan finalizes RouteDecision"
    );
    let route_plan_status_projection =
        read_repo_file("src/execution/route_plan/status_projection.rs");
    assert_no_import_path_prefix(
        "src/execution/route_plan/status_projection.rs",
        &route_plan_status_projection,
        &[
            "crate::execution::route_plan::constructors",
            "super::constructors",
            "crate::execution::route_plan::stale_repair_target",
            "super::stale_repair_target",
        ],
        "must remain route-neutral and must not import route constructors or stale-target selectors",
    );
    let route_plan_status_application =
        read_repo_file("src/execution/route_plan/status_application.rs");
    let route_plan_status_application_visibilities = source_production_function_visibilities(
        "src/execution/route_plan/status_application.rs",
        &route_plan_status_application,
    );
    let route_plan_decision = read_repo_file("src/execution/route_plan/decision.rs");
    let route_plan_decision_paths =
        normalized_code_paths("src/execution/route_plan/decision.rs", &route_plan_decision);
    let route_plan_status_application_paths = normalized_code_paths(
        "src/execution/route_plan/status_application.rs",
        &route_plan_status_application,
    );
    let route_plan_status_application_mutations = status_field_mutations_in_function(
        "src/execution/route_plan/status_application.rs",
        &route_plan_status_application,
        "apply_common_route_status_projection",
    );
    assert_route_plan_status_projection_remains_route_neutral(&route_plan_status_projection);
    assert!(
        route_plan_decision_paths
            .iter()
            .any(|path| path.ends_with("route_semantics::project_execution_blocking"))
            && route_plan_decision_paths.iter().any(|path| path
                .ends_with("route_semantics::external_wait_state_for_phase_detail")),
        "RouteDecision must delegate public blocker/wait projection to the route-plan semantic owner instead of carrying local route heuristics"
    );
    assert!(
        matches!(
            route_plan_status_application_visibilities
                .get("apply_common_route_status_projection")
                .map(String::as_str),
            Some("pub(crate)" | "pub(in crate)")
        ) && matches!(
            route_plan_status_application_visibilities
                .get("apply_route_status_projection_diagnostics")
                .map(String::as_str),
            Some("pub(crate)" | "pub(in crate)")
        )
            && route_plan_status_application_mutations.contains("harness_phase")
            && route_plan_status_application_paths
                .iter()
                .any(|path| path == "crate::execution::harness::HarnessPhase")
            && route_plan_status_application_paths.iter().any(|path| {
                path == "crate::execution::closure_diagnostics::apply_task_boundary_projection_diagnostics"
            })
            && route_plan_status_application_paths.iter().any(|path| {
                path.starts_with("crate::execution::reentry_reconcile::TargetlessStaleReconcile")
            }),
        "route_plan/status_application.rs must own shared route-to-status field assignment, phase-to-harness mapping, and projection diagnostics"
    );

    let public_route_projection =
        read_repo_file("src/execution/read_model/public_route_projection.rs");
    assert_public_route_projection_installs_router_finalized_status(&public_route_projection);
    assert!(
        !public_route_projection.contains(".blocking_projection")
            && !public_route_projection
                .contains("project_execution_blocking(ExecutionBlockingProjectionInputs")
            && !public_route_projection.contains("compute_status_blocking_records(")
            && !public_route_projection.contains("RouteDecision {"),
        "read-model public route projection must consume RouteDecision-owned public route fields instead of revising route decisions or recomputing status blockers"
    );
    assert_no_import_path_prefix(
        "src/execution/read_model/public_route_projection.rs",
        &public_route_projection,
        &[
            "crate::execution::route_plan::constructors",
            "crate::execution::route_plan::decision_support",
            "crate::execution::route_plan::route_facts",
            "crate::execution::route_plan::stale_repair_target",
            "crate::execution::route_plan::status_application",
            "crate::execution::route_plan::status_projection",
            "crate::execution::route_plan::project_execution_blocking",
        ],
        "must not import route-revision or blocker recomputation internals; it may only consume router-owned final projection output",
    );
    let operator_outputs = read_repo_file("src/execution/commands/common/operator_outputs.rs");
    let operator_output_paths = normalized_dependency_paths(
        "src/execution/commands/common/operator_outputs.rs",
        &operator_outputs,
    );
    assert_no_import_path_prefix(
        "src/execution/commands/common/operator_outputs.rs",
        &operator_outputs,
        &[
            "crate::execution::route_plan::constructors",
            "crate::execution::route_plan::decision_support",
            "crate::execution::route_plan::status_projection",
        ],
        "must not import route-revision internals",
    );
    assert!(
        operator_output_paths
            .iter()
            .any(|path| path.starts_with("crate::execution::router::")),
        "command operator outputs must consume router-owned final route/blocker projection instead of copying a pre-final route decision"
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
    assert!(
        operator.contains("route_decision.blocking_scope")
            && operator.contains("route_decision.blocking_task")
            && operator.contains("route_decision.external_wait_state")
            && !operator.contains("let operator_blocking_scope = blocking_scope;")
            && !operator.contains("let operator_blocking_task = blocking_task;")
            && !operator.contains("let operator_external_wait_state = external_wait_state;"),
        "workflow operator must source public blocker/wait envelope fields from finalized RouteDecision, not parallel routing copies"
    );
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
fn status_assembly_keeps_public_route_fields_projection_only() {
    let status_assembly = read_repo_file("src/execution/status_assembly.rs");
    let status_facts = read_repo_file("src/execution/status_assembly/facts.rs");
    let status_review_state = read_repo_file("src/execution/status_assembly/review_state.rs");
    let status_fact_structs =
        source_struct_names("src/execution/status_assembly/facts.rs", &status_facts);
    let status_fact_functions = source_production_function_visibilities(
        "src/execution/status_assembly/facts.rs",
        &status_facts,
    );
    assert!(
        [
            "StatusAssemblyFacts",
            "StatusRepairFollowUpFacts",
            "StatusReviewStateFacts",
            "StatusReviewStateInputs",
        ]
        .into_iter()
        .all(|name| status_fact_structs.contains(name))
            && [
                "effective_review_state_status",
                "effective_route_review_state_status",
            ]
            .into_iter()
            .all(|name| matches!(
                status_fact_functions.get(name).map(String::as_str),
                Some("pub(crate)" | "pub(in crate)")
            )),
        "status assembly must expose route-relevant intermediate state as explicit route-neutral facts"
    );
    let next_action_types = read_repo_file("src/execution/route_plan/next_action_choice/types.rs");
    let next_action_type_functions = source_production_function_visibilities(
        "src/execution/route_plan/next_action_choice/types.rs",
        &next_action_types,
    );
    let next_action_type_calls = rust_source_scan::normalized_call_paths_in_function(
        "src/execution/route_plan/next_action_choice/types.rs",
        &next_action_types,
        "canonical_review_state_status",
    );
    let next_action_type_paths = normalized_code_paths(
        "src/execution/route_plan/next_action_choice/types.rs",
        &next_action_types,
    );
    assert!(
        matches!(
            next_action_type_functions
                .get("canonical_review_state_status")
                .map(String::as_str),
            Some("pub(crate)" | "pub(in crate)")
        ) && next_action_type_calls
            .iter()
            .any(|path| path == "crate::execution::status_assembly::effective_review_state_status")
            && !next_action_type_calls
                .iter()
                .any(|path| path.ends_with("prerelease_branch_closure_refresh_required"))
            && !next_action_type_paths.iter().any(|path| path
                == "crate::execution::phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS"
            )
            && !field_reads_on_ident_in_function(
                "src/execution/route_plan/next_action_choice/types.rs",
                &next_action_types,
                "canonical_review_state_status",
                "status",
            )
            .contains_key("stale_unreviewed_closures"),
        "route planning must use the shared effective review-state status owner instead of carrying a second classifier"
    );
    let route_plan = read_repo_file("src/execution/route_plan.rs");
    let route_constructors = read_repo_file("src/execution/route_plan/constructors.rs");
    let route_facts = read_repo_file("src/execution/route_plan/route_facts.rs");
    let next_action_route = read_repo_file("src/execution/route_plan/next_action_route.rs");
    let route_plan_paths = normalized_code_paths("src/execution/route_plan.rs", &route_plan);
    let route_planning_authority_calls = rust_source_scan::normalized_call_paths_in_function(
        "src/execution/route_plan.rs",
        &route_plan,
        "route_planning_authority_for_status",
    );
    assert!(
        route_planning_authority_calls
            .iter()
            .any(|path| path == "crate::execution::status_assembly::effective_review_state_status")
            && route_plan_paths
                .iter()
                .any(|path| path
                    == "crate::execution::status_assembly::effective_review_state_status")
            && !route_plan
                .contains("use crate::execution::next_action::canonical_review_state_status"),
        "route-plan authority facts must consume shared effective review-state status directly"
    );
    let route_facts_functions =
        source_function_names("src/execution/route_plan/route_facts.rs", &route_facts);
    let route_facts_paths =
        normalized_code_paths("src/execution/route_plan/route_facts.rs", &route_facts);
    assert!(
        !route_facts_functions.contains("effective_route_review_state_status")
            && !field_reads_on_base_ident_in_module(
                "src/execution/route_plan/route_facts.rs",
                &route_facts,
                "status",
            )
            .contains_key("stale_unreviewed_closures")
            && !route_facts_paths
                .iter()
                .any(|path| path.ends_with("REVIEW_STATE_MISSING_CURRENT_CLOSURE")),
        "route facts must not carry a second effective review-state status classifier"
    );
    let route_constructor_calls = rust_source_scan::normalized_call_paths(
        "src/execution/route_plan/constructors.rs",
        &route_constructors,
        &[],
    );
    let route_constructor_status_reads = field_reads_on_base_ident_in_module(
        "src/execution/route_plan/constructors.rs",
        &route_constructors,
        "status",
    );
    assert!(
        route_constructor_calls
            .iter()
            .any(|path| path
                == "crate::execution::status_assembly::effective_route_review_state_status")
            && !route_constructor_status_reads.contains_key("stale_unreviewed_closures"),
        "route constructors must consume the shared effective route review-state status owner instead of carrying an inline classifier"
    );
    let mut duplicate_review_state_classifier_violations = Vec::new();
    for (rel, source) in route_plan_boundary_sources() {
        if rel.ends_with("unit_tests.rs") || rel.ends_with("/tests.rs") {
            continue;
        }
        let local_functions = source_function_names(&rel, &source);
        if local_functions.contains("effective_review_state_status")
            || local_functions.contains("effective_route_review_state_status")
        {
            duplicate_review_state_classifier_violations.push(format!(
                "{rel} defines an effective review-state classifier locally"
            ));
        }
    }
    assert!(
        duplicate_review_state_classifier_violations.is_empty(),
        "route planning must not carry a second effective review-state status classifier outside status_assembly/facts.rs:\n{}",
        duplicate_review_state_classifier_violations.join("\n")
    );
    let next_action_route_paths = normalized_dependency_paths(
        "src/execution/route_plan/next_action_route.rs",
        &next_action_route,
    );
    let next_action_route_calls = rust_source_scan::normalized_call_paths(
        "src/execution/route_plan/next_action_route.rs",
        &next_action_route,
        &[],
    );
    assert!(
        next_action_route_paths
            .iter()
            .any(|path| path
                == "crate::execution::status_assembly::effective_route_review_state_status")
            && next_action_route_calls.iter().any(|path| path
                == "crate::execution::status_assembly::effective_route_review_state_status"),
        "next-action route finalization must consume the shared effective route review-state status owner"
    );
    let status_assembly_structs =
        source_struct_names("src/execution/status_assembly.rs", &status_assembly);
    let status_assembly_output_fields = source_struct_field_names(
        "src/execution/status_assembly.rs",
        &status_assembly,
        "StatusAssemblyOutput",
    );
    let status_review_state_functions = source_production_function_visibilities(
        "src/execution/status_assembly/review_state.rs",
        &status_review_state,
    );
    let status_review_state_calls = rust_source_scan::normalized_call_paths_in_function(
        "src/execution/status_assembly/review_state.rs",
        &status_review_state,
        "derive_status_review_state_fact",
    );
    let status_assembly_functions =
        source_function_names("src/execution/status_assembly.rs", &status_assembly);
    assert!(
        status_assembly_structs.contains("StatusAssemblyOutput")
            && status_assembly_output_fields.contains("facts")
            && matches!(
                status_review_state_functions
                    .get("derive_status_review_state_fact")
                    .map(String::as_str),
                Some("pub(crate)" | "pub(in crate)")
            )
            && status_review_state_calls
                .iter()
                .any(|path| path.ends_with("effective_review_state_status"))
            && !status_assembly_functions.contains("derive_public_review_state_status"),
        "status assembly callers must receive status facts explicitly instead of recovering routing inputs from public route fields"
    );

    let reset_function_name = "reset_route_projection_fields_at_status_boundary";
    let route_projection_fields = status_field_mutations_in_function(
        "src/execution/status_assembly.rs",
        &status_assembly,
        reset_function_name,
    );
    let route_projection_anchor_fields = public_route_projection_anchor_fields();
    assert!(
        route_projection_anchor_fields.is_subset(&route_projection_fields),
        "status assembly route-field reset must clear the stable public route projection anchors; anchors: {route_projection_anchor_fields:?}, reset mutations: {route_projection_fields:?}"
    );
    assert!(
        rust_source_scan::normalized_call_paths_in_function(
            "src/execution/status_assembly.rs",
            &status_assembly,
            "populate_public_status_contract_fields",
        )
        .iter()
        .any(|path| path.ends_with("reset_route_projection_fields_at_status_boundary")),
        "status assembly must clear public route projection fields through the named status-boundary reset"
    );

    let status_assembly_sources = std::iter::once((
        String::from("src/execution/status_assembly.rs"),
        status_assembly.clone(),
    ))
    .chain(
        rust_source_files(&repo_root().join("src/execution/status_assembly"))
            .into_iter()
            .map(|path| {
                let rel = repo_relative(&path);
                let source = read_repo_file(&rel);
                (rel, source)
            }),
    );
    for (rel, source) in status_assembly_sources {
        for (function, mutations) in status_field_mutations_by_function(&rel, &source) {
            if rel == "src/execution/status_assembly.rs" && function == reset_function_name {
                continue;
            }
            let route_mutations = mutations
                .intersection(&route_projection_fields)
                .cloned()
                .collect::<Vec<_>>();
            assert!(
                route_mutations.is_empty(),
                "{rel}::{function} must not write public route projection fields outside the explicit status boundary reset: {route_mutations:?}"
            );
        }
    }

    let reducer = read_repo_file("src/execution/reducer.rs");
    let reducer_status_fact_reads = field_reads_on_ident_in_function(
        "src/execution/reducer.rs",
        &reducer,
        "build_runtime_state_from_event_authority",
        "status_facts",
    );
    assert!(
        reducer_status_fact_reads.get("stale_projection").copied() == Some(1),
        "reducer must consume the status-assembly stale projection once instead of recovering stale routing inputs from public status fields: {reducer_status_fact_reads:?}"
    );
    let projected_status_stale_violations = rust_source_scan::forbidden_call_violations_in_function(
        "src/execution/status_assembly.rs",
        &status_assembly,
        "populate_public_status_contract_fields",
        &["projected_earliest_stale_task_from_status"],
    );
    assert!(
        projected_status_stale_violations.is_empty(),
        "status review-state facts must use the route-neutral stale projection, not route/public status stale fields that are populated later"
    );
    let read_model = read_repo_file("src/execution/read_model.rs");
    let normalize_same_branch_mutations = status_field_mutations_in_function(
        "src/execution/read_model.rs",
        &read_model,
        "normalize_non_started_same_branch_status",
    );
    assert!(
        ["execution_started", "active_task", "resume_step"]
            .into_iter()
            .all(|field| normalize_same_branch_mutations.contains(field)),
        "same-branch normalization should only clear execution-progress facts before route projection"
    );
    assert!(
        normalize_same_branch_mutations
            .intersection(&route_projection_fields)
            .next()
            .is_none()
            && !normalize_same_branch_mutations.contains("blocking_task"),
        "same-branch read-model normalization must not write public route fields before route projection: {normalize_same_branch_mutations:?}"
    );
    let route_plan_status_application =
        read_repo_file("src/execution/route_plan/status_application.rs");
    let route_plan_status_application_visibilities = source_production_function_visibilities(
        "src/execution/route_plan/status_application.rs",
        &route_plan_status_application,
    );
    let public_route_projection =
        read_repo_file("src/execution/read_model/public_route_projection.rs");
    assert_public_route_projection_installs_router_finalized_status(&public_route_projection);
    let status_application_route_mutations = status_field_mutations_in_function(
        "src/execution/route_plan/status_application.rs",
        &route_plan_status_application,
        "apply_common_route_status_projection",
    )
    .intersection(&route_projection_fields)
    .cloned()
    .collect::<BTreeSet<_>>();
    assert!(
        matches!(
            route_plan_status_application_visibilities
                .get("apply_common_route_status_projection")
                .map(String::as_str),
            Some("pub(crate)" | "pub(in crate)")
        ) && status_application_route_mutations.contains("phase")
            && status_application_route_mutations.contains("phase_detail")
            && status_application_route_mutations.contains("next_action")
            && status_application_route_mutations.contains("recommended_public_command")
            && function_calls_path(
                "src/execution/read_model/public_route_projection.rs",
                &public_route_projection,
                "apply_shared_routing_projection_to_read_scope_with_routing",
                "crate::execution::router::project_final_runtime_routing_projection",
            ),
        "route-plan status application must own route field population; read-model public route projection may only install the router-finalized status projection"
    );
}

fn route_neutral_route_output_violations_for_source(rel: &str, source: &str) -> Vec<String> {
    normalized_dependency_paths(rel, source)
        .into_iter()
        .filter(|path| {
            path.starts_with("crate::execution::route_plan::")
                || path.starts_with("crate::execution::next_action::")
        })
        .map(|path| format!("{rel} depends on route-owned public output symbol `{path}`"))
        .collect()
}

#[test]
fn route_neutral_status_layers_do_not_import_route_owned_output_modules() {
    let mut violations = Vec::new();

    for path in std::iter::once(repo_root().join("src/execution/status_assembly.rs"))
        .chain(std::iter::once(
            repo_root().join("src/execution/read_model.rs"),
        ))
        .chain(rust_source_files(
            &repo_root().join("src/execution/status_assembly"),
        ))
        .chain(rust_source_files(
            &repo_root().join("src/execution/read_model"),
        ))
    {
        let rel = repo_relative(&path);
        if rel == "src/execution/read_model/public_route_projection.rs"
            || rel.contains("/execution_command_route_target_tests/")
        {
            continue;
        }
        let source = read_repo_file(&rel);
        violations.extend(route_neutral_route_output_violations_for_source(
            &rel, &source,
        ));
    }

    assert!(
        violations.is_empty(),
        "route-neutral status/read-model layers must not import route-owned public route output modules; consume router-finalized projection at the explicit read-model boundary instead:\n{}",
        violations.join("\n")
    );
}

#[test]
fn status_assembly_child_modules_have_cohesive_semantic_owners() {
    let parent_rel = "src/execution/status_assembly.rs";
    let parent = read_repo_file(parent_rel);
    let parent_modules = source_module_names(parent_rel, &parent);
    let parent_visibilities = source_production_function_visibilities(parent_rel, &parent);
    let public_functions = parent_visibilities
        .iter()
        .filter_map(|(name, visibility)| (visibility == "pub").then_some(name.as_str()))
        .collect::<Vec<_>>();
    assert!(
        !parent_modules.is_empty() && public_functions == ["status_from_context"],
        "{parent_rel} must remain a facade with only the public status DTO entrypoint exposed; public functions: {public_functions:?}"
    );

    const ROUTE_NEUTRAL_FORBIDDEN_IMPORT_PREFIXES: &[&str] = &[
        "crate::execution::router",
        "crate::execution::route_plan",
        "crate::execution::next_action",
        "crate::execution::commands",
        "crate::execution::mutate",
        "crate::execution::read_model",
        "crate::workflow",
    ];
    assert_no_import_path_prefix(
        parent_rel,
        &parent,
        ROUTE_NEUTRAL_FORBIDDEN_IMPORT_PREFIXES,
        "must remain route-neutral status assembly, not route selection, mutation, read-model presentation, or workflow presentation",
    );
    for module in parent_modules {
        let rel = format!("src/execution/status_assembly/{module}.rs");
        let source = read_repo_file(&rel);
        let public_child_functions = source_production_function_visibilities(&rel, &source)
            .into_iter()
            .filter_map(|(name, visibility)| (visibility == "pub").then_some(name))
            .collect::<Vec<_>>();
        assert!(
            public_child_functions.is_empty(),
            "{rel} must stay behind the status assembly facade instead of publishing direct route/status APIs: {public_child_functions:?}"
        );
        assert_no_import_path_prefix(
            &rel,
            &source,
            ROUTE_NEUTRAL_FORBIDDEN_IMPORT_PREFIXES,
            "must remain route-neutral status assembly, not route selection, mutation, read-model presentation, or workflow presentation",
        );
    }
}

#[test]
fn route_to_status_projection_mapping_has_single_owner() {
    let status_application = read_repo_file("src/execution/route_plan/status_application.rs");
    let status_application_paths = normalized_code_paths(
        "src/execution/route_plan/status_application.rs",
        &status_application,
    );
    let status_application_mutations = status_field_mutations_in_function(
        "src/execution/route_plan/status_application.rs",
        &status_application,
        "apply_common_route_status_projection",
    );
    assert!(
        status_application_mutations.contains("harness_phase")
            && status_application.contains("HarnessPhase::parse(route_phase)")
            && status_application.contains("PHASE_TASK_CLOSURE_PENDING")
            && status_application_paths
                .iter()
                .any(|path| path == "crate::execution::harness::HarnessPhase"),
        "route_plan/status_application.rs must apply route-to-status harness phase projection through canonical HarnessPhase parsing"
    );

    let router = read_repo_file("src/execution/router.rs");
    let router_runtime_projection_fields = source_struct_field_names(
        "src/execution/router.rs",
        &router,
        "RuntimeRoutingProjection",
    );
    assert!(
        router_runtime_projection_fields.contains("status_projection")
            && !router.contains("apply_common_route_status_projection(")
            && !router.contains("apply_route_status_projection_diagnostics(")
            && !router.contains("compute_status_blocking_records(")
            && !router.contains("match route_decision.phase.as_str()")
            && !router.contains("HarnessPhase::DocumentReleasePending")
            && !router.contains("HarnessPhase::FinalReviewPending")
            && !router.contains("HarnessPhase::QaPending")
            && !router.contains("HarnessPhase::ReadyForBranchCompletion"),
        "router must consume the route-plan-owned status projection instead of rebuilding route-to-status projection or blocker records"
    );
    let public_route_projection =
        read_repo_file("src/execution/read_model/public_route_projection.rs");
    assert_public_route_projection_installs_router_finalized_status(&public_route_projection);
}

#[test]
fn transfer_handoff_eligibility_uses_public_mutation_authority() {
    let transfer = read_repo_file("src/execution/commands/transfer.rs");
    assert!(
        transfer.contains("let authorization = require_public_mutation_decision(")
            && transfer.contains("MutationEligibilitySource::ExactRoute"),
        "transfer handoff recording must consume typed public mutation authorization, not a presentation recheck"
    );
    for forbidden in ["operator.phase", "operator.phase_detail"] {
        assert!(
            !transfer.contains(forbidden),
            "transfer handoff eligibility must not inspect workflow-operator presentation field `{forbidden}`"
        );
    }
}

#[test]
fn harness_phase_string_parsing_stays_owned_by_harness_phase() {
    let harness_phase_values = HarnessPhase::ALL
        .iter()
        .map(|phase| phase.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        harness_phase_values.len(),
        HarnessPhase::ALL.len(),
        "canonical harness phase values should be unique"
    );

    let protected_sources = [
        "src/execution/status_assembly/overlay.rs",
        "src/execution/status_assembly/late_stage.rs",
        "src/execution/route_plan/status_application.rs",
        "src/workflow/recommendation.rs",
        "src/workflow/operator.rs",
    ];
    let mut violations = Vec::new();
    for rel in protected_sources {
        let source = read_repo_file(rel);
        violations.extend(local_harness_phase_parse_table_violations(
            rel,
            &source,
            &harness_phase_values,
        ));
    }

    assert!(
        violations.is_empty(),
        "harness phase parsing/string-to-HarnessPhase mapping must stay centralized in src/execution/harness.rs:\n{}",
        violations.join("\n")
    );
}

fn local_harness_phase_parse_table_violations(
    rel: &str,
    source: &str,
    harness_phase_values: &BTreeSet<&'static str>,
) -> Vec<String> {
    let syntax = parse_rust_source(rel, source);
    let mut visitor = LocalHarnessPhaseParseTableVisitor {
        rel,
        harness_phase_values,
        violations: Vec::new(),
    };
    visitor.visit_file(&syntax);
    visitor.violations
}

struct LocalHarnessPhaseParseTableVisitor<'a> {
    rel: &'a str,
    harness_phase_values: &'a BTreeSet<&'static str>,
    violations: Vec<String>,
}

impl<'ast> Visit<'ast> for LocalHarnessPhaseParseTableVisitor<'_> {
    fn visit_expr_match(&mut self, match_expr: &'ast syn::ExprMatch) {
        let pattern_matches_canonical_phase = match_expr.arms.iter().any(|arm| {
            pat_contains_harness_phase_value_or_phase_constant(&arm.pat, self.harness_phase_values)
        });
        if pattern_matches_canonical_phase {
            self.violations.push(format!(
                "{}:{} contains a local harness-phase string/constant match table",
                self.rel,
                match_expr.match_token.span.start().line
            ));
        }
        visit::visit_expr_match(self, match_expr);
    }
}

fn pat_contains_harness_phase_value_or_phase_constant(
    pat: &syn::Pat,
    harness_phase_values: &BTreeSet<&'static str>,
) -> bool {
    struct HarnessPhasePatternVisitor<'a> {
        harness_phase_values: &'a BTreeSet<&'static str>,
        found: bool,
    }

    impl<'ast> Visit<'ast> for HarnessPhasePatternVisitor<'_> {
        fn visit_pat(&mut self, pat: &'ast syn::Pat) {
            if let syn::Pat::Lit(literal) = pat
                && let syn::Lit::Str(literal) = &literal.lit
            {
                self.found |= self.harness_phase_values.contains(literal.value().as_str());
            }
            visit::visit_pat(self, pat);
        }

        fn visit_path(&mut self, path: &'ast syn::Path) {
            if path
                .segments
                .last()
                .is_some_and(|segment| segment.ident.to_string().starts_with("PHASE_"))
            {
                self.found = true;
            }
            visit::visit_path(self, path);
        }
    }

    let mut visitor = HarnessPhasePatternVisitor {
        harness_phase_values,
        found: false,
    };
    visitor.visit_pat(pat);
    visitor.found
}

#[test]
fn late_stage_phase_mapping_stays_route_plan_owned() {
    let late_stage_paths = route_plan_next_action_choice_code_paths();
    let route_plan_phase_mapping_dependencies = late_stage_paths
        .iter()
        .filter(|path| path.ends_with("route_semantics::canonical_phase_for_shared_decision"))
        .collect::<Vec<_>>();
    assert!(
        !route_plan_phase_mapping_dependencies.is_empty(),
        "late-stage route selection must delegate phase-detail to phase mapping to route_plan. Route-plan phase-mapping dependencies: {route_plan_phase_mapping_dependencies:?}"
    );
    assert!(
        !late_stage_paths
            .iter()
            .any(|path| path == "crate::execution::phase::PHASE_DOCUMENT_RELEASE_PENDING")
            && !late_stage_paths
                .iter()
                .any(|path| path == "crate::execution::phase::PHASE_FINAL_REVIEW_PENDING")
            && !late_stage_paths
                .iter()
                .any(|path| path == "crate::execution::phase::PHASE_QA_PENDING")
            && !late_stage_paths.iter().any(|path| {
                path == "crate::execution::phase::PHASE_READY_FOR_BRANCH_COMPLETION"
            }),
        "late-stage route selection must not maintain a local phase-detail to phase match table"
    );

    let mut violations = Vec::new();
    for path in rust_source_files(&repo_root().join("src")) {
        let rel = repo_relative(&path);
        if rel == "src/execution/route_plan/route_semantics.rs" {
            continue;
        }
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{rel} should be readable: {error}"));
        violations.extend(local_phase_detail_to_phase_mapping_violations(
            &rel, &source,
        ));
    }

    assert!(
        violations.is_empty(),
        "phase-detail to phase mapping must stay centralized in route_plan/route_semantics.rs: {violations:#?}"
    );
}

fn local_phase_detail_to_phase_mapping_violations(rel: &str, source: &str) -> Vec<String> {
    let syntax = parse_rust_source(rel, source);
    let mut visitor = LocalPhaseDetailMappingVisitor {
        rel,
        violations: Vec::new(),
    };
    visitor.visit_file(&syntax);
    visitor.violations
}

struct LocalPhaseDetailMappingVisitor<'a> {
    rel: &'a str,
    violations: Vec<String>,
}

impl<'ast> Visit<'ast> for LocalPhaseDetailMappingVisitor<'_> {
    fn visit_expr_match(&mut self, match_expr: &'ast syn::ExprMatch) {
        if expr_path_ident(&match_expr.expr).as_deref() == Some("phase_detail")
            && match_expr
                .arms
                .iter()
                .any(|arm| expr_contains_phase_constant(&arm.body))
        {
            self.violations.push(format!(
                "{}:{} contains a local phase_detail match that maps to public phase constants",
                self.rel,
                match_expr.match_token.span.start().line
            ));
        }
        visit::visit_expr_match(self, match_expr);
    }
}

fn expr_contains_phase_constant(expr: &syn::Expr) -> bool {
    struct PhaseConstantVisitor {
        found: bool,
    }

    impl<'ast> Visit<'ast> for PhaseConstantVisitor {
        fn visit_path(&mut self, path: &'ast syn::Path) {
            if path
                .segments
                .last()
                .is_some_and(|segment| segment.ident.to_string().starts_with("PHASE_"))
            {
                self.found = true;
            }
            visit::visit_path(self, path);
        }
    }

    let mut visitor = PhaseConstantVisitor { found: false };
    visitor.visit_expr(expr);
    visitor.found
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
        owner.contains("ExecutionReentryTarget::new(")
            && owner.contains("ExecutionReentryTargetSource::NegativeReviewOrVerificationResult"),
        "{allowed_owner} must remain the focused repair-target selection owner, including negative-result reentry targets"
    );
}

#[test]
fn normal_public_commands_have_shared_route_plan_owner() {
    let allowed_owner = "src/execution/route_plan/public_commands.rs";
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
            let constructs_close_current_task = trimmed
                .contains("PublicCommand::CloseCurrentTask {")
                && !trimmed.contains("{ .. }")
                && !line_is_in_cfg_test_module(&source, byte_offset);
            let constructs_transfer_handoff = trimmed.contains("PublicCommand::TransferHandoff {")
                && !trimmed.contains("{ .. }")
                && !line_is_in_cfg_test_module(&source, byte_offset);
            if constructs_reopen
                || constructs_repair
                || constructs_close_current_task
                || constructs_transfer_handoff
            {
                violations.push(format!("{rel}:{}: {trimmed}", index + 1));
            }
            byte_offset += line.len() + 1;
        }
    }
    assert!(
        violations.is_empty(),
        "production public commands must be constructed through the shared route-plan public command helpers:\n{}",
        violations.join("\n")
    );

    let owner = read_repo_file(allowed_owner);
    let required_variant_construction = [
        "PublicCommand::RepairReviewState {",
        "PublicCommand::Reopen {",
        "PublicCommand::CloseCurrentTask {",
        "PublicCommand::TransferHandoff {",
    ];
    assert!(
        required_variant_construction
            .iter()
            .all(|variant| owner.contains(variant)),
        "{allowed_owner} must remain the shared construction owner for normal public commands by constructing each normal public command variant"
    );
}

#[test]
fn repair_target_selection_does_not_depend_on_command_eligibility() {
    let source = read_repo_file("src/execution/repair_target_selection.rs");
    for forbidden in [
        "command_eligibility",
        "public_execution_mutation_is_authorized",
        "decide_public_mutation",
        "require_public_mutation",
    ] {
        assert!(
            !source.contains(forbidden),
            "repair_target_selection.rs must select targets from read-model/status facts, not mutation eligibility: found `{forbidden}`"
        );
    }
}

#[test]
fn repair_review_state_does_not_patch_public_route_commands_after_projection() {
    let source = read_repo_file("src/execution/review_state.rs");
    let paths = normalized_code_paths("src/execution/review_state.rs", &source);
    assert!(
        !paths
            .iter()
            .any(|path| path == "crate::execution::route_plan::reopen_public_command"),
        "review_state.rs may persist execution-reentry follow-up state, but route_plan must own reopen command binding"
    );
    assert!(
        !source.contains(".bind_public_command("),
        "repair-review-state must not patch ExecutionRoutingState public command fields after route projection"
    );
}

#[test]
fn repair_review_state_public_mutation_lives_in_command_module() {
    let command_rel = "src/execution/commands/repair_review_state.rs";
    let command_source = read_repo_file(command_rel);
    for required in [
        "pub fn repair_review_state_command(",
        "pub fn repair_review_state(",
        "fn require_repair_review_state_mutation(",
        "fn clear_resolved_task_cycle_break_for_repair_review_state(",
        "fn release_resolved_worktree_leases_for_repair_review_state(",
        "fn persist_execution_reentry_repair_target_and_refresh_routing(",
        "fn execute_repair_actions(",
        "require_public_mutation(",
        "persist_review_state_repair_follow_up(",
        "release_worktree_leases_for_current_task_closures_and_persist(",
        "resolve_current_task_closure_postconditions_for_current_workspace_and_persist(",
    ] {
        assert!(
            command_source.contains(required),
            "{command_rel} must own repair-review-state public mutation orchestration: missing `{required}`"
        );
    }

    let review_rel = "src/execution/review_state.rs";
    let review_source = read_repo_file(review_rel);
    for forbidden in [
        "pub fn repair_review_state_command(",
        "pub fn repair_review_state(",
        "fn require_repair_review_state_mutation(",
        "fn persist_execution_reentry_repair_target_and_refresh_routing(",
        "fn execute_repair_actions(",
        "require_public_mutation(",
        "persist_review_state_repair_follow_up(",
        "release_worktree_leases_for_current_task_closures_and_persist(",
        "resolve_current_task_closure_postconditions_for_current_workspace_and_persist(",
    ] {
        assert!(
            !review_source.contains(forbidden),
            "{review_rel} must stay analysis/read-support only for repair-review-state: found `{forbidden}`"
        );
    }
    let review_dependencies = normalized_dependency_paths(review_rel, &review_source);
    for forbidden_path in [
        "crate::execution::command_eligibility::require_public_mutation",
        "crate::execution::command_eligibility::PublicMutationRequest",
        "crate::execution::recording::persist_review_state_repair_follow_up",
        "crate::execution::recording::release_worktree_leases_for_current_task_closures_and_persist",
        "crate::execution::recording::resolve_current_task_closure_postconditions_for_current_workspace_and_persist",
    ] {
        assert!(
            !review_dependencies
                .iter()
                .any(|path| path == forbidden_path),
            "{review_rel} must not import write-capable repair-review-state command dependencies: found `{forbidden_path}`"
        );
    }
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
fn targetless_stale_reconcile_authority_uses_gate_snapshot_not_projected_status() {
    let stale_target_projection = read_repo_file("src/execution/stale_target_projection.rs");
    assert!(
        stale_target_projection
            .contains("pub(crate) fn targetless_stale_authority_for_gate_snapshot")
            && stale_target_projection.contains("gate_snapshot.has_authoritative_stale_target()")
            && !stale_target_projection.contains("clone_from(&status.stale_unreviewed_closures)"),
        "targetless stale reconcile authority and stale-id projection must be derived from reducer gate-snapshot targets, not projected status stale closures"
    );

    let read_model = read_repo_file("src/execution/read_model.rs");
    let read_model_paths = normalized_code_paths("src/execution/read_model.rs", &read_model);
    assert!(
        read_model_paths.iter().any(|path| {
            path == "crate::execution::stale_target_projection::targetless_stale_authority_for_gate_snapshot"
        }),
        "read-model public invariants must receive targetless stale authority from the router/reducer runtime state"
    );

    let query = read_repo_file("src/execution/query.rs");
    let query_routing_paths = normalized_code_paths("src/execution/query.rs", &query);
    assert!(
        query_routing_paths.iter().any(|path| {
            path == "crate::execution::stale_target_projection::targetless_stale_authority_for_gate_snapshot"
        }) && query_routing_paths.iter().any(|path| {
            path == "apply_read_surface_invariants_to_routing_with_targetless_authority"
                || path.ends_with("::apply_read_surface_invariants_to_routing_with_targetless_authority")
        }),
        "workflow routing invariants must receive targetless stale authority from the finalized runtime projection"
    );
    assert!(
        query.contains("diagnostic_route_decision_from_status("),
        "read-surface invariant synchronization may install a route-plan-owned diagnostic decision without rebuilding normal route authority from projected status fields"
    );

    let invariants = read_repo_file("src/execution/invariants.rs");
    assert!(
        invariants.contains("check_runtime_status_invariants_with_targetless_authority")
            && invariants.contains("targetless_stale_authority: Option<TargetlessStaleAuthority>")
            && invariants.contains("status_needs_marker_for_authority(status, authority)")
            && !invariants.contains("status_has_targetless_stale_shape_without_diagnostic"),
        "runtime invariants must require an explicit targetless stale authority object instead of using a status-only targetless predicate"
    );

    let reentry_reconcile = read_repo_file("src/execution/reentry_reconcile.rs");
    assert!(
        reentry_reconcile.contains("pub struct TargetlessStaleAuthority")
            && reentry_reconcile.contains("status_needs_marker_for_authority")
            && !reentry_reconcile.contains("status_has_bound_stale_target")
            && !reentry_reconcile.contains("public_repair_targets")
            && !reentry_reconcile.contains("execution_command_context"),
        "reentry reconcile must not infer targetless stale authority from projected public status fields"
    );
}

#[test]
fn resume_stale_precedence_has_single_semantic_owner() {
    let owner_rel = "src/execution/resume_stale_precedence.rs";
    let owner = read_repo_file(owner_rel);
    assert!(
        owner.contains("pub(crate) struct ResumeStalePrecedence")
            && owner.contains("pub(crate) fn from_inputs")
            && owner.contains("pub(crate) fn for_status_suppression")
            && owner.contains("pub(crate) struct ResumeStatusSuppressionInputs")
            && owner.contains("legal_resume_begin_route")
            && owner.contains("fn exact_resume_stale_task(")
            && owner.contains("fn resume_step_preempts_later_stale_target("),
        "{owner_rel} must own resume/stale precedence, legal begin gating, exact resume binding, and stale-preempted-by-resume selection"
    );
    assert!(
        !owner.contains("load_status_authoritative_overlay_checked")
            && !owner.contains("load_authoritative_transition_state")
            && !owner.contains("status_authoritative_overlay")
            && !owner.contains("task_closure_baseline_repair_candidate_with_stale_target")
            && !owner.contains("stale_unreviewed_allows_task_closure_baseline_bridge"),
        "{owner_rel} must receive preloaded authority facts explicitly instead of loading runtime overlays or calling IO-backed authority helpers"
    );

    for (rel, expected) in [
        (
            "src/execution/status_assembly.rs",
            "ResumeStalePrecedence::for_status_suppression",
        ),
        (
            "src/execution/repair_target_selection.rs",
            "stale_preempted_by_resume",
        ),
        (
            "src/execution/route_plan/planning_facts.rs",
            "ResumeStalePrecedence::from_inputs",
        ),
        (
            "src/execution/route_plan/next_action_choice/execution_ordering.rs",
            "ResumeStalePrecedence::from_inputs",
        ),
    ] {
        let source = read_repo_file(rel);
        let paths = normalized_code_paths(rel, &source);
        assert!(
            paths
                .iter()
                .any(|path| path.starts_with("crate::execution::resume_stale_precedence::"))
                && source.contains(expected),
            "{rel} must consume {owner_rel} for resume/stale precedence instead of recomputing local precedence"
        );
    }

    let forbidden_local_helpers = [
        "fn exact_resume_stale_record_task(",
        "fn stale_resume_begin_route_candidate(",
        "fn resume_step_preempts_later_stale_target(",
        "select_earliest_stale_boundary_candidate(",
        "earliest_stale_task_target == Some(resume_task)",
        "exact_resume_stale_task_target == Some(resume_task)",
        "status.resume_task == authority_inputs.earliest_stale_task()",
        "authority_inputs.earliest_stale_task() == status.resume_task",
    ];
    let mut sources = route_plan_boundary_sources();
    sources.extend([
        (
            String::from("src/execution/status_assembly.rs"),
            read_repo_file("src/execution/status_assembly.rs"),
        ),
        (
            String::from("src/execution/repair_target_selection.rs"),
            read_repo_file("src/execution/repair_target_selection.rs"),
        ),
    ]);
    sources.extend(repair_route_decision_sources());
    let violations = sources
        .into_iter()
        .filter(|(rel, _)| {
            rel != owner_rel
                && rel != "src/execution/stale_target_selection.rs"
                && !rel.ends_with("unit_tests.rs")
        })
        .flat_map(|(rel, source)| {
            forbidden_local_helpers
                .iter()
                .filter(move |pattern| source.contains(**pattern))
                .map(move |pattern| format!("{rel} contains `{pattern}`"))
        })
        .collect::<Vec<_>>();
    assert!(
        violations.is_empty(),
        "resume/stale precedence helpers must not be redefined outside {owner_rel}:\n{}",
        violations.join("\n")
    );

    let execution_ordering =
        read_repo_file("src/execution/route_plan/next_action_choice/execution_ordering.rs");
    assert!(
        !execution_ordering.contains("authority_inputs.earliest_stale_task()")
            && execution_ordering.contains("facts.resume_stale_precedence.earliest_stale_task")
            && execution_ordering
                .contains("resolve_execution_command_route_target_for_next_action"),
        "route ordering must consume ResumeStalePrecedence for earliest stale routing and the route-owned public-authority gate for executable begin routing"
    );
    let execution_routes =
        read_repo_file("src/execution/route_plan/next_action_choice/execution_routes.rs");
    assert!(
        execution_routes.contains("execution_command_route_target_has_public_authority"),
        "execution route decisions must gate resolved command targets through the route-owned public-authority helper before emitting executable resume/begin actions"
    );
    let repair_target_selection = read_repo_file("src/execution/repair_target_selection.rs");
    assert!(
        repair_target_selection.contains("legal_resume_begin_route")
            && repair_target_selection.contains("stale_preempted_by_resume"),
        "repair target selection must gate stale-preempted-by-resume through the shared precedence fact and a legal begin-route predicate"
    );
    assert!(
        !repair_target_selection.contains("authority_inputs.earliest_stale_task()"),
        "repair target selection must consume ResumeStalePrecedence.earliest_stale_task instead of reading authority_inputs.earliest_stale_task() directly"
    );
    assert!(
        repair_target_selection.contains("execution_command_route_target_has_authority")
            && repair_target_selection.contains("legal_resume_begin_route")
            && repair_target_selection.contains("ExecutionReentryTargetSource::ExactRouteCommand"),
        "repair target exact-route fallback must keep begin targets behind the fingerprint-bound legal resume predicate"
    );
    let resume_fallback_violations = std::iter::once((
        String::from("src/execution/repair_target_selection.rs"),
        repair_target_selection,
    ))
    .chain(repair_route_decision_sources())
    .filter(|(_, source)| source.contains(".or(status.resume_task)"))
    .map(|(rel, _)| rel)
    .collect::<Vec<_>>();
    assert!(
        resume_fallback_violations.is_empty(),
        "repair target and baseline-bridge selection must not fall back to raw resume_task fields:\n{}",
        resume_fallback_violations.join("\n")
    );
    let execution_targets = read_repo_file("src/execution/route_plan/execution_targets.rs");
    assert!(
        execution_targets.contains("fingerprint_bound_begin_route_matches_public_status")
            && execution_targets.contains("!status.execution_fingerprint.trim().is_empty()")
            && execution_targets.contains("execution_command_context_matches_route_target")
            && execution_targets.contains("repair_target.expires_when_fingerprint_changes"),
        "legal resume begin authority must be fingerprint-bound and route-owned, not satisfied by resume_task/resume_step alone"
    );
    let execution_target_authority =
        read_repo_file("src/execution/route_plan/execution_target_authority.rs");
    assert!(
        execution_target_authority.contains("target.is_begin()")
            && execution_target_authority.contains("execution_command_route_target_has_authority"),
        "the legal begin predicate must reject legal non-begin routes before they can authorize resume/stale precedence"
    );
    let route_repair_candidate_authority_fn = "route_repair_target_candidates_authorize_target";
    assert!(
        !execution_target_authority.contains("_route_repair_target_candidates"),
        "execution target authority must consume route repair candidates instead of carrying an intentionally ignored parameter"
    );
    assert!(
        execution_target_authority.contains("route_repair_target_candidates_authorize_target"),
        "execution target authority must isolate route repair candidate authorization behind a named helper"
    );
    assert!(
        function_body_source_contains(
            "src/execution/route_plan/execution_target_authority.rs",
            &execution_target_authority,
            route_repair_candidate_authority_fn,
            "route_repair_target_candidates"
        ) && function_body_source_contains(
            "src/execution/route_plan/execution_target_authority.rs",
            &execution_target_authority,
            route_repair_candidate_authority_fn,
            ".iter()"
        ),
        "route repair candidate authority must iterate the reducer-supplied candidate slice"
    );
    assert!(
        function_calls_path(
            "src/execution/route_plan/execution_target_authority.rs",
            &execution_target_authority,
            route_repair_candidate_authority_fn,
            "crate::execution::route_plan::execution_targets::public_repair_target_matches_execution_route"
        ),
        "route repair candidate authority must reuse the shared public repair target matcher"
    );
    assert!(
        function_calls_path(
            "src/execution/route_plan/execution_target_authority.rs",
            &execution_target_authority,
            route_repair_candidate_authority_fn,
            "crate::execution::route_plan::execution_targets::execution_command_route_status_blocks_progress"
        ),
        "route repair candidate authority must preserve the route-owned status blockers"
    );
    assert!(
        function_body_source_contains(
            "src/execution/route_plan/execution_target_authority.rs",
            &execution_target_authority,
            route_repair_candidate_authority_fn,
            "target.is_begin()"
        ) && function_body_source_contains(
            "src/execution/route_plan/execution_target_authority.rs",
            &execution_target_authority,
            route_repair_candidate_authority_fn,
            "status.execution_fingerprint.trim().is_empty()"
        ),
        "route repair candidate authority must reject begin routes without a runtime fingerprint"
    );
    assert!(
        execution_targets.contains("pub(crate) fn public_repair_target_matches_execution_route"),
        "execution target matching should expose one shared helper for status-published targets and route repair candidates"
    );
}

#[test]
fn stale_target_projection_uses_preloaded_authority_for_current_task_stale_records() {
    let owner = read_repo_file("src/execution/stale_target_projection.rs");
    let current_task_stale_paths = rust_source_scan::normalized_call_paths_in_function(
        "src/execution/stale_target_projection.rs",
        &owner,
        "append_current_task_stale_targets",
    );
    assert!(
        current_task_stale_paths.iter().any(|path| {
            path == "crate::execution::current_closure_projection::stale_current_task_closure_records_from_authoritative_state"
        }),
        "stale-target projection must consume the supplied authoritative-state snapshot instead of reloading transition state from disk"
    );
    assert!(
        !current_task_stale_paths.iter().any(|path| {
            path == "crate::execution::current_closure_projection::stale_current_task_closure_records"
        }),
        "stale-target projection must not reload current task stale records when no authoritative state was supplied"
    );
}

#[test]
fn review_and_finish_gates_use_shared_branch_gate_binding_snapshot() {
    let runtime_methods = read_repo_file("src/execution/state/runtime_methods.rs");
    assert!(
        runtime_methods.contains("gate_review_from_context_with_authoritative_state")
            && runtime_methods.contains("gate_finish_from_context_with_authoritative_state")
            && runtime_methods.contains("current_branch_gate_bindings_from_authoritative_state")
            && runtime_methods.contains("gate.current_branch_reviewed_state_id")
            && runtime_methods.contains("gate.current_branch_closure_id")
            && runtime_methods.contains("gate.finish_review_gate_pass_branch_closure_id"),
        "public review/finish gate flows must pass the preloaded authoritative state through gate evaluation and binding projection"
    );
    let review_gate_source = read_repo_file("src/execution/state/review_gate.rs");
    let finish_gate_source = read_repo_file("src/execution/state/finish_gate.rs");
    for (rel, source, authoritative_state_loader_entrypoints) in [
        (
            "src/execution/state/review_gate.rs",
            review_gate_source.as_str(),
            &[
                "gate_review_from_context",
                "gate_review_from_context_internal",
            ][..],
        ),
        (
            "src/execution/state/finish_gate.rs",
            finish_gate_source.as_str(),
            &["gate_finish_from_context"][..],
        ),
    ] {
        let disallowed_loaders = rust_source_scan::forbidden_call_violations(
            rel,
            source,
            &["load_authoritative_transition_state"],
            authoritative_state_loader_entrypoints,
        );
        assert!(
            disallowed_loaders.is_empty(),
            "{rel} gate helpers must consume the caller-supplied authoritative state snapshot instead of reloading it: {disallowed_loaders:?}"
        );
    }
    for (rel, source) in [
        (
            "src/execution/state/runtime_methods.rs",
            runtime_methods.as_str(),
        ),
        (
            "src/execution/state/review_gate.rs",
            review_gate_source.as_str(),
        ),
        (
            "src/execution/state/finish_gate.rs",
            finish_gate_source.as_str(),
        ),
    ] {
        for forbidden in [
            "current_branch_closure_id(context)",
            "current_branch_reviewed_state_id(context)",
            "validated_current_branch_closure_identity(context)",
        ] {
            assert!(
                !source.contains(forbidden),
                "{rel} must reuse shared current-branch gate bindings instead of reloading or recomputing `{forbidden}`"
            );
        }
    }
}

#[test]
fn workflow_doctor_uses_execution_owned_reason_vocabulary() {
    let operator = read_repo_file("src/workflow/operator.rs");
    let operator_dependency_paths =
        normalized_dependency_paths("src/workflow/operator.rs", &operator);
    assert!(
        operator_dependency_paths
            .iter()
            .any(|path| path.starts_with("crate::execution::review_route_tokens"))
            && !operator.contains("final_review_state_not_fresh")
            && !operator.contains("browser_qa_state_not_fresh")
            && !operator.contains("release_docs_state_not_fresh")
            && !operator.contains("plan_fingerprint_mismatch"),
        "workflow doctor synthetic gate-review classification must consume execution-owned diagnostic vocabulary instead of local reason-code families"
    );

    let review_route_tokens = read_repo_file("src/execution/review_route_tokens.rs");
    assert!(
        review_route_tokens.contains("REASON_FINAL_REVIEW_STATE_NOT_FRESH")
            && review_route_tokens.contains("REASON_BROWSER_QA_STATE_NOT_FRESH")
            && review_route_tokens.contains("REASON_RELEASE_DOCS_STATE_NOT_FRESH")
            && review_route_tokens.contains("REASON_PLAN_FINGERPRINT_MISMATCH"),
        "execution review-route vocabulary must own doctor synthetic gate-review reason/failure classification"
    );
}

#[test]
fn late_stage_surface_reason_code_vocabulary_has_single_owner() {
    let owner = read_repo_file("src/execution/current_truth.rs");
    assert!(
        owner.contains("REASON_LATE_STAGE_SURFACE_NOT_DECLARED")
            && owner.contains("REASON_BRANCH_DRIFT_ESCAPES_LATE_STAGE_SURFACE")
            && owner.contains("late_stage_surface_not_declared_reason_code")
            && owner.contains("branch_drift_escapes_late_stage_surface_reason_code"),
        "current_truth.rs must own late-stage surface reason-code constants and predicates"
    );

    let violations = rust_source_files(&repo_root().join("src/execution"))
        .into_iter()
        .map(|path| repo_relative(&path))
        .filter(|rel| rel != "src/execution/current_truth.rs")
        .filter(|rel| {
            let source = read_repo_file(rel);
            source.contains("\"late_stage_surface_not_declared\"")
                || source.contains("\"branch_drift_escapes_late_stage_surface\"")
        })
        .collect::<Vec<_>>();
    assert!(
        violations.is_empty(),
        "late-stage surface reason-code string literals must stay in current_truth.rs and be consumed through shared constants/predicates: {violations:?}"
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
fn branch_rerecording_with_authority_does_not_reload_authoritative_state() {
    let current_truth_rel = "src/execution/current_truth.rs";
    let current_truth = read_repo_file(current_truth_rel);
    for function_name in [
        "branch_closure_rerecording_assessment_with_authority",
        "tracked_paths_changed_since_record_branch_closure_baseline_with_authority",
        "current_branch_task_closure_records_with_authority",
        "branch_closure_identity_for_rerecording_with_authority",
        "current_branch_closure_reviewed_tree_sha_with_authority",
        "late_stage_missing_current_closure_stale_provenance_present_with_authority",
    ] {
        for forbidden_leaf in [
            "load_authoritative_transition_state",
            "still_current_task_closure_records",
            "tracked_paths_changed_since_record_branch_closure_baseline",
            "current_branch_closure_reviewed_tree_sha",
        ] {
            assert!(
                !function_calls_leaf(
                    current_truth_rel,
                    &current_truth,
                    function_name,
                    forbidden_leaf,
                ),
                "{function_name} must consume supplied authority and avoid reload-backed `{forbidden_leaf}`"
            );
        }
    }

    let late_stage_rel = "src/execution/status_assembly/late_stage.rs";
    let late_stage = read_repo_file(late_stage_rel);
    assert!(
        function_calls_leaf(
            late_stage_rel,
            &late_stage,
            "apply_late_stage_precedence_status_overlay",
            "authoritative_late_stage_rederivation_basis_present_with_authority",
        ),
        "late-stage precedence overlay must thread the supplied authoritative state into basis selection"
    );
    assert!(
        !function_calls_leaf(
            late_stage_rel,
            &late_stage,
            "apply_late_stage_precedence_status_overlay",
            "authoritative_late_stage_rederivation_basis_present",
        ),
        "late-stage precedence overlay must not call the reload-backed basis helper"
    );
    for function_name in [
        "authoritative_late_stage_rederivation_basis_present_with_authority",
        "current_task_closure_set_ready_for_late_stage_with_authority",
    ] {
        for forbidden_leaf in [
            "load_authoritative_transition_state",
            "load_status_authoritative_overlay_checked",
            "structural_current_task_closure_failures",
            "still_current_task_closure_records",
            "branch_closure_rerecording_assessment",
            "validated_current_branch_closure_identity",
        ] {
            assert!(
                !function_calls_leaf(late_stage_rel, &late_stage, function_name, forbidden_leaf),
                "{function_name} must use authoritative-state variants instead of reload-backed `{forbidden_leaf}`"
            );
        }
    }

    let status_assembly_rel = "src/execution/status_assembly.rs";
    let status_assembly = read_repo_file(status_assembly_rel);
    assert!(
        function_calls_leaf(
            status_assembly_rel,
            &status_assembly,
            "status_with_facts_from_context_with_overlay_and_projection_detail",
            "authoritative_late_stage_rederivation_basis_present_with_authority",
        ),
        "status assembly must preserve supplied authority when preparing task-boundary late-stage basis"
    );
    assert!(
        function_calls_leaf(
            status_assembly_rel,
            &status_assembly,
            "status_with_facts_from_context_with_overlay_and_projection_detail",
            "suppress_preempted_resume_status_fields",
        ),
        "status assembly must pass resolved overlay inputs into resume suppression instead of making that helper reload"
    );
    assert!(
        !function_calls_leaf(
            status_assembly_rel,
            &status_assembly,
            "suppress_preempted_resume_status_fields",
            "load_status_authoritative_overlay_checked",
        ),
        "resume suppression must consume caller-supplied overlay fields instead of reloading status authority"
    );
    assert!(
        function_calls_leaf(
            status_assembly_rel,
            &status_assembly,
            "status_with_facts_from_context_with_overlay_and_projection_detail",
            "current_task_closure_tasks_for_status_projection",
        ),
        "status assembly must derive task-boundary closure records from the supplied authoritative state once"
    );
    assert!(
        function_calls_leaf(
            status_assembly_rel,
            &status_assembly,
            "apply_current_task_closure_repair_status_overlay",
            "project_current_task_closure_repair_reason_codes_from_authoritative_state",
        ),
        "current task-closure repair projection must consume the supplied authoritative state"
    );
    assert!(
        !function_calls_leaf(
            status_assembly_rel,
            &status_assembly,
            "apply_current_task_closure_repair_status_overlay",
            "project_current_task_closure_repair_reason_codes",
        ),
        "current task-closure repair projection must not call the reload-backed repair reason helper"
    );
    let status_support_rel = "src/execution/status_support.rs";
    let status_support = read_repo_file(status_support_rel);
    for function_name in [
        "task_closure_recording_prerequisites_with_authority",
        "task_closure_baseline_repair_candidate_with_stale_target_and_authority",
    ] {
        assert!(
            function_calls_leaf(
                status_support_rel,
                &status_support,
                function_name,
                "current_task_review_dispatch_id_for_task",
            ),
            "{function_name} must derive task dispatch ids from the caller-supplied overlay"
        );
        assert!(
            !function_calls_leaf(
                status_support_rel,
                &status_support,
                function_name,
                "current_review_dispatch_id_if_still_current",
            ),
            "{function_name} must not reenter reload-backed dispatch currentness under an authority-threaded path"
        );
        assert!(
            !function_calls_leaf(
                status_support_rel,
                &status_support,
                function_name,
                "current_review_dispatch_id_from_lineage",
            ),
            "{function_name} must not reload dispatch lineage under an authority-threaded path"
        );
    }
    let task_state_rel = "src/execution/status_assembly/task_state.rs";
    let task_state = read_repo_file(task_state_rel);
    assert!(
        !function_calls_leaf(
            task_state_rel,
            &task_state,
            "apply_task_boundary_status_overlay",
            "authoritative_late_stage_rederivation_basis_present",
        ),
        "task-boundary status overlay must receive a precomputed basis instead of using the reload-backed helper"
    );
    for forbidden_leaf in [
        "load_authoritative_transition_state",
        "load_status_authoritative_overlay_checked",
        "require_prior_task_closure_for_begin",
        "task_closure_recording_prerequisites",
        "task_closure_baseline_repair_candidate_with_stale_target",
        "stale_unreviewed_allows_task_closure_baseline_bridge",
        "structural_current_task_closure_failures",
        "still_current_task_closure_records",
        "valid_current_task_closure_records",
    ] {
        assert!(
            !function_calls_leaf(
                task_state_rel,
                &task_state,
                "apply_task_boundary_status_overlay",
                forbidden_leaf,
            ),
            "task-boundary status overlay must receive precomputed status inputs instead of reload-backed `{forbidden_leaf}`"
        );
        assert!(
            !function_calls_leaf(
                task_state_rel,
                &task_state,
                "completed_plan_missing_current_closure_task_from_records",
                forbidden_leaf,
            ),
            "completed-plan task-boundary helper must consume caller-supplied closure records instead of reload-backed `{forbidden_leaf}`"
        );
        assert!(
            !function_calls_leaf(
                task_state_rel,
                &task_state,
                "execution_reentry_current_task_closure_targets_from_inputs",
                forbidden_leaf,
            ),
            "execution-reentry target projection must consume caller-supplied structural failures instead of reload-backed `{forbidden_leaf}`"
        );
    }
    for (function_name, forbidden_leaf) in [
        (
            "task_closure_baseline_bridge_preempts_resume",
            "task_closure_baseline_repair_candidate_with_stale_target",
        ),
        (
            "task_closure_baseline_bridge_preempts_resume",
            "stale_unreviewed_allows_task_closure_baseline_bridge",
        ),
        (
            "apply_task_closure_baseline_bridge_from_stale_projection",
            "task_closure_baseline_repair_candidate_with_stale_target",
        ),
    ] {
        assert!(
            !function_calls_leaf(
                status_assembly_rel,
                &status_assembly,
                function_name,
                forbidden_leaf,
            ),
            "{function_name} must consume authority-backed baseline bridge helpers instead of reload-backed `{forbidden_leaf}`"
        );
    }
    let review_state_rel = "src/execution/review_state.rs";
    let review_state = read_repo_file(review_state_rel);
    for (function_name, forbidden_leaf) in [
        (
            "analyze_repair_phase_bundle",
            "branch_closure_rerecording_assessment",
        ),
        (
            "analyze_repair_phase_bundle",
            "task_closure_baseline_bridge_target_task",
        ),
        (
            "unrecoverable_task_scope_authority_loss_task_from_read_scope",
            "task_closure_baseline_repair_candidate_with_stale_target",
        ),
    ] {
        assert!(
            !function_calls_leaf(
                review_state_rel,
                &review_state,
                function_name,
                forbidden_leaf
            ),
            "{function_name} must consume supplied repair-phase authority instead of reload-backed `{forbidden_leaf}`"
        );
    }
    let baseline_bridge_rel = "src/execution/repair_route_decision/baseline_bridge.rs";
    let baseline_bridge = read_repo_file(baseline_bridge_rel);
    for function_name in [
        "task_closure_baseline_bridge_route_task",
        "task_closure_baseline_bridge_allows_stale_boundary_route",
        "task_closure_baseline_bridge_reentry_target",
        "task_closure_baseline_bridge_repair_review_state_route",
        "task_closure_baseline_bridge_route_ready_for_status",
        "task_closure_baseline_bridge_ready_for_task_impl",
    ] {
        for forbidden_leaf in [
            "task_closure_baseline_bridge_target_task",
            "task_closure_baseline_bridge_ready_for_stale_target",
            "task_closure_baseline_repair_candidate_with_stale_target",
            "missing_baseline_bridge_candidate_for_stale_target",
        ] {
            assert!(
                !function_calls_leaf(
                    baseline_bridge_rel,
                    &baseline_bridge,
                    function_name,
                    forbidden_leaf,
                ),
                "{function_name} must consume route-planning authority inputs instead of reload-backed `{forbidden_leaf}`"
            );
        }
    }
    let baseline_bridge_predicates_rel =
        "src/execution/repair_route_decision/baseline_bridge/predicates.rs";
    let baseline_bridge_predicates = read_repo_file(baseline_bridge_predicates_rel);
    assert!(
        !function_calls_leaf(
            baseline_bridge_predicates_rel,
            &baseline_bridge_predicates,
            "baseline_bridge_candidate_present",
            "task_closure_baseline_repair_candidate_with_stale_target",
        ),
        "baseline bridge candidate predicates must consume authority inputs instead of reload-backed baseline candidate helpers"
    );
    let repair_target_selection_rel = "src/execution/repair_target_selection.rs";
    let repair_target_selection = read_repo_file(repair_target_selection_rel);
    assert!(
        !function_calls_leaf(
            repair_target_selection_rel,
            &repair_target_selection,
            "execution_reentry_target",
            "task_closure_baseline_reentry_target",
        ),
        "execution reentry target selection must consume authority-threaded baseline reentry helpers"
    );
    assert!(
        !function_calls_leaf(
            repair_target_selection_rel,
            &repair_target_selection,
            "task_closure_baseline_reentry_target_with_authority",
            "task_closure_baseline_repair_candidate_with_stale_target",
        ),
        "authority-threaded baseline reentry target must not call reload-backed baseline candidate helpers"
    );
    assert!(
        function_calls_leaf(
            repair_target_selection_rel,
            &repair_target_selection,
            "task_closure_baseline_reentry_target_with_authority",
            "task_closure_baseline_repair_candidate_with_stale_target_and_authority",
        ),
        "authority-threaded baseline reentry target should reuse the authority-backed candidate helper"
    );
    let route_plan_rel = "src/execution/route_plan.rs";
    let route_plan = read_repo_file(route_plan_rel);
    assert!(
        route_plan.contains("runtime_state.overlay.as_ref()")
            && route_plan.contains("authoritative_state"),
        "route planning must seed baseline-bridge route inputs with the same overlay and authoritative state used for status reduction"
    );
    let stale_projection_rel = "src/execution/stale_target_projection.rs";
    let stale_projection = read_repo_file(stale_projection_rel);
    for forbidden_leaf in [
        "late_stage_missing_current_closure_stale_provenance_present",
        "stale_current_task_closure_records",
        "task_closure_baseline_bridge_ready_for_stale_target",
    ] {
        assert!(
            !function_calls_leaf(
                stale_projection_rel,
                &stale_projection,
                "project_authoritative_stale_targets",
                forbidden_leaf,
            ),
            "stale-target projection must consume supplied authority instead of reload-backed `{forbidden_leaf}`"
        );
    }
    assert!(
        function_calls_leaf(
            stale_projection_rel,
            &stale_projection,
            "project_authoritative_stale_targets",
            "late_stage_missing_current_closure_stale_provenance_present_with_authority",
        ),
        "stale-target projection should derive missing-current-closure provenance from supplied authority"
    );
    let late_stage_rel = "src/execution/status_assembly/late_stage.rs";
    let late_stage = read_repo_file(late_stage_rel);
    assert!(
        function_calls_leaf(
            late_stage_rel,
            &late_stage,
            "apply_late_stage_precedence_status_overlay",
            "context_all_task_scopes_closed_from_authority",
        ),
        "late-stage status overlay should use the no-reload authority closed-scope helper"
    );
    assert!(
        !function_calls_leaf(
            late_stage_rel,
            &late_stage,
            "apply_late_stage_precedence_status_overlay",
            "context_all_task_scopes_closed_by_authority",
        ),
        "late-stage status overlay must not reload authority when supplied authority is absent"
    );
    for rel in [
        "src/execution/route_plan/next_action_choice/execution_ordering.rs",
        "src/execution/route_plan/next_action_choice/execution_routes.rs",
    ] {
        let source = read_repo_file(rel);
        assert!(
            !source.contains("execution_reentry_requires_review_state_repair(Some(context)"),
            "{rel} must use authority-threaded repair checks instead of reload-capable repair checks"
        );
    }
    let blocking_records_rel = "src/execution/status_assembly/blocking_records.rs";
    let blocking_records = read_repo_file(blocking_records_rel);
    assert!(
        function_calls_leaf(
            blocking_records_rel,
            &blocking_records,
            "derive_structural_current_task_closure_blocking_records",
            "structural_current_task_closure_failures_from_authoritative_state",
        ),
        "structural blocking records must consume the supplied authoritative state"
    );
    for forbidden_leaf in [
        "load_authoritative_transition_state",
        "structural_current_task_closure_failures",
        "still_current_task_closure_records",
        "valid_current_task_closure_records",
    ] {
        assert!(
            !function_calls_leaf(
                blocking_records_rel,
                &blocking_records,
                "derive_structural_current_task_closure_blocking_records",
                forbidden_leaf,
            ),
            "structural blocking records must avoid reload-backed `{forbidden_leaf}`"
        );
    }
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
fn non_command_execution_modules_do_not_import_command_common() {
    let mut source_rels = rust_source_files(&repo_root().join("src/execution"))
        .into_iter()
        .chain(rust_source_files(&repo_root().join("src/workflow")))
        .map(|path| repo_relative(&path))
        .collect::<Vec<_>>();
    source_rels.sort();
    source_rels.dedup();
    let violations = source_rels
        .into_iter()
        .filter(|rel| !rel.starts_with("src/execution/commands/"))
        .filter(|rel| rel != "src/execution/mutate.rs")
        .flat_map(|rel| {
            let source = read_repo_file(&rel);
            normalized_dependency_paths(&rel, &source)
                .into_iter()
                .filter(|path| {
                    path == "crate::execution::commands::common"
                        || path.starts_with("crate::execution::commands::common::")
                })
                .map(move |path| format!("{rel}: forbidden dependency path `{path}`"))
        })
        .collect::<Vec<_>>();
    assert!(
        violations.is_empty(),
        "non-command execution modules must consume neutral execution helpers instead of command-common presentation internals:\n{}",
        violations.join("\n")
    );
}

#[test]
fn reducer_consumes_truth_projection_through_runtime_truth_not_state_facade() {
    let reducer = read_repo_file("src/execution/reducer.rs");
    let dependencies = normalized_dependency_paths("src/execution/reducer.rs", &reducer);
    for required in [
        "crate::execution::runtime_truth::ExecutionDerivedTruth",
        "crate::execution::runtime_truth::compute_status_blocking_records",
        "crate::execution::runtime_truth::derive_execution_truth_from_authority",
        "crate::execution::runtime_truth::derive_execution_truth_from_authority_with_gates_and_projection_detail",
    ] {
        assert!(
            dependencies.iter().any(|path| path == required),
            "reducer.rs must consume reducer truth projection through runtime_truth: missing `{required}` in {dependencies:?}"
        );
    }
    for forbidden in [
        "crate::execution::state::ExecutionDerivedTruth",
        "crate::execution::state::FinalReviewDispatchAuthority",
        "crate::execution::state::compute_status_blocking_records",
        "crate::execution::state::current_task_review_dispatch_id_for_status",
        "crate::execution::state::derive_execution_truth_from_authority",
        "crate::execution::state::derive_execution_truth_from_authority_with_gates",
        "crate::execution::state::derive_execution_truth_from_authority_with_gates_and_projection_detail",
        "crate::execution::read_model::compute_status_blocking_records",
        "crate::execution::read_model::derive_execution_truth_from_authority",
        "crate::execution::read_model::derive_execution_truth_from_authority_with_gates",
        "crate::execution::read_model::derive_execution_truth_from_authority_with_gates_and_projection_detail",
    ] {
        assert!(
            !dependencies.iter().any(|path| path == forbidden),
            "reducer.rs must not consume reducer truth projection through state/read_model compatibility paths: found `{forbidden}` in {dependencies:?}"
        );
    }
}

#[test]
fn runtime_truth_and_reducer_do_not_depend_on_read_model_presentation() {
    for rel in ["src/execution/runtime_truth.rs", "src/execution/reducer.rs"] {
        let source = read_repo_file(rel);
        assert_no_import_path_prefix(
            rel,
            &source,
            &[
                "crate::execution::read_model",
                "crate::execution::read_model_support",
            ],
            "must derive reducer/runtime truth without depending on read-model presentation helpers",
        );
    }

    let mut status_assembly_sources = vec![(
        String::from("src/execution/status_assembly.rs"),
        read_repo_file("src/execution/status_assembly.rs"),
    )];
    status_assembly_sources.extend(
        rust_source_files(&repo_root().join("src/execution/status_assembly"))
            .into_iter()
            .map(|path| {
                let rel = repo_relative(&path);
                let source = read_repo_file(&rel);
                (rel, source)
            }),
    );
    for (rel, source) in status_assembly_sources {
        assert_no_import_path_prefix(
            &rel,
            &source,
            &[
                "crate::execution::read_model",
                "crate::execution::read_model_support",
                "crate::execution::commands",
                "crate::execution::mutate",
            ],
            "status assembly is the lower shared status helper and must not import read-model presentation or mutation layers",
        );
    }

    let status_support = read_repo_file("src/execution/status_support.rs");
    assert_no_import_path_prefix(
        "src/execution/status_support.rs",
        &status_support,
        &[
            "crate::execution::read_model",
            "crate::execution::read_model_support",
            "crate::execution::status_assembly",
            "crate::workflow",
        ],
        "status_support is the lower shared execution-status helper and must not depend on presentation/status assembly layers",
    );
}

#[test]
fn command_common_remains_a_facade_over_bounded_domain_modules() {
    let common_source = read_repo_file("src/execution/commands/common.rs");
    assert!(
        !common_source.contains("\nfn ")
            && !common_source.contains("\npub fn ")
            && !common_source.contains("\npub(super) fn ")
            && !common_source.contains("\npub(in crate::execution::commands) fn "),
        "src/execution/commands/common.rs should not regain production helper bodies"
    );
    let common_modules = source_module_names("src/execution/commands/common.rs", &common_source);
    assert!(
        !common_modules.is_empty(),
        "src/execution/commands/common.rs should remain a facade over focused command-support modules"
    );

    for path in rust_source_files(&repo_root().join("src/execution/commands/common")) {
        let rel = repo_relative(&path);
        if rel.ends_with("/unit_tests.rs") {
            continue;
        }
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{rel} should be readable: {error}"));
        assert_no_import_path_prefix(
            &rel,
            &source,
            &[
                "crate::execution::read_model",
                "crate::execution::read_model_support",
                "crate::workflow",
            ],
            "command-support modules must not import read-model or workflow presentation layers",
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
    let closure_dispatch_paths =
        normalized_dependency_paths("src/execution/closure_dispatch.rs", &closure_dispatch);
    assert!(
        !closure_dispatch_paths
            .iter()
            .any(|path| path.starts_with("crate::execution::closure_dispatch_mutation::")),
        "closure_dispatch.rs must not import mutation helpers: {closure_dispatch_paths:?}"
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
        !closure_dispatch.contains("claim_step_write_authority")
            && !closure_dispatch.contains("persist_if_dirty")
            && !closure_dispatch.contains("fn ensure_review_dispatch_authoritative_bootstrap(")
            && !closure_dispatch.contains("closure_dispatch_mutation")
            && !closure_dispatch.contains("ReviewDispatchMutationAction"),
        "closure_dispatch.rs must not regain or re-export dispatch mutation or bootstrap authority"
    );
    // These pub(crate) functions are the cross-module mutation boundary consumed
    // by runtime methods and command helpers; private recording helper names are
    // intentionally not part of the boundary contract.
    assert!(
        closure_dispatch_mutation.contains("pub(crate) fn ensure_current_review_dispatch_id")
            && closure_dispatch_mutation
                .contains("pub(crate) fn ensure_review_dispatch_authoritative_bootstrap")
            && closure_dispatch_mutation
                .contains("pub(crate) fn record_review_dispatch_strategy_checkpoint")
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

    let status_support = read_repo_file("src/execution/status_support.rs");
    for forbidden in [
        "fn current_review_dispatch_id_if_still_current",
        "fn current_review_dispatch_id_from_lineage",
        "shared_current_task_review_dispatch_id",
        "shared_current_final_review_dispatch_id",
    ] {
        assert!(
            !status_support.contains(forbidden),
            "status_support.rs must not regain dispatch lookup ownership from closure_dispatch: found `{forbidden}`"
        );
    }

    let public_repair_targets = read_repo_file("src/execution/public_repair_targets.rs");
    assert!(
        public_repair_targets.contains(
            "crate::execution::closure_dispatch::current_review_dispatch_id_if_still_current"
        ) && !public_repair_targets.contains("task_review_dispatch_id(task)"),
        "public repair targets must consume closure_dispatch dispatch authority without raw transition dispatch lookup"
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
fn executable_route_surfaces_do_not_reconstruct_from_presentation_dtos() {
    let review_state_rel = "src/execution/review_state.rs";
    let review_state = read_repo_file(review_state_rel);
    for forbidden in [
        "routing_recommended_command",
        "routing_recommended_command_argv",
        "routing_recommended_command_template",
        "routing_required_inputs",
    ] {
        assert!(
            !review_state.contains(forbidden),
            "review_state.rs must not keep DTO fallback helpers that reconstruct executable route surfaces: found `{forbidden}`"
        );
    }
    assert!(
        function_calls_leaf(
            review_state_rel,
            &review_state,
            "final_close_current_task_route",
            "close_current_task_route_from_decision",
        ),
        "final close-current-task recovery must consume a finalized RouteDecision"
    );
    assert!(
        function_body_source_contains(
            review_state_rel,
            &review_state,
            "final_close_current_task_route",
            ".route_decision",
        ),
        "final close-current-task recovery must derive executable authority from the final routing decision"
    );
    for forbidden_fragment in [
        "final_routing.recommended_public_command",
        "final_routing.recommended_public_command_argv",
        "final_routing.recommended_public_command_template",
        "final_routing.required_inputs",
    ] {
        assert!(
            !function_body_source_contains(
                review_state_rel,
                &review_state,
                "final_close_current_task_route",
                forbidden_fragment,
            ),
            "final_close_current_task_route must not reconstruct executable route authority from `{forbidden_fragment}`"
        );
    }

    let repair_review_state_rel = "src/execution/commands/repair_review_state.rs";
    let repair_review_state = read_repo_file(repair_review_state_rel);
    for forbidden_fragment in [
        "task_closure_repair_output_route_decision",
        ".or(Some(&route_decision))",
    ] {
        assert!(
            !repair_review_state.contains(forbidden_fragment),
            "repair-review-state must not feed non-final route decisions into executable close-current-task output: found `{forbidden_fragment}`"
        );
    }
    if let Some(branch_start) =
        repair_review_state.find("route_action.kind == RepairRouteActionKind::CloseCurrentTask")
    {
        let branch_tail = &repair_review_state[branch_start..];
        let branch_end = branch_tail
            .find("if let Some(required_follow_up)")
            .unwrap_or(branch_tail.len());
        let close_current_task_branch = &branch_tail[..branch_end];
        for forbidden_fragment in [
            "route_action.recommended_command()",
            "route_action.recommended_command_argv()",
            "route_action.recommended_command_template()",
            "route_action.required_inputs()",
        ] {
            assert!(
                !close_current_task_branch.contains(forbidden_fragment),
                "repair-review-state close-current-task recovery must not emit executable public surfaces from pre-final route_action: found `{forbidden_fragment}`"
            );
        }
    }

    let operator_rel = "src/workflow/operator.rs";
    let operator = read_repo_file(operator_rel);
    assert!(
        function_body_source_contains(
            operator_rel,
            &operator,
            "review_requires_execution_reentry",
            "operator_execution_command_context",
        ),
        "operator execution-reentry presentation must consume route-decision execution command context"
    );
    for forbidden_fragment in ["gate_review", "gate.allowed", "context.phase"] {
        assert!(
            !function_body_source_contains(
                operator_rel,
                &operator,
                "review_requires_execution_reentry",
                forbidden_fragment,
            ),
            "operator execution-reentry presentation must not recompute route semantics from `{forbidden_fragment}`"
        );
    }
}

#[test]
fn close_current_task_rebuilds_route_state_after_lineage_refresh() {
    let rel = "src/execution/commands/close_current_task.rs";
    let source = read_repo_file(rel);
    let span = rust_source_scan::function_spans(rel, &source)
        .into_iter()
        .find(|span| span.name == "close_current_task")
        .unwrap_or_else(|| panic!("{rel} should define function `close_current_task`"));
    let body = source
        .lines()
        .skip(span.start_line.saturating_sub(1))
        .take(span.end_line.saturating_sub(span.start_line) + 1)
        .collect::<Vec<_>>()
        .join("\n");
    let refresh = body
        .find("refresh_task_closure_authoritative_lineage_with_context")
        .expect("positive close-current-task path should refresh task closure lineage");
    let post_refresh_body = &body[refresh..];
    let status_rebuild = post_refresh_body
        .find("status = status_with_shared_routing_or_context")
        .expect("close-current-task must rebuild shared route status after lineage refresh");
    let operator_rebuild = post_refresh_body
        .find("let operator = current_workflow_operator")
        .expect("close-current-task must rebuild operator projection after lineage refresh");
    let replay_decision = post_refresh_body
        .find("handle_close_current_task_already_current_decision")
        .expect("close-current-task should keep the already-current replay decision");
    assert!(
        status_rebuild < replay_decision,
        "post-refresh already-current replay must use rebuilt shared route status"
    );
    assert!(
        operator_rebuild < replay_decision,
        "post-refresh already-current replay must use rebuilt operator projection"
    );
}

#[test]
fn read_model_support_compatibility_layer_is_removed() {
    assert!(
        !repo_root()
            .join("src/execution/read_model_support.rs")
            .exists(),
        "read_model_support.rs compatibility re-export must stay deleted; import status_support or narrower owners directly"
    );
    let execution_mod = read_repo_file("src/execution/mod.rs");
    assert!(
        !execution_mod.contains("mod read_model_support"),
        "execution/mod.rs must not export the deleted read_model_support compatibility layer"
    );
    for source_path in rust_source_files(&repo_root().join("src/execution")) {
        let rel = repo_relative(&source_path);
        if rel.contains("_tests.rs") || rel.ends_with("/unit_tests.rs") {
            continue;
        }
        let source = read_repo_file(&rel);
        assert_no_import_path_prefix(
            &rel,
            &source,
            &["crate::execution::read_model_support"],
            "must import status_support or the narrower authoritative owner directly",
        );
    }
}

#[test]
fn public_repair_target_reason_vocabulary_has_single_owner() {
    let owner_rel = "src/execution/public_repair_target_reasons.rs";
    let reason_literals = PublicRepairTargetReason::ALL
        .iter()
        .map(|reason| reason.as_str().to_owned())
        .collect::<BTreeSet<_>>();
    assert!(
        reason_literals.len() == PublicRepairTargetReason::ALL.len(),
        "public repair-target reason owner must expose a unique complete reason list"
    );
    assert!(
        reason_literals.contains(PERSISTED_REVIEW_STATE_REPAIR_FOLLOW_UP_REASON_PREFIX),
        "public repair-target reason owner must include the persisted review-state follow-up prefix in the typed vocabulary"
    );
    let dynamic_persisted_follow_up =
        persisted_review_state_repair_follow_up_reason("execution_reentry");
    assert!(
        PublicRepairTargetReason::PersistedReviewStateRepairFollowUp
            .matches(&dynamic_persisted_follow_up),
        "public repair-target reason owner must match dynamic persisted review-state follow-up reasons"
    );
    assert!(
        !PublicRepairTargetReason::PersistedReviewStateRepairFollowUp
            .matches(PERSISTED_REVIEW_STATE_REPAIR_FOLLOW_UP_REASON_PREFIX),
        "the persisted review-state follow-up prefix alone is vocabulary, not a complete dynamic public repair-target reason"
    );
    assert!(
        !PublicRepairTargetReason::PersistedReviewStateRepairFollowUp
            .matches(&persisted_review_state_repair_follow_up_reason("")),
        "empty persisted review-state follow-up suffixes must not match dynamic repair-target reasons"
    );

    let mut violations = Vec::new();
    for (rel, source, scan_mode) in public_repair_target_reason_scan_sources(owner_rel) {
        let literals = match scan_mode {
            PublicRepairTargetReasonScanMode::ProductionCode => {
                rust_production_string_literal_values(&rel, &source)
            }
            PublicRepairTargetReasonScanMode::AllCode => rust_string_literal_values(&rel, &source),
        };
        for literal in literals {
            if reason_literals.contains(&literal)
                || PublicRepairTargetReason::PersistedReviewStateRepairFollowUp.matches(&literal)
            {
                violations.push(format!(
                    "{rel}: raw public repair-target reason `{literal}`"
                ));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "public repair-target reason strings must be produced and matched through public_repair_target_reasons.rs:\n{}",
        violations.join("\n")
    );
}

#[derive(Debug, Clone, Copy)]
enum PublicRepairTargetReasonScanMode {
    ProductionCode,
    AllCode,
}

fn public_repair_target_reason_scan_sources(
    owner_rel: &str,
) -> Vec<(String, String, PublicRepairTargetReasonScanMode)> {
    let mut sources = Vec::new();
    for source_path in rust_source_files(&repo_root().join("src/execution")) {
        let rel = repo_relative(&source_path);
        if rel == owner_rel {
            continue;
        }
        sources.push((
            rel.clone(),
            read_repo_file(&rel),
            PublicRepairTargetReasonScanMode::ProductionCode,
        ));
    }
    for source_path in rust_source_files(&repo_root().join("tests")) {
        let rel = repo_relative(&source_path);
        sources.push((
            rel.clone(),
            read_repo_file(&rel),
            PublicRepairTargetReasonScanMode::AllCode,
        ));
    }
    sources
}

#[test]
fn closure_receipt_diagnostics_stay_out_of_public_route_surfaces() {
    let closure_diagnostics = read_repo_file("src/execution/closure_diagnostics.rs");
    let closure_reason_codes = read_repo_file("src/execution/closure_diagnostics/reason_codes.rs");
    assert!(
        closure_diagnostics.contains("pub(crate) use reason_codes::*")
            && closure_reason_codes.contains("TASK_BOUNDARY_PROJECTION_DIAGNOSTIC_REASON_CODES")
            && closure_diagnostics.contains("task_boundary_projection_diagnostic_reason_code"),
        "closure diagnostics must centralize receipt/projection diagnostic classification"
    );
    let diagnostic_literals = rust_string_literal_values(
        "src/execution/closure_diagnostics/reason_codes.rs",
        &closure_reason_codes,
    );
    assert!(
        diagnostic_literals
            .iter()
            .any(|literal| literal == "prior_task_review_dispatch_missing")
            && diagnostic_literals
                .iter()
                .any(|literal| literal == "prior_task_review_dispatch_stale"),
        "stale/missing task-review dispatch lineage must remain diagnostic-only vocabulary"
    );
    let diagnostic_code_leafs = const_path_leafs(
        "src/execution/closure_diagnostics/reason_codes.rs",
        &closure_reason_codes,
        "TASK_BOUNDARY_PROJECTION_DIAGNOSTIC_REASON_CODES",
    );
    assert!(
        diagnostic_code_leafs
            .contains("TASK_BOUNDARY_DIAGNOSTIC_REASON_PRIOR_TASK_REVIEW_DISPATCH_MISSING")
            && diagnostic_code_leafs
                .contains("TASK_BOUNDARY_DIAGNOSTIC_REASON_PRIOR_TASK_REVIEW_DISPATCH_STALE"),
        "diagnostic reason-code sets must be built from named closure_diagnostics constants"
    );
    let public_code_leafs = const_path_leafs(
        "src/execution/closure_diagnostics/reason_codes.rs",
        &closure_reason_codes,
        "PUBLIC_TASK_BOUNDARY_REASON_CODES",
    );
    assert!(
        !public_code_leafs
            .contains("TASK_BOUNDARY_DIAGNOSTIC_REASON_PRIOR_TASK_REVIEW_DISPATCH_MISSING")
            && !public_code_leafs
                .contains("TASK_BOUNDARY_DIAGNOSTIC_REASON_PRIOR_TASK_REVIEW_DISPATCH_STALE"),
        "stale/missing task-review dispatch lineage must not become public blocking reason vocabulary"
    );
    let recording_blocker_literals =
        rust_string_literal_values("src/execution/closure_diagnostics.rs", &closure_diagnostics);
    assert!(
        !recording_blocker_literals
            .iter()
            .any(|literal| literal == "prior_task_review_dispatch_missing")
            && !recording_blocker_literals
                .iter()
                .any(|literal| literal == "prior_task_review_dispatch_stale"),
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

    let status_support = read_repo_file("src/execution/status_support.rs");
    assert!(
        status_support.contains("diagnostic_reason_codes")
            && status_support.contains("blocking_reason_codes"),
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
            !status_support.contains(forbidden),
            "status_support.rs must not regain receipt parsing/path construction owned by closure_diagnostics: found `{forbidden}`"
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
    let status_application = read_repo_file("src/execution/route_plan/status_application.rs");
    let status_application_paths = normalized_code_paths(
        "src/execution/route_plan/status_application.rs",
        &status_application,
    );
    assert!(
        public_route_projection.contains("status_projection")
            && !public_route_projection.contains("apply_route_status_projection_diagnostics(")
            && status_application_paths.iter().any(|path| {
                path == "crate::execution::closure_diagnostics::apply_task_boundary_projection_diagnostics"
            })
            && !public_route_projection
                .contains("public_task_boundary_decision(status).diagnostic_reason_codes"),
        "read-model public route projection must consume router-finalized diagnostics from route_plan/status_application.rs instead of replaying task-boundary diagnostic projection"
    );
    let route_facts = read_repo_file("src/execution/route_plan/route_facts.rs");
    let public_blocking_literals =
        rust_string_literal_values("src/execution/route_plan/route_facts.rs", &route_facts);
    assert!(
        function_calls_path(
            "src/execution/route_plan/route_facts.rs",
            &route_facts,
            "public_route_blocking_reason_codes",
            "crate::execution::closure_diagnostics::task_boundary_projection_diagnostic_reason_code",
        ) && !public_blocking_literals
            .iter()
            .any(|literal| literal == "prior_task_review_dispatch_missing")
            && !public_blocking_literals
                .iter()
                .any(|literal| literal == "prior_task_review_dispatch_stale"),
        "public route blocking reasons must filter diagnostic reason codes through closure_diagnostics instead of naming stale dispatch lineage as blockers"
    );
    let mutation_guards = read_repo_file("src/execution/commands/common/mutation_guards.rs");
    let begin_failure_literals = rust_string_literal_values(
        "src/execution/commands/common/mutation_guards.rs",
        &mutation_guards,
    );
    assert!(
        !begin_failure_literals
            .iter()
            .any(|literal| literal == "prior_task_review_dispatch_missing")
            && !begin_failure_literals
                .iter()
                .any(|literal| literal == "prior_task_review_dispatch_stale"),
        "public mutation guard failure classification must not consume stale/missing dispatch diagnostics as blockers"
    );
    let workflow_operator = read_repo_file("src/workflow/operator.rs");
    assert!(
        workflow_operator.contains("merge_status_projection_diagnostics")
            && !workflow_operator.contains("for reason_code in &status.projection_diagnostics"),
        "workflow operator must use closure_diagnostics to merge public projection diagnostics"
    );
}

#[test]
fn task_boundary_reason_code_vocabulary_has_single_owner() {
    let owner_rel = "src/execution/closure_diagnostics/reason_codes.rs";
    let owner_source = read_repo_file(owner_rel);
    let reason_literals = rust_string_literal_values(owner_rel, &owner_source)
        .into_iter()
        .filter(|literal| is_task_boundary_reason_code_literal(literal))
        .collect::<BTreeSet<_>>();
    assert!(
        reason_literals.len() >= 14,
        "closure_diagnostics.rs should expose the task-boundary reason-code vocabulary, got {reason_literals:?}"
    );

    let mut violations = Vec::new();
    for path in rust_source_files(&repo_root().join("src")) {
        let rel = repo_relative(&path);
        if rel == owner_rel {
            continue;
        }
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{rel} should be readable: {error}"));
        for literal in rust_string_literal_values(&rel, &source) {
            if reason_literals.contains(&literal) {
                violations.push(format!(
                    "{rel} duplicates task-boundary reason-code literal `{literal}` outside {owner_rel}"
                ));
            }
        }
    }
    violations.sort();
    violations.dedup();
    assert!(
        violations.is_empty(),
        "task-boundary reason-code literals must be sourced from closure_diagnostics.rs across active Rust surfaces:\n{}",
        violations.join("\n")
    );

    let current_truth = read_repo_file("src/execution/current_truth.rs");
    assert!(
        current_truth.contains("task_boundary_begin_block_reason_code")
            && current_truth.contains("task_boundary_verification_diagnostic_reason_code"),
        "current_truth.rs must classify task-boundary reason codes through closure_diagnostics predicates"
    );
    let status_support = read_repo_file("src/execution/status_support.rs");
    for predicate in [
        "task_boundary_blocks_closure_baseline_bridge_reason_code",
        "task_boundary_stale_truth_reason_code",
        "task_closure_recording_blocking_reason_code",
    ] {
        assert!(
            status_support.contains(predicate),
            "status_support.rs must consume shared task-boundary predicate `{predicate}`"
        );
    }
    let repair_target_selection = read_repo_file("src/execution/repair_target_selection.rs");
    assert!(
        repair_target_selection.contains("task_boundary_current_closure_boundary_reason_code")
            && repair_target_selection.contains("task_boundary_current_closure_repair_reason_code"),
        "repair_target_selection.rs must consume shared current-closure boundary predicates"
    );
    let repair_route_sources = repair_route_decision_sources();
    assert!(
        repair_route_sources
            .iter()
            .any(|(_, source)| source.contains("prior_task_closure_progress_edge_required("))
            && repair_route_sources
                .iter()
                .any(|(_, source)| source.contains("task_boundary_progress_edge_reason_code"))
            && repair_route_sources.iter().any(|(_, source)| {
                source.contains("task_boundary_current_closure_structural_reason_code")
            }),
        "repair_route_decision modules must consume shared task-boundary progress/structural predicates"
    );
    let follow_up = read_repo_file("src/execution/follow_up.rs");
    assert!(
        follow_up.contains("task_boundary_overlay_restore_reason_code")
            && follow_up.contains("task_boundary_verification_diagnostic_reason_code"),
        "follow_up.rs must consume shared task-boundary diagnostic predicates"
    );
    let workflow_operator = read_repo_file("src/workflow/operator.rs");
    assert!(
        workflow_operator.contains("TASK_BOUNDARY_DIAGNOSTIC_REASON_TASK_REVIEW_NOT_INDEPENDENT")
            && workflow_operator
                .contains("TASK_BOUNDARY_DIAGNOSTIC_REASON_TASK_REVIEW_ARTIFACT_MALFORMED")
            && workflow_operator.contains("TASK_BOUNDARY_REASON_PRIOR_TASK_REVIEW_NOT_GREEN")
            && workflow_operator
                .contains("task_boundary_closure_baseline_bridge_ready_reason_code"),
        "workflow operator public text must classify task-boundary reason codes through closure_diagnostics constants"
    );
    let doctor_dashboard = read_repo_file("src/workflow/doctor_dashboard.rs");
    assert!(
        doctor_dashboard.contains("TASK_BOUNDARY_REASON_PRIOR_TASK_CURRENT_CLOSURE_MISSING")
            && doctor_dashboard.contains("TASK_BOUNDARY_REASON_PRIOR_TASK_REVIEW_NOT_GREEN")
            && doctor_dashboard
                .contains("TASK_BOUNDARY_REASON_TASK_CLOSURE_BASELINE_REPAIR_CANDIDATE"),
        "workflow doctor dashboard public text must classify task-boundary reason codes through closure_diagnostics constants"
    );
}

fn is_task_boundary_reason_code_literal(literal: &str) -> bool {
    literal.starts_with("prior_task_")
        || literal.starts_with("task_review_")
        || literal.starts_with("task_verification_")
        || literal.starts_with("current_task_closure_")
        || literal == "current_branch_closure_reviewed_state_malformed"
        || matches!(
            literal,
            "task_cycle_break_active"
                | "task_closure_baseline_repair_candidate"
                | "task_closure_baseline_bridge_ready"
        )
}

#[test]
fn gate_reason_code_vocabulary_has_single_owner() {
    let owner_rel = "src/execution/gate_reason_codes.rs";
    let owner_source = read_repo_file(owner_rel);
    let reason_literals = [
        "finish_review_gate_already_current",
        "files_proven_drifted",
        "qa_requirement_missing_or_invalid",
    ];
    for reason_literal in reason_literals {
        assert!(
            owner_source.contains(&format!("\"{reason_literal}\"")),
            "{owner_rel} should define gate reason-code literal `{reason_literal}`"
        );
    }
    for required in [
        "finish_review_gate_already_current_reason_code",
        "files_proven_drifted_reason_code",
        "qa_requirement_missing_or_invalid_reason_code",
        "push_files_proven_drifted_reason_code_once",
    ] {
        assert!(
            owner_source.contains(required),
            "{owner_rel} should define gate reason-code helper `{required}`"
        );
    }

    let mut violations = Vec::new();
    for path in rust_source_files(&repo_root().join("src")) {
        let rel = repo_relative(&path);
        if rel == owner_rel {
            continue;
        }
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{rel} should be readable: {error}"));
        for literal in rust_production_string_literal_values(&rel, &source) {
            if reason_literals.contains(&literal.as_str()) {
                violations.push(format!(
                    "{rel} duplicates gate reason-code literal `{literal}` outside {owner_rel}"
                ));
            }
        }
    }
    violations.sort();
    violations.dedup();
    assert!(
        violations.is_empty(),
        "gate reason-code literals must be sourced from gate_reason_codes.rs across active Rust production surfaces:\n{}",
        violations.join("\n")
    );

    let runtime_methods = read_repo_file("src/execution/state/runtime_methods.rs");
    let follow_up = read_repo_file("src/execution/follow_up.rs");
    assert!(
        runtime_methods.contains("finish_review_gate_already_current_reason_code")
            && runtime_methods.contains("FINISH_REVIEW_GATE_ALREADY_CURRENT")
            && follow_up.contains("finish_review_gate_already_current_reason_code"),
        "finish-gate producer, requery, and follow-up logic must consume the shared gate reason-code owner"
    );
    let closure_graph = read_repo_file("src/execution/closure_graph.rs");
    let stale_target_projection = read_repo_file("src/execution/stale_target_projection.rs");
    let rebuild_evidence = read_repo_file("src/execution/state/rebuild_evidence.rs");
    assert!(
        closure_graph.contains("files_proven_drifted_reason_code")
            && stale_target_projection.contains("push_files_proven_drifted_reason_code_once")
            && rebuild_evidence.contains("FILES_PROVEN_DRIFTED"),
        "stale/provenance producers and classifiers must consume the shared files-proven-drifted reason-code owner"
    );
    for rel in [
        "src/execution/current_truth.rs",
        "src/execution/state/finish_gate.rs",
        "src/execution/route_plan/next_action_choice/execution_routes.rs",
    ] {
        let source = read_repo_file(rel);
        assert!(
            source.contains("qa_requirement_missing_or_invalid_reason_code"),
            "{rel} must classify QA requirement invalidity through gate_reason_codes.rs"
        );
    }
    for rel in [
        "src/execution/state/review_gate.rs",
        "src/execution/status_assembly/late_stage.rs",
        "src/workflow/pivot.rs",
    ] {
        let source = read_repo_file(rel);
        assert!(
            source.contains("QA_REQUIREMENT_MISSING_OR_INVALID"),
            "{rel} must emit QA requirement invalidity through gate_reason_codes.rs"
        );
    }
}

#[test]
fn public_next_action_label_vocabulary_has_single_route_owner() {
    let owner_rel = "src/execution/route_plan/next_action_choice/types.rs";
    let owner_source = read_repo_file(owner_rel);
    let label_constants = next_action_label_constants_from_owner(owner_rel, &owner_source);
    let expected_constant_names = [
        "NEXT_ACTION_RUNTIME_DIAGNOSTIC_REQUIRED",
        "NEXT_ACTION_REPAIR_REVIEW_STATE",
        "NEXT_ACTION_CONTINUE_EXECUTION",
        "NEXT_ACTION_EXECUTION_REENTRY_REQUIRED",
        "NEXT_ACTION_CLOSE_CURRENT_TASK",
        "NEXT_ACTION_RUN_VERIFICATION",
        "NEXT_ACTION_WAIT_FOR_EXTERNAL_REVIEW_RESULT",
        "NEXT_ACTION_RESOLVE_RELEASE_BLOCKER",
        "NEXT_ACTION_ADVANCE_LATE_STAGE",
        "NEXT_ACTION_REQUEST_FINAL_REVIEW",
        "NEXT_ACTION_REFRESH_TEST_PLAN",
        "NEXT_ACTION_RUN_QA",
        "NEXT_ACTION_FINISH_BRANCH",
        "NEXT_ACTION_PLANNING_REENTRY",
        "NEXT_ACTION_HANDOFF",
    ]
    .into_iter()
    .map(String::from)
    .collect::<BTreeSet<_>>();
    let actual_constant_names = label_constants.keys().cloned().collect::<BTreeSet<_>>();
    assert_eq!(
        actual_constant_names, expected_constant_names,
        "route_plan/next_action_choice/types.rs must own exactly the reviewed public next-action label constants"
    );
    let label_literals = label_constants.values().cloned().collect::<BTreeSet<_>>();
    assert_eq!(
        label_literals.len(),
        label_constants.len(),
        "public next-action labels should have one constant per distinct label: {label_constants:?}"
    );

    let strict_route_modules = BTreeSet::from([String::from("src/execution/router.rs")]);
    let mut violations = Vec::new();
    for (rel, source) in production_next_action_label_scan_sources() {
        if strict_route_modules.contains(&rel) {
            violations.extend(next_action_raw_literal_assignment_violations(&rel, &source));
        } else {
            violations.extend(next_action_raw_public_label_assignment_violations(
                &rel,
                &source,
                &label_literals,
            ));
        }
    }
    violations.sort();
    violations.dedup();
    assert!(
        violations.is_empty(),
        "public route modules must source next_action values from next_action.rs constants, not raw string literals:\n{}",
        violations.join("\n")
    );

    let route_plan_next_action_finalization =
        read_repo_file("src/execution/route_plan/next_action_finalization.rs");
    assert!(
        route_plan_next_action_finalization.contains("NEXT_ACTION_CLOSE_CURRENT_TASK")
            && route_plan_next_action_finalization.contains("NEXT_ACTION_ADVANCE_LATE_STAGE"),
        "route_plan/next_action_finalization.rs must use shared next-action label constants for route override bindings"
    );
    let router = read_repo_file("src/execution/router.rs");
    for required in ["NEXT_ACTION_HANDOFF", "NEXT_ACTION_PLANNING_REENTRY"] {
        assert!(
            router.contains(required),
            "router.rs must use shared next-action label constant `{required}`"
        );
    }
    let route_plan_constructors = read_repo_file("src/execution/route_plan/constructors.rs");
    for required in [
        "NEXT_ACTION_CLOSE_CURRENT_TASK",
        "NEXT_ACTION_ADVANCE_LATE_STAGE",
        "NEXT_ACTION_REPAIR_REVIEW_STATE",
    ] {
        assert!(
            route_plan_constructors.contains(required),
            "route_plan/constructors.rs must use shared next-action label constant `{required}`"
        );
    }
    let route_plan_decision_support =
        read_repo_file("src/execution/route_plan/decision_support.rs");
    assert!(
        route_plan_decision_support.contains("NEXT_ACTION_RUNTIME_DIAGNOSTIC_REQUIRED"),
        "route_plan/decision_support.rs must use the shared next-action label constant `NEXT_ACTION_RUNTIME_DIAGNOSTIC_REQUIRED`"
    );
    let route_plan_final_review_dispatch =
        read_repo_file("src/execution/route_plan/final_review_dispatch.rs");
    assert!(
        route_plan_final_review_dispatch.contains("NEXT_ACTION_REQUEST_FINAL_REVIEW"),
        "route_plan final-review override must use the shared next-action label constant `NEXT_ACTION_REQUEST_FINAL_REVIEW`"
    );
}

fn production_next_action_label_scan_sources() -> Vec<(String, String)> {
    rust_source_files(&repo_root().join("src/execution"))
        .into_iter()
        .map(|path| repo_relative(&path))
        .filter(|rel| rel != "src/execution/next_action.rs")
        .filter(|rel| !rel.ends_with("_tests.rs"))
        .filter(|rel| !rel.ends_with("unit_tests.rs"))
        .map(|rel| {
            let source = read_repo_file(&rel);
            (rel, source)
        })
        .collect()
}

fn next_action_label_constants_from_owner(rel: &str, source: &str) -> BTreeMap<String, String> {
    let syntax = parse_rust_source(rel, source);
    syntax
        .items
        .into_iter()
        .filter_map(|item| {
            let syn::Item::Const(item_const) = item else {
                return None;
            };
            let name = item_const.ident.to_string();
            name.starts_with("NEXT_ACTION_").then(|| {
                let value = string_literal_expr_value(&item_const.expr).unwrap_or_else(|| {
                    panic!("{rel} constant `{name}` must be a direct string literal")
                });
                (name, value)
            })
        })
        .collect()
}

fn string_literal_expr_value(expr: &syn::Expr) -> Option<String> {
    match expr {
        syn::Expr::Lit(literal) => {
            if let syn::Lit::Str(literal) = &literal.lit {
                Some(literal.value())
            } else {
                None
            }
        }
        syn::Expr::Group(group) => string_literal_expr_value(&group.expr),
        syn::Expr::Paren(paren) => string_literal_expr_value(&paren.expr),
        syn::Expr::Reference(reference) => string_literal_expr_value(&reference.expr),
        _ => None,
    }
}

fn next_action_raw_literal_assignment_violations(rel: &str, source: &str) -> Vec<String> {
    next_action_raw_literal_assignment_violations_with_filter(rel, source, None)
}

fn next_action_raw_public_label_assignment_violations(
    rel: &str,
    source: &str,
    public_labels: &BTreeSet<String>,
) -> Vec<String> {
    next_action_raw_literal_assignment_violations_with_filter(rel, source, Some(public_labels))
}

fn next_action_raw_literal_assignment_violations_with_filter(
    rel: &str,
    source: &str,
    public_label_filter: Option<&BTreeSet<String>>,
) -> Vec<String> {
    let syntax = parse_rust_source(rel, source);
    let mut visitor = NextActionRawLiteralAssignmentVisitor {
        rel,
        violations: Vec::new(),
        raw_literal_aliases: BTreeMap::new(),
        public_label_filter,
    };
    visitor.visit_file(&syntax);
    visitor.violations
}

struct NextActionRawLiteralAssignmentVisitor<'a> {
    rel: &'a str,
    violations: Vec<String>,
    raw_literal_aliases: BTreeMap<String, Vec<String>>,
    public_label_filter: Option<&'a BTreeSet<String>>,
}

impl<'ast> Visit<'ast> for NextActionRawLiteralAssignmentVisitor<'_> {
    fn visit_field_value(&mut self, field: &'ast syn::FieldValue) {
        if member_name(&field.member).as_deref() == Some("next_action") {
            self.reject_raw_next_action_expr("next_action struct field", &field.expr);
        }
        visit::visit_field_value(self, field);
    }

    fn visit_expr_assign(&mut self, assignment: &'ast syn::ExprAssign) {
        if expr_targets_next_action(&assignment.left) {
            self.reject_raw_next_action_expr("next_action assignment", &assignment.right);
        } else if let Some(alias) = expr_path_ident(&assignment.left) {
            self.record_raw_literal_alias(&alias, &assignment.right);
        }
        visit::visit_expr_assign(self, assignment);
    }

    fn visit_expr_binary(&mut self, binary: &'ast syn::ExprBinary) {
        if matches!(binary.op, syn::BinOp::Eq(_) | syn::BinOp::Ne(_)) {
            if expr_targets_next_action(&binary.left) {
                self.reject_raw_next_action_expr("next_action comparison", &binary.right);
            }
            if expr_targets_next_action(&binary.right) {
                self.reject_raw_next_action_expr("next_action comparison", &binary.left);
            }
        }
        visit::visit_expr_binary(self, binary);
    }

    fn visit_local(&mut self, local: &'ast syn::Local) {
        if let Some(init) = &local.init {
            self.record_local_aliases_from_binding(&local.pat, &init.expr);
            if pat_is_next_action_ident(&local.pat) {
                self.reject_raw_next_action_expr("next_action local binding", &init.expr);
            } else {
                for expr in next_action_tuple_binding_exprs(&local.pat, &init.expr) {
                    self.reject_raw_next_action_expr("next_action tuple binding", expr);
                }
            }
        }
        visit::visit_local(self, local);
    }

    fn visit_item_fn(&mut self, item_fn: &'ast syn::ItemFn) {
        if item_attrs_mark_test_only(&item_fn.attrs) {
            return;
        }
        visit::visit_item_fn(self, item_fn);
    }

    fn visit_item_impl(&mut self, item_impl: &'ast syn::ItemImpl) {
        if item_attrs_mark_test_only(&item_impl.attrs) {
            return;
        }
        visit::visit_item_impl(self, item_impl);
    }

    fn visit_item_mod(&mut self, item_mod: &'ast syn::ItemMod) {
        if item_attrs_mark_test_only(&item_mod.attrs) {
            return;
        }
        visit::visit_item_mod(self, item_mod);
    }
}

impl NextActionRawLiteralAssignmentVisitor<'_> {
    fn reject_raw_next_action_expr(&mut self, context: &str, expr: &syn::Expr) {
        for literal in self.raw_next_action_literals_in_expr(expr) {
            if self
                .public_label_filter
                .is_some_and(|labels| !labels.contains(&literal))
            {
                continue;
            }
            self.violations.push(format!(
                "{} uses raw string literal `{literal}` in {context}",
                self.rel
            ));
        }
    }

    fn record_local_aliases_from_binding(&mut self, pat: &syn::Pat, expr: &syn::Expr) {
        if let Some(alias) = pat_ident_name(pat) {
            self.record_raw_literal_alias(&alias, expr);
            return;
        }

        for (alias, position) in tuple_binding_alias_positions(pat) {
            self.record_raw_literal_alias_from_exprs(
                &alias,
                exprs_for_tuple_positions(expr, &[position]),
            );
        }
    }

    fn record_raw_literal_alias(&mut self, alias: &str, expr: &syn::Expr) {
        self.record_raw_literal_alias_from_exprs(alias, [expr]);
    }

    fn record_raw_literal_alias_from_exprs<'a>(
        &mut self,
        alias: &str,
        exprs: impl IntoIterator<Item = &'a syn::Expr>,
    ) {
        let mut literals = exprs
            .into_iter()
            .filter(|expr| string_value_expr_candidate(expr, &self.raw_literal_aliases))
            .flat_map(|expr| self.raw_next_action_literals_in_expr(expr))
            .collect::<Vec<_>>();
        literals.sort();
        literals.dedup();
        if literals.is_empty() {
            self.raw_literal_aliases.remove(alias);
        } else {
            self.raw_literal_aliases.insert(alias.to_owned(), literals);
        }
    }

    fn raw_next_action_literals_in_expr(&self, expr: &syn::Expr) -> Vec<String> {
        let mut literals = string_literals_in_expr(expr);
        literals.extend(alias_literals_in_expr(expr, &self.raw_literal_aliases));
        literals.sort();
        literals.dedup();
        literals
    }
}

fn item_attrs_mark_test_only(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("test")
            || (attr.path().is_ident("cfg")
                && matches!(&attr.meta, syn::Meta::List(list) if list.tokens.to_string().contains("test")))
    })
}

fn member_name(member: &syn::Member) -> Option<String> {
    match member {
        syn::Member::Named(ident) => Some(ident.to_string()),
        syn::Member::Unnamed(_) => None,
    }
}

fn expr_targets_next_action(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::Path(path) => path
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "next_action"),
        syn::Expr::Field(field) => member_name(&field.member).as_deref() == Some("next_action"),
        syn::Expr::Group(group) => expr_targets_next_action(&group.expr),
        syn::Expr::Paren(paren) => expr_targets_next_action(&paren.expr),
        _ => false,
    }
}

fn expr_is_ident_path(expr: &syn::Expr, ident: &str) -> bool {
    expr_path_ident(expr).as_deref() == Some(ident)
}

fn field_on_base_ident(expr: &syn::Expr, base_ident: &str) -> Option<String> {
    match expr {
        syn::Expr::Field(field) if expr_is_ident_path(&field.base, base_ident) => {
            member_name(&field.member)
        }
        syn::Expr::Field(field) => field_on_base_ident(&field.base, base_ident),
        syn::Expr::Reference(reference) => field_on_base_ident(&reference.expr, base_ident),
        syn::Expr::Group(group) => field_on_base_ident(&group.expr, base_ident),
        syn::Expr::Paren(paren) => field_on_base_ident(&paren.expr, base_ident),
        _ => None,
    }
}

fn status_field_name(expr: &syn::Expr) -> Option<String> {
    match expr {
        syn::Expr::Field(field) if expr_is_ident_path(&field.base, "status") => {
            member_name(&field.member)
        }
        syn::Expr::Reference(reference) => status_field_name(&reference.expr),
        syn::Expr::Group(group) => status_field_name(&group.expr),
        syn::Expr::Paren(paren) => status_field_name(&paren.expr),
        _ => None,
    }
}

fn public_route_projection_anchor_fields() -> BTreeSet<String> {
    [
        "phase_detail",
        "next_action",
        "recommended_public_command_argv",
        "recommended_public_command_template",
        "required_inputs",
        "recommended_command",
        "next_public_action",
        "blockers",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

fn status_field_mutations_by_function(
    rel: &str,
    source: &str,
) -> BTreeMap<String, BTreeSet<String>> {
    let syntax = parse_rust_source(rel, source);
    let mut visitor = StatusFieldMutationByFunctionVisitor {
        function_mutations: BTreeMap::new(),
    };
    visitor.visit_file(&syntax);
    visitor.function_mutations
}

fn status_field_mutations_in_function(
    rel: &str,
    source: &str,
    function_name: &str,
) -> BTreeSet<String> {
    status_field_mutations_by_function(rel, source)
        .remove(function_name)
        .unwrap_or_else(|| panic!("{rel} should define function `{function_name}`"))
}

fn status_field_mutations_in_module(rel: &str, source: &str) -> BTreeSet<String> {
    status_field_mutations_by_function(rel, source)
        .into_values()
        .flatten()
        .collect()
}

fn field_mutations_on_base_ident_in_module(
    rel: &str,
    source: &str,
    base_ident: &str,
) -> BTreeSet<String> {
    let syntax = parse_rust_source(rel, source);
    let mut visitor = FieldMutationOnBaseIdentVisitor {
        base_ident,
        mutations: BTreeSet::new(),
    };
    visitor.visit_file(&syntax);
    visitor.mutations
}

struct FieldMutationOnBaseIdentVisitor<'a> {
    base_ident: &'a str,
    mutations: BTreeSet<String>,
}

impl<'ast> Visit<'ast> for FieldMutationOnBaseIdentVisitor<'_> {
    fn visit_expr_assign(&mut self, assignment: &'ast syn::ExprAssign) {
        if let Some(field) = field_on_base_ident(&assignment.left, self.base_ident) {
            self.mutations.insert(field);
        }
        visit::visit_expr_assign(self, assignment);
    }

    fn visit_expr_method_call(&mut self, method_call: &'ast syn::ExprMethodCall) {
        if matches!(
            method_call.method.to_string().as_str(),
            "clear" | "clone_from" | "extend" | "push"
        ) && let Some(field) = field_on_base_ident(&method_call.receiver, self.base_ident)
        {
            self.mutations.insert(field);
        }
        visit::visit_expr_method_call(self, method_call);
    }
}

struct StatusFieldMutationByFunctionVisitor {
    function_mutations: BTreeMap<String, BTreeSet<String>>,
}

impl<'ast> Visit<'ast> for StatusFieldMutationByFunctionVisitor {
    fn visit_item_fn(&mut self, item_fn: &'ast syn::ItemFn) {
        let mut visitor = StatusFieldMutationVisitor {
            mutations: BTreeSet::new(),
        };
        visitor.visit_block(&item_fn.block);
        self.function_mutations
            .insert(item_fn.sig.ident.to_string(), visitor.mutations);
    }
}

struct StatusFieldMutationVisitor {
    mutations: BTreeSet<String>,
}

impl<'ast> Visit<'ast> for StatusFieldMutationVisitor {
    fn visit_expr_assign(&mut self, assignment: &'ast syn::ExprAssign) {
        if let Some(field) = status_field_name(&assignment.left) {
            self.mutations.insert(field);
        }
        visit::visit_expr_assign(self, assignment);
    }

    fn visit_expr_method_call(&mut self, method_call: &'ast syn::ExprMethodCall) {
        if matches!(
            method_call.method.to_string().as_str(),
            "clear" | "clone_from" | "extend" | "push"
        ) && let Some(field) = status_field_name(&method_call.receiver)
        {
            self.mutations.insert(field);
        }
        visit::visit_expr_method_call(self, method_call);
    }
}

fn field_reads_on_ident_in_function(
    rel: &str,
    source: &str,
    function_name: &str,
    base_ident: &str,
) -> BTreeMap<String, usize> {
    let syntax = parse_rust_source(rel, source);
    let mut visitor = FieldReadByFunctionVisitor {
        function_name,
        base_ident,
        reads: None,
    };
    visitor.visit_file(&syntax);
    visitor
        .reads
        .unwrap_or_else(|| panic!("{rel} should define function `{function_name}`"))
}

fn field_reads_on_base_ident_in_module(
    rel: &str,
    source: &str,
    base_ident: &str,
) -> BTreeMap<String, usize> {
    let syntax = parse_rust_source(rel, source);
    let mut visitor = FieldReadVisitor {
        base_ident,
        reads: BTreeMap::new(),
    };
    visitor.visit_file(&syntax);
    visitor.reads
}

fn branch_route_status_field_recomputation_violations(
    rel: &str,
    source: &str,
    owner_function: &str,
) -> Vec<String> {
    source_production_function_visibilities(rel, source)
        .into_keys()
        .flat_map(|function_name| {
            let reads = field_reads_on_ident_in_function(rel, source, &function_name, "status");
            ["current_task_closures", "current_branch_closure_id"]
                .into_iter()
                .filter(move |field| reads.contains_key(*field))
                .map(move |field| format!("{function_name} reads status.{field}"))
        })
        .filter(|violation| {
            if rel != "src/execution/status_support.rs" {
                return true;
            }
            !violation.starts_with(&format!("{owner_function} reads "))
        })
        .collect()
}

struct FieldReadByFunctionVisitor<'a> {
    function_name: &'a str,
    base_ident: &'a str,
    reads: Option<BTreeMap<String, usize>>,
}

impl<'ast> Visit<'ast> for FieldReadByFunctionVisitor<'_> {
    fn visit_item_fn(&mut self, item_fn: &'ast syn::ItemFn) {
        if item_fn.sig.ident == self.function_name {
            let mut visitor = FieldReadVisitor {
                base_ident: self.base_ident,
                reads: BTreeMap::new(),
            };
            visitor.visit_block(&item_fn.block);
            self.reads = Some(visitor.reads);
            return;
        }
        visit::visit_item_fn(self, item_fn);
    }
}

struct FieldReadVisitor<'a> {
    base_ident: &'a str,
    reads: BTreeMap<String, usize>,
}

impl<'ast> Visit<'ast> for FieldReadVisitor<'_> {
    fn visit_expr_field(&mut self, field: &'ast syn::ExprField) {
        let expr = syn::Expr::Field(field.clone());
        if let Some(field_name) = field_on_base_ident(&expr, self.base_ident) {
            *self.reads.entry(field_name).or_default() += 1;
        }
        visit::visit_expr_field(self, field);
    }
}

fn expr_path_ident(expr: &syn::Expr) -> Option<String> {
    match expr {
        syn::Expr::Path(path) if path.qself.is_none() && path.path.segments.len() == 1 => path
            .path
            .segments
            .first()
            .map(|segment| segment.ident.to_string()),
        syn::Expr::Group(group) => expr_path_ident(&group.expr),
        syn::Expr::Paren(paren) => expr_path_ident(&paren.expr),
        _ => None,
    }
}

fn pat_ident_name(pat: &syn::Pat) -> Option<String> {
    match pat {
        syn::Pat::Ident(ident) => Some(ident.ident.to_string()),
        syn::Pat::Type(typed) => pat_ident_name(&typed.pat),
        syn::Pat::Reference(reference) => pat_ident_name(&reference.pat),
        syn::Pat::Paren(paren) => pat_ident_name(&paren.pat),
        _ => None,
    }
}

fn pat_is_next_action_ident(pat: &syn::Pat) -> bool {
    pat_ident_name(pat).as_deref() == Some("next_action")
}

fn next_action_tuple_binding_exprs<'a>(pat: &syn::Pat, expr: &'a syn::Expr) -> Vec<&'a syn::Expr> {
    let positions = next_action_tuple_binding_positions(pat);
    if positions.is_empty() {
        return Vec::new();
    }
    exprs_for_tuple_positions(expr, &positions)
}

fn tuple_binding_alias_positions(pat: &syn::Pat) -> Vec<(String, usize)> {
    match pat {
        syn::Pat::Tuple(tuple) => tuple
            .elems
            .iter()
            .enumerate()
            .filter_map(|(index, element)| pat_ident_name(element).map(|name| (name, index)))
            .collect(),
        syn::Pat::Type(typed) => tuple_binding_alias_positions(&typed.pat),
        syn::Pat::Reference(reference) => tuple_binding_alias_positions(&reference.pat),
        syn::Pat::Paren(paren) => tuple_binding_alias_positions(&paren.pat),
        _ => Vec::new(),
    }
}

fn next_action_tuple_binding_positions(pat: &syn::Pat) -> Vec<usize> {
    match pat {
        syn::Pat::Tuple(tuple) => tuple
            .elems
            .iter()
            .enumerate()
            .filter_map(|(index, element)| pat_is_next_action_ident(element).then_some(index))
            .collect(),
        syn::Pat::Type(typed) => next_action_tuple_binding_positions(&typed.pat),
        syn::Pat::Reference(reference) => next_action_tuple_binding_positions(&reference.pat),
        syn::Pat::Paren(paren) => next_action_tuple_binding_positions(&paren.pat),
        _ => Vec::new(),
    }
}

fn exprs_for_tuple_positions<'a>(expr: &'a syn::Expr, positions: &[usize]) -> Vec<&'a syn::Expr> {
    match expr {
        syn::Expr::Tuple(tuple) => positions
            .iter()
            .filter_map(|position| tuple.elems.iter().nth(*position))
            .collect(),
        syn::Expr::Match(match_expr) => match_expr
            .arms
            .iter()
            .flat_map(|arm| exprs_for_tuple_positions(&arm.body, positions))
            .collect(),
        syn::Expr::If(if_expr) => {
            let mut expressions =
                exprs_for_tuple_positions_in_block(&if_expr.then_branch, positions);
            if let Some((_, else_branch)) = &if_expr.else_branch {
                expressions.extend(exprs_for_tuple_positions(else_branch, positions));
            }
            expressions
        }
        syn::Expr::Block(block) => exprs_for_tuple_positions_in_block(&block.block, positions),
        syn::Expr::Group(group) => exprs_for_tuple_positions(&group.expr, positions),
        syn::Expr::Paren(paren) => exprs_for_tuple_positions(&paren.expr, positions),
        _ => Vec::new(),
    }
}

fn exprs_for_tuple_positions_in_block<'a>(
    block: &'a syn::Block,
    positions: &[usize],
) -> Vec<&'a syn::Expr> {
    block
        .stmts
        .last()
        .into_iter()
        .flat_map(|statement| match statement {
            syn::Stmt::Expr(expr, None) => exprs_for_tuple_positions(expr, positions),
            _ => Vec::new(),
        })
        .collect()
}

fn string_literals_in_expr(expr: &syn::Expr) -> Vec<String> {
    let mut collector = ExprStringLiteralCollector { values: Vec::new() };
    collector.visit_expr(expr);
    collector.values
}

struct ExprStringLiteralCollector {
    values: Vec<String>,
}

impl<'ast> Visit<'ast> for ExprStringLiteralCollector {
    fn visit_lit_str(&mut self, literal: &'ast syn::LitStr) {
        self.values.push(literal.value());
    }
}

fn alias_literals_in_expr(
    expr: &syn::Expr,
    raw_literal_aliases: &BTreeMap<String, Vec<String>>,
) -> Vec<String> {
    match expr {
        syn::Expr::Path(_) => expr_path_ident(expr)
            .and_then(|alias| raw_literal_aliases.get(&alias).cloned())
            .unwrap_or_default(),
        syn::Expr::MethodCall(method_call) => {
            let mut literals = alias_literals_in_expr(&method_call.receiver, raw_literal_aliases);
            literals.extend(
                method_call
                    .args
                    .iter()
                    .flat_map(|arg| alias_literals_in_expr(arg, raw_literal_aliases)),
            );
            literals
        }
        syn::Expr::Call(call) => call
            .args
            .iter()
            .flat_map(|arg| alias_literals_in_expr(arg, raw_literal_aliases))
            .collect(),
        syn::Expr::Tuple(tuple) => tuple
            .elems
            .iter()
            .flat_map(|element| alias_literals_in_expr(element, raw_literal_aliases))
            .collect(),
        syn::Expr::Match(match_expr) => match_expr
            .arms
            .iter()
            .flat_map(|arm| alias_literals_in_expr(&arm.body, raw_literal_aliases))
            .collect(),
        syn::Expr::If(if_expr) => {
            let mut literals = alias_literals_in_block(&if_expr.then_branch, raw_literal_aliases);
            if let Some((_, else_branch)) = &if_expr.else_branch {
                literals.extend(alias_literals_in_expr(else_branch, raw_literal_aliases));
            }
            literals
        }
        syn::Expr::Block(block) => alias_literals_in_block(&block.block, raw_literal_aliases),
        syn::Expr::Group(group) => alias_literals_in_expr(&group.expr, raw_literal_aliases),
        syn::Expr::Paren(paren) => alias_literals_in_expr(&paren.expr, raw_literal_aliases),
        syn::Expr::Reference(reference) => {
            alias_literals_in_expr(&reference.expr, raw_literal_aliases)
        }
        syn::Expr::Try(try_expr) => alias_literals_in_expr(&try_expr.expr, raw_literal_aliases),
        _ => Vec::new(),
    }
}

fn alias_literals_in_block(
    block: &syn::Block,
    raw_literal_aliases: &BTreeMap<String, Vec<String>>,
) -> Vec<String> {
    block
        .stmts
        .last()
        .into_iter()
        .flat_map(|statement| match statement {
            syn::Stmt::Expr(expr, None) => alias_literals_in_expr(expr, raw_literal_aliases),
            _ => Vec::new(),
        })
        .collect()
}

fn string_value_expr_candidate(
    expr: &syn::Expr,
    raw_literal_aliases: &BTreeMap<String, Vec<String>>,
) -> bool {
    match expr {
        syn::Expr::Lit(literal) => matches!(literal.lit, syn::Lit::Str(_)),
        syn::Expr::Path(_) => {
            expr_path_ident(expr).is_some_and(|alias| raw_literal_aliases.contains_key(&alias))
        }
        syn::Expr::Call(call) => {
            call.path_last_segment_ident()
                .is_some_and(|ident| ident == "from")
                || call
                    .args
                    .iter()
                    .any(|arg| string_value_expr_candidate(arg, raw_literal_aliases))
        }
        syn::Expr::MethodCall(method_call) => {
            matches!(
                method_call.method.to_string().as_str(),
                "clone" | "to_owned" | "to_string"
            ) && string_value_expr_candidate(&method_call.receiver, raw_literal_aliases)
        }
        syn::Expr::Match(match_expr) => match_expr
            .arms
            .iter()
            .any(|arm| string_value_expr_candidate(&arm.body, raw_literal_aliases)),
        syn::Expr::If(if_expr) => {
            string_value_block_candidate(&if_expr.then_branch, raw_literal_aliases)
                || if_expr
                    .else_branch
                    .as_ref()
                    .is_some_and(|(_, else_branch)| {
                        string_value_expr_candidate(else_branch, raw_literal_aliases)
                    })
        }
        syn::Expr::Block(block) => string_value_block_candidate(&block.block, raw_literal_aliases),
        syn::Expr::Group(group) => string_value_expr_candidate(&group.expr, raw_literal_aliases),
        syn::Expr::Paren(paren) => string_value_expr_candidate(&paren.expr, raw_literal_aliases),
        syn::Expr::Reference(reference) => {
            string_value_expr_candidate(&reference.expr, raw_literal_aliases)
        }
        syn::Expr::Try(try_expr) => {
            string_value_expr_candidate(&try_expr.expr, raw_literal_aliases)
        }
        _ => false,
    }
}

trait ExprCallExt {
    fn path_last_segment_ident(&self) -> Option<&syn::Ident>;
}

impl ExprCallExt for syn::ExprCall {
    fn path_last_segment_ident(&self) -> Option<&syn::Ident> {
        match self.func.as_ref() {
            syn::Expr::Path(path) => path.path.segments.last().map(|segment| &segment.ident),
            _ => None,
        }
    }
}

fn string_value_block_candidate(
    block: &syn::Block,
    raw_literal_aliases: &BTreeMap<String, Vec<String>>,
) -> bool {
    block.stmts.last().is_some_and(|statement| match statement {
        syn::Stmt::Expr(expr, None) => string_value_expr_candidate(expr, raw_literal_aliases),
        _ => false,
    })
}

#[test]
fn reduced_runtime_facades_do_not_own_route_or_command_decisioning() {
    let state_rel = "src/execution/state.rs";
    let state = read_repo_file(state_rel);
    let state_modules = source_module_names(state_rel, &state);
    let state_visibilities = source_production_function_visibilities(state_rel, &state);
    assert!(
        !state_modules.is_empty()
            && state.contains("pub use crate::execution::runtime::{ExecutionRuntime, state_dir};")
            && state.contains("pub use crate::execution::status::{")
            && state_visibilities
                .values()
                .all(|visibility| visibility != "pub"),
        "{state_rel} must remain a compatibility facade over execution-state support modules and DTO/runtime re-exports"
    );
    assert_no_import_path_prefix(
        state_rel,
        &state,
        &[
            "crate::execution::router",
            "crate::execution::route_plan",
            "crate::execution::next_action",
            "crate::workflow",
        ],
        "must not own route ordering, public next-action projection, or workflow presentation",
    );

    let mutate_rel = "src/execution/mutate.rs";
    let mutate = read_repo_file(mutate_rel);
    let mutate_functions = source_function_names(mutate_rel, &mutate);
    assert!(
        mutate_functions.is_empty(),
        "{mutate_rel} must remain a pure public mutation-command re-export facade, not regain helper bodies: {mutate_functions:?}"
    );
    let mutate_paths = normalized_dependency_paths(mutate_rel, &mutate);
    let non_command_mutate_paths = mutate_paths
        .iter()
        .filter(|path| !path.starts_with("crate::execution::commands::"))
        .collect::<Vec<_>>();
    assert!(
        non_command_mutate_paths.is_empty(),
        "{mutate_rel} must delegate public mutations to command modules only: {non_command_mutate_paths:?}"
    );

    for rel in [state_rel, mutate_rel] {
        let source = read_repo_file(rel);
        assert!(
            !source.contains("PublicCommand::")
                && !source.contains("recommended_public_command_argv")
                && !source.contains("route_decision:")
                && !source.contains("RouteDecision {"),
            "{rel} must not rebuild executable public-route decisions or typed public-command output"
        );
    }
}

#[test]
fn persistent_fixture_cache_survives_cargo_clean_without_binary_mtime_keys() {
    let rel = "tests/support/persistent_fixture_cache.rs";
    let source = read_repo_file(rel);
    assert!(
        source.contains(".join(\".featureforge\")")
            && source.contains(".join(\"test-cache\")")
            && source.contains("workspace_fixture_source_stamp()"),
        "{rel} must keep expensive fixture templates outside target/ and key them by source inputs so cargo clean does not force cold-cache full-suite reruns"
    );
    for forbidden in [
        ".join(\"target\")",
        "current_exe()",
        ".modified()",
        "compiled_runtime_stamp",
        "current_test_binary_stamp",
        "CARGO_BIN_EXE_featureforge",
    ] {
        assert!(
            !source.contains(forbidden),
            "{rel} must not key persistent fixture caches by build-output paths or binary mtimes; found `{forbidden}`"
        );
    }
}

#[derive(Clone, Copy)]
struct LargeRuntimeModuleBoundary {
    rel: &'static str,
    status: &'static str,
}

const LARGE_RUNTIME_MODULE_BOUNDARIES: &[LargeRuntimeModuleBoundary] = &[
    LargeRuntimeModuleBoundary {
        rel: "src/execution/transitions.rs",
        status: "documented exception",
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
        rel: "src/execution/authority.rs",
        status: "documented exception",
    },
    LargeRuntimeModuleBoundary {
        rel: "src/execution/gates.rs",
        status: "documented exception",
    },
    LargeRuntimeModuleBoundary {
        rel: "src/execution/commands/advance_late_stage.rs",
        status: "scheduled follow-up",
    },
    LargeRuntimeModuleBoundary {
        rel: "src/execution/current_truth.rs",
        status: "scheduled follow-up",
    },
    LargeRuntimeModuleBoundary {
        rel: "src/execution/projection_renderer.rs",
        status: "documented exception",
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
    let boundary_doc =
        read_repo_file("docs/featureforge/reference/execution-runtime-module-boundaries.md");
    let documented_large_modules = LARGE_RUNTIME_MODULE_BOUNDARIES
        .iter()
        .map(|boundary| boundary.rel)
        .collect::<BTreeSet<_>>();
    let mut undocumented_large_modules = Vec::new();
    for path in production_execution_rust_source_files() {
        let rel = repo_relative(&path);
        let source = read_repo_file(&rel);
        if source.lines().count() > 2000 && !documented_large_modules.contains(rel.as_str()) {
            undocumented_large_modules.push(format!(
                "{rel} has {} lines but is missing from LARGE_RUNTIME_MODULE_BOUNDARIES",
                source.lines().count()
            ));
        }
    }
    assert!(
        undocumented_large_modules.is_empty(),
        "production src/execution Rust files above 2000 lines must be documented:\n{}",
        undocumented_large_modules.join("\n")
    );
    for boundary in LARGE_RUNTIME_MODULE_BOUNDARIES {
        let source = read_repo_file(boundary.rel);
        assert!(
            !source_module_names(boundary.rel, &source).is_empty()
                || source_function_names(boundary.rel, &source).len() > 1,
            "{} should remain a documented production execution boundary with meaningful local responsibilities",
            boundary.rel
        );
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

#[test]
fn task5_focused_semantic_modules_have_import_direction_guards() {
    let public_commands_rel = "src/execution/route_plan/public_commands.rs";
    let public_commands = read_repo_file(public_commands_rel);
    let public_command_paths = normalized_dependency_paths(public_commands_rel, &public_commands);
    assert!(
        !public_command_paths
            .iter()
            .any(|path| path == "crate::execution::state::ExecutionContext")
            && !public_command_paths
                .iter()
                .any(|path| path.starts_with("crate::execution::route_plan::next_action_choice")),
        "{public_commands_rel} must bind public commands from route decisions and must not import status-context recomputation paths in production: {public_command_paths:?}"
    );

    let mutation_request_rel = "src/execution/command_eligibility/mutation_request.rs";
    let mutation_request = read_repo_file(mutation_request_rel);
    assert_no_import_path_prefix(
        mutation_request_rel,
        &mutation_request,
        &[
            "crate::execution::commands",
            "crate::execution::mutate",
            "crate::execution::read_model",
            "crate::execution::route_plan",
            "crate::execution::state",
            "crate::workflow",
        ],
        "must stay a command-eligibility DTO/constructor module rather than deriving route, read-model, or mutation state",
    );

    let cleanup_rel = "src/execution/current_task_closure_cleanup.rs";
    let cleanup = read_repo_file(cleanup_rel);
    assert_no_import_path_prefix(
        cleanup_rel,
        &cleanup,
        &[
            "crate::execution::commands",
            "crate::execution::event_log",
            "crate::execution::mutate",
            "crate::execution::read_model",
            "crate::execution::route_plan",
            "crate::execution::router",
            "crate::workflow",
        ],
        "must remain a read-only cleanup-decision helper and not append events, route commands, or import presentation surfaces",
    );

    let task_scope_key_rel = "src/execution/task_scope_key.rs";
    let task_scope_key = read_repo_file(task_scope_key_rel);
    let task_scope_dependencies =
        normalized_expanded_use_paths(task_scope_key_rel, &task_scope_key);
    assert!(
        task_scope_dependencies.is_empty(),
        "{task_scope_key_rel} must remain an import-free parser/formatter leaf module: {task_scope_dependencies:?}"
    );
}

#[test]
fn current_truth_does_not_depend_upward_on_reducer_state() {
    let current_truth = read_repo_file("src/execution/current_truth.rs");
    assert_no_import_path_prefix(
        "src/execution/current_truth.rs",
        &current_truth,
        &["crate::execution::reducer"],
        "must not depend upward on reducer aggregates; callers pass borrowed current-truth inputs",
    );
    assert!(
        !current_truth.contains("RuntimeState"),
        "current_truth.rs must not mention reducer RuntimeState directly"
    );
}

#[test]
fn route_plan_owns_runtime_route_ordering() {
    let route_plan = read_repo_file("src/execution/route_plan.rs");
    let route_plan_functions = source_function_names("src/execution/route_plan.rs", &route_plan);
    let route_plan_sources = route_plan_boundary_sources();
    let route_plan_production_sources = route_plan_production_boundary_sources();
    let route_plan_constructors_rel = "src/execution/route_plan/constructors.rs";
    let route_plan_constructors = read_repo_file(route_plan_constructors_rel);
    let route_plan_decision_rel = "src/execution/route_plan/decision.rs";
    let route_plan_decision = read_repo_file(route_plan_decision_rel);
    let route_plan_planning_facts_rel = "src/execution/route_plan/planning_facts.rs";
    let route_plan_planning_facts = read_repo_file(route_plan_planning_facts_rel);
    let route_plan_repair_follow_up_binding_rel =
        "src/execution/route_plan/repair_follow_up_binding.rs";
    let route_plan_repair_follow_up_binding =
        read_repo_file(route_plan_repair_follow_up_binding_rel);
    let route_plan_state_kind_rel = "src/execution/route_plan/state_kind.rs";
    let route_plan_state_kind = read_repo_file(route_plan_state_kind_rel);
    let route_plan_status_projection_rel = "src/execution/route_plan/status_projection.rs";
    let route_plan_status_projection = read_repo_file(route_plan_status_projection_rel);
    assert_no_import_path_prefix(
        route_plan_status_projection_rel,
        &route_plan_status_projection,
        &[
            "crate::execution::route_plan::constructors",
            "super::constructors",
            "crate::execution::route_plan::stale_repair_target",
            "super::stale_repair_target",
        ],
        "must remain route-neutral and must not import route constructors or stale-target selectors",
    );
    let route_plan_status_projection_paths = normalized_code_paths(
        route_plan_status_projection_rel,
        &route_plan_status_projection,
    );
    assert!(route_plan_functions.contains("plan_runtime_route"));
    let display_command_parse_calls =
        call_path_leaf_violations(&route_plan_production_sources, "parse_display_command");
    assert!(
        display_command_parse_calls.is_empty(),
        "route-plan production modules must not recover command authority from display text:\n{}",
        display_command_parse_calls.join("\n")
    );
    let direct_recommended_command_display_violations = route_plan_production_sources
        .iter()
        .flat_map(|(rel, source)| direct_recommended_command_some_violations(rel, source))
        .collect::<Vec<_>>();
    assert!(
        direct_recommended_command_display_violations.is_empty(),
        "route-plan production modules must derive display commands from typed public-command surfaces, not direct `Some(...)` field initializers:\n{}",
        direct_recommended_command_display_violations.join("\n")
    );
    let route_plan_inlines_canonical_route_decision_literal = source_constructs_struct_literal(
        "src/execution/route_plan.rs",
        &route_plan,
        "PublicRouteDecision",
    ) || source_constructs_struct_literal(
        "src/execution/route_plan.rs",
        &route_plan,
        "RouteDecision",
    );
    assert!(
        !route_plan_inlines_canonical_route_decision_literal,
        "route_plan.rs must keep canonical RouteDecision/PublicRouteDecision literal construction in route-plan child modules"
    );
    assert_route_plan_status_projection_remains_route_neutral(&route_plan_status_projection);
    for (rel, source) in &route_plan_sources {
        assert_no_import_path_prefix(
            rel,
            source,
            &["crate::execution::router"],
            "must not import router.rs; router may project route-plan decisions, but route-plan cannot depend upward on router",
        );
    }
    for (rel, source) in &route_plan_production_sources {
        assert_no_import_path_prefix(
            rel,
            source,
            &["crate::workflow::status"],
            "route-plan production modules must import shared workflow route contracts instead of workflow presentation status",
        );
    }
    let router_rel = "src/execution/router.rs";
    let router = read_repo_file(router_rel);
    assert_no_import_path_prefix(
        router_rel,
        &router,
        &["crate::workflow::status"],
        "execution router must import shared workflow route contracts instead of workflow presentation status",
    );
    let route_plan_decision_structs =
        source_struct_names(route_plan_decision_rel, &route_plan_decision);
    let route_plan_decision_aliases =
        source_type_alias_names(route_plan_decision_rel, &route_plan_decision);
    assert!(
        route_plan_decision_structs.contains("PublicRouteDecision")
            && route_plan_decision_aliases.contains("RouteDecision")
            && route_plan_decision_structs.contains("NextPublicAction")
            && route_plan_decision_structs.contains("Blocker"),
        "route_plan/decision.rs must own route decision DTOs and route presentation DTO types. Structs: {route_plan_decision_structs:?}; aliases: {route_plan_decision_aliases:?}"
    );
    let route_plan_constructor_paths =
        normalized_dependency_paths(route_plan_constructors_rel, &route_plan_constructors);
    assert!(
        !route_plan_constructor_paths
            .iter()
            .any(|path| path.starts_with("crate::execution::repair_route_decision::"))
            && !route_plan_constructor_paths
                .iter()
                .any(|path| path.starts_with("crate::execution::repair_target_selection::")),
        "route-plan typed route construction must not import repair-route authority"
    );
    let route_plan_planning_fact_structs =
        source_struct_names(route_plan_planning_facts_rel, &route_plan_planning_facts);
    let route_plan_planning_fact_paths =
        normalized_code_paths(route_plan_planning_facts_rel, &route_plan_planning_facts);
    assert!(
        route_plan_planning_fact_structs.contains("RoutePlanningFacts")
            && !route_plan_planning_fact_paths
                .iter()
                .any(|path| path.ends_with("NextActionDecision")),
        "{route_plan_planning_facts_rel} must own immutable route-choice fact projection without carrying preselected next-action candidates"
    );
    let route_plan_state_kind_paths =
        normalized_code_paths(route_plan_state_kind_rel, &route_plan_state_kind);
    assert!(
        !route_plan_state_kind_paths
            .iter()
            .any(|path| path.ends_with("NextPublicAction")),
        "{route_plan_state_kind_rel} must own state-kind classification without display-string parsing"
    );
    let repair_follow_up_binding_paths = normalized_dependency_paths(
        route_plan_repair_follow_up_binding_rel,
        &route_plan_repair_follow_up_binding,
    );
    for required_dependency_prefix in [
        "crate::execution::current_truth",
        "crate::execution::follow_up",
        "crate::execution::public_repair_targets",
    ] {
        assert!(
            repair_follow_up_binding_paths
                .iter()
                .any(|path| path.starts_with(required_dependency_prefix)),
            "{route_plan_repair_follow_up_binding_rel} must bind source-route repair state through the shared {required_dependency_prefix} owner: {repair_follow_up_binding_paths:?}"
        );
    }
    let route_plan_status_projection_calls = rust_source_scan::normalized_call_paths(
        route_plan_status_projection_rel,
        &route_plan_status_projection,
        &[],
    );
    let route_plan_status_projection_constructs_repair_target = source_constructs_struct_literal(
        route_plan_status_projection_rel,
        &route_plan_status_projection,
        "PublicRepairTarget",
    );
    assert!(
        route_plan_status_projection_paths
            .iter()
            .any(|path| path.starts_with("crate::execution::public_repair_targets::"))
            && route_plan_status_projection_calls
                .iter()
                .any(|path| path.ends_with("public_repair_targets_for_route_decision"))
            && !route_plan_status_projection_constructs_repair_target
            && !route_plan_status_projection_paths
                .iter()
                .any(|path| path.ends_with("PublicRepairTargetReason")),
        "{route_plan_status_projection_rel} must project status blockers, follow-ups, diagnostics, and centralized public repair targets for the selected route without revising the route or constructing repair-target literals"
    );
    let router = read_repo_file("src/execution/router.rs");
    assert_no_import_path_prefix(
        "src/execution/router.rs",
        &router,
        &[
            "crate::execution::current_truth::resolve_actionable_repair_follow_up",
            "crate::execution::current_truth::CurrentTruthFollowUpInputs",
            "crate::execution::follow_up::repair_follow_up_source_decision_hash",
            "crate::execution::public_repair_targets",
        ],
        "must not bind persisted repair follow-ups or materialize public repair targets outside route_plan/repair_follow_up_binding.rs",
    );
    let router_calls =
        rust_source_scan::normalized_call_paths("src/execution/router.rs", &router, &[]);
    let router_route_decision_mutations = field_mutations_on_base_ident_in_module(
        "src/execution/router.rs",
        &router,
        "route_decision",
    );
    assert!(
        router_route_decision_mutations.is_empty()
            && !router_calls
                .iter()
                .any(|path| path.ends_with("resolve_actionable_repair_follow_up"))
            && !router_calls
                .iter()
                .any(|path| path.ends_with("repair_follow_up_source_decision_hash")),
        "router.rs must project selected routes without mutating RouteDecision or binding persisted repair follow-ups directly; route_decision mutations: {router_route_decision_mutations:?}; calls: {router_calls:?}"
    );
    assert!(
        router_calls
            .iter()
            .any(|path| path.ends_with("plan_runtime_route")),
        "router.rs should call the route_plan owner rather than carrying route-ordering logic inline"
    );

    let status_assembly_exact_route =
        read_repo_file("src/execution/status_assembly/exact_route.rs");
    let status_assembly_task_state = read_repo_file("src/execution/status_assembly/task_state.rs");
    for (rel, source) in [
        (
            "src/execution/status_assembly/exact_route.rs",
            status_assembly_exact_route.as_str(),
        ),
        (
            "src/execution/status_assembly/task_state.rs",
            status_assembly_task_state.as_str(),
        ),
    ] {
        assert_no_import_path_prefix(
            rel,
            source,
            &[
                "crate::execution::route_plan",
                "crate::execution::router",
                "crate::execution::next_action",
                "crate::execution::repair_route_decision",
                "crate::execution::repair_target_selection",
                "crate::execution::stale_target_selection",
                "crate::execution::transitions",
            ],
            "status assembly must validate finalized route projection fields without recomputing route choice",
        );
    }
}

#[test]
fn advance_late_stage_does_not_rederive_review_state_follow_up() {
    let advance_late_stage = read_repo_file("src/execution/commands/advance_late_stage.rs");
    assert!(
        !advance_late_stage.contains("derived_review_state_missing"),
        "advance_late_stage.rs must consume shared route/follow-up decisions instead of re-deriving derived-review-state repair from raw reason codes"
    );
}

#[test]
fn public_command_modules_use_shared_mutation_request_constructors() {
    let mutation_request = read_repo_file("src/execution/command_eligibility/mutation_request.rs");
    for required_constructor in [
        "pub fn begin(",
        "pub fn complete(",
        "pub fn reopen(",
        "pub fn transfer_repair_step(",
        "pub fn transfer_handoff(",
        "pub fn close_current_task(",
        "pub fn advance_late_stage(",
    ] {
        assert!(
            mutation_request.contains(required_constructor),
            "PublicMutationRequest should expose shared constructor `{required_constructor}`"
        );
    }
    assert!(
        mutation_request.contains("PublicCommandKind"),
        "PublicMutationRequest must derive command tokens through the shared PublicCommandKind token owner"
    );
    assert!(
        mutation_request.contains("pub kind: PublicCommandKind"),
        "PublicMutationRequest must store PublicCommandKind as its typed command authority"
    );
    assert!(
        !mutation_request.contains("pub enum PublicMutationKind"),
        "mutation_request.rs must not keep a parallel PublicMutationKind enum that can drift from PublicCommandKind"
    );
    assert!(
        !mutation_request.contains("command_name:"),
        "PublicMutationRequest must derive command names from PublicCommandKind instead of storing a second token field"
    );
    for duplicated_token_literal in [
        "\"begin\"",
        "\"complete\"",
        "\"reopen\"",
        "\"transfer\"",
        "\"close-current-task\"",
        "\"repair-review-state\"",
        "\"advance-late-stage\"",
    ] {
        assert!(
            !mutation_request.contains(duplicated_token_literal),
            "mutation_request.rs must not duplicate public command token literal {duplicated_token_literal}; use PublicCommandKind"
        );
    }
    for rel in [
        "src/execution/route_plan.rs",
        "src/execution/route_plan/status_projection.rs",
        "src/execution/public_repair_targets.rs",
        "src/execution/stale_target_selection.rs",
    ] {
        let source = read_repo_file(rel);
        let production_string_literals = rust_production_string_literal_values(rel, &source)
            .into_iter()
            .collect::<BTreeSet<_>>();
        for duplicated_token_literal in [
            "reopen",
            "close-current-task",
            "repair-review-state",
            "advance-late-stage",
        ] {
            assert!(
                !production_string_literals.contains(duplicated_token_literal),
                "{rel} must source public route/repair-target command token `{duplicated_token_literal}` from PublicCommandKind"
            );
        }
    }
    let mut raw_comparison_violations = Vec::new();
    for (rel, source) in rust_source_files(&repo_root().join("src"))
        .into_iter()
        .map(|path| {
            let rel = repo_relative(&path);
            let source = read_repo_file(&rel);
            (rel, source)
        })
        .filter(|(rel, _)| raw_public_mutation_token_scan_subject(rel))
    {
        raw_comparison_violations.extend(raw_public_mutation_token_usage_violations(&rel, &source));
    }
    assert!(
        raw_comparison_violations.is_empty(),
        "production public mutation token comparisons must go through PublicCommandKind:\n{}",
        raw_comparison_violations.join("\n")
    );

    let mut manual_request_builders = Vec::new();
    for (rel, source) in execution_command_sources() {
        if source_constructs_struct_literal(&rel, &source, "PublicMutationRequest") {
            manual_request_builders.push(rel);
        }
    }
    assert!(
        manual_request_builders.is_empty(),
        "public execution command modules must call shared PublicMutationRequest constructors instead of hand-populating request fields: {manual_request_builders:?}"
    );

    for (rel, expected_constructor) in [
        (
            "src/execution/commands/begin.rs",
            "PublicMutationRequest::begin(",
        ),
        (
            "src/execution/commands/complete.rs",
            "PublicMutationRequest::complete(",
        ),
        (
            "src/execution/commands/reopen.rs",
            "PublicMutationRequest::reopen(",
        ),
        (
            "src/execution/commands/transfer.rs",
            "PublicMutationRequest::transfer_repair_step(",
        ),
        (
            "src/execution/commands/transfer.rs",
            "PublicMutationRequest::transfer_handoff(",
        ),
        (
            "src/execution/commands/common/mutation_guards.rs",
            "PublicMutationRequest::advance_late_stage(",
        ),
    ] {
        assert!(
            read_repo_file(rel).contains(expected_constructor),
            "{rel} should build public mutation requests with shared constructor `{expected_constructor}`"
        );
    }
}

fn raw_public_mutation_token_scan_subject(rel: &str) -> bool {
    rel.starts_with("src/")
        && !rel.starts_with("src/cli/")
        && rel != "src/execution/command_eligibility.rs"
        && rel != "src/execution/command_eligibility/command_kind.rs"
        && !rel.ends_with("_tests.rs")
        && !rel.ends_with("unit_tests.rs")
        && !rel.ends_with("/tests.rs")
}

fn raw_public_mutation_token_usage_violations(rel: &str, source: &str) -> Vec<String> {
    let syntax = parse_rust_source(rel, source);
    let mut visitor = RawPublicMutationTokenUsageVisitor {
        rel,
        violations: Vec::new(),
    };
    visitor.visit_file(&syntax);
    visitor.violations
}

struct RawPublicMutationTokenUsageVisitor<'a> {
    rel: &'a str,
    violations: Vec<String>,
}

impl<'ast> Visit<'ast> for RawPublicMutationTokenUsageVisitor<'_> {
    fn visit_expr_binary(&mut self, binary: &'ast syn::ExprBinary) {
        if matches!(binary.op, syn::BinOp::Eq(_) | syn::BinOp::Ne(_)) {
            self.reject_raw_token_usage(&binary.left);
            self.reject_raw_token_usage(&binary.right);
        }
        visit::visit_expr_binary(self, binary);
    }

    fn visit_pat(&mut self, pat: &'ast syn::Pat) {
        if let syn::Pat::Lit(literal) = pat {
            self.reject_raw_token_literal(&literal.lit);
        }
        visit::visit_pat(self, pat);
    }

    fn visit_item_fn(&mut self, item_fn: &'ast syn::ItemFn) {
        if item_attrs_mark_test_only(&item_fn.attrs) {
            return;
        }
        visit::visit_item_fn(self, item_fn);
    }

    fn visit_item_impl(&mut self, item_impl: &'ast syn::ItemImpl) {
        if item_attrs_mark_test_only(&item_impl.attrs) {
            return;
        }
        visit::visit_item_impl(self, item_impl);
    }

    fn visit_item_mod(&mut self, item_mod: &'ast syn::ItemMod) {
        if item_attrs_mark_test_only(&item_mod.attrs) {
            return;
        }
        visit::visit_item_mod(self, item_mod);
    }
}

impl RawPublicMutationTokenUsageVisitor<'_> {
    fn reject_raw_token_usage(&mut self, expr: &syn::Expr) {
        for token in public_mutation_token_literals_in_expr(expr) {
            self.violations.push(format!(
                "{} matches or compares raw public mutation token `{}`; use PublicCommandKind matching helpers",
                self.rel, token
            ));
        }
    }

    fn reject_raw_token_literal(&mut self, literal: &syn::Lit) {
        if let syn::Lit::Str(literal) = literal {
            let token = literal.value();
            if public_mutation_tokens().contains(token.as_str()) {
                self.violations.push(format!(
                    "{} matches or compares raw public mutation token `{}`; use PublicCommandKind matching helpers",
                    self.rel, token
                ));
            }
        }
    }
}

fn public_mutation_token_literals_in_expr(expr: &syn::Expr) -> Vec<String> {
    let mut values = Vec::new();
    collect_public_mutation_token_literals(expr, &mut values);
    values
}

fn collect_public_mutation_token_literals(expr: &syn::Expr, values: &mut Vec<String>) {
    match expr {
        syn::Expr::Lit(_) | syn::Expr::Group(_) | syn::Expr::Paren(_) | syn::Expr::Reference(_) => {
            if let Some(token) = string_literal_expr_value(expr)
                && public_mutation_tokens().contains(token.as_str())
            {
                values.push(token);
            }
        }
        syn::Expr::Call(call) if expr_path_last_segment_is(&call.func, "Some") => {
            for arg in &call.args {
                collect_public_mutation_token_literals(arg, values);
            }
        }
        syn::Expr::Tuple(tuple) => {
            for elem in &tuple.elems {
                collect_public_mutation_token_literals(elem, values);
            }
        }
        _ => {}
    }
}

fn expr_path_last_segment_is(expr: &syn::Expr, expected: &str) -> bool {
    match expr {
        syn::Expr::Path(path) => path
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == expected),
        syn::Expr::Group(group) => expr_path_last_segment_is(&group.expr, expected),
        syn::Expr::Paren(paren) => expr_path_last_segment_is(&paren.expr, expected),
        _ => false,
    }
}

fn public_mutation_tokens() -> BTreeSet<&'static str> {
    public_mutation_command_tokens().collect()
}

#[test]
fn public_mutation_token_scanner_covers_non_cli_production_wrappers_only() {
    let production_fixture = r#"
fn drift(candidate: Option<&str>) -> bool {
    candidate == Some("begin")
}

fn route(command: &str) -> bool {
    match command {
        "complete" => true,
        _ => false,
    }
}
"#;
    let production_violations =
        raw_public_mutation_token_usage_violations("src/workflow/operator.rs", production_fixture);
    assert!(
        production_violations
            .iter()
            .any(|violation| violation.contains("`begin`")),
        "production scanner should reject Some(\"begin\") comparisons: {production_violations:#?}"
    );
    assert!(
        production_violations
            .iter()
            .any(|violation| violation.contains("`complete`")),
        "production scanner should reject raw mutation-token match arms: {production_violations:#?}"
    );

    let cfg_test_fixture = r#"
#[cfg(test)]
mod tests {
    fn fixture(candidate: Option<&str>) -> bool {
        candidate == Some("begin")
    }
}
"#;
    let cfg_test_violations =
        raw_public_mutation_token_usage_violations("src/workflow/operator.rs", cfg_test_fixture);
    assert!(
        cfg_test_violations.is_empty(),
        "cfg(test) modules should stay outside production token scanner: {cfg_test_violations:#?}"
    );

    assert!(
        !raw_public_mutation_token_scan_subject("src/cli/plan_execution.rs"),
        "CLI parse/argv construction surfaces are explicit scanner exemptions"
    );
    assert!(
        !raw_public_mutation_token_scan_subject("src/execution/command_eligibility.rs"),
        "typed public command parser/renderer ownership is an explicit scanner exemption"
    );
    assert!(
        !raw_public_mutation_token_scan_subject(
            "src/execution/route_plan/next_action_choice/tests.rs"
        ),
        "test-only src modules named tests.rs are explicit scanner exemptions"
    );
    assert!(
        raw_public_mutation_token_scan_subject("src/workflow/operator.rs"),
        "non-CLI production route/status consumers remain in scanner scope"
    );
}

#[test]
fn advance_late_stage_public_mutation_guard_is_mode_bound() {
    let command_eligibility = read_repo_file("src/execution/command_eligibility.rs");
    let mutation_request = read_repo_file("src/execution/command_eligibility/mutation_request.rs");
    assert!(
        mutation_request
            .contains("pub advance_late_stage_mode: Option<PublicAdvanceLateStageMode>"),
        "PublicMutationRequest must carry the routed advance-late-stage operation mode"
    );
    assert!(
        command_eligibility.contains("PublicMutationRequest::advance_late_stage(*mode)"),
        "typed PublicCommand::AdvanceLateStage must be the source of mutation-request mode authority"
    );

    let advance_late_stage = read_repo_file("src/execution/commands/advance_late_stage.rs");
    assert!(
        !source_constructs_struct_literal(
            "src/execution/commands/advance_late_stage.rs",
            &advance_late_stage,
            "PublicMutationRequest"
        ),
        "advance_late_stage.rs must not hand-populate public mutation requests"
    );
    for forbidden in late_stage_mode_variant_patterns() {
        assert!(
            !advance_late_stage.contains(forbidden),
            "advance_late_stage.rs must not construct late-stage route modes locally via `{forbidden}`"
        );
    }
    for forbidden in [
        "DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS",
        "DETAIL_RELEASE_READINESS_RECORDING_READY",
        "DETAIL_RELEASE_BLOCKER_RESOLUTION_REQUIRED",
        "DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED",
        "DETAIL_FINAL_REVIEW_RECORDING_READY",
        "DETAIL_QA_RECORDING_REQUIRED",
        "DETAIL_FINISH_REVIEW_GATE_READY",
        "DETAIL_FINISH_COMPLETION_GATE_READY",
        "PHASE_DOCUMENT_RELEASE_PENDING",
        "PHASE_FINAL_REVIEW_PENDING",
        "PHASE_READY_FOR_BRANCH_COMPLETION",
    ] {
        assert!(
            !advance_late_stage.contains(forbidden),
            "advance_late_stage.rs must not re-decide late-stage public route readiness with `{forbidden}`; use the typed route-mode helper"
        );
    }
}

#[test]
fn public_follow_up_command_mapping_uses_central_kind_taxonomy() {
    let review_route_token_owner = read_repo_file("src/execution/review_route_tokens.rs");
    let review_route_tokens = review_route_decision_tokens(&review_route_token_owner)
        .into_iter()
        .collect::<BTreeSet<_>>();
    let raw_tokens = public_follow_up_tokens()
        .filter(|token| !review_route_tokens.contains(*token))
        .collect::<Vec<_>>();
    assert!(
        !raw_tokens.is_empty(),
        "follow-up scanner should derive non-route-decision public follow-up tokens from FollowUpKind"
    );

    let route_follow_up = read_repo_file("src/execution/route_plan/follow_up.rs");
    let route_follow_up_paths =
        normalized_dependency_paths("src/execution/route_plan/follow_up.rs", &route_follow_up);
    assert!(
        route_follow_up_paths
            .iter()
            .any(|path| path == "crate::execution::follow_up::normalize_follow_up_alias")
            && route_follow_up_paths
                .iter()
                .any(|path| path == "crate::execution::follow_up::FollowUpKind"),
        "route follow-up command mapping must normalize through FollowUpKind"
    );

    let operator_outputs = read_repo_file("src/execution/commands/common/operator_outputs.rs");
    let operator_output_paths = normalized_dependency_paths(
        "src/execution/commands/common/operator_outputs.rs",
        &operator_outputs,
    );
    assert!(
        operator_output_paths
            .iter()
            .any(|path| path == "crate::execution::follow_up::normalize_follow_up_alias")
            && operator_output_paths
                .iter()
                .any(|path| path == "crate::execution::follow_up::FollowUpKind"),
        "operator output helpers must normalize through FollowUpKind before choosing public command surfaces"
    );
    let execution_sources = rust_source_files(&repo_root().join("src/execution"))
        .into_iter()
        .map(|path| {
            let rel = repo_relative(&path);
            let source = read_repo_file(&rel);
            (rel, source)
        })
        .collect::<Vec<_>>();
    let mut raw_token_violations = Vec::new();
    for (rel, source) in &execution_sources {
        let production_string_literals = rust_production_string_literal_values(rel, source)
            .into_iter()
            .collect::<BTreeSet<_>>();
        for raw_token in raw_tokens.iter().copied() {
            if production_string_literals.contains(raw_token)
                && !follow_up_token_literal_allowed_outside_owner(rel, raw_token)
            {
                raw_token_violations.push(format!(
                    "{rel} production code must source public follow-up token {raw_token} from FollowUpKind or canonical route-token helpers"
                ));
            }
        }
    }
    assert!(
        raw_token_violations.is_empty(),
        "public follow-up vocabulary must be centralized across src/execution:\n{}",
        raw_token_violations.join("\n")
    );
    let review_route_tokens = read_repo_file("src/execution/review_route_tokens.rs");
    assert!(
        review_route_tokens.contains("pub(crate) const OUT_OF_PHASE_REQUERY_REQUIRED_CODE"),
        "review_route_tokens.rs should own the out-of-phase diagnostic code constant"
    );
    let mut requery_code_definitions = Vec::new();
    for (rel, source) in &execution_sources {
        for literal in rust_production_string_literal_values(rel, source) {
            if literal == "out_of_phase_requery_required" {
                requery_code_definitions.push(rel.clone());
            }
        }
    }
    assert_eq!(
        requery_code_definitions,
        vec!["src/execution/review_route_tokens.rs"],
        "out-of-phase requery code literal must appear only at OUT_OF_PHASE_REQUERY_REQUIRED_CODE"
    );
}

fn follow_up_token_literal_allowed_outside_owner(rel: &str, raw_token: &str) -> bool {
    if matches!(
        rel,
        "src/execution/review_route_tokens.rs" | "src/execution/follow_up.rs"
    ) {
        return true;
    }

    // These exceptions are boundary owners for serialized event-log or legacy
    // state spellings. They are intentionally file-scoped; the scanned token
    // vocabulary itself is derived from FollowUpKind above.
    match raw_token {
        "close_current_task" => matches!(
            rel,
            "src/execution/commands/common/outputs.rs"
                | "src/execution/commands/common/rebuild_support.rs"
                | "src/execution/event_log.rs"
                | "src/execution/recording.rs"
        ),
        "gate_review" => matches!(
            rel,
            "src/execution/event_log.rs" | "src/execution/stale_target_projection.rs"
        ),
        "gate_finish" => matches!(rel, "src/execution/stale_target_projection.rs"),
        _ => false,
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

#[test]
fn state_facade_does_not_reexport_reducer_truth_projection() {
    let state_reexports = state_reexported_read_or_status_items();
    for forbidden in [
        "ExecutionDerivedTruth",
        "FinalReviewDispatchAuthority",
        "compute_status_blocking_records",
        "current_final_review_dispatch_authority_for_context",
        "current_task_review_dispatch_id_for_status",
        "derive_execution_truth_from_authority",
        "derive_execution_truth_from_authority_with_gates",
    ] {
        assert!(
            !state_reexports.contains(forbidden),
            "state.rs must not re-export reducer-consumed truth or blocking-record projection `{forbidden}`; reducer/read-model consumers should import runtime_truth directly"
        );
    }
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
        )
    ) || is_allowed_late_stage_qa_artifact_writer(rel, call)
}

fn is_allowed_late_stage_qa_artifact_writer(rel: &str, call: &WriterCall) -> bool {
    rel == "src/execution/commands/advance_late_stage.rs"
        && matches!(
            call.callee.as_str(),
            "write_atomic_file" | "crate::paths::write_atomic"
        )
        && matches!(
            call.target_arg.as_deref(),
            Some("authoritative_test_plan_path" | "authoritative_qa_path")
        )
}

#[test]
fn phase_detail_string_literals_are_centralized() {
    let phase_detail_literals = phase_detail_literals_from_phase_module();
    let allowed_src_files = [
        "src/execution/phase.rs",
        // Task 11 explicitly permits the execution-owned precedence table to mirror
        // public phase-detail vocabulary while asserting precedence rows.
        "src/execution/late_stage_precedence.rs",
    ];
    let allowed_test_files = [
        // Public JSON, shell, replay, and schema/golden suites assert literal
        // compatibility at the boundary. Non-golden static contract tests
        // should import phase constants instead of growing this allowlist.
        "tests/contracts_execution_runtime_boundaries.rs",
        "tests/liveness_model_checker.rs",
        "tests/plan_execution.rs",
        "tests/plan_execution_final_review.rs",
        "tests/public_replay_churn.rs",
        "tests/runtime_behavior_golden.rs",
        // Scanner self-tests intentionally spell literals to prove detection.
        "tests/rust_source_scan_contracts.rs",
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
        if allowed_test_files.contains(&rel.as_str()) {
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
    let literals = rust_source_scan::phase_detail_literals_from_source(
        "src/execution/phase.rs",
        &phase_source,
    );
    assert!(
        literals.len() >= 10,
        "phase-detail boundary test should derive the public phase-detail vocabulary from src/execution/phase.rs, got {literals:?}"
    );
    literals
}

fn rust_string_literal_values(rel: &str, source: &str) -> Vec<String> {
    rust_source_scan::rust_string_literal_values(rel, source)
}

fn rust_production_string_literal_values(rel: &str, source: &str) -> Vec<String> {
    rust_source_scan::rust_production_string_literal_values(rel, source)
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
fn focused_extracted_production_modules_do_not_use_parent_globs() {
    let mut violations = Vec::new();
    for rel in focused_explicit_import_module_rels() {
        let source = read_repo_file(&rel);
        if rust_source_scan::production_source_uses_parent_glob(&source) {
            violations.push(format!("{rel} uses a parent glob import from `super`"));
        }
    }
    assert!(
        violations.is_empty(),
        "focused extracted production modules must use explicit imports so boundary drift is reviewable:\n{}",
        violations.join("\n")
    );
}

const FOCUSED_EXPLICIT_IMPORT_FILES: &[&str] = &[
    "src/execution/closure_dispatch.rs",
    "src/execution/closure_dispatch_mutation.rs",
    "src/execution/closure_diagnostics.rs",
    "src/execution/current_closure_projection.rs",
    "src/execution/current_task_closure_cleanup.rs",
    "src/execution/gate_reason_codes.rs",
    "src/execution/public_repair_target_reasons.rs",
    "src/execution/repair_target_selection.rs",
    "src/execution/repair_route_decision.rs",
    "src/execution/resume_stale_precedence.rs",
    "src/execution/route_plan.rs",
    "src/execution/runtime_truth.rs",
    "src/execution/stale_target_projection.rs",
    "src/execution/stale_target_selection.rs",
    "src/execution/status_assembly.rs",
    "src/execution/task_scope_key.rs",
];

const FOCUSED_EXPLICIT_IMPORT_ROOTS: &[&str] = &[
    "src/execution/closure_diagnostics",
    "src/execution/closure_dispatch_mutation",
    "src/execution/command_eligibility",
    "src/execution/commands/common",
    "src/execution/read_model",
    "src/execution/repair_route_decision",
    "src/execution/route_plan",
    "src/execution/stale_target_projection",
    "src/execution/state",
    "src/execution/status_assembly",
];

fn focused_explicit_import_module_rels() -> Vec<String> {
    let mut rels = FOCUSED_EXPLICIT_IMPORT_FILES
        .iter()
        .copied()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    for root in FOCUSED_EXPLICIT_IMPORT_ROOTS {
        rels.extend(
            rust_source_files(&repo_root().join(root))
                .into_iter()
                .map(|path| repo_relative(&path))
                .filter(|rel| !focused_explicit_import_test_module(rel)),
        );
    }
    rels.into_iter().collect()
}

fn focused_explicit_import_test_module(rel: &str) -> bool {
    rel.ends_with("_tests.rs")
        || rel.ends_with("/unit_tests.rs")
        || rel.ends_with("/tests.rs")
        || rel.contains("_tests/")
}
