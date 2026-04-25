#[path = "support/bin.rs"]
mod bin_support;
#[path = "support/dir_tree.rs"]
mod dir_tree_support;
#[path = "support/files.rs"]
mod files_support;
#[path = "support/json.rs"]
mod json_support;
#[path = "../src/workflow/markdown_scan.rs"]
mod markdown_scan_support;
#[path = "support/plan_execution_direct.rs"]
mod plan_execution_direct_support;
#[path = "support/process.rs"]
mod process_support;
#[path = "support/runtime_json.rs"]
mod runtime_json_support;
#[path = "support/runtime_phase_handoff.rs"]
mod runtime_phase_handoff_support;
#[path = "support/workflow_direct.rs"]
mod workflow_direct_support;
#[path = "support/workflow.rs"]
mod workflow_support;

use assert_cmd::cargo::cargo_bin;
use bin_support::compiled_featureforge_path;
use dir_tree_support::copy_dir_recursive;
use featureforge::cli::plan_execution::{
    RecordReviewDispatchArgs, ReviewDispatchScopeArg, StatusArgs as PlanExecutionStatusArgs,
};
use featureforge::contracts::plan::{PLAN_FIDELITY_REQUIRED_SURFACES, parse_plan_file};
use featureforge::contracts::spec::parse_spec_file;
use featureforge::execution::final_review::resolve_release_base_branch;
use featureforge::execution::observability::{
    HarnessEventKind, HarnessObservabilityEvent, HarnessTelemetryCounters, STABLE_EVENT_KINDS,
    STABLE_REASON_CODES,
};
use featureforge::execution::semantic_identity::{
    branch_definition_identity_for_context, task_definition_identity_for_task,
};
use featureforge::execution::state::{
    current_head_sha as runtime_current_head_sha, derive_evidence_rel_path, load_execution_context,
};
use featureforge::git::{
    RepositoryIdentity, discover_repo_identity, discover_repository, discover_slug_identity,
};
use featureforge::paths::{
    branch_storage_key, harness_authoritative_artifact_path, harness_state_path,
};
use featureforge::workflow::manifest::{
    WorkflowManifest, manifest_path, recover_slug_changed_manifest,
};
use featureforge::workflow::status::WorkflowRuntime;
use files_support::write_file;
use json_support::parse_json;
use process_support::{repo_root, run, run_checked};
use runtime_json_support::{discover_execution_runtime, plan_execution_status_json};
use runtime_phase_handoff_support::{workflow_handoff_json, workflow_phase_json};
use serde::Serialize;
use serde_json::{Value, json, to_value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;
use tempfile::TempDir;
use workflow_support::{
    copy_workflow_fixture, init_repo as init_workflow_repo, workflow_fixture_root,
};

const FIXTURE_STRATEGY_CHECKPOINT_FINGERPRINT: &str =
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const FULL_CONTRACT_READY_SPEC_REL: &str =
    "docs/featureforge/specs/2026-03-22-runtime-integration-hardening-design.md";
const FULL_CONTRACT_READY_PLAN_REL: &str =
    "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
const FULL_CONTRACT_READY_SPEC_FIXTURE_REL: &str =
    "specs/2026-03-22-runtime-integration-hardening-design.md";
const FULL_CONTRACT_READY_PLAN_FIXTURE_REL: &str =
    "plans/2026-03-22-runtime-integration-hardening.md";
const FULL_CONTRACT_READY_FIXTURE_SPEC_PATH: &str = "tests/codex-runtime/fixtures/workflow-artifacts/specs/2026-03-22-runtime-integration-hardening-design.md";

struct FullContractReadyFixtureTemplate {
    plan: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkflowRuntimeFixtureQaMode {
    Required,
    NotRequired,
}

#[derive(Debug, Clone)]
struct WorkflowRuntimeExecutionFixtureTemplate {
    repo_root: PathBuf,
    state_root: PathBuf,
}

static FULL_CONTRACT_READY_FIXTURE_TEMPLATE: OnceLock<FullContractReadyFixtureTemplate> =
    OnceLock::new();
static WORKFLOW_RUNTIME_EXECUTION_TEMPLATE_REQUIRED: OnceLock<
    WorkflowRuntimeExecutionFixtureTemplate,
> = OnceLock::new();
static WORKFLOW_RUNTIME_EXECUTION_TEMPLATE_NOT_REQUIRED: OnceLock<
    WorkflowRuntimeExecutionFixtureTemplate,
> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
struct PublicRouteSnapshot {
    phase: String,
    phase_detail: Option<String>,
    review_state_status: String,
    next_action: String,
    recommended_command: Option<String>,
    blocking_scope: Option<String>,
    blocking_task: Option<u32>,
    external_wait_state: Option<String>,
    blocking_reason_codes: Vec<String>,
}

fn public_route_snapshot(value: &Value) -> PublicRouteSnapshot {
    let phase = value
        .get("phase")
        .or_else(|| value.get("harness_phase"))
        .and_then(Value::as_str)
        .unwrap_or_else(|| {
            panic!("route payload must include string `phase` or `harness_phase`: {value}")
        })
        .to_owned();
    let review_state_status = value
        .get("review_state_status")
        .and_then(Value::as_str)
        .unwrap_or_else(|| {
            panic!("route payload must include string `review_state_status`: {value}")
        })
        .to_owned();
    let next_action = value
        .get("next_action")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("route payload must include string `next_action`: {value}"))
        .to_owned();

    PublicRouteSnapshot {
        phase,
        phase_detail: value
            .get("phase_detail")
            .and_then(Value::as_str)
            .map(str::to_owned),
        review_state_status,
        next_action,
        recommended_command: value
            .get("recommended_command")
            .and_then(Value::as_str)
            .map(str::to_owned),
        blocking_scope: value
            .get("blocking_scope")
            .and_then(Value::as_str)
            .map(str::to_owned),
        blocking_task: value
            .get("blocking_task")
            .and_then(Value::as_u64)
            .and_then(|raw| u32::try_from(raw).ok()),
        external_wait_state: value
            .get("external_wait_state")
            .and_then(Value::as_str)
            .map(str::to_owned),
        blocking_reason_codes: value
            .get("blocking_reason_codes")
            .and_then(Value::as_array)
            .map(|codes| {
                codes
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default(),
    }
}

fn assert_public_route_parity(operator: &Value, status: &Value, doctor: Option<&Value>) {
    let operator_route = public_route_snapshot(operator);
    let status_route = public_route_snapshot(status);
    assert_eq!(
        operator_route, status_route,
        "workflow operator and plan execution status must agree on public route fields"
    );
    if let Some(doctor) = doctor {
        let doctor_route = public_route_snapshot(doctor);
        assert_eq!(
            operator_route, doctor_route,
            "workflow doctor top-level route must match workflow operator"
        );
    }
}

fn assert_follow_up_blocker_parity_with_operator(
    operator: &Value,
    follow_up: &Value,
    context: &str,
) {
    if follow_up["action"].as_str() != Some("blocked") {
        return;
    }
    let follow_up_blocking_reason_codes = follow_up
        .get("blocking_reason_codes")
        .and_then(Value::as_array)
        .unwrap_or_else(|| {
            panic!(
                "{context} blocked follow-up must include blocking_reason_codes metadata: {follow_up:?}"
            )
        });
    let operator_blocking_reason_codes = operator
        .get("blocking_reason_codes")
        .and_then(Value::as_array)
        .unwrap_or_else(|| {
            panic!(
                "{context} operator route must include blocking_reason_codes metadata for blocked parity checks: {operator:?}"
            )
        });
    assert!(
        !follow_up_blocking_reason_codes.is_empty(),
        "{context} blocked follow-up must keep a non-empty blocker reason-code set",
    );
    assert!(
        !operator_blocking_reason_codes.is_empty(),
        "{context} operator route must keep a non-empty blocker reason-code set for blocked parity checks",
    );
    assert_eq!(
        follow_up["blocking_scope"], operator["blocking_scope"],
        "{context} blocked follow-up must preserve operator blocking scope"
    );
    assert_eq!(
        follow_up["blocking_task"], operator["blocking_task"],
        "{context} blocked follow-up must preserve operator blocking task"
    );
    assert_eq!(
        follow_up["blocking_reason_codes"], operator["blocking_reason_codes"],
        "{context} blocked follow-up must preserve operator blocker reason-code set"
    );
    if !follow_up["blocking_step"].is_null() || !operator["blocking_step"].is_null() {
        assert_eq!(
            follow_up["blocking_step"], operator["blocking_step"],
            "{context} blocked follow-up must preserve operator blocking step"
        );
    }
    if !follow_up["authoritative_next_action"].is_null() {
        assert_eq!(
            follow_up["authoritative_next_action"], operator["recommended_command"],
            "{context} blocked follow-up authoritative next action must mirror workflow operator"
        );
    }
}

fn run_recommended_plan_execution_command_with_mode(
    repo: &Path,
    state: &Path,
    recommended_command: &str,
    real_cli: bool,
    context: &str,
) -> Value {
    let command_parts = recommended_command.split_whitespace().collect::<Vec<_>>();
    assert!(
        command_parts.len() >= 3,
        "{context} should expose a full featureforge command, got {recommended_command}"
    );
    assert_eq!(
        command_parts[0], "featureforge",
        "{context} recommended command must start with featureforge, got {recommended_command}"
    );
    if command_parts[1] == "plan" && command_parts[2] == "execution" {
        assert!(
            command_parts.len() >= 4,
            "{context} plan-execution command should include a subcommand, got {recommended_command}"
        );
        let command_args = if command_parts
            .get(3)
            .is_some_and(|command| *command == "close-current-task")
            && (recommended_command.contains("pass|fail") || recommended_command.contains("<path>"))
        {
            let plan = command_parts
                .windows(2)
                .find(|window| window[0] == "--plan")
                .map(|window| window[1])
                .expect("close-current-task template command should include --plan");
            let task = command_parts
                .windows(2)
                .find(|window| window[0] == "--task")
                .map(|window| window[1])
                .unwrap_or("1");
            let dispatch_id = command_parts
                .windows(2)
                .find(|window| window[0] == "--dispatch-id")
                .map(|window| window[1]);

            let review_summary_path = repo.join(
                "docs/featureforge/execution-evidence/runtime-remediation-close-current-task-review-summary.md",
            );
            let verification_summary_path = repo.join(
                "docs/featureforge/execution-evidence/runtime-remediation-close-current-task-verification-summary.md",
            );
            write_file(
                &review_summary_path,
                &format!(
                    "Close-current-task command generated from shared template for {context}.\n"
                ),
            );
            write_file(
                &verification_summary_path,
                &format!("Verification summary generated from shared template for {context}.\n"),
            );

            let mut args = vec![
                String::from("close-current-task"),
                String::from("--plan"),
                plan.to_owned(),
                String::from("--task"),
                task.to_owned(),
            ];
            if let Some(dispatch_id) = dispatch_id {
                args.push(String::from("--dispatch-id"));
                args.push(dispatch_id.to_owned());
            }
            args.extend([
                String::from("--review-result"),
                String::from("pass"),
                String::from("--review-summary-file"),
                review_summary_path
                    .to_str()
                    .expect("review summary path should stay utf-8")
                    .to_owned(),
                String::from("--verification-result"),
                String::from("pass"),
                String::from("--verification-summary-file"),
                verification_summary_path
                    .to_str()
                    .expect("verification summary path should stay utf-8")
                    .to_owned(),
            ]);
            args
        } else {
            command_parts[3..]
                .iter()
                .map(|part| (*part).to_owned())
                .collect::<Vec<_>>()
        };

        let command_args_refs = command_args.iter().map(String::as_str).collect::<Vec<_>>();
        if real_cli {
            return run_plan_execution_json_real_cli(repo, state, &command_args_refs, context);
        }
        return run_plan_execution_json(repo, state, &command_args_refs, context);
    }

    if command_parts[1] == "workflow" {
        let command_args = command_parts[1..]
            .iter()
            .map(|part| (*part).to_owned())
            .collect::<Vec<_>>();
        let command_args_refs = command_args.iter().map(String::as_str).collect::<Vec<_>>();
        let output = if real_cli {
            run_rust_featureforge_with_env_real_cli(repo, state, &command_args_refs, &[], context)
        } else {
            run_rust_featureforge_with_env(repo, state, &command_args_refs, &[], context)
        };
        return parse_json(&output, context);
    }

    panic!(
        "{context} recommended command must route through `plan execution` or `workflow`, got {recommended_command}"
    );
}

fn run_recommended_plan_execution_command(
    repo: &Path,
    state: &Path,
    recommended_command: &str,
    context: &str,
) -> Value {
    run_recommended_plan_execution_command_with_mode(
        repo,
        state,
        recommended_command,
        false,
        context,
    )
}

fn run_recommended_plan_execution_command_real_cli(
    repo: &Path,
    state: &Path,
    recommended_command: &str,
    context: &str,
) -> Value {
    run_recommended_plan_execution_command_with_mode(
        repo,
        state,
        recommended_command,
        true,
        context,
    )
}

fn assert_parity_probe_budget(scenario_id: &str, consumed_probe_commands: usize, max: usize) {
    assert!(
        consumed_probe_commands <= max,
        "scenario {scenario_id} exceeded parity-probe command target: consumed {consumed_probe_commands}, target {max}"
    );
}

fn normalize_workflow_status_snapshot(mut value: Value) -> Value {
    let object = value
        .as_object_mut()
        .expect("workflow status payload should stay a JSON object");
    object.remove("manifest_path");
    object.remove("root");
    value
}

fn write_manifest(path: &Path, manifest: &WorkflowManifest) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("manifest parent should be creatable");
    }
    let json = serde_json::to_string(manifest).expect("manifest json should serialize");
    fs::write(path, json).expect("manifest should be writable");
}

struct PlanFidelityReviewArtifactInput<'a> {
    artifact_rel: &'a str,
    plan_path: &'a str,
    plan_revision: u32,
    spec_path: &'a str,
    spec_revision: u32,
    review_verdict: &'a str,
    reviewer_source: &'a str,
    reviewer_id: &'a str,
    verified_surfaces: &'a [&'a str],
}

fn write_plan_fidelity_review_artifact(repo: &Path, input: PlanFidelityReviewArtifactInput<'_>) {
    let artifact_path = repo.join(input.artifact_rel);
    let plan_fingerprint =
        sha256_hex(&fs::read(repo.join(input.plan_path)).expect("plan should be readable"));
    let spec_fingerprint =
        sha256_hex(&fs::read(repo.join(input.spec_path)).expect("spec should be readable"));
    let verified_requirement_ids = parse_spec_file(repo.join(input.spec_path))
        .map(|spec| {
            spec.requirements
                .iter()
                .map(|requirement| requirement.id.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if let Some(parent) = artifact_path.parent() {
        fs::create_dir_all(parent).expect("review artifact parent should exist");
    }
    fs::write(
        artifact_path,
        format!(
            "## Plan Fidelity Review Summary\n\n**Review Stage:** featureforge:plan-fidelity-review\n**Review Verdict:** {review_verdict}\n**Reviewed Plan:** `{plan_path}`\n**Reviewed Plan Revision:** {plan_revision}\n**Reviewed Plan Fingerprint:** {plan_fingerprint}\n**Reviewed Spec:** `{spec_path}`\n**Reviewed Spec Revision:** {spec_revision}\n**Reviewed Spec Fingerprint:** {spec_fingerprint}\n**Reviewer Source:** {reviewer_source}\n**Reviewer ID:** {reviewer_id}\n**Distinct From Stages:** featureforge:writing-plans, featureforge:plan-eng-review\n**Verified Surfaces:** {}\n**Verified Requirement IDs:** {}\n",
            input.verified_surfaces.join(", "),
            verified_requirement_ids.join(", "),
            review_verdict = input.review_verdict,
            plan_path = input.plan_path,
            plan_revision = input.plan_revision,
            spec_path = input.spec_path,
            spec_revision = input.spec_revision,
            reviewer_source = input.reviewer_source,
            reviewer_id = input.reviewer_id,
        ),
    )
    .expect("plan-fidelity review artifact should write");
}

fn write_minimal_plan_fidelity_spec(repo: &Path, spec_path: &str) {
    write_file(
        &repo.join(spec_path),
        "# Approved Spec\n\n**Workflow State:** CEO Approved\n**Spec Revision:** 1\n**Last Reviewed By:** plan-ceo-review\n\n## Requirement Index\n\n- [REQ-001][behavior] The draft plan must complete an independent fidelity review before engineering review.\n",
    );
}

fn public_harness_phases_from_spec() -> Vec<String> {
    fn parse_phases(
        spec: &str,
        start_line: &str,
        end_predicate: impl Fn(&str) -> bool,
    ) -> BTreeSet<String> {
        spec.lines()
            .scan((false, false), |state, line| {
                let (in_phase_section, saw_entry) = state;
                let trimmed = line.trim();
                if trimmed == start_line {
                    *in_phase_section = true;
                    *saw_entry = false;
                    return Some(None);
                }
                if *in_phase_section && *saw_entry && end_predicate(trimmed) {
                    *in_phase_section = false;
                    return Some(None);
                }
                if *in_phase_section {
                    let parsed = trimmed
                        .strip_prefix("- `")
                        .and_then(|value| value.strip_suffix('`'))
                        .map(str::to_owned);
                    if parsed.is_some() {
                        *saw_entry = true;
                    }
                    return Some(parsed);
                }
                Some(None)
            })
            .flatten()
            .collect()
    }

    let mut phases = parse_phases(
        include_str!(
            "../docs/archive/featureforge/specs/2026-03-25-featureforge-execution-harness-spec.md"
        ),
        "### Public phase model",
        |trimmed| trimmed.starts_with("### "),
    );
    phases.extend(parse_phases(
        include_str!(
            "../docs/archive/featureforge/specs/2026-04-01-workflow-public-phase-contract.md"
        ),
        "The public `phase` should cover these operator moments:",
        |trimmed| trimmed.is_empty() || trimmed.starts_with("The public API shape"),
    ));
    phases.into_iter().collect()
}

fn public_harness_phase_from_spec(phase: &str) -> String {
    public_harness_phases_from_spec()
        .into_iter()
        .find(|candidate| candidate == phase)
        .unwrap_or_else(|| {
            panic!("spec should include `{phase}` in the public harness phase model section")
        })
}

fn init_repo(test_name: &str) -> (TempDir, TempDir) {
    let (repo_dir, state_dir) = init_workflow_repo(test_name);
    let repo_path = repo_dir.path();

    let mut git_remote_add = Command::new("git");
    git_remote_add
        .args([
            "remote",
            "add",
            "origin",
            &format!("git@github.com:example/{test_name}.git"),
        ])
        .current_dir(repo_path);
    run_checked(git_remote_add, "git remote add origin");

    (repo_dir, state_dir)
}

fn clear_directory(path: &Path) {
    if !path.exists() {
        fs::create_dir_all(path).expect("destination directory should be creatable");
        return;
    }
    for entry in fs::read_dir(path).expect("destination directory should be readable") {
        let entry = entry.expect("destination entry should be readable");
        let entry_path = entry.path();
        if entry
            .file_type()
            .expect("destination entry type should be readable")
            .is_dir()
        {
            fs::remove_dir_all(&entry_path)
                .unwrap_or_else(|error| panic!("failed to remove {:?}: {error}", entry_path));
        } else {
            fs::remove_file(&entry_path)
                .unwrap_or_else(|error| panic!("failed to remove {:?}: {error}", entry_path));
        }
    }
}

fn populate_execution_fixture_from_template(
    template: &WorkflowRuntimeExecutionFixtureTemplate,
    repo: &Path,
    state_dir: &Path,
) {
    clear_directory(repo);
    clear_directory(state_dir);
    copy_dir_recursive(&template.repo_root, repo);
    copy_dir_recursive(&template.state_root, state_dir);
}

fn workflow_runtime_execution_fixture_template(
    mode: WorkflowRuntimeFixtureQaMode,
) -> &'static WorkflowRuntimeExecutionFixtureTemplate {
    let store = match mode {
        WorkflowRuntimeFixtureQaMode::Required => &WORKFLOW_RUNTIME_EXECUTION_TEMPLATE_REQUIRED,
        WorkflowRuntimeFixtureQaMode::NotRequired => {
            &WORKFLOW_RUNTIME_EXECUTION_TEMPLATE_NOT_REQUIRED
        }
    };
    store.get_or_init(|| {
        let (repo_dir, state_dir) = init_repo(match mode {
            WorkflowRuntimeFixtureQaMode::Required => "workflow-runtime-template-required",
            WorkflowRuntimeFixtureQaMode::NotRequired => "workflow-runtime-template-not-required",
        });
        let repo = repo_dir.path();
        let state = state_dir.path();
        complete_workflow_fixture_execution_with_qa_requirement_slow(
            repo,
            state,
            FULL_CONTRACT_READY_PLAN_REL,
            match mode {
                WorkflowRuntimeFixtureQaMode::Required => "required",
                WorkflowRuntimeFixtureQaMode::NotRequired => "not-required",
            },
        );
        let template = WorkflowRuntimeExecutionFixtureTemplate {
            repo_root: repo.to_path_buf(),
            state_root: state.to_path_buf(),
        };
        std::mem::forget(repo_dir);
        std::mem::forget(state_dir);
        template
    })
}

fn inject_current_topology_sections(plan_source: &str) -> String {
    const INSERT_AFTER: &str = "## Requirement Coverage Matrix\n\n- REQ-001 -> Task 1\n- REQ-004 -> Task 1\n- VERIFY-001 -> Task 1\n";
    const TOPOLOGY_BLOCK: &str = "\n## Execution Strategy\n\n- Execute Task 1 last. It is the only task in this fixture and closes the execution graph for route-time workflow validation.\n\n## Dependency Diagram\n\n```text\nTask 1\n```\n";

    if plan_source.contains("## Execution Strategy")
        && plan_source.contains("## Dependency Diagram")
    {
        return plan_source.to_owned();
    }

    plan_source.replacen(INSERT_AFTER, &format!("{INSERT_AFTER}{TOPOLOGY_BLOCK}"), 1)
}

fn full_contract_ready_fixture_template() -> &'static FullContractReadyFixtureTemplate {
    FULL_CONTRACT_READY_FIXTURE_TEMPLATE.get_or_init(|| {
        let fixture_root = workflow_fixture_root();
        let plan_source =
            fs::read_to_string(fixture_root.join(FULL_CONTRACT_READY_PLAN_FIXTURE_REL))
                .expect("plan fixture should load");
        let plan = inject_current_topology_sections(&plan_source).replace(
            FULL_CONTRACT_READY_FIXTURE_SPEC_PATH,
            FULL_CONTRACT_READY_SPEC_REL,
        );

        FullContractReadyFixtureTemplate { plan }
    })
}

fn install_full_contract_ready_artifacts(repo: &Path) {
    let spec_rel = FULL_CONTRACT_READY_SPEC_REL;
    let plan_rel = FULL_CONTRACT_READY_PLAN_REL;
    let template = full_contract_ready_fixture_template();
    let spec_path = repo.join(spec_rel);
    let plan_path = repo.join(plan_rel);

    copy_workflow_fixture(FULL_CONTRACT_READY_SPEC_FIXTURE_REL, &spec_path);

    if let Some(parent) = plan_path.parent() {
        fs::create_dir_all(parent).expect("plan fixture parent should be creatable");
    }
    fs::write(&plan_path, &template.plan).expect("plan fixture should write");
}

#[test]
fn full_contract_ready_fixture_template_is_memoized_and_contract_ready() {
    let first = full_contract_ready_fixture_template();
    let second = full_contract_ready_fixture_template();

    assert!(
        std::ptr::eq(first, second),
        "fixture template should be memoized to avoid repeated fixture preparation IO"
    );
    assert!(
        first
            .plan
            .contains("docs/featureforge/specs/2026-03-22-runtime-integration-hardening-design.md"),
        "plan template should reference the repo-relative spec path"
    );
    assert!(
        !first.plan.contains(
            "tests/codex-runtime/fixtures/workflow-artifacts/specs/2026-03-22-runtime-integration-hardening-design.md"
        ),
        "plan template should not retain fixture-root absolute references"
    );
    assert!(
        first.plan.contains("## Execution Strategy")
            && first.plan.contains("## Dependency Diagram"),
        "plan template should include topology sections required by current contract fixtures"
    );
}

#[cfg(unix)]
fn create_dir_symlink(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).expect("directory symlink should be creatable");
}

#[cfg(windows)]
fn create_dir_symlink(target: &Path, link: &Path) {
    std::os::windows::fs::symlink_dir(target, link).expect("directory symlink should be creatable");
}

fn run_shell_status_helper(repo: &Path, state_dir: &Path, args: &[&str], context: &str) -> Output {
    if args.first().copied() == Some("status") {
        let direct_args = std::iter::once("workflow")
            .chain(args.iter().copied())
            .collect::<Vec<_>>();
        match workflow_direct_support::try_run_workflow_output_direct(
            repo,
            state_dir,
            &direct_args,
            context,
            true,
        ) {
            Ok(Some(output)) => return output,
            Ok(None) => {}
            Err(error) => panic!("{error}"),
        }
    }
    // Keep this helper on the real binary path because these tests assert the
    // shell-level failure contract, including out-of-repo routing and stderr framing.
    let mut command = Command::new(compiled_featureforge_path());
    command
        .current_dir(repo)
        .env("FEATUREFORGE_STATE_DIR", state_dir)
        .args(["workflow"])
        .args(args);
    run(command, context)
}

fn run_shell_status_json(repo: &Path, state_dir: &Path, args: &[&str], context: &str) -> Value {
    let output = run_shell_status_helper(repo, state_dir, args, context);
    parse_json(&output, context)
}

fn run_rust_featureforge(repo: &Path, state_dir: &Path, args: &[&str], context: &str) -> Output {
    if let Some(output) = try_direct_workflow_output(repo, state_dir, args, &[], context) {
        return output;
    }
    let mut command = Command::new(compiled_featureforge_path());
    command
        .current_dir(repo)
        .env("FEATUREFORGE_STATE_DIR", state_dir)
        .args(args);
    run(command, context)
}

fn run_workflow_plan_fidelity_json(
    repo: &Path,
    state_dir: &Path,
    args: &[&str],
    context: &str,
) -> Value {
    let direct_args = std::iter::once("workflow")
        .chain(std::iter::once("plan-fidelity"))
        .chain(args.iter().copied())
        .collect::<Vec<_>>();
    match workflow_direct_support::try_run_workflow_output_direct(
        repo,
        state_dir,
        &direct_args,
        context,
        true,
    ) {
        Ok(Some(output)) => return parse_json(&output, context),
        Ok(None) => {}
        Err(error) => panic!("{error}"),
    }
    let mut command = Command::new(compiled_featureforge_path());
    command
        .current_dir(repo)
        .env("FEATUREFORGE_STATE_DIR", state_dir)
        .args(["workflow", "plan-fidelity"])
        .args(args);
    parse_json(&run(command, context), context)
}

fn run_workflow_plan_fidelity_json_from_dir(
    current_dir: &Path,
    state_dir: &Path,
    args: &[&str],
    context: &str,
) -> Value {
    let direct_args = std::iter::once("workflow")
        .chain(std::iter::once("plan-fidelity"))
        .chain(args.iter().copied())
        .collect::<Vec<_>>();
    match workflow_direct_support::try_run_workflow_output_direct(
        current_dir,
        state_dir,
        &direct_args,
        context,
        true,
    ) {
        Ok(Some(output)) => return parse_json(&output, context),
        Ok(None) => {}
        Err(error) => panic!("{error}"),
    }
    let mut command = Command::new(compiled_featureforge_path());
    command
        .current_dir(current_dir)
        .env("FEATUREFORGE_STATE_DIR", state_dir)
        .args(["workflow", "plan-fidelity"])
        .args(args);
    parse_json(&run(command, context), context)
}

fn run_rust_featureforge_with_env(
    repo: &Path,
    state_dir: &Path,
    args: &[&str],
    extra_env: &[(&str, &str)],
    context: &str,
) -> Output {
    if let Some(output) = try_direct_workflow_output(repo, state_dir, args, extra_env, context) {
        return output;
    }
    let mut command = Command::new(compiled_featureforge_path());
    command
        .current_dir(repo)
        .env("FEATUREFORGE_STATE_DIR", state_dir)
        .args(args);
    for (key, value) in extra_env {
        command.env(key, value);
    }
    run(command, context)
}

fn run_rust_featureforge_with_env_real_cli(
    repo: &Path,
    state_dir: &Path,
    args: &[&str],
    extra_env: &[(&str, &str)],
    context: &str,
) -> Output {
    let mut command = Command::new(compiled_featureforge_path());
    command
        .current_dir(repo)
        .env("FEATUREFORGE_STATE_DIR", state_dir)
        .args(args);
    for (key, value) in extra_env {
        command.env(key, value);
    }
    run(command, context)
}

fn try_direct_workflow_output(
    repo: &Path,
    state_dir: &Path,
    args: &[&str],
    extra_env: &[(&str, &str)],
    context: &str,
) -> Option<Output> {
    if args.first().copied() != Some("workflow") || !workflow_runtime_env_is_direct_safe(extra_env)
    {
        // Keep all env-influenced workflow checks on the subprocess path so boundary tests keep
        // exercising the real CLI env wiring.
        return None;
    }
    match workflow_direct_support::try_run_workflow_output_direct(
        repo, state_dir, args, context, true,
    ) {
        Ok(Some(output)) => Some(output),
        Ok(None) => None,
        Err(error) => panic!("{error}"),
    }
}

fn workflow_runtime_env_is_direct_safe(extra_env: &[(&str, &str)]) -> bool {
    extra_env.iter().all(|(key, _)| {
        matches!(
            *key,
            "FEATUREFORGE_SESSION_KEY" | "FEATUREFORGE_WORKFLOW_REQUIRE_SESSION_ENTRY"
        )
    })
}

fn missing_null_fields(object: &Value, fields: &[&str]) -> Vec<String> {
    fields
        .iter()
        .copied()
        .filter(|field| !object.get(*field).is_some_and(Value::is_null))
        .map(str::to_owned)
        .collect()
}

fn set_remote_url(repo: &Path, url: &str) {
    let mut git_remote_set = Command::new("git");
    git_remote_set
        .args(["remote", "set-url", "origin", url])
        .current_dir(repo);
    run_checked(git_remote_set, "git remote set-url origin");
}

fn remove_origin_remote(repo: &Path) {
    let mut git_remote_remove = Command::new("git");
    git_remote_remove
        .args(["remote", "remove", "origin"])
        .current_dir(repo);
    run_checked(git_remote_remove, "git remote remove origin");
}

fn run_plan_execution_json(repo: &Path, state_dir: &Path, args: &[&str], context: &str) -> Value {
    if let ["explain-review-state", "--plan", plan_rel] = args {
        return run_internal_explain_review_state_json(repo, state_dir, plan_rel, context);
    }
    match plan_execution_direct_support::try_run_plan_execution_output_direct(
        repo, state_dir, args, context,
    ) {
        Ok(Some(output)) => return parse_json(&output, context),
        Ok(None) => {}
        Err(error) => panic!("{error}"),
    }
    let mut command = Command::new(compiled_featureforge_path());
    command
        .current_dir(repo)
        .env("FEATUREFORGE_STATE_DIR", state_dir)
        .args(["plan", "execution"])
        .args(args);
    parse_json(&run(command, context), context)
}

fn run_internal_explain_review_state_json(
    repo: &Path,
    state_dir: &Path,
    plan: &str,
    context: &str,
) -> Value {
    let args = PlanExecutionStatusArgs {
        plan: plan.into(),
        external_review_result_ready: false,
    };
    parse_direct_json_result(
        plan_execution_direct_support::run_internal_explain_review_state_json(
            repo, state_dir, &args,
        ),
        context,
    )
}

fn run_plan_execution_json_real_cli(
    repo: &Path,
    state_dir: &Path,
    args: &[&str],
    context: &str,
) -> Value {
    let mut command = Command::new(compiled_featureforge_path());
    command
        .current_dir(repo)
        .env("FEATUREFORGE_STATE_DIR", state_dir)
        .args(["plan", "execution"])
        .args(args);
    parse_json(&run(command, context), context)
}

fn run_internal_plan_execution_preflight_json(
    repo: &Path,
    state_dir: &Path,
    plan: &str,
    context: &str,
) -> Value {
    let args = PlanExecutionStatusArgs {
        plan: plan.into(),
        external_review_result_ready: false,
    };
    parse_direct_json_result(
        plan_execution_direct_support::run_runtime_preflight_gate_json(repo, state_dir, &args),
        context,
    )
}

fn run_internal_plan_execution_gate_review_json(
    repo: &Path,
    state_dir: &Path,
    plan: &str,
    context: &str,
) -> Value {
    let args = PlanExecutionStatusArgs {
        plan: plan.into(),
        external_review_result_ready: false,
    };
    parse_direct_json_result(
        plan_execution_direct_support::run_runtime_review_gate_json(repo, state_dir, &args),
        context,
    )
}

fn run_runtime_review_dispatch_authority_json(
    repo: &Path,
    state_dir: &Path,
    plan: &str,
    scope: ReviewDispatchScopeArg,
    task: Option<u32>,
    context: &str,
) -> Value {
    let args = RecordReviewDispatchArgs {
        plan: plan.into(),
        scope,
        task,
    };
    parse_direct_json_result(
        plan_execution_direct_support::run_runtime_review_dispatch_authority_json(
            repo, state_dir, &args,
        ),
        context,
    )
}

fn run_internal_reconcile_review_state_json(
    repo: &Path,
    state_dir: &Path,
    plan: &str,
    context: &str,
) -> Value {
    let args = PlanExecutionStatusArgs {
        plan: plan.into(),
        external_review_result_ready: false,
    };
    parse_direct_json_result(
        plan_execution_direct_support::run_internal_reconcile_review_state_json(
            repo, state_dir, &args,
        ),
        context,
    )
}

fn run_internal_plan_execution_output(result: Result<Value, String>) -> Output {
    match result {
        Ok(value) => value_to_json_output(value),
        Err(error) => output_with_code(1, Vec::new(), error.into_bytes()),
    }
}

fn run_workflow_preflight_json_direct(
    repo: &Path,
    state_dir: &Path,
    plan: &str,
    context: &str,
) -> Value {
    let runtime = discover_execution_runtime(repo, state_dir, context);
    let args = PlanExecutionStatusArgs {
        plan: plan.into(),
        external_review_result_ready: false,
    };
    to_value(
        runtime
            .preflight_gate(&args)
            .unwrap_or_else(|error| panic!("{context} should succeed: {error:?}")),
    )
    .unwrap_or_else(|error| panic!("{context} should serialize workflow preflight result: {error}"))
}

fn run_workflow_preflight_output(
    repo: &Path,
    state_dir: &Path,
    plan: &str,
    context: &str,
) -> Output {
    value_to_json_output(run_workflow_preflight_json_direct(
        repo, state_dir, plan, context,
    ))
}

fn run_workflow_gate_review_json_direct(
    repo: &Path,
    state_dir: &Path,
    plan: &str,
    context: &str,
) -> Value {
    let runtime = discover_execution_runtime(repo, state_dir, context);
    let args = PlanExecutionStatusArgs {
        plan: plan.into(),
        external_review_result_ready: false,
    };
    to_value(
        runtime
            .review_gate(&args)
            .unwrap_or_else(|error| panic!("{context} should succeed: {error:?}")),
    )
    .unwrap_or_else(|error| {
        panic!("{context} should serialize workflow gate-review result: {error}")
    })
}

fn run_workflow_gate_review_output(
    repo: &Path,
    state_dir: &Path,
    plan: &str,
    context: &str,
) -> Output {
    value_to_json_output(run_workflow_gate_review_json_direct(
        repo, state_dir, plan, context,
    ))
}

fn run_workflow_gate_finish_json_direct(
    repo: &Path,
    state_dir: &Path,
    plan: &str,
    context: &str,
) -> Value {
    let runtime = discover_execution_runtime(repo, state_dir, context);
    let args = PlanExecutionStatusArgs {
        plan: plan.into(),
        external_review_result_ready: false,
    };
    to_value(
        runtime
            .finish_gate(&args)
            .unwrap_or_else(|error| panic!("{context} should succeed: {error:?}")),
    )
    .unwrap_or_else(|error| {
        panic!("{context} should serialize workflow gate-finish result: {error}")
    })
}

fn run_workflow_gate_finish_output(
    repo: &Path,
    state_dir: &Path,
    plan: &str,
    context: &str,
) -> Output {
    value_to_json_output(run_workflow_gate_finish_json_direct(
        repo, state_dir, plan, context,
    ))
}

fn value_to_json_output(value: Value) -> Output {
    output_with_code(
        0,
        json_line(&value).expect("json should serialize"),
        Vec::new(),
    )
}

fn parse_direct_json_result(result: Result<Value, String>, context: &str) -> Value {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{context} should succeed: {error:?}"),
    }
}

fn output_with_code(code: i32, stdout: Vec<u8>, stderr: Vec<u8>) -> Output {
    Output {
        status: exit_status(code),
        stdout,
        stderr,
    }
}

fn json_line<T: Serialize>(value: &T) -> Result<Vec<u8>, serde_json::Error> {
    let mut encoded = serde_json::to_vec(value)?;
    encoded.push(b'\n');
    Ok(encoded)
}

#[cfg(unix)]
fn exit_status(code: i32) -> std::process::ExitStatus {
    use std::os::unix::process::ExitStatusExt;

    std::process::ExitStatus::from_raw(code << 8)
}

#[cfg(not(unix))]
fn exit_status(code: i32) -> std::process::ExitStatus {
    panic!("exit_status helper is only implemented for unix test targets, got code {code}");
}

fn current_branch_name(repo: &Path) -> String {
    discover_slug_identity(repo).branch_name
}

fn expected_release_base_branch(repo: &Path) -> String {
    let current_branch = current_branch_name(repo);
    resolve_release_base_branch(&repo.join(".git"), &current_branch).unwrap_or(current_branch)
}

fn current_head_sha(repo: &Path) -> String {
    runtime_current_head_sha(repo).expect("head sha should resolve")
}

fn current_head_tree_sha(repo: &Path) -> String {
    discover_repository(repo)
        .expect("head tree helper should discover repository")
        .head_tree_id_or_empty()
        .expect("head tree helper should resolve HEAD tree")
        .detach()
        .to_string()
}

fn repo_slug(repo: &Path) -> String {
    discover_slug_identity(repo).repo_slug
}

fn sha256_hex(contents: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(contents);
    format!("{:x}", hasher.finalize())
}

fn semantic_execution_context(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
) -> featureforge::execution::state::ExecutionContext {
    let runtime = discover_execution_runtime(
        repo,
        state_dir,
        "workflow_runtime semantic identity fixture",
    );
    load_execution_context(&runtime, Path::new(plan_rel))
        .expect("workflow_runtime semantic identity fixture should load execution context")
}

fn branch_contract_identity(repo: &Path, state_dir: &Path, plan_rel: &str) -> String {
    let context = semantic_execution_context(repo, state_dir, plan_rel);
    branch_definition_identity_for_context(&context)
}

fn task_contract_identity(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    task_number: u32,
) -> String {
    let context = semantic_execution_context(repo, state_dir, plan_rel);
    task_definition_identity_for_task(&context, task_number)
        .expect("workflow_runtime task semantic identity fixture should compute")
        .expect("workflow_runtime task semantic identity fixture should exist")
}

fn project_artifact_dir(repo: &Path, state_dir: &Path) -> PathBuf {
    state_dir.join("projects").join(repo_slug(repo))
}

fn preflight_acceptance_state_path(repo: &Path, state_dir: &Path) -> PathBuf {
    let branch = current_branch_name(repo);
    let safe_branch = branch_storage_key(&branch);
    state_dir
        .join("projects")
        .join(repo_slug(repo))
        .join("branches")
        .join(safe_branch)
        .join("execution-preflight")
        .join("acceptance-state.json")
}

fn write_branch_test_plan_artifact(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    browser_required: &str,
) -> PathBuf {
    let branch = current_branch_name(repo);
    let safe_branch = branch_storage_key(&branch);
    let head_sha = current_head_sha(repo);
    let artifact_path = project_artifact_dir(repo, state_dir)
        .join(format!("tester-{safe_branch}-test-plan-20260324-120000.md"));
    let source = format!(
        "# Test Plan\n**Source Plan:** `{plan_rel}`\n**Source Plan Revision:** 1\n**Branch:** {branch}\n**Repo:** {}\n**Head SHA:** {head_sha}\n**Browser QA Required:** {browser_required}\n**Generated By:** featureforge:plan-eng-review\n**Generated At:** 2026-03-24T12:00:00Z\n\n## Affected Pages / Routes\n- none\n\n## Key Interactions\n- late-stage workflow routing uses this artifact for QA scoping\n\n## Edge Cases\n- current-branch artifact freshness must stay aligned with the approved plan revision\n\n## Critical Paths\n- branch completion stays blocked until review, QA, and release-readiness artifacts are fresh when required\n",
        repo_slug(repo)
    );
    write_file(&artifact_path, &source);
    let authoritative_path = harness_authoritative_artifact_path(
        state_dir,
        &repo_slug(repo),
        &branch,
        &format!("test-plan-{}.md", sha256_hex(source.as_bytes())),
    );
    write_file(&authoritative_path, &source);
    artifact_path
}

fn write_branch_review_artifact(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    base_branch: &str,
) -> PathBuf {
    let branch = current_branch_name(repo);
    let safe_branch = branch_storage_key(&branch);
    let strategy_checkpoint_fingerprint = run_plan_execution_json(
        repo,
        state_dir,
        &["status", "--plan", plan_rel],
        "plan execution status for workflow review artifact fixture",
    )["last_strategy_checkpoint_fingerprint"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(FIXTURE_STRATEGY_CHECKPOINT_FINGERPRINT)
        .to_owned();
    let reviewer_artifact_path = project_artifact_dir(repo, state_dir).join(format!(
        "tester-{safe_branch}-independent-review-20260324-120950.md"
    ));
    let reviewer_artifact_source = format!(
        "# Code Review Result\n**Review Stage:** featureforge:requesting-code-review\n**Reviewer Provenance:** dedicated-independent\n**Reviewer Source:** fresh-context-subagent\n**Reviewer ID:** reviewer-fixture-001\n**Strategy Checkpoint Fingerprint:** {strategy_checkpoint_fingerprint}\n**Distinct From Stages:** featureforge:executing-plans, featureforge:subagent-driven-development\n**Recorded Execution Deviations:** none\n**Deviation Review Verdict:** not_required\n**Source Plan:** `{plan_rel}`\n**Source Plan Revision:** 1\n**Branch:** {branch}\n**Repo:** {}\n**Base Branch:** {base_branch}\n**Head SHA:** {}\n**Result:** pass\n**Generated By:** featureforge:requesting-code-review\n**Generated At:** 2026-03-24T12:09:50Z\n\n## Summary\n- dedicated independent reviewer artifact fixture.\n",
        repo_slug(repo),
        current_head_sha(repo)
    );
    write_file(&reviewer_artifact_path, &reviewer_artifact_source);
    let reviewer_artifact_fingerprint =
        sha256_hex(&fs::read(&reviewer_artifact_path).expect("reviewer artifact should read"));
    let artifact_path = project_artifact_dir(repo, state_dir).join(format!(
        "tester-{safe_branch}-code-review-20260324-121000.md"
    ));
    write_file(
        &artifact_path,
        &format!(
            "# Code Review Result\n**Review Stage:** featureforge:requesting-code-review\n**Reviewer Provenance:** dedicated-independent\n**Reviewer Source:** fresh-context-subagent\n**Reviewer ID:** reviewer-fixture-001\n**Strategy Checkpoint Fingerprint:** {strategy_checkpoint_fingerprint}\n**Reviewer Artifact Path:** `{}`\n**Reviewer Artifact Fingerprint:** {reviewer_artifact_fingerprint}\n**Distinct From Stages:** featureforge:executing-plans, featureforge:subagent-driven-development\n**Source Plan:** `{plan_rel}`\n**Source Plan Revision:** 1\n**Branch:** {branch}\n**Repo:** {}\n**Base Branch:** {base_branch}\n**Head SHA:** {}\n**Recorded Execution Deviations:** none\n**Deviation Review Verdict:** not_required\n**Result:** pass\n**Generated By:** featureforge:requesting-code-review\n**Generated At:** 2026-03-24T12:10:00Z\n\n## Summary\n- synthetic code-review fixture for workflow phase coverage.\n",
            reviewer_artifact_path.display(),
            repo_slug(repo),
            current_head_sha(repo)
        ),
    );
    artifact_path
}

fn write_branch_release_artifact(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    base_branch: &str,
) -> PathBuf {
    let branch = current_branch_name(repo);
    let safe_branch = branch_storage_key(&branch);
    let artifact_path = project_artifact_dir(repo, state_dir).join(format!(
        "tester-{safe_branch}-release-readiness-20260324-121500.md"
    ));
    write_file(
        &artifact_path,
        &format!(
            "# Release Readiness Result\n**Source Plan:** `{plan_rel}`\n**Source Plan Revision:** 1\n**Branch:** {branch}\n**Repo:** {}\n**Base Branch:** {base_branch}\n**Head SHA:** {}\n**Result:** pass\n**Generated By:** featureforge:document-release\n**Generated At:** 2026-03-24T12:15:00Z\n\n## Summary\n- synthetic release-readiness fixture for workflow phase coverage.\n",
            repo_slug(repo),
            current_head_sha(repo)
        ),
    );
    publish_authoritative_release_truth(repo, state_dir, plan_rel, &artifact_path, base_branch);
    artifact_path
}

fn write_branch_qa_artifact(
    repo: &Path,
    state_dir: &Path,
    plan_rel: &str,
    test_plan_path: &Path,
) -> PathBuf {
    let branch = current_branch_name(repo);
    let safe_branch = branch_storage_key(&branch);
    let artifact_path = project_artifact_dir(repo, state_dir).join(format!(
        "tester-{safe_branch}-test-outcome-20260324-121200.md"
    ));
    write_file(
        &artifact_path,
        &format!(
            "# QA Result\n**Source Plan:** `{plan_rel}`\n**Source Plan Revision:** 1\n**Source Test Plan:** `{}`\n**Branch:** {branch}\n**Repo:** {}\n**Head SHA:** {}\n**Result:** pass\n**Generated By:** featureforge/qa\n**Generated At:** 2026-03-24T12:12:00Z\n\n## Summary\n- synthetic QA fixture for workflow late-gate downstream provenance coverage.\n",
            test_plan_path.display(),
            repo_slug(repo),
            current_head_sha(repo)
        ),
    );
    artifact_path
}

fn replace_in_file(path: &Path, from: &str, to: &str) {
    let source = fs::read_to_string(path).expect("fixture file should be readable for mutation");
    let updated = source.replace(from, to);
    assert_ne!(
        source,
        updated,
        "fixture mutation should change the file contents for {}",
        path.display()
    );
    fs::write(path, updated).expect("fixture file should be writable for mutation");
}

fn insert_step_with_execution_note_after_step(
    path: &Path,
    after_step: u32,
    occurrence: usize,
    step: u32,
    title: &str,
    note: &str,
) {
    assert!(
        occurrence > 0,
        "step note insertion occurrence must be at least 1"
    );
    let source = fs::read_to_string(path).expect("fixture file should be readable for note insert");
    let lines = source.lines().collect::<Vec<_>>();
    let unchecked_marker = format!("- [ ] **Step {after_step}:");
    let checked_marker = format!("- [x] **Step {after_step}:");
    let mut seen = 0usize;
    let mut inserted = false;
    let mut updated_lines = Vec::new();
    let mut cursor = 0usize;
    while cursor < lines.len() {
        let line = lines[cursor];
        updated_lines.push(line.to_owned());
        if !inserted && (line.starts_with(&unchecked_marker) || line.starts_with(&checked_marker)) {
            seen += 1;
            if seen == occurrence {
                let mut note_cursor = cursor + 1;
                while note_cursor < lines.len() && lines[note_cursor].is_empty() {
                    updated_lines.push(lines[note_cursor].to_owned());
                    note_cursor += 1;
                }
                if note_cursor < lines.len()
                    && lines[note_cursor]
                        .trim_start()
                        .starts_with("**Execution Note:** ")
                {
                    updated_lines.push(lines[note_cursor].to_owned());
                    cursor = note_cursor;
                }
                updated_lines.push(format!("- [ ] **Step {step}: {title}**"));
                updated_lines.push(format!("  **Execution Note:** {note}"));
                inserted = true;
            }
        }
        cursor += 1;
    }
    assert!(
        inserted,
        "fixture note insertion should find Step {after_step} occurrence {occurrence} in {}",
        path.display()
    );
    let updated = format!("{}\n", updated_lines.join("\n"));
    fs::write(path, updated).expect("fixture file should be writable for note insert");
}

fn set_plan_qa_requirement(repo: &Path, plan_rel: &str, qa_requirement: &str) {
    let plan_path = repo.join(plan_rel);
    let source = fs::read_to_string(&plan_path).expect("fixture plan should be readable");
    let updated = if let Some(current_line) = source
        .lines()
        .find(|line| line.starts_with("**QA Requirement:**"))
    {
        source.replace(
            current_line,
            &format!("**QA Requirement:** {qa_requirement}"),
        )
    } else {
        source.replace(
            "**Last Reviewed By:** plan-eng-review\n",
            &format!(
                "**Last Reviewed By:** plan-eng-review\n**QA Requirement:** {qa_requirement}\n"
            ),
        )
    };
    assert_ne!(
        source, updated,
        "plan QA requirement rewrite should change the fixture contents"
    );
    fs::write(&plan_path, updated).expect("fixture plan should be writable");
}

fn rewrite_source_test_plan_header(source: &str, source_test_plan: &Path) -> String {
    let replacement = format!("**Source Test Plan:** `{}`", source_test_plan.display());
    let mut replaced = false;
    let rewritten = source
        .lines()
        .map(|line| {
            if line.trim().starts_with("**Source Test Plan:**") {
                replaced = true;
                replacement.clone()
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        replaced,
        "QA artifact should include a Source Test Plan header"
    );
    format!("{rewritten}\n")
}

fn remove_source_test_plan_header(source: &str) -> String {
    let mut removed = false;
    let rewritten = source
        .lines()
        .filter(|line| {
            let keep = !line.trim().starts_with("**Source Test Plan:**");
            if !keep {
                removed = true;
            }
            keep
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        removed,
        "QA artifact should include a Source Test Plan header"
    );
    format!("{rewritten}\n")
}

fn blank_source_test_plan_header(source: &str) -> String {
    let mut replaced = false;
    let rewritten = source
        .lines()
        .map(|line| {
            if line.trim().starts_with("**Source Test Plan:**") {
                replaced = true;
                String::from("**Source Test Plan:** ``")
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        replaced,
        "QA artifact should include a Source Test Plan header"
    );
    format!("{rewritten}\n")
}

fn prepare_preflight_acceptance_workspace(repo: &Path, branch_name: &str) {
    let mut checkout = Command::new("git");
    checkout
        .args(["checkout", "-B", branch_name])
        .current_dir(repo);
    run_checked(checkout, "git checkout preflight acceptance branch");
}

fn complete_workflow_fixture_execution(repo: &Path, state: &Path, plan_rel: &str) {
    complete_workflow_fixture_execution_with_qa_requirement(repo, state, plan_rel, "not-required");
}

fn complete_workflow_fixture_execution_with_qa_requirement(
    repo: &Path,
    state: &Path,
    plan_rel: &str,
    qa_requirement: &str,
) {
    if plan_rel == FULL_CONTRACT_READY_PLAN_REL {
        let mode = match qa_requirement {
            "required" => Some(WorkflowRuntimeFixtureQaMode::Required),
            "not-required" => Some(WorkflowRuntimeFixtureQaMode::NotRequired),
            _ => None,
        };
        if let Some(mode) = mode {
            populate_execution_fixture_from_template(
                workflow_runtime_execution_fixture_template(mode),
                repo,
                state,
            );
            return;
        }
    }
    complete_workflow_fixture_execution_with_qa_requirement_slow(
        repo,
        state,
        plan_rel,
        qa_requirement,
    );
}

fn complete_workflow_fixture_execution_with_qa_requirement_slow(
    repo: &Path,
    state: &Path,
    plan_rel: &str,
    qa_requirement: &str,
) {
    install_full_contract_ready_artifacts(repo);
    set_plan_qa_requirement(repo, plan_rel, qa_requirement);
    write_file(
        &repo.join("tests/workflow_runtime.rs"),
        "synthetic route proof\n",
    );
    prepare_preflight_acceptance_workspace(repo, "workflow-runtime-fixture");

    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status before workflow routing fixture",
    );
    let preflight_json = run_internal_plan_execution_preflight_json(
        repo,
        state,
        plan_rel,
        "plan execution preflight before workflow routing fixture",
    );
    assert_eq!(preflight_json["allowed"], true);
    let begin_json = run_plan_execution_json(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_json["execution_fingerprint"]
                .as_str()
                .expect("status fingerprint should be present"),
        ],
        "plan execution begin for workflow routing fixture",
    );
    run_plan_execution_json(
        repo,
        state,
        &[
            "complete",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "Completed the routing fixture task.",
            "--manual-verify-summary",
            "Verified by workflow runtime fixture setup.",
            "--file",
            "tests/workflow_runtime.rs",
            "--expect-execution-fingerprint",
            begin_json["execution_fingerprint"]
                .as_str()
                .expect("begin fingerprint should be present"),
        ],
        "plan execution complete for workflow routing fixture",
    );
    seed_current_branch_closure_truth(repo, state, plan_rel, 1);
}

fn update_authoritative_harness_state(
    repo: &Path,
    state: &Path,
    branch: &str,
    plan_rel: &str,
    plan_revision: u32,
    updates: &[(&str, Value)],
) {
    let repo_slug = repo_slug(repo);
    let authoritative_state_path = harness_state_path(state, &repo_slug, branch);
    let mut payload: Value = match fs::read_to_string(&authoritative_state_path) {
        Ok(source) => serde_json::from_str(&source)
            .expect("authoritative harness state should stay valid json"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let status_json = run_plan_execution_json(
                repo,
                state,
                &["status", "--plan", plan_rel],
                "status for synthesized authoritative harness state",
            );
            let execution_run_id = status_json["execution_run_id"]
                .as_str()
                .expect("status should expose execution_run_id for synthesized authoritative state")
                .to_string();
            let chunk_id = status_json["chunk_id"].as_str().map(str::to_owned);
            let mut object = serde_json::Map::new();
            object.insert("schema_version".to_string(), Value::from(1));
            object.insert(
                "run_identity".to_string(),
                Value::Object(serde_json::Map::from_iter([
                    (
                        "execution_run_id".to_string(),
                        Value::from(execution_run_id),
                    ),
                    ("source_plan_path".to_string(), Value::from(plan_rel)),
                    (
                        "source_plan_revision".to_string(),
                        Value::from(plan_revision),
                    ),
                ])),
            );
            if let Some(chunk_id) = chunk_id {
                object.insert("chunk_id".to_string(), Value::from(chunk_id));
            }
            object.insert(
                "active_worktree_lease_fingerprints".to_string(),
                Value::Array(Vec::new()),
            );
            object.insert(
                "active_worktree_lease_bindings".to_string(),
                Value::Array(Vec::new()),
            );
            Value::Object(object)
        }
        Err(error) => {
            panic!("authoritative harness state should be readable for fixture mutation: {error}")
        }
    };
    let object = payload
        .as_object_mut()
        .expect("authoritative harness state should remain a json object");
    object
        .entry("strategy_state".to_string())
        .or_insert_with(|| Value::from("ready"));
    object
        .entry("strategy_checkpoint_kind".to_string())
        .or_insert_with(|| Value::from("initial_dispatch"));
    object
        .entry("last_strategy_checkpoint_fingerprint".to_string())
        .or_insert_with(|| Value::from(FIXTURE_STRATEGY_CHECKPOINT_FINGERPRINT));
    object
        .entry("strategy_reset_required".to_string())
        .or_insert_with(|| Value::Bool(false));
    for (key, value) in updates {
        object.insert((*key).to_string(), value.clone());
    }
    write_file(
        &authoritative_state_path,
        &serde_json::to_string(&payload).expect("authoritative harness state should serialize"),
    );
    featureforge::execution::event_log::sync_fixture_event_log_for_tests(
        &authoritative_state_path,
        &payload,
    )
    .expect("authoritative workflow-runtime fixture update should sync typed event authority");
}

fn update_current_history_record_field(
    repo: &Path,
    state: &Path,
    history_field: &str,
    current_id_field: &str,
    record_field: &str,
    value: Value,
) {
    let branch = current_branch_name(repo);
    let repo_slug = repo_slug(repo);
    let state_path = harness_state_path(state, &repo_slug, &branch);
    let source =
        fs::read_to_string(&state_path).expect("authoritative harness state should be readable");
    let mut payload: Value =
        serde_json::from_str(&source).expect("authoritative harness state should be valid json");
    let root = payload
        .as_object_mut()
        .expect("authoritative harness state should remain an object");
    let current_record_id = root
        .get(current_id_field)
        .and_then(Value::as_str)
        .filter(|record_id| !record_id.trim().is_empty())
        .expect("current authoritative record id should be present")
        .to_owned();
    let record = root
        .get_mut(history_field)
        .and_then(Value::as_object_mut)
        .and_then(|history| history.get_mut(&current_record_id))
        .and_then(Value::as_object_mut)
        .expect("current authoritative history record should be present");
    record.insert(record_field.to_owned(), value);
    write_file(
        &state_path,
        &serde_json::to_string(&payload).expect("authoritative harness state should serialize"),
    );
    featureforge::execution::event_log::sync_fixture_event_log_for_tests(&state_path, &payload)
        .expect("authoritative workflow-runtime history update should sync typed event authority");
}

fn current_release_readiness_record_id(repo: &Path, state: &Path) -> Option<String> {
    let branch = current_branch_name(repo);
    let state_path = harness_state_path(state, &repo_slug(repo), &branch);
    let source = fs::read_to_string(&state_path).ok()?;
    let payload: Value = serde_json::from_str(&source).ok()?;
    payload
        .get("current_release_readiness_record_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn current_final_review_record_id(repo: &Path, state: &Path) -> Option<String> {
    let branch = current_branch_name(repo);
    let state_path = harness_state_path(state, &repo_slug(repo), &branch);
    let source = fs::read_to_string(&state_path).ok()?;
    let payload: Value = serde_json::from_str(&source).ok()?;
    payload
        .get("current_final_review_record_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn seed_current_branch_closure_truth(
    repo: &Path,
    state: &Path,
    plan_rel: &str,
    plan_revision: u32,
) {
    let branch = current_branch_name(repo);
    let base_branch = expected_release_base_branch(repo);
    let reviewed_state_id = format!("git_tree:{}", current_head_tree_sha(repo));
    let execution_run_id = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "workflow runtime status for current task-closure fixture provenance",
    )["execution_run_id"]
        .as_str()
        .expect("status should expose execution_run_id for current task-closure fixtures")
        .to_owned();
    let branch_contract_identity = branch_contract_identity(repo, state, plan_rel);
    let task_contract_identity = task_contract_identity(repo, state, plan_rel, 1);
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        plan_revision,
        &[
            ("dependency_index_state", Value::from("fresh")),
            (
                "current_branch_closure_id",
                Value::from("branch-release-closure"),
            ),
            (
                "current_branch_closure_reviewed_state_id",
                Value::from(reviewed_state_id.clone()),
            ),
            (
                "current_branch_closure_contract_identity",
                Value::from(branch_contract_identity.clone()),
            ),
            (
                "branch_closure_records",
                json!({
                    "branch-release-closure": {
                        "branch_closure_id": "branch-release-closure",
                        "source_plan_path": plan_rel,
                        "source_plan_revision": plan_revision,
                        "repo_slug": repo_slug(repo),
                        "branch_name": branch.clone(),
                        "base_branch": base_branch,
                        "reviewed_state_id": reviewed_state_id,
                        "contract_identity": branch_contract_identity,
                        "effective_reviewed_branch_surface": "repo_tracked_content",
                        "source_task_closure_ids": ["task-1-closure"],
                        "provenance_basis": "task_closure_lineage",
                        "closure_status": "current",
                        "superseded_branch_closure_ids": []
                    }
                }),
            ),
            (
                "current_task_closure_records",
                json!({
                    "task-1": {
                        "dispatch_id": "fixture-task-dispatch",
                        "closure_record_id": "task-1-closure",
                        "source_plan_path": plan_rel,
                        "source_plan_revision": plan_revision,
                        "execution_run_id": execution_run_id.clone(),
                        "reviewed_state_id": reviewed_state_id.clone(),
                        "contract_identity": task_contract_identity.clone(),
                        "effective_reviewed_surface_paths": ["README.md"],
                        "review_result": "pass",
                        "review_summary_hash": sha256_hex(b"workflow runtime task closure review fixture"),
                        "verification_result": "pass",
                        "verification_summary_hash": sha256_hex(b"workflow runtime task closure verification fixture"),
                        "closure_status": "current"
                    }
                }),
            ),
            (
                "task_closure_record_history",
                json!({
                    "task-1-closure": {
                        "dispatch_id": "fixture-task-dispatch",
                        "closure_record_id": "task-1-closure",
                        "source_plan_path": plan_rel,
                        "source_plan_revision": plan_revision,
                        "execution_run_id": execution_run_id,
                        "reviewed_state_id": reviewed_state_id.clone(),
                        "contract_identity": task_contract_identity,
                        "effective_reviewed_surface_paths": ["README.md"],
                        "review_result": "pass",
                        "review_summary_hash": sha256_hex(b"workflow runtime task closure review fixture"),
                        "verification_result": "pass",
                        "verification_summary_hash": sha256_hex(b"workflow runtime task closure verification fixture"),
                        "closure_status": "current"
                    }
                }),
            ),
        ],
    );
}

fn write_runtime_remediation_fs15_plan(repo: &Path, plan_rel: &str, spec_rel: &str) {
    let source = format!(
        r#"# Runtime Remediation FS-15 Plan

**Workflow State:** Engineering Approved
**Plan Revision:** 1
**Execution Mode:** featureforge:executing-plans
**Source Spec:** `{spec_rel}`
**Source Spec Revision:** 1
**Last Reviewed By:** plan-eng-review

## Requirement Coverage Matrix

- REQ-001 -> Task 1, Task 2, Task 6
- VERIFY-001 -> Task 1, Task 2, Task 6

## Execution Strategy

- Repair Task 1 through the public task-closure route before resuming forward work.
- Execute Task 2 before Task 6 to keep stale-boundary ordering deterministic for FS-15 routing checks.

## Dependency Diagram

```text
Task 1 -> Task 2 -> Task 6
```

## Task 1: FS-15 earlier repair target

**Spec Coverage:** REQ-001, VERIFY-001
**Goal:** Recreates the earlier repair transition before stale-boundary targeting continues.

**Context:**
- Spec Coverage: REQ-001, VERIFY-001.

**Constraints:**
- Keep each task to one step for deterministic stale-target routing assertions.

**Done when:**
- Recreates the earlier repair transition before stale-boundary targeting continues.

**Files:**
- Modify: `tests/workflow_runtime.rs`

- [ ] **Step 1: Repair task 1 through the public closure route**

## Task 2: FS-15 earliest stale boundary

**Spec Coverage:** REQ-001, VERIFY-001
**Goal:** Task 2 represents the earliest unresolved stale boundary.

**Context:**
- Spec Coverage: REQ-001, VERIFY-001.

**Constraints:**
- Keep each task to one step for deterministic stale-target routing assertions.

**Done when:**
- Task 2 represents the earliest unresolved stale boundary.

**Files:**
- Modify: `tests/workflow_runtime.rs`

- [ ] **Step 1: Execute task 2 baseline step**

## Task 6: FS-15 later stale overlay target

**Spec Coverage:** REQ-001, VERIFY-001
**Goal:** Task 6 represents the later stale overlay that must not outrank Task 2.

**Context:**
- Spec Coverage: REQ-001, VERIFY-001.

**Constraints:**
- Keep each task to one step for deterministic stale-target routing assertions.

**Done when:**
- Task 6 represents the later stale overlay that must not outrank Task 2.

**Files:**
- Modify: `tests/workflow_runtime.rs`

- [ ] **Step 1: Execute task 6 baseline step**
"#
    );
    write_file(&repo.join(plan_rel), &source);
}

fn write_runtime_remediation_fs11_plan(repo: &Path, plan_rel: &str, spec_rel: &str) {
    let source = format!(
        r#"# Runtime Remediation FS-11 Plan

**Workflow State:** Engineering Approved
**Plan Revision:** 1
**Execution Mode:** featureforge:executing-plans
**Source Spec:** `{spec_rel}`
**Source Spec Revision:** 1
**Last Reviewed By:** plan-eng-review

## Requirement Coverage Matrix

- REQ-001 -> Task 2, Task 3
- VERIFY-001 -> Task 2, Task 3

## Execution Strategy

- Seed a forward resume overlay on Task 3 Step 6 while keeping Task 2 as the earliest stale boundary.

## Dependency Diagram

```text
Task 2 -> Task 3
```

## Task 2: Earliest stale boundary task

**Spec Coverage:** REQ-001, VERIFY-001
**Goal:** Task 2 remains the earliest unresolved stale boundary.

**Context:**
- Spec Coverage: REQ-001, VERIFY-001.

**Constraints:**
- Keep one step for deterministic stale-boundary reopen targeting.

**Done when:**
- Task 2 remains the earliest unresolved stale boundary.

**Files:**
- Modify: `tests/workflow_runtime.rs`

- [ ] **Step 1: Execute task 2 baseline step**

## Task 3: Forward resume overlay task

**Spec Coverage:** REQ-001, VERIFY-001
**Goal:** Task 3 Step 6 is the forward overlay target that must never outrank Task 2.

**Context:**
- Spec Coverage: REQ-001, VERIFY-001.

**Constraints:**
- Keep six steps to preserve the exact Task 3 Step 6 contradiction shape.

**Done when:**
- Task 3 Step 6 is the forward overlay target that must never outrank Task 2.

**Files:**
- Modify: `tests/workflow_runtime.rs`

- [ ] **Step 1: Build Task 3 step scaffold**
- [ ] **Step 2: Build Task 3 step scaffold**
- [ ] **Step 3: Build Task 3 step scaffold**
- [ ] **Step 4: Build Task 3 step scaffold**
- [ ] **Step 5: Build Task 3 step scaffold**
- [ ] **Step 6: Build Task 3 step scaffold**
"#
    );
    write_file(&repo.join(plan_rel), &source);
}

fn write_runtime_fs14_fs16_task_boundary_plan(repo: &Path, plan_rel: &str, spec_rel: &str) {
    let source = format!(
        r#"# Runtime Remediation FS-14/FS-16 Task-Boundary Plan

**Workflow State:** Engineering Approved
**Plan Revision:** 1
**Execution Mode:** none
**Source Spec:** `{spec_rel}`
**Source Spec Revision:** 1
**Last Reviewed By:** plan-eng-review

## Requirement Coverage Matrix

- REQ-001 -> Task 1
- REQ-004 -> Task 1
- VERIFY-001 -> Task 2

## Execution Strategy

- Execute Task 1 serially. It establishes boundary gating before follow-on work begins.
- Execute Task 2 serially after Task 1. It validates task-boundary workflow routing.

## Dependency Diagram

```text
Task 1 -> Task 2
```

## Task 1: Core flow

**Spec Coverage:** REQ-001, REQ-004
**Goal:** Task 1 execution reaches a boundary gate before Task 2 starts.

**Context:**
- Spec Coverage: REQ-001, REQ-004.

**Constraints:**
- Keep fixture inputs deterministic.

**Done when:**
- Task 1 execution reaches a boundary gate before Task 2 starts.

**Files:**
- Modify: `tests/workflow_runtime.rs`

- [ ] **Step 1: Prepare workflow fixture output**
- [ ] **Step 2: Validate workflow fixture output**

## Task 2: Follow-on flow

**Spec Coverage:** VERIFY-001
**Goal:** Task 2 should remain blocked until Task 1 closure requirements are met.

**Context:**
- Spec Coverage: VERIFY-001.

**Constraints:**
- Preserve deterministic task-boundary diagnostics.

**Done when:**
- Task 2 should remain blocked until Task 1 closure requirements are met.

**Files:**
- Modify: `tests/workflow_runtime.rs`

- [ ] **Step 1: Start the follow-on task**
"#
    );
    write_file(&repo.join(plan_rel), &source);
}

fn setup_runtime_fs14_fs16_task_boundary_fixture(
    repo: &Path,
    state: &Path,
    plan_rel: &str,
) -> String {
    install_full_contract_ready_artifacts(repo);
    write_runtime_fs14_fs16_task_boundary_plan(repo, plan_rel, FULL_CONTRACT_READY_SPEC_REL);
    prepare_preflight_acceptance_workspace(repo, "runtime-remediation-fs14-fs16-task-boundary");

    let status_before_begin = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-14/FS-16 status before task-boundary fixture begin",
    );
    let preflight = run_internal_plan_execution_preflight_json(
        repo,
        state,
        plan_rel,
        "FS-14/FS-16 preflight before task-boundary fixture execution",
    );
    assert_eq!(
        preflight["allowed"],
        Value::Bool(true),
        "FS-14/FS-16 fixture preflight should allow execution",
    );
    let begin_task1_step1 = run_plan_execution_json(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_before_begin["execution_fingerprint"]
                .as_str()
                .expect("FS-14/FS-16 status should expose execution fingerprint before begin"),
        ],
        "FS-14/FS-16 begin task 1 step 1 fixture bootstrap",
    );
    let complete_task1_step1 = run_plan_execution_json(
        repo,
        state,
        &[
            "complete",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "FS-14/FS-16 completed task 1 step 1 fixture bootstrap.",
            "--manual-verify-summary",
            "FS-14/FS-16 fixture verification summary for task 1 step 1.",
            "--file",
            "tests/workflow_runtime.rs",
            "--expect-execution-fingerprint",
            begin_task1_step1["execution_fingerprint"]
                .as_str()
                .expect("FS-14/FS-16 begin should expose execution fingerprint"),
        ],
        "FS-14/FS-16 complete task 1 step 1 fixture bootstrap",
    );
    let begin_task1_step2 = run_plan_execution_json(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "2",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            complete_task1_step1["execution_fingerprint"]
                .as_str()
                .expect("FS-14/FS-16 complete should expose execution fingerprint"),
        ],
        "FS-14/FS-16 begin task 1 step 2 fixture bootstrap",
    );
    run_plan_execution_json(
        repo,
        state,
        &[
            "complete",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "2",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "FS-14/FS-16 completed task 1 step 2 fixture bootstrap.",
            "--manual-verify-summary",
            "FS-14/FS-16 fixture verification summary for task 1 step 2.",
            "--file",
            "tests/workflow_runtime.rs",
            "--expect-execution-fingerprint",
            begin_task1_step2["execution_fingerprint"]
                .as_str()
                .expect("FS-14/FS-16 begin should expose execution fingerprint"),
        ],
        "FS-14/FS-16 complete task 1 step 2 fixture bootstrap",
    );
    let branch = current_branch_name(repo);
    update_authoritative_harness_state(repo, state, &branch, plan_rel, 1, &[]);
    let dispatch = run_runtime_review_dispatch_authority_json(
        repo,
        state,
        plan_rel,
        ReviewDispatchScopeArg::Task,
        Some(1),
        "FS-14/FS-16 record-review-dispatch fixture bootstrap",
    );
    assert_eq!(
        dispatch["allowed"],
        Value::Bool(true),
        "FS-14/FS-16 dispatch bootstrap should succeed: {dispatch:?}",
    );
    dispatch["dispatch_id"]
        .as_str()
        .expect("FS-14/FS-16 fixture dispatch bootstrap should expose dispatch_id")
        .to_owned()
}

fn publish_authoritative_release_truth(
    repo: &Path,
    state: &Path,
    plan_rel: &str,
    release_path: &Path,
    base_branch: &str,
) {
    let branch = current_branch_name(repo);
    let reviewed_state_id = format!("git_tree:{}", current_head_tree_sha(repo));
    let release_source = fs::read_to_string(release_path)
        .expect("workflow release artifact should be readable for authoritative publication");
    let release_fingerprint = sha256_hex(release_source.as_bytes());
    let release_summary =
        "Workflow runtime release-readiness fixture for authoritative late-stage routing.";
    let release_summary_hash = sha256_hex(release_summary.as_bytes());
    let release_record_id = format!("release-readiness-record-{release_fingerprint}");
    seed_current_branch_closure_truth(repo, state, plan_rel, 1);
    write_file(
        &harness_authoritative_artifact_path(
            state,
            &repo_slug(repo),
            &branch,
            &format!("release-docs-{release_fingerprint}.md"),
        ),
        &release_source,
    );
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            ("dependency_index_state", Value::from("fresh")),
            ("release_docs_state", Value::from("fresh")),
            (
                "last_release_docs_artifact_fingerprint",
                Value::from(release_fingerprint.clone()),
            ),
            ("current_release_readiness_result", Value::from("ready")),
            (
                "current_release_readiness_summary_hash",
                Value::from(release_summary_hash.clone()),
            ),
            (
                "current_release_readiness_record_id",
                Value::from(release_record_id.clone()),
            ),
            (
                "release_readiness_record_history",
                json!({
                    release_record_id.clone(): {
                        "record_id": release_record_id,
                        "record_sequence": 1,
                        "record_status": "current",
                        "branch_closure_id": "branch-release-closure",
                        "source_plan_path": plan_rel,
                        "source_plan_revision": 1,
                        "repo_slug": repo_slug(repo),
                        "branch_name": branch,
                        "base_branch": base_branch,
                        "reviewed_state_id": reviewed_state_id,
                        "result": "ready",
                        "release_docs_fingerprint": release_fingerprint,
                        "summary": release_summary,
                        "summary_hash": release_summary_hash,
                        "generated_by_identity": "featureforge/release-readiness"
                    }
                }),
            ),
        ],
    );
}

fn publish_authoritative_qa_truth(
    repo: &Path,
    state: &Path,
    plan_rel: &str,
    test_plan_path: &Path,
    qa_path: &Path,
    base_branch: &str,
) -> (String, String) {
    let branch = current_branch_name(repo);
    let reviewed_state_id = format!("git_tree:{}", current_head_tree_sha(repo));
    let authoritative_test_plan_source = fs::read_to_string(test_plan_path)
        .expect("workflow test-plan artifact should be readable for authoritative publication");
    let authoritative_test_plan_fingerprint = sha256_hex(authoritative_test_plan_source.as_bytes());
    let authoritative_test_plan_path = harness_authoritative_artifact_path(
        state,
        &repo_slug(repo),
        &branch,
        &format!("test-plan-{authoritative_test_plan_fingerprint}.md"),
    );
    write_file(
        &authoritative_test_plan_path,
        &authoritative_test_plan_source,
    );

    let authoritative_qa_source = rewrite_source_test_plan_header(
        &fs::read_to_string(qa_path)
            .expect("workflow QA artifact should be readable for authoritative publication"),
        &authoritative_test_plan_path,
    );
    let authoritative_qa_fingerprint = sha256_hex(authoritative_qa_source.as_bytes());
    let qa_summary = "Workflow runtime QA fixture for authoritative late-stage routing.";
    let qa_summary_hash = sha256_hex(qa_summary.as_bytes());
    let qa_record_id = format!("browser-qa-record-{authoritative_qa_fingerprint}");
    let final_review_record_id = current_final_review_record_id(repo, state);
    seed_current_branch_closure_truth(repo, state, plan_rel, 1);
    write_file(
        &harness_authoritative_artifact_path(
            state,
            &repo_slug(repo),
            &branch,
            &format!("browser-qa-{authoritative_qa_fingerprint}.md"),
        ),
        &authoritative_qa_source,
    );
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            ("dependency_index_state", Value::from("fresh")),
            ("browser_qa_state", Value::from("fresh")),
            (
                "last_browser_qa_artifact_fingerprint",
                Value::from(authoritative_qa_fingerprint.clone()),
            ),
            (
                "current_qa_branch_closure_id",
                Value::from("branch-release-closure"),
            ),
            ("current_qa_result", Value::from("pass")),
            (
                "current_qa_summary_hash",
                Value::from(qa_summary_hash.clone()),
            ),
            ("current_qa_record_id", Value::from(qa_record_id.clone())),
            (
                "browser_qa_record_history",
                json!({
                    qa_record_id.clone(): {
                        "record_id": qa_record_id,
                        "record_sequence": 1,
                        "record_status": "current",
                        "branch_closure_id": "branch-release-closure",
                        "final_review_record_id": final_review_record_id,
                        "source_plan_path": plan_rel,
                        "source_plan_revision": 1,
                        "repo_slug": repo_slug(repo),
                        "branch_name": branch,
                        "base_branch": base_branch,
                        "reviewed_state_id": reviewed_state_id,
                        "result": "pass",
                        "browser_qa_fingerprint": authoritative_qa_fingerprint.clone(),
                        "source_test_plan_fingerprint": authoritative_test_plan_fingerprint.clone(),
                        "summary": qa_summary,
                        "summary_hash": qa_summary_hash,
                        "generated_by_identity": "featureforge/qa"
                    }
                }),
            ),
        ],
    );
    (
        authoritative_test_plan_fingerprint,
        authoritative_qa_fingerprint,
    )
}

fn publish_authoritative_final_review_truth(
    repo: &Path,
    state: &Path,
    plan_rel: &str,
    review_path: &Path,
) {
    let branch = current_branch_name(repo);
    let base_branch = expected_release_base_branch(repo);
    let reviewed_state_id = format!("git_tree:{}", current_head_tree_sha(repo));
    let review_source = fs::read_to_string(review_path)
        .expect("workflow review artifact should be readable for authoritative publication");
    let review_fingerprint = sha256_hex(review_source.as_bytes());
    let final_review_summary =
        "Workflow runtime final-review fixture for authoritative late-stage routing.";
    let final_review_summary_hash = sha256_hex(final_review_summary.as_bytes());
    let final_review_record_id = format!("final-review-record-{review_fingerprint}");
    let release_readiness_record_id = current_release_readiness_record_id(repo, state);
    let browser_qa_required = match parse_plan_file(repo.join(plan_rel))
        .expect("plan should parse for authoritative final-review publication")
        .qa_requirement
        .as_deref()
    {
        Some("required") => Some(true),
        Some("not-required") => Some(false),
        _ => None,
    };
    seed_current_branch_closure_truth(repo, state, plan_rel, 1);
    write_file(
        &harness_authoritative_artifact_path(
            state,
            &repo_slug(repo),
            &branch,
            &format!("final-review-{review_fingerprint}.md"),
        ),
        &review_source,
    );
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            ("dependency_index_state", Value::from("fresh")),
            ("final_review_state", Value::from("fresh")),
            ("browser_qa_state", Value::from("not_required")),
            (
                "last_final_review_artifact_fingerprint",
                Value::from(review_fingerprint.clone()),
            ),
            (
                "current_final_review_branch_closure_id",
                Value::from("branch-release-closure"),
            ),
            (
                "current_final_review_dispatch_id",
                Value::from("fixture-final-review-dispatch"),
            ),
            (
                "current_final_review_reviewer_source",
                Value::from("fresh-context-subagent"),
            ),
            (
                "current_final_review_reviewer_id",
                Value::from("reviewer-fixture-001"),
            ),
            ("current_final_review_result", Value::from("pass")),
            (
                "current_final_review_summary_hash",
                Value::from(final_review_summary_hash.clone()),
            ),
            (
                "current_final_review_record_id",
                Value::from(final_review_record_id.clone()),
            ),
            (
                "final_review_record_history",
                json!({
                    final_review_record_id.clone(): {
                        "record_id": final_review_record_id,
                        "record_sequence": 1,
                        "record_status": "current",
                        "branch_closure_id": "branch-release-closure",
                        "release_readiness_record_id": release_readiness_record_id,
                        "dispatch_id": "fixture-final-review-dispatch",
                        "reviewer_source": "fresh-context-subagent",
                        "reviewer_id": "reviewer-fixture-001",
                        "result": "pass",
                        "final_review_fingerprint": review_fingerprint,
                        "browser_qa_required": browser_qa_required,
                        "source_plan_path": plan_rel,
                        "source_plan_revision": 1,
                        "repo_slug": repo_slug(repo),
                        "branch_name": branch.clone(),
                        "base_branch": base_branch,
                        "reviewed_state_id": reviewed_state_id,
                        "summary": final_review_summary,
                        "summary_hash": final_review_summary_hash
                    }
                }),
            ),
            (
                "finish_review_gate_pass_branch_closure_id",
                Value::from("branch-release-closure"),
            ),
        ],
    );
}

fn write_dispatched_branch_review_artifact(
    repo: &Path,
    state: &Path,
    plan_rel: &str,
    base_branch: &str,
) -> PathBuf {
    let release_path = write_branch_release_artifact(repo, state, plan_rel, base_branch);
    publish_authoritative_release_truth(repo, state, plan_rel, &release_path, base_branch);
    let initial_review_path = write_branch_review_artifact(repo, state, plan_rel, base_branch);
    publish_authoritative_final_review_truth(repo, state, plan_rel, &initial_review_path);
    let gate_review = run_runtime_review_dispatch_authority_json(
        repo,
        state,
        plan_rel,
        ReviewDispatchScopeArg::FinalReview,
        None,
        "plan execution gate-review dispatch for workflow review fixture",
    );
    assert_eq!(
        gate_review["allowed"],
        Value::Bool(true),
        "workflow review fixture should prime a passing gate-review dispatch before minting a final-review artifact: {gate_review:?}"
    );
    let review_path = write_branch_review_artifact(repo, state, plan_rel, base_branch);
    publish_authoritative_final_review_truth(repo, state, plan_rel, &review_path);
    review_path
}

fn enable_session_decision(state: &Path, session_key: &str) {
    let decision_path = state
        .join("session-entry")
        .join("using-featureforge")
        .join(session_key);
    write_file(&decision_path, "enabled\n");
}

#[test]
fn shell_workflow_resolve_exposes_wrapper_contract_fields() {
    let (repo_dir, state_dir) = init_repo("workflow-resolve-contract");
    let repo = repo_dir.path();
    let state = state_dir.path();

    let output = run_shell_status_helper(
        repo,
        state,
        &["resolve"],
        "shell helper resolve should stay removed from the compiled workflow CLI",
    );
    assert!(!output.status.success());
    let failure: Value = serde_json::from_slice(&output.stderr)
        .or_else(|_| serde_json::from_slice(&output.stdout))
        .expect("resolve removal failure should emit valid json");
    assert_eq!(failure["error_class"], "InvalidCommandInput");
    assert!(
        failure["message"]
            .as_str()
            .is_some_and(|message| message.contains("unrecognized subcommand 'resolve'")),
        "workflow resolve should stay removed from the compiled workflow CLI, got {failure:?}"
    );
}

#[test]
fn shell_workflow_resolve_failures_use_runtime_failure_contract() {
    let outside_repo = TempDir::new().expect("outside repo tempdir should be available");
    let state_dir = TempDir::new().expect("state tempdir should be available");

    let output = run_shell_status_helper(
        outside_repo.path(),
        state_dir.path(),
        &["resolve"],
        "shell helper resolve failure contract",
    );
    assert!(
        !output.status.success(),
        "resolve outside repo should fail, got {:?}",
        output.status
    );

    let failure: Value = serde_json::from_slice(&output.stderr)
        .or_else(|_| serde_json::from_slice(&output.stdout))
        .expect("resolve removal failure should emit valid json");
    assert_eq!(failure["error_class"], "InvalidCommandInput");
    assert!(
        failure["message"]
            .as_str()
            .is_some_and(|message| message.contains("unrecognized subcommand 'resolve'")),
        "workflow resolve should fail at CLI parsing before repo-context inspection, got {failure:?}"
    );
}

#[test]
fn direct_workflow_resolve_failures_use_runtime_failure_contract() {
    let outside_repo = TempDir::new().expect("outside repo tempdir should be available");
    let state_dir = TempDir::new().expect("state tempdir should be available");

    let output = run_rust_featureforge(
        outside_repo.path(),
        state_dir.path(),
        &["workflow", "resolve"],
        "direct helper resolve failure contract",
    );
    assert!(
        !output.status.success(),
        "resolve outside repo should fail, got {:?}",
        output.status
    );

    let failure: Value = serde_json::from_slice(&output.stderr)
        .or_else(|_| serde_json::from_slice(&output.stdout))
        .expect("resolve removal failure should emit valid json");
    assert_eq!(failure["error_class"], "InvalidCommandInput");
    assert!(
        failure["message"]
            .as_str()
            .is_some_and(|message| message.contains("unrecognized subcommand 'resolve'")),
        "workflow resolve should fail at helper parsing before repo-context inspection, got {failure:?}"
    );
}

#[test]
fn direct_read_only_workflow_failures_preserve_cli_text_contract() {
    let outside_repo = TempDir::new().expect("outside repo tempdir should be available");
    let state_dir = TempDir::new().expect("state tempdir should be available");

    let output = run_rust_featureforge(
        outside_repo.path(),
        state_dir.path(),
        &["workflow", "next"],
        "direct helper read-only workflow failure contract",
    );
    assert!(
        !output.status.success(),
        "workflow next should stay removed"
    );
    let failure: Value = serde_json::from_slice(&output.stderr)
        .or_else(|_| serde_json::from_slice(&output.stdout))
        .expect("workflow next removal failure should emit valid json");
    assert_eq!(failure["error_class"], "InvalidCommandInput");
    assert!(
        failure["message"]
            .as_str()
            .is_some_and(|message| message.contains("unrecognized subcommand 'next'")),
        "workflow next should fail at CLI parsing, got {failure:?}"
    );
}

#[test]
fn canonical_workflow_status_matches_helper_for_manifest_backed_missing_spec() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-manifest-backed");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let missing_spec = "docs/featureforge/specs/2026-03-24-rust-missing-spec-design.md";
    let identity = discover_repo_identity(repo).expect("repo identity should resolve");
    write_manifest(
        &manifest_path(&identity, state),
        &WorkflowManifest {
            version: 1,
            repo_root: fs::canonicalize(repo)
                .expect("repo root should canonicalize")
                .to_string_lossy()
                .into_owned(),
            branch: identity.branch_name.clone(),
            expected_spec_path: String::from(missing_spec),
            expected_plan_path: String::from(
                "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md",
            ),
            status: String::from("implementation_ready"),
            next_skill: String::new(),
            reason: String::from("implementation_ready"),
            note: String::from("implementation_ready"),
            updated_at: String::from("2026-03-24T00:00:00Z"),
        },
    );

    let helper_json = run_shell_status_json(
        repo,
        state,
        &["status", "--refresh"],
        "shell helper status refresh for missing spec",
    );
    let rust_output = run_rust_featureforge(
        repo,
        state,
        &["workflow", "status", "--refresh"],
        "rust canonical workflow status refresh for missing spec",
    );
    let rust_json = parse_json(
        &rust_output,
        "rust canonical workflow status refresh for missing spec",
    );

    assert_eq!(rust_json["status"], helper_json["status"]);
    assert_eq!(rust_json["next_skill"], helper_json["next_skill"]);
    assert_eq!(rust_json["spec_path"], helper_json["spec_path"]);
    assert_eq!(rust_json["reason"], helper_json["reason"]);
    assert_eq!(rust_json["reason_codes"], helper_json["reason_codes"]);
    assert_eq!(rust_json["diagnostics"], helper_json["diagnostics"]);
}

#[test]
fn canonical_workflow_status_matches_helper_for_ambiguous_specs() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-ambiguity");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let fixture_root = workflow_fixture_root();

    fs::create_dir_all(repo.join("docs/featureforge/specs"))
        .expect("specs directory should be creatable");
    fs::copy(
        fixture_root.join("specs/2026-01-22-document-review-system-design.md"),
        repo.join("docs/featureforge/specs/2026-01-22-document-review-system-design.md"),
    )
    .expect("first fixture spec should copy");
    fs::copy(
        fixture_root.join("specs/2026-02-19-visual-brainstorming-refactor-design.md"),
        repo.join("docs/featureforge/specs/2026-02-19-visual-brainstorming-refactor-design.md"),
    )
    .expect("second fixture spec should copy");

    let _helper_warmup = run_shell_status_json(
        repo,
        state,
        &["status", "--refresh"],
        "shell helper status refresh for ambiguous specs",
    );
    let helper_json = run_shell_status_json(
        repo,
        state,
        &["status", "--refresh"],
        "shell helper status refresh for ambiguous specs after manifest warmup",
    );
    let rust_output = run_rust_featureforge(
        repo,
        state,
        &["workflow", "status", "--refresh"],
        "rust canonical workflow status refresh for ambiguous specs",
    );
    let rust_json = parse_json(
        &rust_output,
        "rust canonical workflow status refresh for ambiguous specs",
    );

    assert_eq!(rust_json["status"], helper_json["status"]);
    assert_eq!(rust_json["next_skill"], helper_json["next_skill"]);
    assert_eq!(rust_json["reason"], helper_json["reason"]);
    assert_eq!(rust_json["reason_codes"], helper_json["reason_codes"]);
    assert_eq!(
        rust_json["spec_candidate_count"],
        helper_json["spec_candidate_count"]
    );
}

#[test]
fn canonical_workflow_status_ambiguous_specs_matches_checked_in_snapshot() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-ambiguity-snapshot");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let fixture_root = workflow_fixture_root();

    fs::create_dir_all(repo.join("docs/featureforge/specs"))
        .expect("specs directory should be creatable");
    fs::copy(
        fixture_root.join("specs/2026-01-22-document-review-system-design.md"),
        repo.join("docs/featureforge/specs/2026-01-22-document-review-system-design.md"),
    )
    .expect("first fixture spec should copy");
    fs::copy(
        fixture_root.join("specs/2026-01-22-document-review-system-design-v2.md"),
        repo.join("docs/featureforge/specs/2026-01-22-document-review-system-design-v2.md"),
    )
    .expect("second fixture spec should copy");

    let actual = normalize_workflow_status_snapshot(parse_json(
        &run_rust_featureforge(
            repo,
            state,
            &["workflow", "status", "--refresh"],
            "rust canonical workflow status refresh for ambiguous-spec snapshot",
        ),
        "rust canonical workflow status refresh for ambiguous-spec snapshot",
    ));
    let expected: Value = serde_json::from_str(
        &fs::read_to_string(repo_root().join("tests/fixtures/differential/workflow-status.json"))
            .expect("checked-in workflow-status snapshot should be readable"),
    )
    .expect("checked-in workflow-status snapshot should parse");

    assert_eq!(actual, expected);
}

#[test]
fn canonical_workflow_expect_and_sync_preserve_missing_spec_semantics() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-expect-sync");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let missing_spec = "docs/featureforge/specs/2026-03-24-rust-sync-missing-spec.md";
    let session_key = "workflow-runtime-expect-sync";

    let expect_output = run_rust_featureforge(
        repo,
        state,
        &[
            "workflow",
            "expect",
            "--artifact",
            "spec",
            "--path",
            missing_spec,
        ],
        "rust canonical workflow expect missing spec",
    );
    assert!(
        expect_output.status.success(),
        "rust canonical workflow expect should succeed, got {:?}\nstdout:\n{}\nstderr:\n{}",
        expect_output.status,
        String::from_utf8_lossy(&expect_output.stdout),
        String::from_utf8_lossy(&expect_output.stderr)
    );

    let sync_output = run_rust_featureforge(
        repo,
        state,
        &["workflow", "sync", "--artifact", "spec"],
        "rust canonical workflow sync missing spec",
    );
    assert!(
        sync_output.status.success(),
        "rust canonical workflow sync should succeed, got {:?}\nstdout:\n{}\nstderr:\n{}",
        sync_output.status,
        String::from_utf8_lossy(&sync_output.stdout),
        String::from_utf8_lossy(&sync_output.stderr)
    );
    let sync_stdout =
        String::from_utf8(sync_output.stdout).expect("sync output should be valid utf-8");
    assert!(sync_stdout.contains("missing_artifact"));
    assert!(sync_stdout.contains(missing_spec));
    assert!(sync_stdout.contains("featureforge:brainstorming"));

    let status_json = parse_json(
        &run_rust_featureforge(
            repo,
            state,
            &["workflow", "status", "--refresh"],
            "rust canonical workflow status refresh after sync",
        ),
        "rust canonical workflow status refresh after sync",
    );
    assert_eq!(status_json["status"], "needs_brainstorming");
    assert_eq!(status_json["spec_path"], missing_spec);
    assert_eq!(status_json["reason"], "missing_expected_spec");
    assert_eq!(status_json["reason_codes"][0], "missing_expected_spec");

    write_file(
        &state
            .join("session-entry")
            .join("using-featureforge")
            .join(session_key),
        "enabled\n",
    );

    let phase_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "phase", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "rust canonical workflow phase after missing-spec sync",
        ),
        "rust canonical workflow phase after missing-spec sync",
    );
    assert_eq!(
        phase_json["phase"], "pivot_required",
        "phase JSON should preserve authoritative contract-drafting pivot route, got {phase_json}"
    );
    assert_eq!(phase_json["next_skill"], "featureforge:brainstorming");
    assert_eq!(phase_json["next_action"], "pivot / return to planning");
}

#[test]
fn canonical_workflow_status_routes_draft_plan_to_eng_review_after_matching_pass_receipt() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-plan-fidelity-pass");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_path = "docs/featureforge/specs/2026-01-22-document-review-system-design.md";
    let plan_path = "docs/featureforge/plans/2026-01-22-document-review-system.md";

    fs::create_dir_all(repo.join("docs/featureforge/specs")).expect("spec directory should exist");
    write_minimal_plan_fidelity_spec(repo, spec_path);
    write_file(
        &repo.join(plan_path),
        "# Draft Plan\n\n**Workflow State:** Draft\n**Plan Revision:** 1\n**Execution Mode:** none\n**Source Spec:** `docs/featureforge/specs/2026-01-22-document-review-system-design.md`\n**Source Spec Revision:** 1\n**Last Reviewed By:** writing-plans\n\n## Requirement Coverage Matrix\n\n- REQ-001 -> Task 1\n\n## Execution Strategy\n\n- Execute Task 1 last. It is the only task in this fixture and closes the execution graph for route-time workflow validation.\n\n## Dependency Diagram\n\n```text\nTask 1\n```\n\n## Task 1: Prepare the draft plan for review\n\n**Spec Coverage:** REQ-001\n**Goal:** The draft plan is ready for engineering review.\n\n**Context:**\n- Spec Coverage: REQ-001.\n\n**Constraints:**\n- Keep the fixture minimal.\n**Done when:**\n- The draft plan is ready for engineering review.\n\n**Files:**\n- Test: `tests/workflow_runtime.rs`\n\n- [ ] **Step 1: Review the draft plan**\n",
    );
    write_plan_fidelity_review_artifact(
        repo,
        PlanFidelityReviewArtifactInput {
            artifact_rel: ".featureforge/reviews/plan-fidelity-pass.md",
            plan_path,
            plan_revision: 1,
            spec_path,
            spec_revision: 1,
            review_verdict: "pass",
            reviewer_source: "fresh-context-subagent",
            reviewer_id: "reviewer-019d",
            verified_surfaces: &PLAN_FIDELITY_REQUIRED_SURFACES,
        },
    );

    let receipt_json = run_workflow_plan_fidelity_json(
        repo,
        state,
        &[
            "record",
            "--plan",
            plan_path,
            "--review-artifact",
            ".featureforge/reviews/plan-fidelity-pass.md",
            "--json",
        ],
        "workflow plan-fidelity record should succeed for matching draft plan",
    );
    assert_eq!(receipt_json["status"], "ok");

    let status_json = parse_json(
        &run_rust_featureforge(
            repo,
            state,
            &["workflow", "status", "--refresh"],
            "workflow status should route matching plan-fidelity receipt to eng review",
        ),
        "workflow status should route matching plan-fidelity receipt to eng review",
    );

    assert_eq!(status_json["status"], "plan_draft");
    assert_eq!(status_json["next_skill"], "featureforge:plan-eng-review");
    assert_eq!(status_json["plan_fidelity_receipt"]["state"], "pass");
    assert!(
        status_json["plan_fidelity_receipt"]["verified_requirement_index"]
            .as_bool()
            .expect("requirement index verification should be present")
    );
    assert!(
        status_json["plan_fidelity_receipt"]["verified_execution_topology"]
            .as_bool()
            .expect("execution topology verification should be present")
    );
    assert!(
        status_json["plan_fidelity_receipt"]["verified_task_contract"]
            .as_bool()
            .expect("task contract verification should be present")
    );
    assert!(
        status_json["plan_fidelity_receipt"]["verified_task_determinism"]
            .as_bool()
            .expect("task determinism verification should be present")
    );
    assert!(
        status_json["plan_fidelity_receipt"]["verified_spec_reference_fidelity"]
            .as_bool()
            .expect("spec reference fidelity verification should be present")
    );
    assert!(
        !status_json["reason_codes"]
            .as_array()
            .expect("reason_codes should be an array")
            .iter()
            .any(|value| value == "missing_plan_fidelity_receipt"),
        "matching pass receipts should clear the missing receipt reason"
    );
}

#[test]
fn canonical_workflow_status_normalizes_dot_slash_source_spec_paths() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-plan-fidelity-dot-slash-spec");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_path = "docs/featureforge/specs/2026-01-22-document-review-system-design.md";
    let plan_path = "docs/featureforge/plans/2026-01-22-document-review-system.md";

    fs::create_dir_all(repo.join("docs/featureforge/specs")).expect("spec directory should exist");
    write_minimal_plan_fidelity_spec(repo, spec_path);
    write_file(
        &repo.join(plan_path),
        "# Draft Plan\n\n**Workflow State:** Draft\n**Plan Revision:** 1\n**Execution Mode:** none\n**Source Spec:** `./docs/featureforge/specs/2026-01-22-document-review-system-design.md`\n**Source Spec Revision:** 1\n**Last Reviewed By:** writing-plans\n\n## Requirement Coverage Matrix\n\n- REQ-001 -> Task 1\n\n## Task 1: Prepare the draft plan for review\n\n**Spec Coverage:** REQ-001\n**Goal:** The draft plan is ready for engineering review.\n\n**Context:**\n- Spec Coverage: REQ-001.\n\n**Constraints:**\n- Keep the fixture minimal.\n**Done when:**\n- The draft plan is ready for engineering review.\n\n**Files:**\n- Test: `tests/workflow_runtime.rs`\n\n- [ ] **Step 1: Review the draft plan**\n",
    );
    write_plan_fidelity_review_artifact(
        repo,
        PlanFidelityReviewArtifactInput {
            artifact_rel: ".featureforge/reviews/plan-fidelity-dot-slash-spec.md",
            plan_path,
            plan_revision: 1,
            spec_path,
            spec_revision: 1,
            review_verdict: "pass",
            reviewer_source: "fresh-context-subagent",
            reviewer_id: "reviewer-dot-slash-spec",
            verified_surfaces: &PLAN_FIDELITY_REQUIRED_SURFACES,
        },
    );
    let receipt_json = run_workflow_plan_fidelity_json(
        repo,
        state,
        &[
            "record",
            "--plan",
            plan_path,
            "--review-artifact",
            ".featureforge/reviews/plan-fidelity-dot-slash-spec.md",
            "--json",
        ],
        "workflow plan-fidelity record should normalize ./docs Source Spec headers",
    );
    assert_eq!(receipt_json["status"], "ok");

    let status_json = parse_json(
        &run_rust_featureforge(
            repo,
            state,
            &["workflow", "status", "--refresh"],
            "workflow status should normalize ./docs Source Spec headers",
        ),
        "workflow status should normalize ./docs Source Spec headers",
    );

    assert_eq!(status_json["status"], "plan_draft");
    assert_eq!(status_json["next_skill"], "featureforge:plan-eng-review");
}

#[test]
fn canonical_workflow_status_rejects_stale_plan_fidelity_receipt_after_plan_revision_changes() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-plan-fidelity-stale");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_path = "docs/featureforge/specs/2026-01-22-document-review-system-design.md";
    let plan_path = "docs/featureforge/plans/2026-01-22-document-review-system.md";

    fs::create_dir_all(repo.join("docs/featureforge/specs")).expect("spec directory should exist");
    write_minimal_plan_fidelity_spec(repo, spec_path);
    write_file(
        &repo.join(plan_path),
        "# Draft Plan\n\n**Workflow State:** Draft\n**Plan Revision:** 1\n**Execution Mode:** none\n**Source Spec:** `docs/featureforge/specs/2026-01-22-document-review-system-design.md`\n**Source Spec Revision:** 1\n**Last Reviewed By:** writing-plans\n\n## Requirement Coverage Matrix\n\n- REQ-001 -> Task 1\n\n## Execution Strategy\n\n- Execute Task 1 last. It is the only task in this fixture and closes the execution graph for route-time workflow validation.\n\n## Dependency Diagram\n\n```text\nTask 1\n```\n\n## Task 1: Prepare the draft plan for review\n\n**Spec Coverage:** REQ-001\n**Goal:** The draft plan is ready for engineering review.\n\n**Context:**\n- Spec Coverage: REQ-001.\n\n**Constraints:**\n- Keep the fixture minimal.\n**Done when:**\n- The draft plan is ready for engineering review.\n\n**Files:**\n- Test: `tests/workflow_runtime.rs`\n\n- [ ] **Step 1: Review the draft plan**\n",
    );
    write_plan_fidelity_review_artifact(
        repo,
        PlanFidelityReviewArtifactInput {
            artifact_rel: ".featureforge/reviews/plan-fidelity-stale.md",
            plan_path,
            plan_revision: 1,
            spec_path,
            spec_revision: 1,
            review_verdict: "pass",
            reviewer_source: "fresh-context-subagent",
            reviewer_id: "reviewer-019d",
            verified_surfaces: &PLAN_FIDELITY_REQUIRED_SURFACES,
        },
    );

    let receipt_json = run_workflow_plan_fidelity_json(
        repo,
        state,
        &[
            "record",
            "--plan",
            plan_path,
            "--review-artifact",
            ".featureforge/reviews/plan-fidelity-stale.md",
            "--json",
        ],
        "workflow plan-fidelity record should succeed before stale-plan mutation",
    );
    assert_eq!(receipt_json["status"], "ok");

    write_file(
        &repo.join(plan_path),
        "# Draft Plan\n\n**Workflow State:** Draft\n**Plan Revision:** 2\n**Execution Mode:** none\n**Source Spec:** `docs/featureforge/specs/2026-01-22-document-review-system-design.md`\n**Source Spec Revision:** 1\n**Last Reviewed By:** writing-plans\n\n## Requirement Coverage Matrix\n\n- REQ-001 -> Task 1\n\n## Execution Strategy\n\n- Execute Task 1 last. It is the only task in this fixture and closes the execution graph for route-time workflow validation.\n\n## Dependency Diagram\n\n```text\nTask 1\n```\n\n## Task 1: Prepare the revised draft plan for review\n\n**Spec Coverage:** REQ-001\n**Goal:** The revised draft plan is ready for engineering review.\n\n**Context:**\n- Spec Coverage: REQ-001.\n\n**Constraints:**\n- Keep the fixture minimal.\n**Done when:**\n- The revised draft plan is ready for engineering review.\n\n**Files:**\n- Test: `tests/workflow_runtime.rs`\n\n- [ ] **Step 1: Review the revised draft plan**\n",
    );

    let status_json = parse_json(
        &run_rust_featureforge(
            repo,
            state,
            &["workflow", "status", "--refresh"],
            "workflow status should fail closed on stale plan-fidelity receipts",
        ),
        "workflow status should fail closed on stale plan-fidelity receipts",
    );

    assert_eq!(status_json["status"], "plan_draft");
    assert_eq!(
        status_json["next_skill"],
        "featureforge:plan-fidelity-review"
    );
    assert!(
        status_json["reason_codes"]
            .as_array()
            .expect("reason_codes should be an array")
            .iter()
            .any(|value| value == "stale_plan_fidelity_receipt"),
        "plan revision drift should stale the prior plan-fidelity receipt"
    );
}

#[test]
fn canonical_workflow_status_routes_draft_plan_without_fidelity_receipt_to_plan_fidelity_review() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-draft-plan-missing-fidelity");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_path = "docs/featureforge/specs/2026-01-22-document-review-system-design.md";
    let plan_path = "docs/featureforge/plans/2026-01-22-document-review-system.md";

    fs::create_dir_all(repo.join("docs/featureforge/specs")).expect("spec directory should exist");
    write_minimal_plan_fidelity_spec(repo, spec_path);
    write_file(
        &repo.join(plan_path),
        "# Draft Plan\n\n**Workflow State:** Draft\n**Plan Revision:** 1\n**Execution Mode:** none\n**Source Spec:** `docs/featureforge/specs/2026-01-22-document-review-system-design.md`\n**Source Spec Revision:** 1\n**Last Reviewed By:** writing-plans\n\n## Requirement Coverage Matrix\n\n- REQ-001 -> Task 1\n\n## Execution Strategy\n\n- Execute Task 1 last. It is the only task in this fixture and closes the execution graph for route-time workflow validation.\n\n## Dependency Diagram\n\n```text\nTask 1\n```\n\n## Task 1: Prepare the draft plan for review\n\n**Spec Coverage:** REQ-001\n**Goal:** The draft plan is ready for engineering review.\n\n**Context:**\n- Spec Coverage: REQ-001.\n\n**Constraints:**\n- Keep the fixture minimal.\n**Done when:**\n- The draft plan is ready for engineering review.\n\n**Files:**\n- Test: `tests/workflow_runtime.rs`\n\n- [ ] **Step 1: Review the draft plan**\n",
    );

    let status_json = parse_json(
        &run_rust_featureforge(
            repo,
            state,
            &["workflow", "status", "--refresh"],
            "rust canonical workflow status refresh for draft plan missing fidelity receipt",
        ),
        "rust canonical workflow status refresh for draft plan missing fidelity receipt",
    );

    assert_eq!(status_json["status"], "plan_draft");
    assert_eq!(
        status_json["next_skill"],
        "featureforge:plan-fidelity-review"
    );
    assert_eq!(
        status_json["reason_codes"][0],
        "missing_plan_fidelity_receipt"
    );
    assert_eq!(
        status_json["diagnostics"][0]["code"],
        "missing_plan_fidelity_receipt"
    );
    assert_eq!(status_json["plan_fidelity_receipt"]["state"], "missing");
    assert!(
        !status_json["plan_fidelity_receipt"]["verified_requirement_index"]
            .as_bool()
            .expect("requirement index verification should be present")
    );
    assert!(
        !status_json["plan_fidelity_receipt"]["verified_execution_topology"]
            .as_bool()
            .expect("execution topology verification should be present")
    );
    assert!(
        !status_json["plan_fidelity_receipt"]["verified_task_contract"]
            .as_bool()
            .expect("task contract verification should be present")
    );
    assert!(
        !status_json["plan_fidelity_receipt"]["verified_task_determinism"]
            .as_bool()
            .expect("task determinism verification should be present")
    );
    assert!(
        !status_json["plan_fidelity_receipt"]["verified_spec_reference_fidelity"]
            .as_bool()
            .expect("spec reference fidelity verification should be present")
    );
}

#[test]
fn canonical_workflow_status_routes_draft_plan_with_non_independent_fidelity_receipt_to_plan_fidelity_review()
 {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-draft-plan-non-independent-fidelity");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_path = "docs/featureforge/specs/2026-01-22-document-review-system-design.md";
    let plan_path = "docs/featureforge/plans/2026-01-22-document-review-system.md";

    fs::create_dir_all(repo.join("docs/featureforge/specs")).expect("spec directory should exist");
    write_minimal_plan_fidelity_spec(repo, spec_path);
    write_file(
        &repo.join(plan_path),
        "# Draft Plan\n\n**Workflow State:** Draft\n**Plan Revision:** 1\n**Execution Mode:** none\n**Source Spec:** `docs/featureforge/specs/2026-01-22-document-review-system-design.md`\n**Source Spec Revision:** 1\n**Last Reviewed By:** writing-plans\n\n## Requirement Coverage Matrix\n\n- REQ-001 -> Task 1\n\n## Execution Strategy\n\n- Execute Task 1 last. It is the only task in this fixture and closes the execution graph for route-time workflow validation.\n\n## Dependency Diagram\n\n```text\nTask 1\n```\n\n## Task 1: Prepare the draft plan for review\n\n**Spec Coverage:** REQ-001\n**Goal:** The draft plan is ready for engineering review.\n\n**Context:**\n- Spec Coverage: REQ-001.\n\n**Constraints:**\n- Keep the fixture minimal.\n**Done when:**\n- The draft plan is ready for engineering review.\n\n**Files:**\n- Test: `tests/workflow_runtime.rs`\n\n- [ ] **Step 1: Review the draft plan**\n",
    );
    write_plan_fidelity_review_artifact(
        repo,
        PlanFidelityReviewArtifactInput {
            artifact_rel: ".featureforge/reviews/plan-fidelity-non-independent.md",
            plan_path,
            plan_revision: 1,
            spec_path,
            spec_revision: 1,
            review_verdict: "pass",
            reviewer_source: "fresh-context-subagent",
            reviewer_id: "reviewer-independent",
            verified_surfaces: &PLAN_FIDELITY_REQUIRED_SURFACES,
        },
    );
    let receipt_json = run_workflow_plan_fidelity_json(
        repo,
        state,
        &[
            "record",
            "--plan",
            plan_path,
            "--review-artifact",
            ".featureforge/reviews/plan-fidelity-non-independent.md",
            "--json",
        ],
        "workflow plan-fidelity record should succeed before mutating reviewer provenance",
    );
    assert_eq!(receipt_json["status"], "ok");
    let receipt_path = PathBuf::from(
        receipt_json["receipt_path"]
            .as_str()
            .expect("receipt path should be present"),
    );
    let mut receipt = serde_json::from_str::<Value>(
        &fs::read_to_string(&receipt_path).expect("recorded receipt should be readable"),
    )
    .expect("recorded receipt should parse as json");
    receipt["reviewer_provenance"]["reviewer_source"] = Value::String(String::from("same-context"));
    fs::write(
        &receipt_path,
        serde_json::to_string_pretty(&receipt).expect("mutated receipt should serialize"),
    )
    .expect("mutated receipt should be writable");

    let status_json = parse_json(
        &run_rust_featureforge(
            repo,
            state,
            &["workflow", "status", "--refresh"],
            "rust canonical workflow status refresh for draft plan non-independent fidelity receipt",
        ),
        "rust canonical workflow status refresh for draft plan non-independent fidelity receipt",
    );

    assert_eq!(status_json["status"], "plan_draft");
    assert_eq!(
        status_json["next_skill"],
        "featureforge:plan-fidelity-review"
    );
    assert!(
        status_json["reason_codes"]
            .as_array()
            .expect("reason_codes should be an array")
            .iter()
            .any(|value| value == "plan_fidelity_receipt_not_independent"),
        "non-independent reviewer provenance should fail closed with explicit reason code"
    );
}

#[test]
fn canonical_workflow_status_routes_draft_plan_with_non_pass_fidelity_receipt_to_plan_fidelity_review()
 {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-draft-plan-non-pass-fidelity");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_path = "docs/featureforge/specs/2026-01-22-document-review-system-design.md";
    let plan_path = "docs/featureforge/plans/2026-01-22-document-review-system.md";

    fs::create_dir_all(repo.join("docs/featureforge/specs")).expect("spec directory should exist");
    write_minimal_plan_fidelity_spec(repo, spec_path);
    write_file(
        &repo.join(plan_path),
        "# Draft Plan\n\n**Workflow State:** Draft\n**Plan Revision:** 1\n**Execution Mode:** none\n**Source Spec:** `docs/featureforge/specs/2026-01-22-document-review-system-design.md`\n**Source Spec Revision:** 1\n**Last Reviewed By:** writing-plans\n\n## Requirement Coverage Matrix\n\n- REQ-001 -> Task 1\n\n## Execution Strategy\n\n- Execute Task 1 last. It is the only task in this fixture and closes the execution graph for route-time workflow validation.\n\n## Dependency Diagram\n\n```text\nTask 1\n```\n\n## Task 1: Prepare the draft plan for review\n\n**Spec Coverage:** REQ-001\n**Goal:** The draft plan is ready for engineering review.\n\n**Context:**\n- Spec Coverage: REQ-001.\n\n**Constraints:**\n- Keep the fixture minimal.\n**Done when:**\n- The draft plan is ready for engineering review.\n\n**Files:**\n- Test: `tests/workflow_runtime.rs`\n\n- [ ] **Step 1: Review the draft plan**\n",
    );
    write_plan_fidelity_review_artifact(
        repo,
        PlanFidelityReviewArtifactInput {
            artifact_rel: ".featureforge/reviews/plan-fidelity-non-pass-gate.md",
            plan_path,
            plan_revision: 1,
            spec_path,
            spec_revision: 1,
            review_verdict: "pass",
            reviewer_source: "fresh-context-subagent",
            reviewer_id: "reviewer-non-pass-gate",
            verified_surfaces: &PLAN_FIDELITY_REQUIRED_SURFACES,
        },
    );
    let receipt_json = run_workflow_plan_fidelity_json(
        repo,
        state,
        &[
            "record",
            "--plan",
            plan_path,
            "--review-artifact",
            ".featureforge/reviews/plan-fidelity-non-pass-gate.md",
            "--json",
        ],
        "workflow plan-fidelity record should succeed before mutating verdict",
    );
    assert_eq!(receipt_json["status"], "ok");
    let receipt_path = PathBuf::from(
        receipt_json["receipt_path"]
            .as_str()
            .expect("receipt path should be present"),
    );
    let mut receipt = serde_json::from_str::<Value>(
        &fs::read_to_string(&receipt_path).expect("recorded receipt should be readable"),
    )
    .expect("recorded receipt should parse as json");
    receipt["verdict"] = Value::String(String::from("needs-changes"));
    fs::write(
        &receipt_path,
        serde_json::to_string_pretty(&receipt).expect("mutated receipt should serialize"),
    )
    .expect("mutated receipt should be writable");

    let status_json = parse_json(
        &run_rust_featureforge(
            repo,
            state,
            &["workflow", "status", "--refresh"],
            "rust canonical workflow status refresh for draft plan non-pass fidelity receipt",
        ),
        "rust canonical workflow status refresh for draft plan non-pass fidelity receipt",
    );

    assert_eq!(status_json["status"], "plan_draft");
    assert_eq!(
        status_json["next_skill"],
        "featureforge:plan-fidelity-review"
    );
    assert!(
        status_json["reason_codes"]
            .as_array()
            .expect("reason_codes should be an array")
            .iter()
            .any(|value| value == "plan_fidelity_receipt_not_pass"),
        "non-pass verdicts should fail closed with explicit reason code"
    );
}

#[test]
fn canonical_workflow_status_routes_draft_plan_with_malformed_fidelity_receipt_to_plan_fidelity_review()
 {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-draft-plan-malformed-fidelity");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_path = "docs/featureforge/specs/2026-01-22-document-review-system-design.md";
    let plan_path = "docs/featureforge/plans/2026-01-22-document-review-system.md";

    fs::create_dir_all(repo.join("docs/featureforge/specs")).expect("spec directory should exist");
    write_minimal_plan_fidelity_spec(repo, spec_path);
    write_file(
        &repo.join(plan_path),
        "# Draft Plan\n\n**Workflow State:** Draft\n**Plan Revision:** 1\n**Execution Mode:** none\n**Source Spec:** `docs/featureforge/specs/2026-01-22-document-review-system-design.md`\n**Source Spec Revision:** 1\n**Last Reviewed By:** writing-plans\n\n## Requirement Coverage Matrix\n\n- REQ-001 -> Task 1\n\n## Execution Strategy\n\n- Execute Task 1 last. It is the only task in this fixture and closes the execution graph for route-time workflow validation.\n\n## Dependency Diagram\n\n```text\nTask 1\n```\n\n## Task 1: Prepare the draft plan for review\n\n**Spec Coverage:** REQ-001\n**Goal:** The draft plan is ready for engineering review.\n\n**Context:**\n- Spec Coverage: REQ-001.\n\n**Constraints:**\n- Keep the fixture minimal.\n**Done when:**\n- The draft plan is ready for engineering review.\n\n**Files:**\n- Test: `tests/workflow_runtime.rs`\n\n- [ ] **Step 1: Review the draft plan**\n",
    );
    write_plan_fidelity_review_artifact(
        repo,
        PlanFidelityReviewArtifactInput {
            artifact_rel: ".featureforge/reviews/plan-fidelity-malformed-gate.md",
            plan_path,
            plan_revision: 1,
            spec_path,
            spec_revision: 1,
            review_verdict: "pass",
            reviewer_source: "fresh-context-subagent",
            reviewer_id: "reviewer-malformed-gate",
            verified_surfaces: &PLAN_FIDELITY_REQUIRED_SURFACES,
        },
    );
    let receipt_json = run_workflow_plan_fidelity_json(
        repo,
        state,
        &[
            "record",
            "--plan",
            plan_path,
            "--review-artifact",
            ".featureforge/reviews/plan-fidelity-malformed-gate.md",
            "--json",
        ],
        "workflow plan-fidelity record should succeed before corrupting the receipt",
    );
    assert_eq!(receipt_json["status"], "ok");
    let receipt_path = PathBuf::from(
        receipt_json["receipt_path"]
            .as_str()
            .expect("receipt path should be present"),
    );
    fs::write(&receipt_path, "{not json").expect("corrupted receipt should be writable");

    let status_json = parse_json(
        &run_rust_featureforge(
            repo,
            state,
            &["workflow", "status", "--refresh"],
            "rust canonical workflow status refresh for draft plan malformed fidelity receipt",
        ),
        "rust canonical workflow status refresh for draft plan malformed fidelity receipt",
    );

    assert_eq!(status_json["status"], "plan_draft");
    assert_eq!(
        status_json["next_skill"],
        "featureforge:plan-fidelity-review"
    );
    assert!(
        status_json["reason_codes"]
            .as_array()
            .expect("reason_codes should be an array")
            .iter()
            .any(|value| value == "malformed_plan_fidelity_receipt"),
        "malformed receipt payloads should fail closed with explicit reason code"
    );
}

#[test]
fn workflow_plan_fidelity_record_rejects_incomplete_verification_artifacts() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-plan-fidelity-incomplete-artifact");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_path = "docs/featureforge/specs/2026-01-22-document-review-system-design.md";
    let plan_path = "docs/featureforge/plans/2026-01-22-document-review-system.md";

    fs::create_dir_all(repo.join("docs/featureforge/specs")).expect("spec directory should exist");
    write_minimal_plan_fidelity_spec(repo, spec_path);
    write_file(
        &repo.join(plan_path),
        "# Draft Plan\n\n**Workflow State:** Draft\n**Plan Revision:** 1\n**Execution Mode:** none\n**Source Spec:** `docs/featureforge/specs/2026-01-22-document-review-system-design.md`\n**Source Spec Revision:** 1\n**Last Reviewed By:** writing-plans\n\n## Requirement Coverage Matrix\n\n- REQ-001 -> Task 1\n\n## Task 1: Prepare the draft plan for review\n\n**Spec Coverage:** REQ-001\n**Goal:** The draft plan is ready for engineering review.\n\n**Context:**\n- Spec Coverage: REQ-001.\n\n**Constraints:**\n- Keep the fixture minimal.\n**Done when:**\n- The draft plan is ready for engineering review.\n\n**Files:**\n- Test: `tests/workflow_runtime.rs`\n\n- [ ] **Step 1: Review the draft plan**\n",
    );
    write_plan_fidelity_review_artifact(
        repo,
        PlanFidelityReviewArtifactInput {
            artifact_rel: ".featureforge/reviews/plan-fidelity-incomplete.md",
            plan_path,
            plan_revision: 1,
            spec_path,
            spec_revision: 1,
            review_verdict: "pass",
            reviewer_source: "fresh-context-subagent",
            reviewer_id: "reviewer-incomplete",
            verified_surfaces: &[],
        },
    );

    let output = run_rust_featureforge(
        repo,
        state,
        &[
            "workflow",
            "plan-fidelity",
            "record",
            "--plan",
            plan_path,
            "--review-artifact",
            ".featureforge/reviews/plan-fidelity-incomplete.md",
            "--json",
        ],
        "workflow plan-fidelity record should reject incomplete verification artifacts",
    );
    assert!(
        !output.status.success(),
        "record should fail closed when required verification surfaces are missing, got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn workflow_plan_fidelity_record_rejects_non_pass_verdicts() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-plan-fidelity-non-pass-verdict");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_path = "docs/featureforge/specs/2026-01-22-document-review-system-design.md";
    let plan_path = "docs/featureforge/plans/2026-01-22-document-review-system.md";

    fs::create_dir_all(repo.join("docs/featureforge/specs")).expect("spec directory should exist");
    write_minimal_plan_fidelity_spec(repo, spec_path);
    write_file(
        &repo.join(plan_path),
        "# Draft Plan\n\n**Workflow State:** Draft\n**Plan Revision:** 1\n**Execution Mode:** none\n**Source Spec:** `docs/featureforge/specs/2026-01-22-document-review-system-design.md`\n**Source Spec Revision:** 1\n**Last Reviewed By:** writing-plans\n\n## Requirement Coverage Matrix\n\n- REQ-001 -> Task 1\n\n## Task 1: Prepare the draft plan for review\n\n**Spec Coverage:** REQ-001\n**Goal:** The draft plan is ready for engineering review.\n\n**Context:**\n- Spec Coverage: REQ-001.\n\n**Constraints:**\n- Keep the fixture minimal.\n**Done when:**\n- The draft plan is ready for engineering review.\n\n**Files:**\n- Test: `tests/workflow_runtime.rs`\n\n- [ ] **Step 1: Review the draft plan**\n",
    );
    write_plan_fidelity_review_artifact(
        repo,
        PlanFidelityReviewArtifactInput {
            artifact_rel: ".featureforge/reviews/plan-fidelity-non-pass.md",
            plan_path,
            plan_revision: 1,
            spec_path,
            spec_revision: 1,
            review_verdict: "clear",
            reviewer_source: "fresh-context-subagent",
            reviewer_id: "reviewer-non-pass",
            verified_surfaces: &PLAN_FIDELITY_REQUIRED_SURFACES,
        },
    );

    let output = run_rust_featureforge(
        repo,
        state,
        &[
            "workflow",
            "plan-fidelity",
            "record",
            "--plan",
            plan_path,
            "--review-artifact",
            ".featureforge/reviews/plan-fidelity-non-pass.md",
            "--json",
        ],
        "workflow plan-fidelity record should reject non-pass review verdicts",
    );
    assert!(
        !output.status.success(),
        "record should fail closed when the review verdict is not pass, got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn workflow_plan_fidelity_record_normalizes_dot_slash_review_targets() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-plan-fidelity-dot-slash-artifact");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_path = "docs/featureforge/specs/2026-01-22-document-review-system-design.md";
    let plan_path = "docs/featureforge/plans/2026-01-22-document-review-system.md";

    fs::create_dir_all(repo.join("docs/featureforge/specs")).expect("spec directory should exist");
    write_minimal_plan_fidelity_spec(repo, spec_path);
    write_file(
        &repo.join(plan_path),
        "# Draft Plan\n\n**Workflow State:** Draft\n**Plan Revision:** 1\n**Execution Mode:** none\n**Source Spec:** `./docs/featureforge/specs/2026-01-22-document-review-system-design.md`\n**Source Spec Revision:** 1\n**Last Reviewed By:** writing-plans\n\n## Requirement Coverage Matrix\n\n- REQ-001 -> Task 1\n\n## Task 1: Prepare the draft plan for review\n\n**Spec Coverage:** REQ-001\n**Goal:** The draft plan is ready for engineering review.\n\n**Context:**\n- Spec Coverage: REQ-001.\n\n**Constraints:**\n- Keep the fixture minimal.\n**Done when:**\n- The draft plan is ready for engineering review.\n\n**Files:**\n- Test: `tests/workflow_runtime.rs`\n\n- [ ] **Step 1: Review the draft plan**\n",
    );
    write_plan_fidelity_review_artifact(
        repo,
        PlanFidelityReviewArtifactInput {
            artifact_rel: ".featureforge/reviews/plan-fidelity-dot-slash-targets.md",
            plan_path,
            plan_revision: 1,
            spec_path,
            spec_revision: 1,
            review_verdict: "pass",
            reviewer_source: "fresh-context-subagent",
            reviewer_id: "reviewer-dot-slash-targets",
            verified_surfaces: &PLAN_FIDELITY_REQUIRED_SURFACES,
        },
    );
    replace_in_file(
        &repo.join(".featureforge/reviews/plan-fidelity-dot-slash-targets.md"),
        &format!("**Reviewed Plan:** `{plan_path}`"),
        &format!("**Reviewed Plan:** `./{plan_path}`"),
    );
    replace_in_file(
        &repo.join(".featureforge/reviews/plan-fidelity-dot-slash-targets.md"),
        &format!("**Reviewed Spec:** `{spec_path}`"),
        &format!("**Reviewed Spec:** `./{spec_path}`"),
    );

    let receipt_json = run_workflow_plan_fidelity_json(
        repo,
        state,
        &[
            "record",
            "--plan",
            plan_path,
            "--review-artifact",
            ".featureforge/reviews/plan-fidelity-dot-slash-targets.md",
            "--json",
        ],
        "workflow plan-fidelity record should normalize dot-slash review targets",
    );
    assert_eq!(receipt_json["status"], "ok");
}

#[test]
fn workflow_plan_fidelity_record_rejects_stale_review_artifact_fingerprints() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-plan-fidelity-stale-artifact");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_path = "docs/featureforge/specs/2026-01-22-document-review-system-design.md";
    let plan_path = "docs/featureforge/plans/2026-01-22-document-review-system.md";

    fs::create_dir_all(repo.join("docs/featureforge/specs")).expect("spec directory should exist");
    write_minimal_plan_fidelity_spec(repo, spec_path);
    write_file(
        &repo.join(plan_path),
        "# Draft Plan\n\n**Workflow State:** Draft\n**Plan Revision:** 1\n**Execution Mode:** none\n**Source Spec:** `docs/featureforge/specs/2026-01-22-document-review-system-design.md`\n**Source Spec Revision:** 1\n**Last Reviewed By:** writing-plans\n\n## Requirement Coverage Matrix\n\n- REQ-001 -> Task 1\n\n## Task 1: Prepare the draft plan for review\n\n**Spec Coverage:** REQ-001\n**Goal:** The draft plan is ready for engineering review.\n\n**Context:**\n- Spec Coverage: REQ-001.\n\n**Constraints:**\n- Keep the fixture minimal.\n**Done when:**\n- The draft plan is ready for engineering review.\n\n**Files:**\n- Test: `tests/workflow_runtime.rs`\n\n- [ ] **Step 1: Review the draft plan**\n",
    );
    write_plan_fidelity_review_artifact(
        repo,
        PlanFidelityReviewArtifactInput {
            artifact_rel: ".featureforge/reviews/plan-fidelity-stale-fingerprint.md",
            plan_path,
            plan_revision: 1,
            spec_path,
            spec_revision: 1,
            review_verdict: "pass",
            reviewer_source: "fresh-context-subagent",
            reviewer_id: "reviewer-stale-fingerprint",
            verified_surfaces: &PLAN_FIDELITY_REQUIRED_SURFACES,
        },
    );
    replace_in_file(
        &repo.join(plan_path),
        "Prepare the draft plan for review",
        "Prepare the changed draft plan for review",
    );

    let output = run_rust_featureforge(
        repo,
        state,
        &[
            "workflow",
            "plan-fidelity",
            "record",
            "--plan",
            plan_path,
            "--review-artifact",
            ".featureforge/reviews/plan-fidelity-stale-fingerprint.md",
            "--json",
        ],
        "workflow plan-fidelity record should reject stale plan-fingerprint bindings",
    );
    assert!(
        !output.status.success(),
        "record should fail closed when the review artifact fingerprint no longer matches the draft plan, got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn workflow_plan_fidelity_record_resolves_repo_relative_paths_from_subdirectories() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-plan-fidelity-subdir");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_path = "docs/featureforge/specs/2026-01-22-document-review-system-design.md";
    let plan_path = "docs/featureforge/plans/2026-01-22-document-review-system.md";

    fs::create_dir_all(repo.join("docs/featureforge/specs")).expect("spec directory should exist");
    write_minimal_plan_fidelity_spec(repo, spec_path);
    write_file(
        &repo.join(plan_path),
        "# Draft Plan\n\n**Workflow State:** Draft\n**Plan Revision:** 1\n**Execution Mode:** none\n**Source Spec:** `docs/featureforge/specs/2026-01-22-document-review-system-design.md`\n**Source Spec Revision:** 1\n**Last Reviewed By:** writing-plans\n\n## Requirement Coverage Matrix\n\n- REQ-001 -> Task 1\n\n## Task 1: Prepare the draft plan for review\n\n**Spec Coverage:** REQ-001\n**Goal:** The draft plan is ready for engineering review.\n\n**Context:**\n- Spec Coverage: REQ-001.\n\n**Constraints:**\n- Keep the fixture minimal.\n**Done when:**\n- The draft plan is ready for engineering review.\n\n**Files:**\n- Test: `tests/workflow_runtime.rs`\n\n- [ ] **Step 1: Review the draft plan**\n",
    );
    write_plan_fidelity_review_artifact(
        repo,
        PlanFidelityReviewArtifactInput {
            artifact_rel: ".featureforge/reviews/plan-fidelity-subdir.md",
            plan_path,
            plan_revision: 1,
            spec_path,
            spec_revision: 1,
            review_verdict: "pass",
            reviewer_source: "fresh-context-subagent",
            reviewer_id: "reviewer-subdir",
            verified_surfaces: &PLAN_FIDELITY_REQUIRED_SURFACES,
        },
    );
    fs::create_dir_all(repo.join("src/runtime")).expect("subdirectory should exist");

    let receipt_json = run_workflow_plan_fidelity_json_from_dir(
        &repo.join("src/runtime"),
        state,
        &[
            "record",
            "--plan",
            plan_path,
            "--review-artifact",
            ".featureforge/reviews/plan-fidelity-subdir.md",
            "--json",
        ],
        "workflow plan-fidelity record should resolve repo-relative paths from subdirectories",
    );
    assert_eq!(receipt_json["status"], "ok");
}

#[test]
fn workflow_plan_fidelity_record_rejects_malformed_spec_requirement_index() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-plan-fidelity-malformed-spec");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_path = "docs/featureforge/specs/2026-01-22-document-review-system-design.md";
    let plan_path = "docs/featureforge/plans/2026-01-22-document-review-system.md";

    fs::create_dir_all(repo.join("docs/featureforge/specs")).expect("spec directory should exist");
    write_file(
        &repo.join(spec_path),
        "# Approved Spec\n\n**Workflow State:** CEO Approved\n**Spec Revision:** 1\n**Last Reviewed By:** plan-ceo-review\n\n## Summary\n\nMalformed fixture without a Requirement Index.\n",
    );
    write_file(
        &repo.join(plan_path),
        "# Draft Plan\n\n**Workflow State:** Draft\n**Plan Revision:** 1\n**Execution Mode:** none\n**Source Spec:** `docs/featureforge/specs/2026-01-22-document-review-system-design.md`\n**Source Spec Revision:** 1\n**Last Reviewed By:** writing-plans\n\n## Requirement Coverage Matrix\n\n- REQ-001 -> Task 1\n\n## Task 1: Prepare the draft plan for review\n\n**Spec Coverage:** REQ-001\n**Goal:** The draft plan is ready for engineering review.\n\n**Context:**\n- Spec Coverage: REQ-001.\n\n**Constraints:**\n- Keep the fixture minimal.\n**Done when:**\n- The draft plan is ready for engineering review.\n\n**Files:**\n- Test: `tests/workflow_runtime.rs`\n\n- [ ] **Step 1: Review the draft plan**\n",
    );
    write_plan_fidelity_review_artifact(
        repo,
        PlanFidelityReviewArtifactInput {
            artifact_rel: ".featureforge/reviews/plan-fidelity-malformed-spec.md",
            plan_path,
            plan_revision: 1,
            spec_path,
            spec_revision: 1,
            review_verdict: "pass",
            reviewer_source: "fresh-context-subagent",
            reviewer_id: "reviewer-malformed-spec",
            verified_surfaces: &PLAN_FIDELITY_REQUIRED_SURFACES,
        },
    );

    let output = run_rust_featureforge(
        repo,
        state,
        &[
            "workflow",
            "plan-fidelity",
            "record",
            "--plan",
            plan_path,
            "--review-artifact",
            ".featureforge/reviews/plan-fidelity-malformed-spec.md",
            "--json",
        ],
        "workflow plan-fidelity record should reject malformed approved specs",
    );
    assert!(
        !output.status.success(),
        "record should fail closed on malformed approved specs, got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let status_json = parse_json(
        &run_rust_featureforge(
            repo,
            state,
            &["workflow", "status", "--refresh"],
            "workflow status should fail closed after malformed-spec review recording fails",
        ),
        "workflow status should fail closed after malformed-spec review recording fails",
    );
    assert_eq!(status_json["status"], "plan_draft");
    assert_eq!(status_json["next_skill"], "featureforge:writing-plans");
}

#[test]
fn workflow_plan_fidelity_record_rejects_invalid_ceo_review_provenance_on_source_spec() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-plan-fidelity-invalid-spec-reviewer");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_path = "docs/featureforge/specs/2026-01-22-document-review-system-design.md";
    let plan_path = "docs/featureforge/plans/2026-01-22-document-review-system.md";

    fs::create_dir_all(repo.join("docs/featureforge/specs")).expect("spec directory should exist");
    write_file(
        &repo.join(spec_path),
        "# Approved Spec\n\n**Workflow State:** CEO Approved\n**Spec Revision:** 1\n**Last Reviewed By:** brainstorming\n\n## Requirement Index\n\n- [REQ-001][behavior] The draft plan must complete an independent fidelity review before engineering review.\n",
    );
    write_file(
        &repo.join(plan_path),
        "# Draft Plan\n\n**Workflow State:** Draft\n**Plan Revision:** 1\n**Execution Mode:** none\n**Source Spec:** `docs/featureforge/specs/2026-01-22-document-review-system-design.md`\n**Source Spec Revision:** 1\n**Last Reviewed By:** writing-plans\n\n## Requirement Coverage Matrix\n\n- REQ-001 -> Task 1\n\n## Task 1: Prepare the draft plan for review\n\n**Spec Coverage:** REQ-001\n**Goal:** The draft plan is ready for engineering review.\n\n**Context:**\n- Spec Coverage: REQ-001.\n\n**Constraints:**\n- Keep the fixture minimal.\n**Done when:**\n- The draft plan is ready for engineering review.\n\n**Files:**\n- Test: `tests/workflow_runtime.rs`\n\n- [ ] **Step 1: Review the draft plan**\n",
    );
    write_plan_fidelity_review_artifact(
        repo,
        PlanFidelityReviewArtifactInput {
            artifact_rel: ".featureforge/reviews/plan-fidelity-invalid-spec-reviewer.md",
            plan_path,
            plan_revision: 1,
            spec_path,
            spec_revision: 1,
            review_verdict: "pass",
            reviewer_source: "fresh-context-subagent",
            reviewer_id: "reviewer-invalid-spec-reviewer",
            verified_surfaces: &PLAN_FIDELITY_REQUIRED_SURFACES,
        },
    );

    let output = run_rust_featureforge(
        repo,
        state,
        &[
            "workflow",
            "plan-fidelity",
            "record",
            "--plan",
            plan_path,
            "--review-artifact",
            ".featureforge/reviews/plan-fidelity-invalid-spec-reviewer.md",
            "--json",
        ],
        "workflow plan-fidelity record should reject invalid CEO review provenance on the source spec",
    );
    assert!(
        !output.status.success(),
        "record should fail closed when the source spec is not workflow-valid CEO-approved, got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let status_json = parse_json(
        &run_rust_featureforge(
            repo,
            state,
            &["workflow", "status", "--refresh"],
            "workflow status should fail closed when the source spec approval headers are semantically invalid",
        ),
        "workflow status should fail closed when the source spec approval headers are semantically invalid",
    );
    assert_eq!(status_json["next_skill"], "featureforge:plan-ceo-review");
}

#[test]
fn workflow_plan_fidelity_record_rejects_out_of_repo_source_spec_paths() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-plan-fidelity-external-source-spec");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_path = "docs/featureforge/specs/2026-01-22-document-review-system-design.md";
    let plan_path = "docs/featureforge/plans/2026-01-22-document-review-system.md";

    fs::create_dir_all(repo.join("docs/featureforge/specs")).expect("spec directory should exist");
    write_minimal_plan_fidelity_spec(repo, spec_path);
    write_file(
        &repo.join(plan_path),
        "# Draft Plan\n\n**Workflow State:** Draft\n**Plan Revision:** 1\n**Execution Mode:** none\n**Source Spec:** `../external/docs/featureforge/specs/outside-spec.md`\n**Source Spec Revision:** 1\n**Last Reviewed By:** writing-plans\n\n## Requirement Coverage Matrix\n\n- REQ-001 -> Task 1\n\n## Task 1: Prepare the draft plan for review\n\n**Spec Coverage:** REQ-001\n**Goal:** The draft plan is ready for engineering review.\n\n**Context:**\n- Spec Coverage: REQ-001.\n\n**Constraints:**\n- Keep the fixture minimal.\n**Done when:**\n- The draft plan is ready for engineering review.\n\n**Files:**\n- Test: `tests/workflow_runtime.rs`\n\n- [ ] **Step 1: Review the draft plan**\n",
    );
    write_plan_fidelity_review_artifact(
        repo,
        PlanFidelityReviewArtifactInput {
            artifact_rel: ".featureforge/reviews/plan-fidelity-external-source-spec.md",
            plan_path,
            plan_revision: 1,
            spec_path,
            spec_revision: 1,
            review_verdict: "pass",
            reviewer_source: "fresh-context-subagent",
            reviewer_id: "reviewer-external-source-spec",
            verified_surfaces: &PLAN_FIDELITY_REQUIRED_SURFACES,
        },
    );

    let output = run_rust_featureforge(
        repo,
        state,
        &[
            "workflow",
            "plan-fidelity",
            "record",
            "--plan",
            plan_path,
            "--review-artifact",
            ".featureforge/reviews/plan-fidelity-external-source-spec.md",
            "--json",
        ],
        "workflow plan-fidelity record should reject out-of-repo Source Spec paths",
    );
    assert!(
        !output.status.success(),
        "record should fail closed when Source Spec escapes the repo, got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
#[cfg(unix)]
fn canonical_workflow_status_refresh_preserves_route_when_manifest_write_fails() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-manifest-write-conflict");
    let repo = repo_dir.path();
    let state = state_dir.path();

    install_full_contract_ready_artifacts(repo);

    let original_permissions = fs::metadata(state)
        .expect("state dir metadata should be readable")
        .permissions();
    let mut read_only_permissions = original_permissions.clone();
    read_only_permissions.set_mode(0o555);
    fs::set_permissions(state, read_only_permissions).expect("state dir should become read-only");

    let output = run_rust_featureforge(
        repo,
        state,
        &["workflow", "status", "--refresh"],
        "workflow status refresh with non-writable state dir",
    );

    fs::set_permissions(state, original_permissions)
        .expect("state dir permissions should be restorable");

    assert!(
        output.status.success(),
        "status refresh should still succeed when manifest persistence fails, got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let status_json = parse_json(
        &output,
        "workflow status refresh with non-writable state dir",
    );
    assert_eq!(status_json["status"], "implementation_ready");
    assert_ne!(status_json["next_skill"], "featureforge:brainstorming");
    assert!(
        status_json["reason_codes"]
            .as_array()
            .expect("reason_codes should stay an array")
            .iter()
            .any(|value| value == &Value::String(String::from("manifest_write_conflict")))
    );
    assert_eq!(
        status_json["diagnostics"][0]["code"],
        Value::String(String::from("manifest_write_conflict"))
    );
}

#[test]
fn canonical_workflow_status_refresh_accepts_active_implementation_target_specs() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-active-implementation-target");
    let repo = repo_dir.path();
    let state = state_dir.path();

    let spec_rel = "docs/featureforge/specs/2026-04-01-runtime-shift.md";
    let plan_rel = "docs/featureforge/plans/2026-04-01-runtime-shift.md";
    write_file(
        &repo.join("docs/featureforge/specs/ACTIVE_IMPLEMENTATION_TARGET.md"),
        &format!(
            "# Active Implementation Target\n\n**Status:** authoritative index for the active April supersession-aware documentation corpus\n\n## Active Normative Specs\n\n- `{spec_rel}`\n"
        ),
    );
    write_file(
        &repo.join("docs/featureforge/specs/2026-03-01-legacy-design.md"),
        "# Legacy Design\n\n**Workflow State:** CEO Approved\n**Spec Revision:** 1\n**Last Reviewed By:** plan-ceo-review\n\n## Requirement Index\n\n- [REQ-LEGACY][behavior] Historical fixture only.\n",
    );
    write_file(
        &repo.join(spec_rel),
        "# Runtime Shift\n\n**Workflow State:** Implementation Target\n**Spec Revision:** 1\n**Last Reviewed By:** clean-context review loop\n**Implementation Target:** Current\n\n## Requirement Index\n\n- [REQ-001][behavior] Active implementation-target specs participate in workflow routing.\n",
    );
    write_file(
        &repo.join(plan_rel),
        &format!(
            "# Runtime Shift Plan\n\n**Workflow State:** Engineering Approved\n**Plan Revision:** 1\n**Execution Mode:** none\n**Source Spec:** `{spec_rel}`\n**Source Spec Revision:** 1\n**Last Reviewed By:** plan-eng-review\n\n## Requirement Coverage Matrix\n\n- REQ-001 -> Task 1\n\n## Task 1: Route the active implementation target\n\n**Spec Coverage:** REQ-001\n**Goal:** Workflow status resolves the April implementation-target spec.\n\n**Context:**\n- Spec Coverage: REQ-001.\n\n**Constraints:**\n- Keep the fixture minimal.\n**Done when:**\n- Workflow status resolves the April implementation-target spec.\n\n**Files:**\n- Test: `tests/workflow_runtime.rs`\n\n- [ ] **Step 1: Recognize the active implementation target**\n"
        ),
    );

    let status_json = parse_json(
        &run_rust_featureforge(
            repo,
            state,
            &["workflow", "status", "--refresh"],
            "workflow status refresh with active implementation-target spec",
        ),
        "workflow status refresh with active implementation-target spec",
    );

    assert_ne!(status_json["status"], "stale_plan");
    assert_eq!(status_json["spec_path"], Value::from(spec_rel));
    assert_eq!(status_json["plan_path"], Value::from(plan_rel));
}

#[test]
fn canonical_workflow_status_routes_lone_stale_approved_plan_as_stale() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-stale-approved-plan");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_file(
        &repo.join("docs/featureforge/specs/2026-01-22-document-review-system-design-v2.md"),
        "# Approved Spec, Newer Path\n\n**Workflow State:** CEO Approved\n**Spec Revision:** 1\n**Last Reviewed By:** plan-ceo-review\n\n## Notes\n",
    );
    write_file(
        &repo.join("docs/featureforge/plans/2026-01-22-document-review-system.md"),
        "# Approved Plan, Stale Source Path\n\n**Workflow State:** Engineering Approved\n**Plan Revision:** 1\n**Execution Mode:** none\n**Source Spec:** `docs/featureforge/specs/2026-01-22-document-review-system-design.md`\n**Source Spec Revision:** 1\n**Last Reviewed By:** plan-eng-review\n\n## Requirement Coverage Matrix\n\n- REQ-001 -> Task 1\n\n## Task 1: Preserve the stale source path case\n\n**Spec Coverage:** REQ-001\n**Goal:** The plan remains structurally valid while its source-spec path goes stale.\n\n**Context:**\n- Spec Coverage: REQ-001.\n\n**Constraints:**\n- Keep the fixture minimal.\n**Done when:**\n- The plan remains structurally valid while its source-spec path goes stale.\n\n**Files:**\n- Test: `tests/workflow_runtime.rs`\n\n- [ ] **Step 1: Detect the stale source path**\n",
    );

    let status_json = parse_json(
        &run_rust_featureforge(
            repo,
            state,
            &["workflow", "status", "--refresh"],
            "rust canonical workflow status refresh for stale approved plan",
        ),
        "rust canonical workflow status refresh for stale approved plan",
    );

    assert_eq!(status_json["status"], "stale_plan");
    assert_eq!(status_json["next_skill"], "featureforge:writing-plans");
    assert_eq!(status_json["contract_state"], "stale");
    assert_eq!(status_json["reason_codes"][0], "stale_spec_plan_linkage");
    assert_eq!(
        status_json["diagnostics"][0]["code"],
        "stale_spec_plan_linkage"
    );
}

#[test]
fn canonical_workflow_status_routes_stale_source_revision_as_stale() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-stale-approved-revision");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_file(
        &repo.join("docs/featureforge/specs/2026-01-22-document-review-system-design.md"),
        "# Approved Spec\n\n**Workflow State:** CEO Approved\n**Spec Revision:** 2\n**Last Reviewed By:** plan-ceo-review\n\n## Requirement Index\n\n- [REQ-001][behavior] The route should expose stale approved plans when the source-spec revision drifts.\n",
    );
    write_file(
        &repo.join("docs/featureforge/plans/2026-01-22-document-review-system.md"),
        "# Approved Plan, Stale Source Revision\n\n**Workflow State:** Engineering Approved\n**Plan Revision:** 1\n**Execution Mode:** none\n**Source Spec:** `docs/featureforge/specs/2026-01-22-document-review-system-design.md`\n**Source Spec Revision:** 1\n**Last Reviewed By:** plan-eng-review\n\n## Requirement Coverage Matrix\n\n- REQ-001 -> Task 1\n\n## Task 1: Preserve the stale source revision case\n\n**Spec Coverage:** REQ-001\n**Goal:** The plan remains structurally valid while its source-spec revision goes stale.\n\n**Context:**\n- Spec Coverage: REQ-001.\n\n**Constraints:**\n- Keep the fixture minimal.\n**Done when:**\n- The plan remains structurally valid while its source-spec revision goes stale.\n\n**Files:**\n- Test: `tests/workflow_runtime.rs`\n\n- [ ] **Step 1: Detect the stale source revision**\n",
    );

    let status_json = parse_json(
        &run_rust_featureforge(
            repo,
            state,
            &["workflow", "status", "--refresh"],
            "rust canonical workflow status refresh for stale approved revision",
        ),
        "rust canonical workflow status refresh for stale approved revision",
    );

    assert_eq!(status_json["status"], "stale_plan");
    assert_eq!(status_json["next_skill"], "featureforge:writing-plans");
    assert_eq!(status_json["contract_state"], "stale");
    assert_eq!(status_json["reason_codes"][0], "stale_spec_plan_linkage");
    assert_eq!(
        status_json["diagnostics"][0]["code"],
        "stale_spec_plan_linkage"
    );
}

#[cfg(unix)]
#[test]
fn workflow_status_argv0_alias_no_longer_dispatches_to_canonical_tree() {
    use std::os::unix::fs::symlink;

    let (repo_dir, state_dir) = init_repo("workflow-runtime-argv0");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_file(
        &repo.join("docs/featureforge/specs/2026-03-24-draft-spec-design.md"),
        "# Draft Spec\n\n**Workflow State:** Draft\n**Spec Revision:** 1\n**Last Reviewed By:** brainstorming\n",
    );

    let alias_dir = TempDir::new().expect("alias tempdir should be available");
    let alias_path = alias_dir.path().join("featureforge-workflow-status");
    symlink(cargo_bin("featureforge"), &alias_path)
        .expect("argv0 alias symlink should be creatable");

    let alias_output = run(
        {
            let mut command = Command::new(&alias_path);
            command
                .current_dir(repo)
                .env("FEATUREFORGE_STATE_DIR", state)
                .args(["--refresh"]);
            command
        },
        "rust argv0 workflow-status alias",
    );
    assert!(
        !alias_output.status.success(),
        "argv0 alias should no longer dispatch successfully\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&alias_output.stdout),
        String::from_utf8_lossy(&alias_output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&alias_output.stderr).contains("featureforge-workflow-status"),
        "stderr should mention the rejected legacy alias entrypoint\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&alias_output.stdout),
        String::from_utf8_lossy(&alias_output.stderr)
    );
}

#[test]
fn canonical_workflow_status_refresh_recovers_old_manifest_after_slug_change() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-cross-slug-old");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_path = "docs/featureforge/specs/2026-03-24-cross-slug-design.md";
    let expected_plan = "docs/featureforge/plans/2026-03-24-cross-slug-plan.md";

    write_file(
        &repo.join(spec_path),
        "# Approved Spec\n\n**Workflow State:** CEO Approved\n**Spec Revision:** 1\n**Last Reviewed By:** plan-ceo-review\n",
    );

    let old_identity = discover_repo_identity(repo).expect("old repo identity should resolve");
    let old_manifest_path = manifest_path(&old_identity, state);
    write_manifest(
        &old_manifest_path,
        &WorkflowManifest {
            version: 1,
            repo_root: old_identity.repo_root.to_string_lossy().into_owned(),
            branch: old_identity.branch_name.clone(),
            expected_spec_path: spec_path.to_owned(),
            expected_plan_path: expected_plan.to_owned(),
            status: String::from("spec_approved_needs_plan"),
            next_skill: String::from("featureforge:writing-plans"),
            reason: String::from("missing_expected_plan,expect_set"),
            note: String::from("missing_expected_plan,expect_set"),
            updated_at: String::from("2026-03-24T00:00:00Z"),
        },
    );

    set_remote_url(
        repo,
        "https://example.com/example/workflow-runtime-cross-slug-new.git",
    );
    let new_identity = discover_repo_identity(repo).expect("new repo identity should resolve");
    let new_manifest_path = manifest_path(&new_identity, state);
    assert_ne!(
        old_manifest_path, new_manifest_path,
        "slug change should move the manifest path"
    );
    let recovered = recover_slug_changed_manifest(&new_identity, state, &new_manifest_path)
        .expect("cross-slug manifest should be recoverable from sibling state");
    assert_eq!(recovered.expected_plan_path, expected_plan);

    let route = WorkflowRuntime {
        identity: new_identity.clone(),
        state_dir: state.to_path_buf(),
        manifest_path: new_manifest_path.clone(),
        manifest: Some(recovered.clone()),
        manifest_warning: None,
        manifest_recovery_reasons: vec![String::from("repo_slug_recovered")],
    }
    .status()
    .expect("status should preserve the recovered expected plan path");
    assert_eq!(route.plan_path, expected_plan);

    let refreshed_route = WorkflowRuntime {
        identity: new_identity,
        state_dir: state.to_path_buf(),
        manifest_path: new_manifest_path.clone(),
        manifest: Some(recovered),
        manifest_warning: None,
        manifest_recovery_reasons: vec![String::from("repo_slug_recovered")],
    }
    .status_refresh()
    .expect("status refresh should preserve recovery metadata and write the new manifest");

    assert_eq!(refreshed_route.status, "spec_approved_needs_plan");
    assert_eq!(refreshed_route.plan_path, expected_plan);
    assert!(refreshed_route.reason.contains("repo_slug_recovered"));
    assert!(
        refreshed_route
            .reason_codes
            .iter()
            .any(|value| value == "repo_slug_recovered")
    );

    let new_manifest_json = fs::read_to_string(&new_manifest_path)
        .expect("recovered manifest should be written at the new slug path");
    assert!(new_manifest_json.contains(expected_plan));
}

#[test]
fn shared_markdown_scan_helper_collects_nested_markdown_only() {
    let fixture = TempDir::new().expect("markdown scan fixture should exist");
    write_file(&fixture.path().join("top.md"), "# top\n");
    write_file(&fixture.path().join("nested/plan.md"), "# nested\n");
    write_file(&fixture.path().join("nested/notes.txt"), "not markdown\n");

    let mut actual = markdown_scan_support::markdown_files_under(fixture.path())
        .into_iter()
        .map(|path| {
            path.strip_prefix(fixture.path())
                .expect("fixture file should stay under fixture root")
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect::<Vec<_>>();
    actual.sort();

    assert_eq!(
        actual,
        vec![String::from("nested/plan.md"), String::from("top.md")]
    );
}

#[test]
fn canonical_manifest_path_distinguishes_exact_branch_names() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-branch-identity");
    let repo = repo_dir.path();
    let state = state_dir.path();

    let mut git_checkout = Command::new("git");
    git_checkout
        .args(["checkout", "-B", "feature/x"])
        .current_dir(repo);
    run_checked(git_checkout, "git checkout feature/x");
    let slash_identity = discover_repo_identity(repo).expect("feature/x identity should resolve");
    let slash_manifest_path = manifest_path(&slash_identity, state);

    let mut git_checkout = Command::new("git");
    git_checkout
        .args(["checkout", "-B", "feature-x"])
        .current_dir(repo);
    run_checked(git_checkout, "git checkout feature-x");
    let dash_identity = discover_repo_identity(repo).expect("feature-x identity should resolve");
    let dash_manifest_path = manifest_path(&dash_identity, state);

    assert_ne!(
        slash_manifest_path, dash_manifest_path,
        "workflow manifests should stay exact-branch scoped",
    );
}

#[test]
fn canonical_manifest_path_uses_canonical_repo_slug_directory() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-manifest-slug");
    let repo = repo_dir.path();
    let state = state_dir.path();

    let identity = discover_repo_identity(repo).expect("repo identity should resolve");
    let manifest = manifest_path(&identity, state);

    assert_eq!(
        manifest
            .parent()
            .expect("manifest path should have a parent"),
        state.join("projects").join(repo_slug(repo))
    );
}

#[test]
fn canonical_workflow_status_refresh_limits_cross_slug_manifest_recovery_scan() {
    let (repo_dir, state_dir) = init_repo("workflow-runtime-cross-slug-budget");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_path = "docs/featureforge/specs/2026-03-24-budget-limit-design.md";
    let expected_plan = "docs/featureforge/plans/2026-03-24-budget-limit-plan.md";

    write_file(
        &repo.join(spec_path),
        "# Approved Spec\n\n**Workflow State:** CEO Approved\n**Spec Revision:** 1\n**Last Reviewed By:** plan-ceo-review\n",
    );

    let current_identity =
        discover_repo_identity(repo).expect("current repo identity should resolve");
    let current_manifest_path = manifest_path(&current_identity, state);
    let manifest_name = current_manifest_path
        .file_name()
        .expect("manifest path should have a file name")
        .to_owned();

    for index in 1..=12 {
        let decoy_dir = state.join("projects").join(format!("decoy-{index:02}"));
        write_manifest(
            &decoy_dir.join(&manifest_name),
            &WorkflowManifest {
                version: 1,
                repo_root: format!("/tmp/not-the-current-repo-{index:02}"),
                branch: current_identity.branch_name.clone(),
                expected_spec_path: String::new(),
                expected_plan_path: String::new(),
                status: String::from("needs_brainstorming"),
                next_skill: String::from("featureforge:brainstorming"),
                reason: String::from("decoy"),
                note: String::from("decoy"),
                updated_at: String::from("2026-03-24T00:00:00Z"),
            },
        );
    }

    write_manifest(
        &state.join("projects/zzz-old-slug").join(&manifest_name),
        &WorkflowManifest {
            version: 1,
            repo_root: current_identity.repo_root.to_string_lossy().into_owned(),
            branch: current_identity.branch_name.clone(),
            expected_spec_path: spec_path.to_owned(),
            expected_plan_path: expected_plan.to_owned(),
            status: String::from("spec_approved_needs_plan"),
            next_skill: String::from("featureforge:writing-plans"),
            reason: String::from("repo_slug_recovered"),
            note: String::from("repo_slug_recovered"),
            updated_at: String::from("2026-03-24T00:00:00Z"),
        },
    );

    let status_json = parse_json(
        &run_rust_featureforge(
            repo,
            state,
            &["workflow", "status", "--refresh"],
            "rust canonical workflow status refresh for slug recovery scan budget",
        ),
        "rust canonical workflow status refresh for slug recovery scan budget",
    );

    assert_eq!(status_json["status"], "spec_approved_needs_plan");
    assert_eq!(status_json["plan_path"], "");
    assert!(
        !status_json["reason"]
            .as_str()
            .unwrap_or("")
            .contains("repo_slug_recovered")
    );

    let manifest_json = fs::read_to_string(current_manifest_path)
        .expect("current manifest should be written after refresh");
    assert!(!manifest_json.contains(expected_plan));
}

#[test]
fn canonical_workflow_status_accepts_manifest_selected_plan_with_legacy_symlink_repo_root() {
    let (repo_dir, state_dir) = init_repo("workflow-status-symlink-manifest");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let alias_root = state.join("workflow-status-symlink-manifest-checkout");
    create_dir_symlink(repo, &alias_root);

    let spec_path = "docs/featureforge/specs/2026-03-24-symlink-manifest-spec.md";
    let plan_a = "docs/featureforge/plans/2026-03-24-symlink-manifest-a.md";
    let plan_b = "docs/featureforge/plans/2026-03-24-symlink-manifest-b.md";

    write_file(
        &repo.join(spec_path),
        "# Approved Spec\n\n**Workflow State:** CEO Approved\n**Spec Revision:** 1\n**Last Reviewed By:** plan-ceo-review\n",
    );
    for plan_path in [plan_a, plan_b] {
        write_file(
            &repo.join(plan_path),
            &format!(
                "# Draft Plan\n\n**Workflow State:** Draft\n**Plan Revision:** 1\n**Execution Mode:** none\n**Source Spec:** `{spec_path}`\n**Source Spec Revision:** 1\n**Last Reviewed By:** writing-plans\n"
            ),
        );
    }

    let identity = discover_repo_identity(repo).expect("repo identity should resolve");
    write_manifest(
        &manifest_path(&identity, state),
        &WorkflowManifest {
            version: 1,
            repo_root: alias_root.to_string_lossy().into_owned(),
            branch: identity.branch_name.clone(),
            expected_spec_path: spec_path.to_owned(),
            expected_plan_path: plan_a.to_owned(),
            status: String::from("plan_draft"),
            next_skill: String::from("featureforge:plan-eng-review"),
            reason: String::from("legacy-symlink-manifest"),
            note: String::from("legacy-symlink-manifest"),
            updated_at: String::from("2026-03-25T00:00:00Z"),
        },
    );

    let status_json = parse_json(
        &run_rust_featureforge(
            &alias_root,
            state,
            &["workflow", "status"],
            "workflow status should accept legacy symlinked manifest repo roots",
        ),
        "workflow status should accept legacy symlinked manifest repo roots",
    );

    assert_eq!(status_json["status"], "plan_draft");
    assert_eq!(status_json["plan_path"], plan_a);
    assert!(
        !status_json["reason_codes"]
            .as_array()
            .unwrap_or(&Vec::new())
            .iter()
            .any(|value| value == "repo_root_mismatch"),
        "legacy symlink manifests should not be treated as repo_root mismatches"
    );
}

#[test]
fn canonical_workflow_status_ignores_manifest_selected_spec_when_branch_mismatches() {
    let (repo_dir, state_dir) = init_repo("workflow-status-manifest-branch-mismatch");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_a = "docs/featureforge/specs/2026-03-24-branch-mismatch-a.md";
    let spec_b = "docs/featureforge/specs/2026-03-24-branch-mismatch-b.md";

    for spec_path in [spec_a, spec_b] {
        write_file(
            &repo.join(spec_path),
            "# Approved Spec\n\n**Workflow State:** CEO Approved\n**Spec Revision:** 1\n**Last Reviewed By:** plan-ceo-review\n",
        );
    }

    let identity = discover_repo_identity(repo).expect("repo identity should resolve");
    write_manifest(
        &manifest_path(&identity, state),
        &WorkflowManifest {
            version: 1,
            repo_root: identity.repo_root.to_string_lossy().into_owned(),
            branch: String::from("other-branch"),
            expected_spec_path: spec_a.to_owned(),
            expected_plan_path: String::new(),
            status: String::from("spec_approved_needs_plan"),
            next_skill: String::from("featureforge:writing-plans"),
            reason: String::from("stale-branch-manifest"),
            note: String::from("stale-branch-manifest"),
            updated_at: String::from("2026-03-24T00:00:00Z"),
        },
    );

    let status_json = parse_json(
        &run_rust_featureforge(
            repo,
            state,
            &["workflow", "status"],
            "workflow status should ignore a branch-mismatched manifest-selected spec",
        ),
        "workflow status should ignore a branch-mismatched manifest-selected spec",
    );

    assert_eq!(status_json["status"], "spec_draft");
    assert_eq!(status_json["plan_path"], "");
    assert!(
        status_json["reason_codes"]
            .as_array()
            .unwrap_or(&Vec::new())
            .iter()
            .any(|value| value == "ambiguous_spec_candidates"),
        "branch-mismatched manifests should not suppress ambiguous current spec candidates"
    );
}

#[test]
fn canonical_workflow_status_ignores_manifest_selected_plan_when_repo_root_mismatches() {
    let (repo_dir, state_dir) = init_repo("workflow-status-manifest-root-mismatch");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_path = "docs/featureforge/specs/2026-03-24-root-mismatch-spec.md";
    let plan_a = "docs/featureforge/plans/2026-03-24-root-mismatch-a.md";
    let plan_b = "docs/featureforge/plans/2026-03-24-root-mismatch-b.md";

    write_file(
        &repo.join(spec_path),
        "# Approved Spec\n\n**Workflow State:** CEO Approved\n**Spec Revision:** 1\n**Last Reviewed By:** plan-ceo-review\n",
    );
    for plan_path in [plan_a, plan_b] {
        write_file(
            &repo.join(plan_path),
            &format!(
                "# Draft Plan\n\n**Workflow State:** Draft\n**Plan Revision:** 1\n**Execution Mode:** none\n**Source Spec:** `{spec_path}`\n**Source Spec Revision:** 1\n**Last Reviewed By:** writing-plans\n"
            ),
        );
    }

    let identity = discover_repo_identity(repo).expect("repo identity should resolve");
    write_manifest(
        &manifest_path(&identity, state),
        &WorkflowManifest {
            version: 1,
            repo_root: String::from("/tmp/another-repo"),
            branch: identity.branch_name.clone(),
            expected_spec_path: spec_path.to_owned(),
            expected_plan_path: plan_a.to_owned(),
            status: String::from("plan_draft"),
            next_skill: String::from("featureforge:plan-eng-review"),
            reason: String::from("stale-root-manifest"),
            note: String::from("stale-root-manifest"),
            updated_at: String::from("2026-03-24T00:00:00Z"),
        },
    );

    let status_json = parse_json(
        &run_rust_featureforge(
            repo,
            state,
            &["workflow", "status"],
            "workflow status should ignore a repo-root-mismatched manifest-selected plan",
        ),
        "workflow status should ignore a repo-root-mismatched manifest-selected plan",
    );

    assert_eq!(status_json["status"], "spec_approved_needs_plan");
    assert_eq!(status_json["plan_path"], "");
    assert!(
        status_json["reason_codes"]
            .as_array()
            .unwrap_or(&Vec::new())
            .iter()
            .any(|value| value == "ambiguous_plan_candidates"),
        "repo-root-mismatched manifests should not suppress ambiguous current plan candidates"
    );
}

#[test]
fn canonical_workflow_status_refresh_recovers_legacy_symlinked_local_repo_manifest() {
    let (repo_dir, state_dir) = init_repo("workflow-local-symlink-recovery");
    let repo = repo_dir.path();
    let state = state_dir.path();
    remove_origin_remote(repo);

    let alias_root = state.join("workflow-local-symlink-checkout");
    create_dir_symlink(repo, &alias_root);

    let spec_path = "docs/featureforge/specs/2026-03-24-local-symlink-spec.md";
    let plan_a = "docs/featureforge/plans/2026-03-24-local-symlink-a.md";
    let plan_b = "docs/featureforge/plans/2026-03-24-local-symlink-b.md";

    write_file(
        &repo.join(spec_path),
        "# Approved Spec\n\n**Workflow State:** CEO Approved\n**Spec Revision:** 1\n**Last Reviewed By:** plan-ceo-review\n",
    );
    for plan_path in [plan_a, plan_b] {
        write_file(
            &repo.join(plan_path),
            &format!(
                "# Draft Plan\n\n**Workflow State:** Draft\n**Plan Revision:** 1\n**Execution Mode:** none\n**Source Spec:** `{spec_path}`\n**Source Spec Revision:** 1\n**Last Reviewed By:** writing-plans\n"
            ),
        );
    }

    let current_identity = discover_repo_identity(repo).expect("repo identity should resolve");
    let legacy_identity = RepositoryIdentity {
        repo_root: alias_root.clone(),
        remote_url: None,
        branch_name: current_identity.branch_name.clone(),
    };
    let current_manifest_path = manifest_path(&current_identity, state);
    let legacy_manifest_path = manifest_path(&legacy_identity, state);
    assert_ne!(
        current_manifest_path, legacy_manifest_path,
        "canonicalized local repo roots should move the manifest path"
    );

    write_manifest(
        &legacy_manifest_path,
        &WorkflowManifest {
            version: 1,
            repo_root: alias_root.to_string_lossy().into_owned(),
            branch: current_identity.branch_name.clone(),
            expected_spec_path: spec_path.to_owned(),
            expected_plan_path: plan_a.to_owned(),
            status: String::from("plan_draft"),
            next_skill: String::from("featureforge:plan-eng-review"),
            reason: String::from("legacy-local-symlink-manifest"),
            note: String::from("legacy-local-symlink-manifest"),
            updated_at: String::from("2026-03-25T00:00:00Z"),
        },
    );

    let phase_json = parse_json(
        &run_rust_featureforge(
            &alias_root,
            state,
            &["workflow", "phase", "--json"],
            "workflow phase should preserve legacy local symlink manifest recovery reasons",
        ),
        "workflow phase should preserve legacy local symlink manifest recovery reasons",
    );
    assert!(
        phase_json["route"]["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes.iter().any(|value| value == "repo_slug_recovered")),
        "workflow phase should preserve recovered manifest reason codes in the route payload"
    );
    assert_eq!(phase_json["plan_path"], plan_a);

    let handoff_json = parse_json(
        &run_rust_featureforge(
            &alias_root,
            state,
            &["workflow", "handoff", "--json"],
            "workflow handoff should preserve legacy local symlink manifest recovery reasons",
        ),
        "workflow handoff should preserve legacy local symlink manifest recovery reasons",
    );
    assert!(
        handoff_json["route"]["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes.iter().any(|value| value == "repo_slug_recovered")),
        "workflow handoff should preserve recovered manifest reason codes in the route payload"
    );
    assert_eq!(handoff_json["plan_path"], plan_a);

    let status_json = parse_json(
        &run_rust_featureforge(
            &alias_root,
            state,
            &["workflow", "status", "--refresh"],
            "workflow status should recover legacy local symlink manifests",
        ),
        "workflow status should recover legacy local symlink manifests",
    );

    assert_eq!(status_json["status"], "plan_draft");
    assert_eq!(status_json["plan_path"], plan_a);

    let rewritten: WorkflowManifest = serde_json::from_str(
        &fs::read_to_string(&current_manifest_path)
            .expect("canonical manifest should be rewritten on refresh"),
    )
    .expect("rewritten canonical manifest should parse");
    assert_eq!(
        rewritten.repo_root,
        current_identity.repo_root.to_string_lossy().into_owned()
    );
    assert_eq!(rewritten.expected_plan_path, plan_a);
}

#[test]
fn canonical_workflow_operator_accepts_manifest_selected_ready_route_with_extra_approved_candidates()
 {
    let (repo_dir, state_dir) = init_repo("workflow-manifest-selected-ready-route");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-manifest-selected-ready-route";
    let spec_path = "docs/featureforge/specs/2026-03-22-runtime-integration-hardening-design.md";
    let plan_path = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let extra_spec_path = "docs/featureforge/specs/2026-03-24-extra-approved-spec.md";
    let extra_plan_path = "docs/featureforge/plans/2026-03-24-extra-approved-plan.md";

    let mut git_checkout = Command::new("git");
    git_checkout
        .args(["checkout", "-B", "workflow-manifest-ready"])
        .current_dir(repo);
    run_checked(git_checkout, "git checkout workflow-manifest-ready");

    install_full_contract_ready_artifacts(repo);
    enable_session_decision(state, session_key);
    write_file(
        &repo.join(extra_spec_path),
        "# Extra Approved Spec\n\n**Workflow State:** CEO Approved\n**Spec Revision:** 1\n**Last Reviewed By:** plan-ceo-review\n",
    );
    write_file(
        &repo.join(extra_plan_path),
        &format!(
            "# Extra Approved Plan\n\n**Workflow State:** Engineering Approved\n**Plan Revision:** 1\n**Execution Mode:** none\n**Source Spec:** `{spec_path}`\n**Source Spec Revision:** 1\n**Last Reviewed By:** plan-eng-review\n"
        ),
    );

    let identity = discover_repo_identity(repo).expect("repo identity should resolve");
    write_manifest(
        &manifest_path(&identity, state),
        &WorkflowManifest {
            version: 1,
            repo_root: fs::canonicalize(repo)
                .expect("repo root should canonicalize")
                .to_string_lossy()
                .into_owned(),
            branch: identity.branch_name.clone(),
            expected_spec_path: String::from(spec_path),
            expected_plan_path: String::from(plan_path),
            status: String::from("implementation_ready"),
            next_skill: String::new(),
            reason: String::from("implementation_ready"),
            note: String::from("implementation_ready"),
            updated_at: String::from("2026-03-24T00:00:00Z"),
        },
    );

    let status_json = parse_json(
        &run_rust_featureforge(
            repo,
            state,
            &["workflow", "status"],
            "workflow status for manifest-selected ready route",
        ),
        "workflow status for manifest-selected ready route",
    );
    assert_eq!(status_json["status"], "implementation_ready");
    assert_eq!(status_json["spec_path"], spec_path);
    assert_eq!(status_json["plan_path"], plan_path);

    let phase_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "phase", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow phase for manifest-selected ready route",
        ),
        "workflow phase for manifest-selected ready route",
    );
    assert_eq!(phase_json["phase"], "execution_preflight");
    assert_eq!(phase_json["route_status"], "implementation_ready");
    assert_eq!(phase_json["plan_path"], plan_path);

    let handoff_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "handoff", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow handoff for manifest-selected ready route",
        ),
        "workflow handoff for manifest-selected ready route",
    );
    assert_eq!(handoff_json["phase"], "execution_preflight");
    assert_eq!(handoff_json["route_status"], "implementation_ready");
    assert_eq!(handoff_json["plan_path"], plan_path);
}

#[test]
fn canonical_workflow_operator_plan_override_selects_explicit_ready_plan_amid_ambiguity() {
    let (repo_dir, state_dir) = init_repo("workflow-operator-plan-override-ready-route");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_path = "docs/featureforge/specs/2026-03-22-runtime-integration-hardening-design.md";
    let plan_path = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let extra_plan_path = "docs/featureforge/plans/2026-03-24-extra-approved-plan.md";

    let mut git_checkout = Command::new("git");
    git_checkout
        .args(["checkout", "-B", "workflow-operator-explicit-plan"])
        .current_dir(repo);
    run_checked(git_checkout, "git checkout workflow-operator-explicit-plan");

    install_full_contract_ready_artifacts(repo);
    write_file(
        &repo.join(extra_plan_path),
        &format!(
            "# Extra Approved Plan\n\n**Workflow State:** Engineering Approved\n**Plan Revision:** 1\n**Execution Mode:** none\n**Source Spec:** `{spec_path}`\n**Source Spec Revision:** 1\n**Last Reviewed By:** plan-eng-review\n"
        ),
    );

    let explicit_plan_arg = format!("./{plan_path}");
    let operator_json = parse_json(
        &run_rust_featureforge_with_env_real_cli(
            repo,
            state,
            &[
                "workflow",
                "operator",
                "--plan",
                &explicit_plan_arg,
                "--json",
            ],
            &[],
            "workflow operator should honor an explicit approved plan even when the repo-wide resolver is ambiguous",
        ),
        "workflow operator should honor an explicit approved plan even when the repo-wide resolver is ambiguous",
    );

    assert_eq!(operator_json["phase"], "execution_preflight");
    assert_eq!(operator_json["spec_path"], spec_path);
    assert_eq!(operator_json["plan_path"], plan_path);
    if let Some(recommended_command) = operator_json["recommended_command"].as_str() {
        assert!(
            recommended_command.contains(plan_path)
                || recommended_command.contains("<approved-plan-path>"),
            "operator recommended command should either preserve the explicit --plan override path or remain a generic approved-plan template, got {recommended_command:?}"
        );
        assert!(
            !recommended_command.contains(extra_plan_path),
            "operator recommended command should not drift to sibling approved plans, got {recommended_command:?}"
        );
    }
}

#[test]
fn canonical_workflow_operator_rejects_missing_plan_override() {
    let (repo_dir, state_dir) = init_repo("workflow-operator-missing-plan-override");
    let repo = repo_dir.path();
    let state = state_dir.path();
    install_full_contract_ready_artifacts(repo);

    let output = run_rust_featureforge_with_env(
        repo,
        state,
        &[
            "workflow",
            "operator",
            "--plan",
            "docs/featureforge/plans/does-not-exist.md",
            "--json",
        ],
        &[],
        "workflow operator should fail closed when --plan override points to a missing file",
    );
    assert!(
        !output.status.success(),
        "workflow operator should reject missing --plan overrides, got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let payload = if output.stderr.is_empty() {
        &output.stdout
    } else {
        &output.stderr
    };
    let error_json: Value =
        serde_json::from_slice(payload).expect("missing override error should be json");
    assert_eq!(
        error_json["error_class"],
        Value::from("InvalidCommandInput")
    );
    assert!(
        error_json["message"]
            .as_str()
            .is_some_and(|message| message.contains("Workflow plan override file does not exist.")),
        "expected missing override message, got {error_json:?}"
    );
}

#[test]
fn canonical_workflow_operator_plan_override_resolves_override_source_spec_instead_of_route_spec() {
    let (repo_dir, state_dir) = init_repo("workflow-operator-plan-override-source-spec");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let primary_spec_path =
        "docs/featureforge/specs/2026-03-22-runtime-integration-hardening-design.md";
    let primary_plan_path = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let override_spec_path = "docs/featureforge/specs/2026-03-24-override-spec.md";
    let override_plan_path = "docs/featureforge/plans/2026-03-24-override-plan.md";

    let mut git_checkout = Command::new("git");
    git_checkout
        .args(["checkout", "-B", "workflow-operator-explicit-source-spec"])
        .current_dir(repo);
    run_checked(
        git_checkout,
        "git checkout workflow-operator-explicit-source-spec",
    );

    install_full_contract_ready_artifacts(repo);
    write_file(
        &repo.join(override_spec_path),
        "# Override Approved Spec\n\n**Workflow State:** CEO Approved\n**Spec Revision:** 2\n**Last Reviewed By:** plan-ceo-review\n\n## Requirement Index\n\n- [REQ-001][behavior] Override plan routing should use the override source spec.\n",
    );
    write_file(
        &repo.join(override_plan_path),
        &format!(
            "# Override Approved Plan\n\n**Workflow State:** Engineering Approved\n**Plan Revision:** 1\n**Execution Mode:** none\n**Source Spec:** `{override_spec_path}`\n**Source Spec Revision:** 2\n**Last Reviewed By:** plan-eng-review\n"
        ),
    );

    let identity = discover_repo_identity(repo).expect("repo identity should resolve");
    write_manifest(
        &manifest_path(&identity, state),
        &WorkflowManifest {
            version: 1,
            repo_root: fs::canonicalize(repo)
                .expect("repo root should canonicalize")
                .to_string_lossy()
                .into_owned(),
            branch: identity.branch_name.clone(),
            expected_spec_path: String::from(primary_spec_path),
            expected_plan_path: String::from(primary_plan_path),
            status: String::from("implementation_ready"),
            next_skill: String::new(),
            reason: String::from("implementation_ready"),
            note: String::from("implementation_ready"),
            updated_at: String::from("2026-03-24T00:00:00Z"),
        },
    );

    let operator_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &[
                "workflow",
                "operator",
                "--plan",
                override_plan_path,
                "--json",
            ],
            &[],
            "workflow operator should resolve explicit override plan against the override plan source spec",
        ),
        "workflow operator should resolve explicit override plan against the override plan source spec",
    );

    assert_eq!(operator_json["spec_path"], override_spec_path);
    assert_eq!(operator_json["plan_path"], override_plan_path);
    if let Some(recommended_command) = operator_json["recommended_command"].as_str() {
        assert!(
            recommended_command.contains(override_plan_path)
                || recommended_command.contains("<approved-plan-path>"),
            "operator recommended command should either preserve the explicit override plan path or remain a generic approved-plan template, got {recommended_command:?}"
        );
        assert!(
            !recommended_command.contains(primary_plan_path),
            "operator recommended command should not drift back to manifest primary plan path, got {recommended_command:?}"
        );
    }
}

#[test]
fn canonical_workflow_status_treats_ceo_approved_specs_without_ceo_review_as_draft() {
    let (repo_dir, state_dir) = init_repo("workflow-status-approved-spec-reviewer-mismatch");
    let repo = repo_dir.path();
    let state = state_dir.path();

    write_file(
        &repo.join("docs/featureforge/specs/2026-03-24-reviewer-mismatch-design.md"),
        "# Approved Spec\n\n**Workflow State:** CEO Approved\n**Spec Revision:** 1\n**Last Reviewed By:** brainstorming\n\n## Requirement Index\n\n- [REQ-001][behavior] Routing should reject approval-owner drift.\n",
    );

    let status_json = parse_json(
        &run_rust_featureforge(
            repo,
            state,
            &["workflow", "status", "--refresh"],
            "workflow status should reject approved specs without CEO review ownership",
        ),
        "workflow status should reject approved specs without CEO review ownership",
    );

    assert_eq!(status_json["status"], "spec_draft");
    assert_eq!(status_json["next_skill"], "featureforge:plan-ceo-review");
}

#[test]
fn canonical_workflow_status_treats_eng_approved_plans_without_eng_review_as_draft() {
    let (repo_dir, state_dir) = init_repo("workflow-status-approved-plan-reviewer-mismatch");
    let repo = repo_dir.path();
    let state = state_dir.path();

    install_full_contract_ready_artifacts(repo);
    let plan_path =
        repo.join("docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md");
    let source = fs::read_to_string(&plan_path).expect("plan fixture should be readable");
    fs::write(
        &plan_path,
        source.replace(
            "**Last Reviewed By:** plan-eng-review",
            "**Last Reviewed By:** writing-plans",
        ),
    )
    .expect("plan fixture should be writable");

    let status_json = parse_json(
        &run_rust_featureforge(
            repo,
            state,
            &["workflow", "status", "--refresh"],
            "workflow status should reject approved plans without ENG review ownership",
        ),
        "workflow status should reject approved plans without ENG review ownership",
    );

    assert_eq!(status_json["status"], "plan_draft");
    assert_eq!(status_json["next_skill"], "featureforge:writing-plans");
}

#[test]
fn canonical_workflow_phase_omits_session_entry_from_public_json() {
    let (repo_dir, state_dir) = init_repo("workflow-phase-canonical-session-entry");
    let repo = repo_dir.path();
    let state = state_dir.path();

    let phase_json = parse_json(
        &run_rust_featureforge(
            repo,
            state,
            &["workflow", "phase", "--json"],
            "rust canonical workflow phase should read canonical session-entry state",
        ),
        "rust canonical workflow phase should read canonical session-entry state",
    );

    assert!(phase_json.get("session_entry").is_none());
    assert_eq!(phase_json["route"]["schema_version"], 3);
}

#[test]
fn canonical_workflow_operator_routes_ready_plan_without_session_entry_gate() {
    let (repo_dir, state_dir) = init_repo("workflow-no-session-entry-gate");
    let repo = repo_dir.path();
    let state = state_dir.path();

    let mut git_checkout = Command::new("git");
    git_checkout
        .args(["checkout", "-B", "workflow-no-session-entry-gate"])
        .current_dir(repo);
    run_checked(git_checkout, "git checkout workflow-no-session-entry-gate");

    install_full_contract_ready_artifacts(repo);
    let execution_preflight_phase = public_harness_phase_from_spec("execution_preflight");

    let phase_json = parse_json(
        &run_rust_featureforge(
            repo,
            state,
            &["workflow", "phase", "--json"],
            "workflow phase should route directly without a session-entry gate",
        ),
        "workflow phase should route directly without a session-entry gate",
    );
    assert_eq!(
        phase_json["phase"],
        Value::String(execution_preflight_phase.clone())
    );
    assert_eq!(phase_json["next_action"], "execution preflight");
    assert!(phase_json.get("session_entry").is_none());
    assert_eq!(phase_json["schema_version"], 3);
    assert_eq!(phase_json["route"]["schema_version"], 3);

    let doctor_json = parse_json(
        &run_rust_featureforge(
            repo,
            state,
            &["workflow", "doctor", "--json"],
            "workflow doctor should route directly without a session-entry gate",
        ),
        "workflow doctor should route directly without a session-entry gate",
    );
    assert_eq!(
        doctor_json["phase"],
        Value::String(execution_preflight_phase.clone())
    );
    assert_eq!(doctor_json["next_action"], "execution preflight");
    assert!(doctor_json.get("session_entry").is_none());
    assert_eq!(doctor_json["schema_version"], 3);
    assert_eq!(doctor_json["route"]["schema_version"], 3);

    let handoff_json = parse_json(
        &run_rust_featureforge(
            repo,
            state,
            &["workflow", "handoff", "--json"],
            "workflow handoff should route directly without a session-entry gate",
        ),
        "workflow handoff should route directly without a session-entry gate",
    );
    assert_eq!(
        handoff_json["phase"],
        Value::String(execution_preflight_phase)
    );
    assert_eq!(handoff_json["next_action"], "execution preflight");
    assert_eq!(handoff_json["recommended_skill"], Value::from(""));
    assert!(handoff_json.get("session_entry").is_none());
    assert_eq!(handoff_json["schema_version"], 3);
    assert_eq!(handoff_json["route"]["schema_version"], 3);
}

#[test]
fn canonical_workflow_status_ignores_strict_session_entry_gate_env() {
    let (repo_dir, state_dir) = init_repo("workflow-status-strict-session-entry-gate");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-status-strict-session-entry-gate";

    install_full_contract_ready_artifacts(repo);
    let identity = discover_repo_identity(repo).expect("repo identity should resolve");
    let workflow_manifest_path = manifest_path(&identity, state);

    let status_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "status", "--refresh"],
            &[
                ("FEATUREFORGE_SESSION_KEY", session_key),
                ("FEATUREFORGE_WORKFLOW_REQUIRE_SESSION_ENTRY", "1"),
            ],
            "workflow status should ignore the removed strict session-entry gate env",
        ),
        "workflow status should ignore the removed strict session-entry gate env",
    );
    assert_eq!(status_json["schema_version"], 3);
    assert_eq!(status_json["status"], "implementation_ready");
    assert!(
        !status_json["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes.iter().any(|code| {
                code == "session_entry_unresolved" || code == "session_entry_bypassed"
            })),
        "workflow status should not expose removed strict session-entry reason codes"
    );

    let enabled_manifest: WorkflowManifest = serde_json::from_str(
        &fs::read_to_string(&workflow_manifest_path)
            .expect("workflow manifest should be written after strict refresh"),
    )
    .expect("workflow manifest json should parse after strict refresh");
    assert_eq!(
        enabled_manifest.expected_spec_path,
        status_json["spec_path"].as_str().unwrap_or(""),
        "strict refresh should persist the selected expected spec path"
    );
    assert_eq!(
        enabled_manifest.expected_plan_path,
        status_json["plan_path"].as_str().unwrap_or(""),
        "strict refresh should persist the selected expected plan path"
    );

    let bypassed_decision_path = state
        .join("session-entry")
        .join("using-featureforge")
        .join("workflow-status-strict-session-entry-gate-bypassed");
    write_file(&bypassed_decision_path, "bypassed\n");
    let bypassed_status_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "status", "--refresh"],
            &[
                (
                    "FEATUREFORGE_SESSION_KEY",
                    "workflow-status-strict-session-entry-gate-bypassed",
                ),
                ("FEATUREFORGE_WORKFLOW_REQUIRE_SESSION_ENTRY", "1"),
            ],
            "workflow status should ignore bypassed session-entry files after gate removal",
        ),
        "workflow status should ignore bypassed session-entry files after gate removal",
    );
    assert_eq!(bypassed_status_json["schema_version"], 3);
    assert_eq!(bypassed_status_json["status"], "implementation_ready");
    assert!(
        !bypassed_status_json["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes.iter().any(|code| {
                code == "session_entry_unresolved" || code == "session_entry_bypassed"
            })),
        "workflow status should not expose removed strict session-entry reason codes"
    );
    let manifest_after_bypassed_session: WorkflowManifest = serde_json::from_str(
        &fs::read_to_string(&workflow_manifest_path)
            .expect("workflow manifest should remain readable after bypassed strict refresh"),
    )
    .expect("workflow manifest should parse after bypassed strict refresh");
    assert_eq!(
        manifest_after_bypassed_session.expected_spec_path, enabled_manifest.expected_spec_path,
        "bypassed session-entry files should not clear the selected expected spec path"
    );
    assert_eq!(
        manifest_after_bypassed_session.expected_plan_path, enabled_manifest.expected_plan_path,
        "bypassed session-entry files should not clear the selected expected plan path"
    );
}

#[test]
fn canonical_workflow_operator_ignores_spawned_subagent_context_markers() {
    let (repo_dir, state_dir) = init_repo("workflow-spawned-subagent");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-spawned-subagent";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    let mut git_checkout = Command::new("git");
    git_checkout
        .args(["checkout", "-B", "workflow-spawned-subagent"])
        .current_dir(repo);
    run_checked(git_checkout, "git checkout workflow-spawned-subagent");

    install_full_contract_ready_artifacts(repo);
    let execution_preflight_phase = public_harness_phase_from_spec("execution_preflight");

    let spawned_subagent_env = [
        ("FEATUREFORGE_SESSION_KEY", session_key),
        ("FEATUREFORGE_SPAWNED_SUBAGENT", "1"),
    ];

    let operator_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &spawned_subagent_env,
            "workflow operator should bypass session-entry gate for spawned subagents",
        ),
        "workflow operator should bypass session-entry gate for spawned subagents",
    );

    assert_eq!(
        operator_json["phase"],
        Value::String(execution_preflight_phase.clone())
    );
    assert_eq!(operator_json["next_action"], "execution preflight");
    assert!(operator_json.get("session_entry").is_none());
}

#[test]
fn canonical_workflow_operator_ignores_spawned_subagent_opt_in_markers() {
    let (repo_dir, state_dir) = init_repo("workflow-spawned-subagent-opt-in");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-spawned-subagent-opt-in";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    let mut git_checkout = Command::new("git");
    git_checkout
        .args(["checkout", "-B", "workflow-spawned-subagent-opt-in"])
        .current_dir(repo);
    run_checked(
        git_checkout,
        "git checkout workflow-spawned-subagent-opt-in",
    );

    install_full_contract_ready_artifacts(repo);
    let execution_preflight_phase = public_harness_phase_from_spec("execution_preflight");

    let spawned_subagent_env = [
        ("FEATUREFORGE_SESSION_KEY", session_key),
        ("FEATUREFORGE_SPAWNED_SUBAGENT", "1"),
        ("FEATUREFORGE_SPAWNED_SUBAGENT_OPT_IN", "1"),
    ];

    let operator_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &spawned_subagent_env,
            "workflow operator should honor spawned-subagent opt-in",
        ),
        "workflow operator should honor spawned-subagent opt-in",
    );

    assert_eq!(
        operator_json["phase"],
        Value::String(execution_preflight_phase)
    );
    assert_eq!(operator_json["next_action"], "execution preflight");
    assert!(operator_json.get("session_entry").is_none());
}

#[test]
fn canonical_workflow_phase_routes_enabled_ready_plan_to_execution_preflight() {
    let (repo_dir, state_dir) = init_repo("workflow-phase-ready-plan");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-ready-plan";

    let mut git_checkout = Command::new("git");
    git_checkout
        .args(["checkout", "-B", "workflow-phase-ready"])
        .current_dir(repo);
    run_checked(git_checkout, "git checkout workflow-phase-ready");

    install_full_contract_ready_artifacts(repo);
    let phase_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "phase", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "rust canonical workflow phase should route ready plans to execution preflight",
        ),
        "rust canonical workflow phase should route ready plans to execution preflight",
    );
    assert_eq!(phase_json["route_status"], "implementation_ready");
    assert_eq!(phase_json["phase"], "execution_preflight");
    assert_eq!(phase_json["next_action"], "execution preflight");
    assert!(phase_json.get("session_entry").is_none());
    assert_eq!(phase_json["schema_version"], 3);
    assert_eq!(phase_json["route"]["schema_version"], 3);
}

#[test]
fn canonical_workflow_gate_review_is_read_only_before_dispatch() {
    let (repo_dir, state_dir) = init_repo("workflow-record-review-dispatch-cycle-tracking");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-record-review-dispatch-cycle-tracking";
    let decision_path = state
        .join("session-entry")
        .join("using-featureforge")
        .join(session_key);
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    install_full_contract_ready_artifacts(repo);
    prepare_preflight_acceptance_workspace(repo, "workflow-record-review-dispatch");
    write_file(&decision_path, "enabled\n");

    let preflight_json = parse_json(
        &run_workflow_preflight_output(
            repo,
            state,
            plan_rel,
            "workflow preflight before workflow gate-review dispatch cycle tracking",
        ),
        "workflow preflight before workflow gate-review dispatch cycle tracking",
    );
    assert_eq!(preflight_json["allowed"], true);

    let status_before_begin = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status before begin for workflow gate-review dispatch cycle tracking",
    );
    let begin_json = run_plan_execution_json(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_before_begin["execution_fingerprint"]
                .as_str()
                .expect("status should include execution_fingerprint before begin"),
        ],
        "begin active reviewable work before workflow gate-review dispatch cycle tracking",
    );
    assert_eq!(begin_json["active_task"], 1);
    assert_eq!(begin_json["active_step"], 1);
    let branch = current_branch_name(repo);
    update_authoritative_harness_state(repo, state, &branch, plan_rel, 1, &[]);
    let status_before_gate_review = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status before workflow gate-review read-only check",
    );

    let gate_review_json = parse_json(
        &run_workflow_gate_review_output(
            repo,
            state,
            plan_rel,
            "workflow gate review should stay read-only while active work is still in progress",
        ),
        "workflow gate review should stay read-only while active work is still in progress",
    );
    assert_eq!(gate_review_json["allowed"], false);
    assert_eq!(gate_review_json["failure_class"], "ExecutionStateNotReady");
    assert!(
        gate_review_json["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes.iter().any(|code| code == "active_step_in_progress")),
        "workflow gate review should fail while active work is still in progress"
    );

    let status_after_gate_review = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status after workflow gate-review read-only check",
    );
    assert_eq!(
        status_after_gate_review["strategy_checkpoint_kind"],
        status_before_gate_review["strategy_checkpoint_kind"],
        "workflow gate-review should not mutate strategy checkpoint kind"
    );
    assert_eq!(
        status_after_gate_review["last_strategy_checkpoint_fingerprint"],
        status_before_gate_review["last_strategy_checkpoint_fingerprint"],
        "workflow gate-review should not mutate strategy checkpoint fingerprint"
    );
    assert!(
        status_after_gate_review["strategy_state"] == status_before_gate_review["strategy_state"],
        "workflow gate-review should not mutate strategy state"
    );

    let gate_review_dispatch_json = run_runtime_review_dispatch_authority_json(
        repo,
        state,
        plan_rel,
        ReviewDispatchScopeArg::Task,
        Some(1),
        "plan execution record-review-dispatch should mint dispatch lineage even when review is blocked",
    );
    assert_eq!(gate_review_dispatch_json["allowed"], false);
    assert_eq!(
        gate_review_dispatch_json["failure_class"],
        "ExecutionStateNotReady"
    );
    assert!(
        gate_review_dispatch_json["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes.iter().any(|code| code == "active_step_in_progress")),
        "workflow record-review-dispatch should still report the active-step block reason"
    );

    let status_after_gate_review_dispatch = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status after workflow record-review-dispatch mutation check",
    );
    assert_eq!(
        status_after_gate_review_dispatch["last_strategy_checkpoint_fingerprint"],
        status_after_gate_review["last_strategy_checkpoint_fingerprint"],
        "workflow record-review-dispatch must not mint a strategy checkpoint fingerprint while active work is still in progress"
    );
    assert_eq!(
        status_after_gate_review_dispatch["strategy_checkpoint_kind"],
        status_after_gate_review["strategy_checkpoint_kind"],
        "workflow record-review-dispatch must not change strategy checkpoint kind while active work is still in progress"
    );
}

#[test]
fn workflow_read_commands_do_not_persist_preflight_acceptance() {
    let (repo_dir, state_dir) = init_repo("workflow-read-only-preflight-boundary");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-read-only-preflight-boundary";
    let decision_path = state
        .join("session-entry")
        .join("using-featureforge")
        .join(session_key);
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    let mut git_checkout = Command::new("git");
    git_checkout
        .args(["checkout", "-B", "workflow-read-only-preflight"])
        .current_dir(repo);
    run_checked(git_checkout, "git checkout workflow-read-only-preflight");

    install_full_contract_ready_artifacts(repo);
    write_file(&decision_path, "enabled\n");

    let phase_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "phase", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow phase should not persist preflight acceptance",
        ),
        "workflow phase should not persist preflight acceptance",
    );
    assert_eq!(phase_json["phase"], "execution_preflight");
    let doctor_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "doctor", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow doctor should not persist preflight acceptance",
        ),
        "workflow doctor should not persist preflight acceptance",
    );
    assert_eq!(doctor_json["preflight"]["allowed"], true);
    let handoff_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "handoff", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow handoff should not persist preflight acceptance",
        ),
        "workflow handoff should not persist preflight acceptance",
    );
    assert_eq!(handoff_json["next_action"], "execution preflight");

    let status_after_reads = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status after workflow read commands",
    );
    assert!(
        status_after_reads["execution_run_id"].is_null(),
        "workflow read commands must not persist preflight acceptance"
    );
    assert_eq!(
        status_after_reads["harness_phase"], "implementation_handoff",
        "without explicit preflight acceptance, harness phase should stay implementation_handoff"
    );

    let begin_without_preflight = run_rust_featureforge_with_env(
        repo,
        state,
        &[
            "plan",
            "execution",
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_after_reads["execution_fingerprint"]
                .as_str()
                .expect("status fingerprint should be present"),
        ],
        &[("FEATUREFORGE_SESSION_KEY", session_key)],
        "begin should remain blocked before explicit preflight acceptance",
    );
    assert!(
        !begin_without_preflight.status.success(),
        "begin should fail before explicit preflight acceptance, got {:?}\nstdout:\n{}\nstderr:\n{}",
        begin_without_preflight.status,
        String::from_utf8_lossy(&begin_without_preflight.stdout),
        String::from_utf8_lossy(&begin_without_preflight.stderr)
    );
    let begin_payload = if begin_without_preflight.stdout.is_empty() {
        &begin_without_preflight.stderr
    } else {
        &begin_without_preflight.stdout
    };
    let begin_error: Value =
        serde_json::from_slice(begin_payload).expect("begin failure should emit json");
    assert_eq!(begin_error["error_class"], "ExecutionStateNotReady");

    let preflight_json = run_internal_plan_execution_preflight_json(
        repo,
        state,
        plan_rel,
        "explicit plan execution preflight acceptance",
    );
    assert_eq!(preflight_json["allowed"], true);

    let status_after_preflight = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status after explicit plan execution preflight acceptance",
    );
    assert!(
        status_after_preflight["execution_run_id"]
            .as_str()
            .is_some_and(|value| !value.is_empty()),
        "explicit plan execution preflight should persist execution_run_id"
    );
}

#[test]
fn canonical_workflow_operator_ready_plan_pins_observability_seam_corpus() {
    let workflow_seam_event_kinds = [
        "phase_transition",
        "gate_result",
        "recommendation_proposed",
        "policy_accepted",
        "downstream_gate_rejected",
    ];
    let workflow_seam_minimum_envelope_fields = [
        "event_kind",
        "timestamp",
        "execution_run_id",
        "authoritative_sequence",
        "source_plan_path",
        "source_plan_revision",
        "harness_phase",
        "chunk_id",
        "command_name",
        "gate_name",
        "failure_class",
        "reason_codes",
    ];
    let workflow_seam_telemetry_counter_keys = [
        "phase_transition_count",
        "gate_failures_by_gate",
        "downstream_gate_rejection_count",
        "write_authority_conflict_count",
        "repo_state_drift_count",
    ];
    let workflow_seam_folded_diagnostics = ["write_authority_conflict", "repo_state_drift"];

    let stable_event_kinds: BTreeSet<&str> = STABLE_EVENT_KINDS.iter().copied().collect();
    let unknown_event_kinds: Vec<&str> = workflow_seam_event_kinds
        .iter()
        .copied()
        .filter(|event_kind| !stable_event_kinds.contains(event_kind))
        .collect();
    assert!(
        unknown_event_kinds.is_empty(),
        "workflow operator observability seam should use only runtime-owned event kinds, unknown: {unknown_event_kinds:?}"
    );

    let stable_reason_codes: BTreeSet<&str> = STABLE_REASON_CODES.iter().copied().collect();
    let unknown_reason_codes: Vec<&str> = workflow_seam_folded_diagnostics
        .iter()
        .copied()
        .filter(|reason_code| !stable_reason_codes.contains(reason_code))
        .collect();
    assert!(
        unknown_reason_codes.is_empty(),
        "workflow operator observability seam should fold only stable runtime diagnostics, unknown: {unknown_reason_codes:?}"
    );

    let mut probe_event =
        HarnessObservabilityEvent::new(HarnessEventKind::PhaseTransition, "2026-03-26T12:00:00Z");
    for reason_code in workflow_seam_folded_diagnostics {
        probe_event.add_reason_code(reason_code);
    }
    let serialized_probe_event =
        to_value(probe_event).expect("workflow observability seam probe event should serialize");
    let event_object = serialized_probe_event
        .as_object()
        .expect("workflow observability seam probe should serialize to a JSON object");
    let missing_envelope_fields: Vec<&str> = workflow_seam_minimum_envelope_fields
        .iter()
        .copied()
        .filter(|field| !event_object.contains_key(*field))
        .collect();
    assert!(
        missing_envelope_fields.is_empty(),
        "workflow operator observability seam should pin minimum envelope fields, missing: {missing_envelope_fields:?}"
    );
    assert_eq!(
        event_object.get("event_kind").and_then(Value::as_str),
        Some("phase_transition"),
        "workflow seam observability envelope should keep event_kind machine-readable"
    );
    assert!(
        event_object
            .get("timestamp")
            .and_then(Value::as_str)
            .is_some_and(|timestamp| !timestamp.is_empty()),
        "workflow seam observability envelope should keep timestamp as a non-empty string"
    );
    assert!(
        event_object
            .get("reason_codes")
            .and_then(Value::as_array)
            .is_some_and(|codes| {
                let code_set: BTreeSet<&str> = codes.iter().filter_map(Value::as_str).collect();
                workflow_seam_folded_diagnostics
                    .iter()
                    .all(|reason_code| code_set.contains(reason_code))
            }),
        "workflow seam observability envelope should keep folded conflict/drift diagnostics machine-readable"
    );

    let serialized_counters = to_value(HarnessTelemetryCounters::default())
        .expect("workflow observability seam counters should serialize");
    let counter_object = serialized_counters
        .as_object()
        .expect("workflow observability seam counters should serialize to a JSON object");
    let missing_counter_keys: Vec<&str> = workflow_seam_telemetry_counter_keys
        .iter()
        .copied()
        .filter(|counter_key| !counter_object.contains_key(*counter_key))
        .collect();
    assert!(
        missing_counter_keys.is_empty(),
        "workflow operator observability seam should pin required telemetry counter keys, missing: {missing_counter_keys:?}"
    );
}

#[test]
fn canonical_workflow_operator_surfaces_fail_closed_when_session_entry_is_bypassed() {
    let (repo_dir, state_dir) = init_repo("workflow-phase-bypassed-session");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-bypassed-session";
    let decision_path = state
        .join("session-entry")
        .join("using-featureforge")
        .join(session_key);

    let mut git_checkout = Command::new("git");
    git_checkout
        .args(["checkout", "-B", "workflow-phase-bypassed"])
        .current_dir(repo);
    run_checked(git_checkout, "git checkout workflow-phase-bypassed");

    install_full_contract_ready_artifacts(repo);
    write_file(&decision_path, "bypassed\n");

    let phase_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "phase", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow phase should fail closed when session-entry is bypassed",
        ),
        "workflow phase should fail closed when session-entry is bypassed",
    );
    let handoff_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "handoff", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow handoff should fail closed when session-entry is bypassed",
        ),
        "workflow handoff should fail closed when session-entry is bypassed",
    );
    let execution_preflight_phase = public_harness_phase_from_spec("execution_preflight");
    assert_eq!(
        phase_json["phase"],
        Value::String(execution_preflight_phase.clone())
    );
    assert_eq!(phase_json["next_action"], "execution preflight");
    assert!(phase_json.get("session_entry").is_none());

    assert_eq!(
        handoff_json["phase"],
        Value::String(execution_preflight_phase)
    );
    assert_eq!(handoff_json["next_action"], "execution preflight");
    assert_eq!(handoff_json["recommended_skill"], Value::from(""));
    assert!(handoff_json.get("session_entry").is_none());
}

#[test]
fn canonical_workflow_phase_routes_enabled_stale_plan_to_plan_writing() {
    let (repo_dir, state_dir) = init_repo("workflow-phase-stale-plan");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-stale-plan";

    write_file(
        &repo.join("docs/featureforge/specs/2026-01-22-document-review-system-design-v2.md"),
        "# Approved Spec, Newer Path\n\n**Workflow State:** CEO Approved\n**Spec Revision:** 1\n**Last Reviewed By:** plan-ceo-review\n\n## Notes\n",
    );
    write_file(
        &repo.join("docs/featureforge/plans/2026-01-22-document-review-system.md"),
        "# Approved Plan, Stale Source Path\n\n**Workflow State:** Engineering Approved\n**Plan Revision:** 1\n**Execution Mode:** none\n**Source Spec:** `docs/featureforge/specs/2026-01-22-document-review-system-design.md`\n**Source Spec Revision:** 1\n**Last Reviewed By:** plan-eng-review\n\n## Requirement Coverage Matrix\n\n- REQ-001 -> Task 1\n\n## Task 1: Preserve the stale source path case\n\n**Spec Coverage:** REQ-001\n**Goal:** The plan remains structurally valid while its source-spec path goes stale.\n\n**Context:**\n- Spec Coverage: REQ-001.\n\n**Constraints:**\n- Keep the fixture minimal.\n**Done when:**\n- The plan remains structurally valid while its source-spec path goes stale.\n\n**Files:**\n- Test: `tests/workflow_runtime.rs`\n\n- [ ] **Step 1: Detect the stale source path**\n",
    );
    let phase_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "phase", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "rust canonical workflow phase should route stale plans to plan writing",
        ),
        "rust canonical workflow phase should route stale plans to plan writing",
    );

    assert_eq!(phase_json["route_status"], "stale_plan");
    assert_eq!(phase_json["phase"], "pivot_required");
    assert_eq!(phase_json["next_action"], "pivot / return to planning");
    assert_eq!(phase_json["next_skill"], "featureforge:writing-plans");
    assert!(phase_json.get("session_entry").is_none());
}

#[test]
fn canonical_workflow_phase_keeps_corrupt_manifest_read_only() {
    let (repo_dir, state_dir) = init_repo("workflow-phase-corrupt-manifest");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let spec_path = repo.join("docs/featureforge/specs/2026-03-24-corrupt-phase-spec.md");

    write_file(
        &spec_path,
        "# Phase Corrupt Manifest Spec\n\n**Workflow State:** Draft\n**Spec Revision:** 1\n**Last Reviewed By:** brainstorming\n",
    );

    let refresh_output = run_rust_featureforge(
        repo,
        state,
        &["workflow", "status", "--refresh"],
        "rust canonical workflow status refresh should seed the manifest before corrupt phase inspection",
    );
    assert!(
        refresh_output.status.success(),
        "workflow status refresh should succeed before corrupt manifest inspection, got {:?}\nstdout:\n{}\nstderr:\n{}",
        refresh_output.status,
        String::from_utf8_lossy(&refresh_output.stdout),
        String::from_utf8_lossy(&refresh_output.stderr)
    );

    let identity = discover_repo_identity(repo).expect("repo identity should resolve");
    let manifest_path = manifest_path(&identity, state);
    fs::write(&manifest_path, "{ \"broken\": true\n")
        .expect("corrupt manifest fixture should be writable");
    let before_bytes = fs::read(&manifest_path).expect("corrupt manifest fixture should exist");

    let phase_json = parse_json(
        &run_rust_featureforge(
            repo,
            state,
            &["workflow", "phase", "--json"],
            "rust canonical workflow phase should inspect corrupt manifests without repairing them",
        ),
        "rust canonical workflow phase should inspect corrupt manifests without repairing them",
    );
    assert!(phase_json["phase"].is_string());

    let after_bytes = fs::read(&manifest_path)
        .expect("workflow phase should leave the corrupt manifest in place");
    assert_eq!(after_bytes, before_bytes);

    let parent = manifest_path
        .parent()
        .expect("manifest fixture should have a parent directory");
    let backup_prefix = format!(
        "{}.corrupt-",
        manifest_path
            .file_name()
            .expect("manifest fixture should have a file name")
            .to_string_lossy()
    );
    let backup_written = fs::read_dir(parent)
        .expect("manifest directory should stay readable")
        .flatten()
        .any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(&backup_prefix)
        });
    assert!(
        !backup_written,
        "workflow phase should not create corrupt-manifest backups for read-only inspection"
    );
}

#[test]
fn canonical_workflow_public_text_surfaces_prefer_operator_and_status_over_removed_helpers() {
    let (repo_dir, state_dir) = init_repo("workflow-public-text-commands");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-public-text-commands";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    let mut git_checkout = Command::new("git");
    git_checkout
        .args(["checkout", "-B", "workflow-public-text"])
        .current_dir(repo);
    run_checked(git_checkout, "git checkout workflow-public-text");

    install_full_contract_ready_artifacts(repo);
    for removed in ["next", "artifacts", "explain"] {
        let output = run_rust_featureforge_with_env_real_cli(
            repo,
            state,
            &["workflow", removed],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            &format!("workflow {removed} should stay removed on ready plans"),
        );
        assert!(
            !output.status.success(),
            "workflow {removed} should stay removed"
        );
        let failure: Value = serde_json::from_slice(&output.stderr)
            .or_else(|_| serde_json::from_slice(&output.stdout))
            .expect("removed helper should emit json parse failure");
        assert_eq!(failure["error_class"], "InvalidCommandInput");
        assert!(
            failure["message"].as_str().is_some_and(
                |message| message.contains(&format!("unrecognized subcommand '{removed}'"))
            ),
            "workflow {removed} should fail at CLI parsing, got {failure:?}"
        );
    }

    let operator_output = run_rust_featureforge_with_env(
        repo,
        state,
        &["workflow", "operator", "--plan", plan_rel],
        &[("FEATUREFORGE_SESSION_KEY", session_key)],
        "workflow operator text should be available on ready plans",
    );
    assert!(operator_output.status.success());
    let operator_stdout = String::from_utf8_lossy(&operator_output.stdout);
    assert!(operator_stdout.contains("Workflow operator"));
    assert!(operator_stdout.contains("Next action: execution preflight"));
    assert!(operator_stdout.contains(
        "Spec: docs/featureforge/specs/2026-03-22-runtime-integration-hardening-design.md"
    ));
    assert!(
        operator_stdout
            .contains("Plan: docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md")
    );
    assert!(!operator_stdout.contains("session-entry"));

    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status should be available on ready plans",
    );
    assert_eq!(status_json["state_kind"], "actionable_public_command");
    assert!(
        status_json["next_public_action"]["command"]
            .as_str()
            .is_some_and(|command| command.starts_with("featureforge plan execution begin "))
    );
    assert_eq!(status_json["phase_detail"], "execution_preflight_required");
}

#[test]
fn canonical_workflow_public_json_commands_work_for_ready_plan() {
    let (repo_dir, state_dir) = init_repo("workflow-public-json-commands");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-public-json-commands";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    let mut git_checkout = Command::new("git");
    git_checkout
        .args(["checkout", "-B", "workflow-public-json"])
        .current_dir(repo);
    run_checked(git_checkout, "git checkout workflow-public-json");

    install_full_contract_ready_artifacts(repo);
    let execution_preflight_phase = public_harness_phase_from_spec("execution_preflight");

    let doctor_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "doctor", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "rust canonical workflow doctor should be available on ready plans",
        ),
        "rust canonical workflow doctor should be available on ready plans",
    );
    assert_eq!(
        doctor_json["phase"],
        Value::String(execution_preflight_phase.clone())
    );
    assert_eq!(doctor_json["route_status"], "implementation_ready");
    assert_eq!(doctor_json["next_action"], "execution preflight");
    assert_eq!(
        doctor_json["spec_path"],
        "docs/featureforge/specs/2026-03-22-runtime-integration-hardening-design.md"
    );
    assert_eq!(doctor_json["plan_path"], plan_rel);
    assert_eq!(doctor_json["contract_state"], "valid");
    assert!(doctor_json.get("session_entry").is_none());
    assert_eq!(doctor_json["schema_version"], 3);
    assert_eq!(doctor_json["route"]["schema_version"], 3);
    assert_eq!(doctor_json["execution_status"]["execution_started"], "no");
    assert_eq!(doctor_json["preflight"]["allowed"], true);
    assert_eq!(doctor_json["gate_review"], Value::Null);
    assert_eq!(doctor_json["gate_finish"], Value::Null);

    let handoff_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "handoff", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "rust canonical workflow handoff should be available on ready plans",
        ),
        "rust canonical workflow handoff should be available on ready plans",
    );
    assert_eq!(
        handoff_json["phase"],
        Value::String(execution_preflight_phase)
    );
    assert_eq!(handoff_json["route_status"], "implementation_ready");
    assert_eq!(handoff_json["execution_started"], "no");
    assert_eq!(handoff_json["next_action"], "execution preflight");
    assert_eq!(
        handoff_json["spec_path"],
        "docs/featureforge/specs/2026-03-22-runtime-integration-hardening-design.md"
    );
    assert_eq!(handoff_json["plan_path"], plan_rel);
    assert_eq!(handoff_json["recommended_skill"], Value::from(""));
    assert_eq!(handoff_json["recommendation"], Value::Null);
    assert_eq!(handoff_json["recommendation_reason"], Value::from(""));
    assert_eq!(handoff_json["state_kind"], "actionable_public_command");
    assert_eq!(
        handoff_json["semantic_workspace_tree_id"],
        doctor_json["execution_status"]["semantic_workspace_tree_id"]
    );
    assert_eq!(
        handoff_json["raw_workspace_tree_id"],
        doctor_json["execution_status"]["raw_workspace_tree_id"]
    );
    assert!(handoff_json.get("session_entry").is_none());
    assert_eq!(handoff_json["schema_version"], 3);
    assert_eq!(handoff_json["route"]["schema_version"], 3);

    let preflight_json = parse_json(
        &run_workflow_preflight_output(
            repo,
            state,
            plan_rel,
            "rust canonical workflow preflight should be available on ready plans",
        ),
        "rust canonical workflow preflight should be available on ready plans",
    );
    assert_eq!(preflight_json["allowed"], true);

    let gate_review_json = parse_json(
        &run_workflow_gate_review_output(
            repo,
            state,
            plan_rel,
            "rust canonical workflow gate review should be available on ready plans",
        ),
        "rust canonical workflow gate review should be available on ready plans",
    );
    assert_eq!(gate_review_json["allowed"], false);
    assert_eq!(gate_review_json["failure_class"], "ExecutionStateNotReady");
    assert_eq!(
        gate_review_json["reason_codes"][0],
        "unfinished_steps_remaining"
    );

    let gate_finish_json = parse_json(
        &run_workflow_gate_finish_output(
            repo,
            state,
            plan_rel,
            "rust canonical workflow gate finish should be available on ready plans",
        ),
        "rust canonical workflow gate finish should be available on ready plans",
    );
    assert_eq!(gate_finish_json["allowed"], false);
    assert_eq!(gate_finish_json["failure_class"], "ExecutionStateNotReady");
    assert_eq!(
        gate_finish_json["reason_codes"][0],
        "unfinished_steps_remaining"
    );
}

#[test]
fn canonical_workflow_doctor_exposes_harness_state_before_execution_starts() {
    let (repo_dir, state_dir) = init_repo("workflow-public-harness-state");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-public-harness-state";
    let decision_path = state
        .join("session-entry")
        .join("using-featureforge")
        .join(session_key);

    install_full_contract_ready_artifacts(repo);
    write_file(&decision_path, "enabled\n");

    let implementation_handoff_phase = public_harness_phase_from_spec("implementation_handoff");
    let execution_preflight_phase = public_harness_phase_from_spec("execution_preflight");

    let doctor_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "doctor", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow doctor for harness state fixture",
        ),
        "workflow doctor for harness state fixture",
    );

    assert_eq!(
        doctor_json["phase"],
        Value::String(execution_preflight_phase)
    );
    assert_eq!(doctor_json["route_status"], "implementation_ready");
    assert_eq!(doctor_json["next_action"], "execution preflight");
    assert_eq!(doctor_json["execution_status"]["execution_started"], "no");
    assert_eq!(doctor_json["gate_review"], Value::Null);
    assert_eq!(doctor_json["gate_finish"], Value::Null);

    let execution_status = &doctor_json["execution_status"];
    let execution_run_id = execution_status
        .get("execution_run_id")
        .expect("workflow doctor should expose execution_run_id");
    assert!(
        execution_run_id.is_null(),
        "workflow doctor should expose execution_run_id as null before preflight acceptance, got {execution_run_id:?}"
    );
    assert_eq!(
        execution_status["harness_phase"],
        Value::String(implementation_handoff_phase)
    );
    assert_eq!(
        execution_status["latest_authoritative_sequence"],
        Value::from(0)
    );

    let missing_pre_acceptance_null_fields = missing_null_fields(
        execution_status,
        &[
            "chunking_strategy",
            "evaluator_policy",
            "reset_policy",
            "review_stack",
        ],
    );
    assert!(
        missing_pre_acceptance_null_fields.is_empty(),
        "workflow doctor should expose pre-acceptance policy fields as required-and-null before execution preflight accepts run identity, missing null fields: {missing_pre_acceptance_null_fields:?}"
    );

    for field in [
        "aggregate_evaluation_state",
        "repo_state_baseline_head_sha",
        "repo_state_baseline_worktree_fingerprint",
        "repo_state_drift_state",
        "dependency_index_state",
        "final_review_state",
        "browser_qa_state",
        "release_docs_state",
        "last_final_review_artifact_fingerprint",
        "last_browser_qa_artifact_fingerprint",
        "last_release_docs_artifact_fingerprint",
    ] {
        assert!(
            execution_status.get(field).is_some(),
            "workflow doctor should expose {field} in execution_status"
        );
    }

    for field in ["write_authority_holder", "write_authority_worktree"] {
        let value = execution_status
            .get(field)
            .unwrap_or_else(|| panic!("workflow doctor should expose {field}"));
        assert!(
            value.is_null() || value.as_str().is_some_and(|value| !value.is_empty()),
            "workflow doctor should expose {field} as null when unknown pre-start or as non-empty diagnostic metadata once known, got {value:?}"
        );
    }

    let missing_null_fields = missing_null_fields(
        execution_status,
        &[
            "active_contract_path",
            "active_contract_fingerprint",
            "last_evaluation_report_path",
            "last_evaluation_report_fingerprint",
            "last_evaluation_evaluator_kind",
        ],
    );
    assert!(
        missing_null_fields.is_empty(),
        "workflow doctor should keep active pointers authoritative-only before execution starts, missing null fields: {missing_null_fields:?}"
    );

    for field in [
        "required_evaluator_kinds",
        "completed_evaluator_kinds",
        "pending_evaluator_kinds",
        "non_passing_evaluator_kinds",
        "open_failed_criteria",
        "reason_codes",
    ] {
        assert!(
            execution_status
                .get(field)
                .and_then(Value::as_array)
                .is_some(),
            "workflow doctor should expose array field {field} in execution_status"
        );
    }

    let review_stack = execution_status
        .get("review_stack")
        .expect("workflow doctor should expose review_stack in execution_status");
    assert!(
        review_stack.is_null(),
        "workflow doctor should expose review_stack as required-and-null before execution preflight accepts policy, got {review_stack:?}"
    );
}

#[test]
fn canonical_workflow_doctor_shares_authoritative_state_across_same_branch_worktrees() {
    let (repo_dir, state_dir) = init_repo("workflow-public-same-branch-worktree");
    let repo_a = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-public-same-branch-worktree";
    let decision_path = state
        .join("session-entry")
        .join("using-featureforge")
        .join(session_key);
    let linked_worktree_root = TempDir::new().expect("linked worktree tempdir should exist");
    let repo_b = linked_worktree_root
        .path()
        .join("same-branch-linked-worktree");
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    let mut git_checkout = Command::new("git");
    git_checkout
        .args(["checkout", "-B", "workflow-public-same-branch-worktree"])
        .current_dir(repo_a);
    run_checked(
        git_checkout,
        "git checkout workflow-public-same-branch-worktree",
    );

    let mut git_worktree_add = Command::new("git");
    git_worktree_add
        .arg("worktree")
        .arg("add")
        .arg("--force")
        .arg(&repo_b)
        .arg("workflow-public-same-branch-worktree")
        .current_dir(repo_a);
    run_checked(
        git_worktree_add,
        "git worktree add --force same-branch-linked-worktree",
    );

    install_full_contract_ready_artifacts(repo_a);
    install_full_contract_ready_artifacts(&repo_b);
    write_file(&decision_path, "enabled\n");

    let status_a = run_plan_execution_json(
        repo_a,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status before same-branch worktree authoritative sharing fixture",
    );
    let preflight_a = parse_json(
        &run_workflow_preflight_output(
            repo_a,
            state,
            plan_rel,
            "workflow preflight before same-branch worktree authoritative sharing fixture",
        ),
        "workflow preflight before same-branch worktree authoritative sharing fixture",
    );
    assert_eq!(preflight_a["allowed"], true);
    run_plan_execution_json(
        repo_a,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_a["execution_fingerprint"]
                .as_str()
                .expect("status fingerprint should be present"),
        ],
        "plan execution begin for same-branch worktree authoritative sharing fixture",
    );

    let doctor_a = parse_json(
        &run_rust_featureforge_with_env(
            repo_a,
            state,
            &["workflow", "doctor", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow doctor for same-branch authoritative scope on repo A",
        ),
        "workflow doctor for same-branch authoritative scope on repo A",
    );
    let doctor_b = parse_json(
        &run_rust_featureforge_with_env(
            &repo_b,
            state,
            &["workflow", "doctor", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow doctor for same-branch authoritative scope on repo B",
        ),
        "workflow doctor for same-branch authoritative scope on repo B",
    );

    for doctor in [&doctor_a, &doctor_b] {
        assert_eq!(doctor["phase"], "executing");
        assert_eq!(doctor["execution_status"]["execution_started"], "yes");
        assert_eq!(doctor["execution_status"]["active_task"], 1);
        assert_eq!(doctor["execution_status"]["active_step"], 1);
        let execution_run_id = doctor["execution_status"]
            .get("execution_run_id")
            .expect("same-branch worktrees should expose execution_run_id");
        assert!(
            execution_run_id
                .as_str()
                .is_some_and(|value| !value.is_empty()),
            "same-branch worktrees should expose a non-empty shared execution_run_id after execution_preflight acceptance and execution start, got {execution_run_id:?}"
        );
        assert!(
            doctor["execution_status"]["latest_authoritative_sequence"]
                .as_u64()
                .is_some(),
            "same-branch worktrees should expose numeric authoritative sequence diagnostics once execution starts"
        );
        let reason_codes = doctor["execution_status"]["reason_codes"]
            .as_array()
            .expect("same-branch worktrees should expose execution reason_codes as an array");
        if reason_codes
            .iter()
            .any(|value| value == &Value::String(String::from("write_authority_conflict")))
        {
            assert!(
                doctor["execution_status"]["write_authority_holder"].is_string(),
                "write_authority_conflict should keep authority holder metadata visible"
            );
            assert!(
                doctor["execution_status"]["write_authority_worktree"].is_string(),
                "write_authority_conflict should keep authority worktree metadata visible"
            );
        }
    }

    let run_id_a = doctor_a["execution_status"]["execution_run_id"]
        .as_str()
        .expect("repo A should expose an execution_run_id after execution_preflight acceptance");
    let run_id_b = doctor_b["execution_status"]["execution_run_id"]
        .as_str()
        .expect("repo B should expose an execution_run_id after execution_preflight acceptance");
    assert_eq!(
        run_id_a, run_id_b,
        "same-branch worktrees should share one authoritative execution run after execution starts"
    );

    assert_eq!(
        doctor_a["execution_status"]["latest_authoritative_sequence"],
        doctor_b["execution_status"]["latest_authoritative_sequence"],
        "same-branch worktrees should share authoritative sequence state"
    );

    let holder_a = &doctor_a["execution_status"]["write_authority_holder"];
    let holder_b = &doctor_b["execution_status"]["write_authority_holder"];
    let worktree_a = &doctor_a["execution_status"]["write_authority_worktree"];
    let worktree_b = &doctor_b["execution_status"]["write_authority_worktree"];

    for (field, value) in [
        ("write_authority_holder", holder_a),
        ("write_authority_holder", holder_b),
        ("write_authority_worktree", worktree_a),
        ("write_authority_worktree", worktree_b),
    ] {
        assert!(
            value.is_null() || value.as_str().is_some_and(|value| !value.is_empty()),
            "same-branch worktrees should expose {field} as null when authority diagnostics are not yet emitted, or as non-empty diagnostics once emitted, got {value:?}"
        );
    }

    assert_eq!(
        holder_a, holder_b,
        "same-branch worktrees should agree on the shared authority holder"
    );
    assert_eq!(
        worktree_a, worktree_b,
        "same-branch worktrees should agree on the authoritative worktree diagnostic"
    );
}

#[test]
fn plan_execution_status_and_explain_share_started_state_across_same_branch_worktrees() {
    let (repo_dir, state_dir) = init_repo("workflow-same-branch-status-and-explain");
    let repo_a = repo_dir.path();
    let state = state_dir.path();
    let linked_worktree_root = TempDir::new().expect("linked worktree tempdir should exist");
    let repo_b = linked_worktree_root
        .path()
        .join("same-branch-status-and-explain");
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    let mut git_checkout = Command::new("git");
    git_checkout
        .args(["checkout", "-B", "workflow-same-branch-status-and-explain"])
        .current_dir(repo_a);
    run_checked(
        git_checkout,
        "git checkout workflow-same-branch-status-and-explain",
    );

    let mut git_worktree_add = Command::new("git");
    git_worktree_add
        .arg("worktree")
        .arg("add")
        .arg("--force")
        .arg(&repo_b)
        .arg("workflow-same-branch-status-and-explain")
        .current_dir(repo_a);
    run_checked(
        git_worktree_add,
        "git worktree add --force same-branch-status-and-explain",
    );

    install_full_contract_ready_artifacts(repo_a);
    install_full_contract_ready_artifacts(&repo_b);

    let status_before_begin = run_plan_execution_json(
        repo_a,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status before same-branch status/explain sharing fixture",
    );
    let preflight = run_internal_plan_execution_preflight_json(
        repo_a,
        state,
        plan_rel,
        "preflight before same-branch status/explain sharing fixture",
    );
    assert_eq!(preflight["allowed"], true);
    run_plan_execution_json(
        repo_a,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_before_begin["execution_fingerprint"]
                .as_str()
                .expect("status fingerprint should be present"),
        ],
        "plan execution begin for same-branch status/explain sharing fixture",
    );

    let status_a = run_plan_execution_json(
        repo_a,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status for same-branch status/explain sharing on repo A",
    );
    let status_b = run_plan_execution_json(
        &repo_b,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status for same-branch status/explain sharing on repo B",
    );

    for status in [&status_a, &status_b] {
        assert_eq!(status["execution_started"], "yes", "json: {status}");
        assert_eq!(status["active_task"], Value::from(1), "json: {status}");
        assert_eq!(status["active_step"], Value::from(1), "json: {status}");
    }
    assert_eq!(
        status_a["execution_run_id"], status_b["execution_run_id"],
        "same-branch worktrees should share the started execution run in plan execution status"
    );
    assert_eq!(
        status_a["phase_detail"], status_b["phase_detail"],
        "same-branch worktrees should agree on phase detail in plan execution status"
    );

    let explain_a = run_plan_execution_json(
        repo_a,
        state,
        &["explain-review-state", "--plan", plan_rel],
        "plan execution explain-review-state for same-branch sharing on repo A",
    );
    let explain_b = run_plan_execution_json(
        &repo_b,
        state,
        &["explain-review-state", "--plan", plan_rel],
        "plan execution explain-review-state for same-branch sharing on repo B",
    );

    assert_eq!(
        explain_a["next_action"], explain_b["next_action"],
        "same-branch worktrees should agree on explain-review-state next_action"
    );
    assert_eq!(
        explain_a["recommended_command"], explain_b["recommended_command"],
        "same-branch worktrees should agree on explain-review-state recommended_command"
    );
    assert_eq!(
        explain_a["trace_summary"], explain_b["trace_summary"],
        "same-branch worktrees should agree on explain-review-state trace_summary"
    );
}

#[test]
fn plan_execution_repair_and_reconcile_share_started_state_across_same_branch_worktrees() {
    let (repo_dir, state_dir) = init_repo("workflow-same-branch-repair-and-reconcile");
    let repo_a = repo_dir.path();
    let state = state_dir.path();
    let linked_worktree_root = TempDir::new().expect("linked worktree tempdir should exist");
    let repo_b = linked_worktree_root
        .path()
        .join("same-branch-repair-and-reconcile");
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    let mut git_checkout = Command::new("git");
    git_checkout
        .args([
            "checkout",
            "-B",
            "workflow-same-branch-repair-and-reconcile",
        ])
        .current_dir(repo_a);
    run_checked(
        git_checkout,
        "git checkout workflow-same-branch-repair-and-reconcile",
    );

    let mut git_worktree_add = Command::new("git");
    git_worktree_add
        .arg("worktree")
        .arg("add")
        .arg("--force")
        .arg(&repo_b)
        .arg("workflow-same-branch-repair-and-reconcile")
        .current_dir(repo_a);
    run_checked(
        git_worktree_add,
        "git worktree add --force same-branch-repair-and-reconcile",
    );

    install_full_contract_ready_artifacts(repo_a);
    install_full_contract_ready_artifacts(&repo_b);

    let status_before_begin = run_plan_execution_json(
        repo_a,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status before same-branch repair/reconcile fixture",
    );
    let preflight = run_internal_plan_execution_preflight_json(
        repo_a,
        state,
        plan_rel,
        "preflight before same-branch repair/reconcile fixture",
    );
    assert_eq!(preflight["allowed"], true);
    run_plan_execution_json(
        repo_a,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_before_begin["execution_fingerprint"]
                .as_str()
                .expect("status fingerprint should be present"),
        ],
        "plan execution begin for same-branch repair/reconcile fixture",
    );

    let status_b_after_begin = run_plan_execution_json(
        &repo_b,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status after begin for same-branch repair/reconcile fixture on repo B",
    );
    assert_eq!(status_b_after_begin["execution_started"], "yes");

    seed_current_branch_closure_truth(repo_a, state, plan_rel, 1);
    let branch = current_branch_name(repo_a);
    update_authoritative_harness_state(
        repo_a,
        state,
        &branch,
        plan_rel,
        1,
        &[
            ("current_branch_closure_reviewed_state_id", Value::Null),
            ("current_branch_closure_contract_identity", Value::Null),
        ],
    );

    let reconcile_b = run_internal_reconcile_review_state_json(
        &repo_b,
        state,
        plan_rel,
        "reconcile-review-state from same-branch non-authoritative worktree",
    );
    assert_eq!(
        reconcile_b["action"], "reconciled",
        "reconcile-review-state from a same-branch non-authoritative worktree should restore missing derived overlays when recoverable",
    );
    assert!(
        reconcile_b["actions_performed"]
            .as_array()
            .is_some_and(|actions| !actions.is_empty()),
        "reconcile-review-state should report restored overlay actions",
    );

    let authoritative_state_path = harness_state_path(state, &repo_slug(repo_a), &branch);
    let reconciled_state: Value = serde_json::from_str(
        &fs::read_to_string(&authoritative_state_path)
            .expect("authoritative state should be readable after reconcile"),
    )
    .expect("authoritative state should remain valid json after reconcile");
    assert!(
        reconciled_state["current_branch_closure_reviewed_state_id"]
            .as_str()
            .is_some_and(|value| !value.is_empty()),
        "reconcile-review-state should restore current_branch_closure_reviewed_state_id via authoritative records"
    );
    assert!(
        reconciled_state["current_branch_closure_contract_identity"]
            .as_str()
            .is_some_and(|value| !value.is_empty()),
        "reconcile-review-state should restore current_branch_closure_contract_identity via authoritative records"
    );

    let mut malformed_state = reconciled_state;
    malformed_state["current_branch_closure_reviewed_state_id"] =
        Value::from("git_tree:not-a-tree");
    if let Some(record) = malformed_state
        .get_mut("branch_closure_records")
        .and_then(Value::as_object_mut)
        .and_then(|records| records.get_mut("branch-release-closure"))
        .and_then(Value::as_object_mut)
    {
        record.insert(
            String::from("reviewed_state_id"),
            Value::from("git_tree:not-a-tree"),
        );
    } else {
        panic!(
            "same-branch repair fixture should include branch-release-closure in authoritative branch_closure_records"
        );
    }
    write_file(
        &authoritative_state_path,
        &serde_json::to_string(&malformed_state)
            .expect("malformed authoritative state fixture should serialize"),
    );

    let repair_b = run_plan_execution_json(
        &repo_b,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "repair-review-state from same-branch non-authoritative worktree",
    );
    let action = repair_b["action"]
        .as_str()
        .expect("repair-review-state should expose action");
    let repaired_state: Value = serde_json::from_str(
        &fs::read_to_string(&authoritative_state_path)
            .expect("authoritative state should be readable after repair"),
    )
    .expect("authoritative state should remain valid json after repair");
    if action == "blocked" {
        let required_follow_up = repair_b["required_follow_up"]
            .as_str()
            .expect("blocked repair-review-state should emit required_follow_up");
        let operator_b = parse_json(
            &run_rust_featureforge_with_env(
                &repo_b,
                state,
                &["workflow", "operator", "--plan", plan_rel, "--json"],
                &[],
                "same-branch workflow/operator route after blocked repair",
            ),
            "same-branch workflow/operator route after blocked repair",
        );
        assert_eq!(
            operator_b["required_follow_up"],
            Value::from(required_follow_up),
            "blocked repair-review-state should persist the exact follow-up route surfaced by workflow/operator"
        );
        let persisted_follow_up = if required_follow_up == "advance_late_stage" {
            "record_branch_closure"
        } else {
            required_follow_up
        };
        assert_eq!(
            repaired_state["review_state_repair_follow_up"],
            Value::from(persisted_follow_up),
            "blocked repair-review-state from a same-branch non-authoritative worktree should persist the authoritative follow-up reroute",
        );
    } else if action == "reconciled" {
        assert_eq!(
            repair_b["required_follow_up"],
            Value::Null,
            "reconciled repair-review-state should not persist a follow-up reroute when reconciliation succeeds",
        );
        assert!(
            repair_b["actions_performed"]
                .as_array()
                .is_some_and(|actions| !actions.is_empty()),
            "reconciled repair-review-state should report restored authoritative fields",
        );
        assert_eq!(
            repaired_state["review_state_repair_follow_up"],
            Value::Null,
            "reconciled repair-review-state should clear stale persisted follow-up reroutes",
        );
    } else if action == "already_current" {
        assert_eq!(
            repair_b["required_follow_up"],
            Value::Null,
            "already_current repair-review-state should not emit a follow-up reroute",
        );
        assert!(
            repair_b["actions_performed"]
                .as_array()
                .is_some_and(|actions| actions.is_empty()),
            "already_current repair-review-state should not report reconciliation rewrites",
        );
    } else {
        panic!(
            "repair-review-state should either fail closed, reconcile, or report already_current in same-branch overlay recovery; got {repair_b}"
        );
    }
}

#[test]
fn plan_execution_status_and_explain_do_not_share_started_state_from_detached_worktree() {
    let (repo_dir, state_dir) = init_repo("workflow-same-branch-detached-status-and-explain");
    let repo_a = repo_dir.path();
    let state = state_dir.path();
    let linked_worktree_root = TempDir::new().expect("linked worktree tempdir should exist");
    let repo_b = linked_worktree_root
        .path()
        .join("same-branch-detached-status-and-explain");
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    let mut git_checkout = Command::new("git");
    git_checkout
        .args([
            "checkout",
            "-B",
            "workflow-same-branch-detached-status-and-explain",
        ])
        .current_dir(repo_a);
    run_checked(
        git_checkout,
        "git checkout workflow-same-branch-detached-status-and-explain",
    );

    let mut git_worktree_add = Command::new("git");
    git_worktree_add
        .arg("worktree")
        .arg("add")
        .arg("--force")
        .arg(&repo_b)
        .arg("workflow-same-branch-detached-status-and-explain")
        .current_dir(repo_a);
    run_checked(
        git_worktree_add,
        "git worktree add --force same-branch-detached-status-and-explain",
    );

    install_full_contract_ready_artifacts(repo_a);
    install_full_contract_ready_artifacts(&repo_b);

    let status_before_begin = run_plan_execution_json(
        repo_a,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status before detached same-branch status/explain sharing fixture",
    );
    let preflight = run_internal_plan_execution_preflight_json(
        repo_a,
        state,
        plan_rel,
        "preflight before detached same-branch status/explain sharing fixture",
    );
    assert_eq!(preflight["allowed"], true);
    run_plan_execution_json(
        repo_a,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_before_begin["execution_fingerprint"]
                .as_str()
                .expect("status fingerprint should be present"),
        ],
        "plan execution begin for detached same-branch status/explain sharing fixture",
    );

    let mut git_detach = Command::new("git");
    git_detach
        .args(["checkout", "--detach", "HEAD"])
        .current_dir(&repo_b);
    run_checked(
        git_detach,
        "git checkout --detach HEAD for same-branch detached sharing fixture",
    );

    let status_a = run_plan_execution_json(
        repo_a,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status for detached same-branch sharing on repo A",
    );
    let status_b = run_plan_execution_json(
        &repo_b,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status for detached same-branch sharing on repo B",
    );

    assert_eq!(status_a["execution_started"], "yes", "json: {status_a}");
    assert_eq!(status_a["active_task"], Value::from(1), "json: {status_a}");
    assert_eq!(status_a["active_step"], Value::from(1), "json: {status_a}");
    assert_eq!(status_b["execution_started"], "no", "json: {status_b}");
    assert_eq!(status_b["active_task"], Value::Null, "json: {status_b}");
    assert_eq!(status_b["active_step"], Value::Null, "json: {status_b}");
    assert_eq!(
        status_b["execution_run_id"],
        Value::Null,
        "detached worktrees must fail closed instead of borrowing another branch's started execution run"
    );

    let explain_a = run_plan_execution_json(
        repo_a,
        state,
        &["explain-review-state", "--plan", plan_rel],
        "plan execution explain-review-state for detached same-branch sharing on repo A",
    );
    let explain_b = run_plan_execution_json(
        &repo_b,
        state,
        &["explain-review-state", "--plan", plan_rel],
        "plan execution explain-review-state for detached same-branch sharing on repo B",
    );
    assert_ne!(
        explain_a["recommended_command"], explain_b["recommended_command"],
        "detached worktrees must not borrow another branch's explain-review-state recommended_command"
    );
}

#[test]
fn same_branch_worktrees_do_not_adopt_started_state_when_execution_fingerprint_differs() {
    let (repo_dir, state_dir) = init_repo("workflow-same-branch-fingerprint-guard");
    let repo_a = repo_dir.path();
    let state = state_dir.path();
    let linked_worktree_root = TempDir::new().expect("linked worktree tempdir should exist");
    let repo_b = linked_worktree_root
        .path()
        .join("same-branch-fingerprint-guard");
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let evidence_rel = derive_evidence_rel_path(plan_rel, 1);

    let mut git_checkout = Command::new("git");
    git_checkout
        .args(["checkout", "-B", "workflow-same-branch-fingerprint-guard"])
        .current_dir(repo_a);
    run_checked(
        git_checkout,
        "git checkout workflow-same-branch-fingerprint-guard",
    );

    let mut git_worktree_add = Command::new("git");
    git_worktree_add
        .arg("worktree")
        .arg("add")
        .arg("--force")
        .arg(&repo_b)
        .arg("workflow-same-branch-fingerprint-guard")
        .current_dir(repo_a);
    run_checked(
        git_worktree_add,
        "git worktree add --force same-branch-fingerprint-guard",
    );

    install_full_contract_ready_artifacts(repo_a);
    install_full_contract_ready_artifacts(&repo_b);
    let repo_b_evidence_path = repo_b.join(&evidence_rel);
    if let Some(parent) = repo_b_evidence_path.parent() {
        fs::create_dir_all(parent).expect("evidence fixture parent should be creatable");
    }
    fs::write(&repo_b_evidence_path, "### Task 1 Step 1\n")
        .expect("repo B evidence divergence should write");

    let status_a_before_begin = run_plan_execution_json(
        repo_a,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status before same-branch fingerprint guard fixture",
    );
    let status_b_before_begin = run_plan_execution_json(
        &repo_b,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status before same-branch fingerprint guard fixture on repo B",
    );
    let explain_b_before_begin = run_plan_execution_json(
        &repo_b,
        state,
        &["explain-review-state", "--plan", plan_rel],
        "plan execution explain-review-state before same-branch fingerprint guard fixture on repo B",
    );

    assert_ne!(
        status_a_before_begin["execution_fingerprint"],
        status_b_before_begin["execution_fingerprint"],
        "repo-local evidence divergence should produce a distinct execution fingerprint before same-branch adoption is considered"
    );
    assert_eq!(status_b_before_begin["execution_started"], "no");

    let preflight = run_internal_plan_execution_preflight_json(
        repo_a,
        state,
        plan_rel,
        "preflight before same-branch fingerprint guard fixture",
    );
    assert_eq!(preflight["allowed"], true);
    run_plan_execution_json(
        repo_a,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_a_before_begin["execution_fingerprint"]
                .as_str()
                .expect("status fingerprint should be present"),
        ],
        "plan execution begin for same-branch fingerprint guard fixture",
    );

    let status_b_after_begin = run_plan_execution_json(
        &repo_b,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status after same-branch fingerprint guard fixture on repo B",
    );
    let explain_b_after_begin = run_plan_execution_json(
        &repo_b,
        state,
        &["explain-review-state", "--plan", plan_rel],
        "plan execution explain-review-state after same-branch fingerprint guard fixture on repo B",
    );

    assert_eq!(status_b_after_begin["execution_started"], "no");
    assert!(
        status_b_after_begin["active_task"].is_null(),
        "repo B should not borrow repo A's active task when execution fingerprints differ"
    );
    assert!(
        status_b_after_begin["active_step"].is_null(),
        "repo B should not borrow repo A's active step when execution fingerprints differ"
    );
    assert_eq!(
        status_b_before_begin["phase_detail"], status_b_after_begin["phase_detail"],
        "repo B should preserve its local phase detail when same-branch fingerprints differ"
    );
    assert_eq!(
        explain_b_before_begin["next_action"], explain_b_after_begin["next_action"],
        "repo B explain-review-state should preserve its local next_action when same-branch fingerprints differ"
    );
    assert_eq!(
        explain_b_before_begin["recommended_command"], explain_b_after_begin["recommended_command"],
        "repo B explain-review-state should preserve its local recommended command when same-branch fingerprints differ"
    );
    assert_eq!(
        explain_b_before_begin["trace_summary"], explain_b_after_begin["trace_summary"],
        "repo B explain-review-state should preserve its local trace summary when same-branch fingerprints differ"
    );
}

#[test]
fn same_branch_worktrees_do_not_adopt_started_state_when_tracked_workspace_differs() {
    let (repo_dir, state_dir) = init_repo("workflow-same-branch-workspace-guard");
    let repo_a = repo_dir.path();
    let state = state_dir.path();
    let linked_worktree_root = TempDir::new().expect("linked worktree tempdir should exist");
    let repo_b = linked_worktree_root
        .path()
        .join("same-branch-workspace-guard");
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    let mut git_checkout = Command::new("git");
    git_checkout
        .args(["checkout", "-B", "workflow-same-branch-workspace-guard"])
        .current_dir(repo_a);
    run_checked(
        git_checkout,
        "git checkout workflow-same-branch-workspace-guard",
    );

    let mut git_worktree_add = Command::new("git");
    git_worktree_add
        .arg("worktree")
        .arg("add")
        .arg("--force")
        .arg(&repo_b)
        .arg("workflow-same-branch-workspace-guard")
        .current_dir(repo_a);
    run_checked(
        git_worktree_add,
        "git worktree add --force same-branch-workspace-guard",
    );

    install_full_contract_ready_artifacts(repo_a);
    install_full_contract_ready_artifacts(&repo_b);
    let repo_b_readme_path = repo_b.join("README.md");
    let repo_b_readme = fs::read_to_string(&repo_b_readme_path)
        .expect("repo B README should be readable before tracked workspace guard");
    fs::write(
        &repo_b_readme_path,
        format!("{repo_b_readme}tracked workspace divergence before same-branch adoption\n"),
    )
    .expect("repo B README should accept tracked workspace divergence");

    let status_a_before_begin = run_plan_execution_json(
        repo_a,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status before same-branch tracked workspace guard fixture",
    );
    let status_b_before_begin = run_plan_execution_json(
        &repo_b,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status before same-branch tracked workspace guard fixture on repo B",
    );
    let operator_b_before_begin = parse_json(
        &run_rust_featureforge_with_env(
            &repo_b,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "workflow operator before same-branch tracked workspace guard fixture on repo B",
        ),
        "workflow operator before same-branch tracked workspace guard fixture on repo B",
    );
    let explain_b_before_begin = run_plan_execution_json(
        &repo_b,
        state,
        &["explain-review-state", "--plan", plan_rel],
        "explain-review-state before same-branch tracked workspace guard fixture on repo B",
    );

    assert_ne!(
        status_a_before_begin["semantic_workspace_tree_id"],
        status_b_before_begin["semantic_workspace_tree_id"],
        "tracked workspace divergence should produce a distinct semantic_workspace_tree_id before same-branch adoption is considered"
    );
    assert_eq!(status_b_before_begin["execution_started"], "no");

    let preflight = run_internal_plan_execution_preflight_json(
        repo_a,
        state,
        plan_rel,
        "preflight before same-branch tracked workspace guard fixture",
    );
    assert_eq!(preflight["allowed"], true);
    run_plan_execution_json(
        repo_a,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_a_before_begin["execution_fingerprint"]
                .as_str()
                .expect("status fingerprint should be present"),
        ],
        "plan execution begin for same-branch tracked workspace guard fixture",
    );

    let status_b_after_begin = run_plan_execution_json(
        &repo_b,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status after same-branch tracked workspace guard fixture on repo B",
    );
    let operator_b_after_begin = parse_json(
        &run_rust_featureforge_with_env(
            &repo_b,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "workflow operator after same-branch tracked workspace guard fixture on repo B",
        ),
        "workflow operator after same-branch tracked workspace guard fixture on repo B",
    );
    let explain_b_after_begin = run_plan_execution_json(
        &repo_b,
        state,
        &["explain-review-state", "--plan", plan_rel],
        "explain-review-state after same-branch tracked workspace guard fixture on repo B",
    );

    assert_eq!(status_b_after_begin["execution_started"], "no");
    assert_eq!(
        status_b_before_begin["semantic_workspace_tree_id"],
        status_b_after_begin["semantic_workspace_tree_id"],
        "repo B should preserve its own semantic_workspace_tree_id when tracked workspace divergence blocks same-branch adoption"
    );
    assert!(
        status_b_after_begin["active_task"].is_null(),
        "repo B should not borrow repo A's active task when tracked workspace state differs"
    );
    assert!(
        status_b_after_begin["active_step"].is_null(),
        "repo B should not borrow repo A's active step when tracked workspace state differs"
    );
    assert!(
        operator_b_after_begin["phase"]
            .as_str()
            .is_some_and(|phase| {
                phase == "implementation_handoff" || phase == "execution_preflight"
            }),
        "repo B workflow operator phase should remain non-executing when tracked workspace state differs: {operator_b_after_begin:?}"
    );
    assert_eq!(
        operator_b_before_begin["next_action"], operator_b_after_begin["next_action"],
        "repo B workflow operator next_action should remain local when tracked workspace state differs"
    );
    assert_eq!(
        operator_b_before_begin["recommended_command"],
        operator_b_after_begin["recommended_command"],
        "repo B workflow operator recommended_command should remain local when tracked workspace state differs"
    );
    assert_eq!(
        explain_b_before_begin["next_action"], explain_b_after_begin["next_action"],
        "repo B explain-review-state next_action should remain local when tracked workspace state differs"
    );
    assert_eq!(
        explain_b_before_begin["recommended_command"], explain_b_after_begin["recommended_command"],
        "repo B explain-review-state recommended_command should remain local when tracked workspace state differs"
    );
    assert_eq!(
        explain_b_before_begin["trace_summary"], explain_b_after_begin["trace_summary"],
        "repo B explain-review-state trace_summary should remain local when tracked workspace state differs"
    );
}

#[test]
fn canonical_workflow_doctor_does_not_adopt_started_status_across_different_branch_worktrees() {
    let (repo_dir, state_dir) = init_repo("workflow-public-cross-branch-worktree");
    let repo_a = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-public-cross-branch-worktree";
    let decision_path = state
        .join("session-entry")
        .join("using-featureforge")
        .join(session_key);
    let linked_worktree_root = TempDir::new().expect("linked worktree tempdir should exist");
    let repo_b = linked_worktree_root
        .path()
        .join("cross-branch-linked-worktree");
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    let mut git_checkout = Command::new("git");
    git_checkout
        .args(["checkout", "-B", "workflow-public-cross-branch-worktree-a"])
        .current_dir(repo_a);
    run_checked(
        git_checkout,
        "git checkout workflow-public-cross-branch-worktree-a",
    );

    let mut git_worktree_add = Command::new("git");
    git_worktree_add
        .arg("worktree")
        .arg("add")
        .arg("--force")
        .arg("-b")
        .arg("workflow-public-cross-branch-worktree-b")
        .arg(&repo_b)
        .arg("HEAD")
        .current_dir(repo_a);
    run_checked(
        git_worktree_add,
        "git worktree add --force -b workflow-public-cross-branch-worktree-b cross-branch-linked-worktree HEAD",
    );

    install_full_contract_ready_artifacts(repo_a);
    install_full_contract_ready_artifacts(&repo_b);
    write_file(&decision_path, "enabled\n");

    let status_a = run_plan_execution_json(
        repo_a,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status before cross-branch worktree sharing fixture",
    );
    let preflight_a = parse_json(
        &run_workflow_preflight_output(
            repo_a,
            state,
            plan_rel,
            "workflow preflight before cross-branch worktree sharing fixture",
        ),
        "workflow preflight before cross-branch worktree sharing fixture",
    );
    assert_eq!(preflight_a["allowed"], true);
    run_plan_execution_json(
        repo_a,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_a["execution_fingerprint"]
                .as_str()
                .expect("status fingerprint should be present"),
        ],
        "plan execution begin for cross-branch worktree sharing fixture",
    );

    let doctor_a = parse_json(
        &run_rust_featureforge_with_env(
            repo_a,
            state,
            &["workflow", "doctor", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow doctor for cross-branch scope on repo A",
        ),
        "workflow doctor for cross-branch scope on repo A",
    );
    let doctor_b = parse_json(
        &run_rust_featureforge_with_env(
            &repo_b,
            state,
            &["workflow", "doctor", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow doctor for cross-branch scope on repo B",
        ),
        "workflow doctor for cross-branch scope on repo B",
    );

    assert_eq!(doctor_a["phase"], "executing");
    assert_eq!(doctor_a["execution_status"]["execution_started"], "yes");
    assert_eq!(doctor_a["execution_status"]["active_task"], 1);
    assert_eq!(doctor_a["execution_status"]["active_step"], 1);

    assert_ne!(
        doctor_b["phase"], "executing",
        "cross-branch worktrees must not inherit started execution routing from another branch"
    );
    assert_eq!(
        doctor_b["execution_status"]["execution_started"], "no",
        "cross-branch worktrees must not inherit started execution status from another branch"
    );
    assert!(
        doctor_b["execution_status"]["active_task"].is_null(),
        "cross-branch worktrees should not expose an active task when local execution has not started"
    );
    assert!(
        doctor_b["execution_status"]["active_step"].is_null(),
        "cross-branch worktrees should not expose an active step when local execution has not started"
    );
}

#[test]
fn canonical_workflow_routes_started_execution_back_to_the_current_execution_flow() {
    let (repo_dir, state_dir) = init_repo("workflow-phase-started-execution");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-started-execution";
    let decision_path = state
        .join("session-entry")
        .join("using-featureforge")
        .join(session_key);
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    install_full_contract_ready_artifacts(repo);
    write_file(&decision_path, "enabled\n");
    prepare_preflight_acceptance_workspace(repo, "workflow-phase-started-execution");

    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status before started-execution routing fixture",
    );
    let preflight_json = run_internal_plan_execution_preflight_json(
        repo,
        state,
        plan_rel,
        "plan execution preflight before started-execution routing fixture",
    );
    assert_eq!(preflight_json["allowed"], true);
    run_plan_execution_json(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_json["execution_fingerprint"]
                .as_str()
                .expect("status fingerprint should be present"),
        ],
        "plan execution begin for started-execution routing fixture",
    );

    let phase_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "phase", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow phase for started-execution routing fixture",
        ),
        "workflow phase for started-execution routing fixture",
    );
    assert_eq!(phase_json["phase"], "handoff_required");
    assert_eq!(phase_json["next_action"], "continue execution");

    let doctor_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "doctor", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow doctor for started-execution routing fixture",
        ),
        "workflow doctor for started-execution routing fixture",
    );
    assert_eq!(doctor_json["phase"], "executing");
    assert_eq!(doctor_json["execution_status"]["execution_started"], "yes");
    assert_eq!(doctor_json["execution_status"]["active_task"], 1);
    assert_eq!(doctor_json["execution_status"]["active_step"], 1);
    assert_eq!(doctor_json["preflight"], Value::Null);
    assert_eq!(doctor_json["gate_review"], Value::Null);
    assert_eq!(doctor_json["gate_finish"], Value::Null);

    let handoff_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "handoff", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow handoff for started-execution routing fixture",
        ),
        "workflow handoff for started-execution routing fixture",
    );
    assert_eq!(handoff_json["phase"], "handoff_required");
    assert_eq!(handoff_json["execution_started"], "yes");
    assert_eq!(handoff_json["next_action"], "continue execution");
    assert_eq!(
        handoff_json["recommended_skill"],
        "featureforge:executing-plans"
    );
    assert_eq!(
        handoff_json["recommendation_reason"],
        "Execution already started for the approved plan revision; continue with the current execution flow."
    );
}

#[test]
fn workflow_phase_routes_task_boundary_blocked() {
    let (repo_dir, state_dir) = init_repo("workflow-phase-task-boundary-blocked");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-task-boundary-blocked";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let spec_rel = "docs/featureforge/specs/2026-03-22-runtime-integration-hardening-design.md";

    install_full_contract_ready_artifacts(repo);
    write_file(
        &repo.join(plan_rel),
        &format!(
            r#"# Runtime Integration Hardening Implementation Plan

**Workflow State:** Engineering Approved
**Plan Revision:** 1
**Execution Mode:** none
**Source Spec:** `{spec_rel}`
**Source Spec Revision:** 1
**Last Reviewed By:** plan-eng-review

## Requirement Coverage Matrix

- REQ-001 -> Task 1
- REQ-004 -> Task 1
- VERIFY-001 -> Task 2

## Execution Strategy

- Execute Task 1 serially. It establishes boundary gating before follow-on work begins.
- Execute Task 2 serially after Task 1. It validates task-boundary workflow routing.

## Dependency Diagram

```text
Task 1 -> Task 2
```

## Task 1: Core flow

**Spec Coverage:** REQ-001, REQ-004
**Goal:** Task 1 execution reaches a boundary gate before Task 2 starts.

**Context:**
- Spec Coverage: REQ-001, REQ-004.

**Constraints:**
- Keep fixture inputs deterministic.

**Done when:**
- Task 1 execution reaches a boundary gate before Task 2 starts.

**Files:**
- Modify: `tests/workflow_runtime.rs`

- [ ] **Step 1: Prepare workflow fixture output**
- [ ] **Step 2: Validate workflow fixture output**

## Task 2: Follow-on flow

**Spec Coverage:** VERIFY-001
**Goal:** Task 2 should remain blocked until Task 1 closure requirements are met.

**Context:**
- Spec Coverage: VERIFY-001.

**Constraints:**
- Preserve deterministic task-boundary diagnostics.

**Done when:**
- Task 2 should remain blocked until Task 1 closure requirements are met.

**Files:**
- Modify: `tests/workflow_runtime.rs`

- [ ] **Step 1: Start the follow-on task**
"#
        ),
    );

    enable_session_decision(state, session_key);
    prepare_preflight_acceptance_workspace(repo, "workflow-phase-task-boundary-blocked");

    let status_before_begin = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status before task-boundary blocked workflow fixture execution",
    );
    let preflight = run_internal_plan_execution_preflight_json(
        repo,
        state,
        plan_rel,
        "preflight for task-boundary blocked workflow fixture execution",
    );
    assert_eq!(preflight["allowed"], true);
    let begin_task1_step1 = run_plan_execution_json(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_before_begin["execution_fingerprint"]
                .as_str()
                .expect("status should expose execution fingerprint before begin"),
        ],
        "begin task 1 step 1 for task-boundary blocked workflow fixture",
    );
    let complete_task1_step1 = run_plan_execution_json(
        repo,
        state,
        &[
            "complete",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "Completed task 1 step 1 for task-boundary blocked workflow fixture.",
            "--manual-verify-summary",
            "Verified by workflow task-boundary fixture setup.",
            "--file",
            "tests/workflow_runtime.rs",
            "--expect-execution-fingerprint",
            begin_task1_step1["execution_fingerprint"]
                .as_str()
                .expect("begin should expose execution fingerprint for complete"),
        ],
        "complete task 1 step 1 for task-boundary blocked workflow fixture",
    );
    let begin_task1_step2 = run_plan_execution_json(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "2",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            complete_task1_step1["execution_fingerprint"]
                .as_str()
                .expect("complete should expose execution fingerprint for next begin"),
        ],
        "begin task 1 step 2 for task-boundary blocked workflow fixture",
    );
    run_plan_execution_json(
        repo,
        state,
        &[
            "complete",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "2",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "Completed task 1 step 2 for task-boundary blocked workflow fixture.",
            "--manual-verify-summary",
            "Verified by workflow task-boundary fixture setup.",
            "--file",
            "tests/workflow_runtime.rs",
            "--expect-execution-fingerprint",
            begin_task1_step2["execution_fingerprint"]
                .as_str()
                .expect("begin should expose execution fingerprint for complete"),
        ],
        "complete task 1 step 2 for task-boundary blocked workflow fixture",
    );
    let branch = current_branch_name(repo);
    update_authoritative_harness_state(repo, state, &branch, plan_rel, 1, &[]);
    run_runtime_review_dispatch_authority_json(
        repo,
        state,
        plan_rel,
        ReviewDispatchScopeArg::Task,
        Some(1),
        "record task-boundary review dispatch for blocked workflow fixture",
    );
    let mismatch_dispatch_output = run_internal_plan_execution_output(
        plan_execution_direct_support::run_runtime_review_dispatch_authority_json(
            repo,
            state,
            &RecordReviewDispatchArgs {
                plan: plan_rel.into(),
                scope: ReviewDispatchScopeArg::Task,
                task: Some(2),
            },
        ),
    );
    assert!(
        !mismatch_dispatch_output.status.success(),
        "task-boundary blocked fixture should fail closed for non-blocking redispatch target\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&mismatch_dispatch_output.stdout),
        String::from_utf8_lossy(&mismatch_dispatch_output.stderr)
    );
    let mismatch_dispatch_payload = if mismatch_dispatch_output.stdout.is_empty() {
        mismatch_dispatch_output.stderr.as_slice()
    } else {
        mismatch_dispatch_output.stdout.as_slice()
    };
    let mismatch_dispatch_failure: Value = serde_json::from_slice(mismatch_dispatch_payload)
        .expect("task-boundary blocked mismatch redispatch should emit json failure payload");
    assert_eq!(
        mismatch_dispatch_failure["error_class"],
        Value::from("InvalidCommandInput"),
        "task-boundary blocked mismatch redispatch should fail with InvalidCommandInput, got {mismatch_dispatch_failure:?}"
    );
    assert!(
        mismatch_dispatch_failure["message"]
            .as_str()
            .is_some_and(|message| {
                message.contains("does not match the current task review-dispatch target")
            }),
        "task-boundary blocked mismatch redispatch should explain the current dispatch target contract, got {mismatch_dispatch_failure:?}"
    );

    let execution_status = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "execution status after task 1 completion for task-boundary blocked workflow fixture",
    );
    assert_eq!(execution_status["active_task"], Value::Null);
    assert_eq!(execution_status["blocking_task"], Value::from(1));
    assert_eq!(execution_status["blocking_step"], Value::Null);
    assert!(
        execution_status["reason_codes"]
            .as_array()
            .is_some_and(|codes| {
                codes
                    .iter()
                    .any(|code| code.as_str() == Some("prior_task_current_closure_missing"))
            }),
        "execution status should surface prior_task_current_closure_missing for task-boundary blocked fixture, got {execution_status:?}"
    );

    let phase_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "phase", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow phase for task-boundary blocked routing fixture",
        ),
        "workflow phase for task-boundary blocked routing fixture",
    );
    let doctor_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "doctor", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow doctor for task-boundary blocked routing fixture",
        ),
        "workflow doctor for task-boundary blocked routing fixture",
    );
    let handoff_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "handoff", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow handoff for task-boundary blocked routing fixture",
        ),
        "workflow handoff for task-boundary blocked routing fixture",
    );

    let expected_phase = public_harness_phase_from_spec("task_closure_pending");
    assert_eq!(
        phase_json["phase"], expected_phase,
        "task-boundary blocked phase fixture should route to task_closure_pending; phase payload: {phase_json:?}"
    );
    assert_eq!(phase_json["next_action"], "close current task");
    assert_eq!(doctor_json["phase"], expected_phase);
    assert_eq!(doctor_json["next_action"], "close current task");
    assert_eq!(handoff_json["phase"], expected_phase);
    assert_eq!(handoff_json["next_action"], "close current task");
    assert_eq!(handoff_json["state_kind"], "actionable_public_command");
    assert_eq!(
        handoff_json["next_public_action"]["command"],
        handoff_json["recommended_command"]
    );
    assert_eq!(
        handoff_json["recommended_skill"],
        "featureforge:executing-plans"
    );
    assert!(
        handoff_json["recommendation_reason"]
            .as_str()
            .is_some_and(|reason| {
                reason.contains("Task 1 closure is ready to record/refresh")
                    && reason.contains("Follow the routed command")
                    && reason.contains("close-current-task")
            }),
        "workflow handoff should surface task-boundary closure-recording guidance, got {handoff_json:?}"
    );
    assert!(
        doctor_json["execution_status"]["reason_codes"]
            .as_array()
            .is_some_and(|codes| {
                codes
                    .iter()
                    .any(|code| code.as_str() == Some("prior_task_current_closure_missing"))
            }),
        "workflow doctor should preserve execution reason-code parity for task-boundary blocks, got {doctor_json:?}"
    );
}

#[test]
fn workflow_handoff_prefers_shared_task_closure_route_over_forged_dispatch_reason_code() {
    let (repo_dir, state_dir) = init_repo("workflow-phase-task-boundary-dispatch-blocked");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-task-boundary-dispatch-blocked";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let spec_rel = "docs/featureforge/specs/2026-03-22-runtime-integration-hardening-design.md";

    install_full_contract_ready_artifacts(repo);
    write_file(
        &repo.join(plan_rel),
        &format!(
            r#"# Runtime Integration Hardening Implementation Plan

**Workflow State:** Engineering Approved
**Plan Revision:** 1
**Execution Mode:** none
**Source Spec:** `{spec_rel}`
**Source Spec Revision:** 1
**Last Reviewed By:** plan-eng-review

## Requirement Coverage Matrix

- REQ-001 -> Task 1
- REQ-004 -> Task 1
- VERIFY-001 -> Task 2

## Execution Strategy

- Execute Task 1 serially. It establishes boundary gating before follow-on work begins.
- Execute Task 2 serially after Task 1. It validates task-boundary workflow routing.

## Dependency Diagram

```text
Task 1 -> Task 2
```

## Task 1: Core flow

**Spec Coverage:** REQ-001, REQ-004
**Goal:** Task 1 reaches task-boundary closure gate before Task 2 starts.

**Context:**
- Spec Coverage: REQ-001, REQ-004.

**Constraints:**
- Keep fixture input deterministic.

**Done when:**
- Task 1 reaches task-boundary closure gate before Task 2 starts.

**Files:**
- Modify: `tests/workflow_runtime.rs`

- [ ] **Step 1: Prepare workflow fixture output**

## Task 2: Follow-on flow

**Spec Coverage:** VERIFY-001
**Goal:** Task 2 remains blocked until task-boundary closure is satisfied.

**Context:**
- Spec Coverage: VERIFY-001.

**Constraints:**
- Preserve task-boundary diagnostics.

**Done when:**
- Task 2 remains blocked until task-boundary closure is satisfied.

**Files:**
- Modify: `tests/workflow_runtime.rs`

- [ ] **Step 1: Start the follow-on task**
"#
        ),
    );

    enable_session_decision(state, session_key);
    prepare_preflight_acceptance_workspace(repo, "workflow-phase-task-boundary-dispatch-blocked");

    let status_before_begin = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status before begin for workflow dispatch-blocked fixture",
    );
    let preflight = run_internal_plan_execution_preflight_json(
        repo,
        state,
        plan_rel,
        "preflight for workflow dispatch-blocked fixture",
    );
    assert_eq!(preflight["allowed"], true);

    let begin_task1_step1 = run_plan_execution_json(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_before_begin["execution_fingerprint"]
                .as_str()
                .expect("status should expose execution fingerprint before begin"),
        ],
        "begin task 1 step 1 for workflow dispatch-blocked fixture",
    );
    run_plan_execution_json(
        repo,
        state,
        &[
            "complete",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "Completed task 1 step 1 for workflow dispatch-blocked fixture.",
            "--manual-verify-summary",
            "Verified by workflow dispatch-blocked fixture setup.",
            "--file",
            "tests/workflow_runtime.rs",
            "--expect-execution-fingerprint",
            begin_task1_step1["execution_fingerprint"]
                .as_str()
                .expect("begin should expose execution fingerprint for complete"),
        ],
        "complete task 1 step 1 for workflow dispatch-blocked fixture",
    );

    let branch = current_branch_name(repo);
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            ("blocking_task", Value::from(1)),
            ("blocking_step", Value::Null),
            (
                "reason_codes",
                json!(["prior_task_review_dispatch_missing"]),
            ),
        ],
    );

    let doctor_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "doctor", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow doctor for task-boundary dispatch-blocked fixture",
        ),
        "workflow doctor for task-boundary dispatch-blocked fixture",
    );
    let phase_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "phase", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow phase for task-boundary dispatch-blocked fixture",
        ),
        "workflow phase for task-boundary dispatch-blocked fixture",
    );
    let expected_operator_follow_up = "Task 1 closure is ready to record/refresh";
    assert_eq!(
        phase_json["next_action"],
        Value::from("close current task"),
        "workflow phase json should follow the shared task-closure route even when harness reason codes are forged, got {phase_json:?}"
    );
    assert!(
        phase_json["recommended_command"]
            .as_str()
            .is_some_and(|command| command.contains("close-current-task")),
        "workflow phase json should expose the shared close-current-task command instead of honoring forged dispatch-only reason codes, got {phase_json:?}"
    );
    assert!(
        phase_json["next_step"]
            .as_str()
            .is_some_and(|next_step| next_step.contains(expected_operator_follow_up)),
        "workflow phase json should include task-closure recording guidance from the shared routing engine, got {phase_json:?}"
    );
    assert_eq!(
        doctor_json["next_action"],
        Value::from("close current task"),
        "workflow doctor should follow the shared task-closure route even when harness reason codes are forged, got {doctor_json:?}"
    );
    assert!(
        doctor_json["recommended_command"]
            .as_str()
            .is_some_and(|command| command.contains("close-current-task")),
        "workflow doctor should expose the shared close-current-task command instead of honoring forged dispatch-only reason codes, got {doctor_json:?}"
    );
    assert!(
        doctor_json["next_step"]
            .as_str()
            .is_some_and(|next_step| next_step.contains(expected_operator_follow_up)),
        "workflow doctor should include task-closure recording guidance from the shared routing engine, got {doctor_json:?}"
    );

    let doctor_output = run_rust_featureforge_with_env(
        repo,
        state,
        &["workflow", "doctor"],
        &[("FEATUREFORGE_SESSION_KEY", session_key)],
        "workflow doctor text for task-boundary dispatch-blocked fixture",
    );
    assert!(doctor_output.status.success());
    let doctor_stdout = String::from_utf8_lossy(&doctor_output.stdout);
    assert!(
        doctor_stdout.contains(expected_operator_follow_up),
        "doctor stdout:\n{doctor_stdout}"
    );

    let handoff_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "handoff", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow handoff for task-boundary dispatch-blocked fixture",
        ),
        "workflow handoff for task-boundary dispatch-blocked fixture",
    );
    assert!(
        handoff_json["recommendation_reason"]
            .as_str()
            .is_some_and(|reason| reason.contains(expected_operator_follow_up)),
        "workflow handoff should include task-review dispatch guidance for dispatch-blocked routing, got {handoff_json:?}"
    );

    let handoff_output = run_rust_featureforge_with_env(
        repo,
        state,
        &["workflow", "handoff"],
        &[("FEATUREFORGE_SESSION_KEY", session_key)],
        "workflow handoff text for task-boundary dispatch-blocked fixture",
    );
    assert!(handoff_output.status.success());
    let handoff_stdout = String::from_utf8_lossy(&handoff_output.stdout);
    assert!(
        handoff_stdout.contains(expected_operator_follow_up),
        "workflow handoff text should include task-review dispatch guidance, got:\n{handoff_stdout}"
    );
}

#[test]
fn canonical_workflow_routes_blocked_preflight_back_to_execution_handoff() {
    let (repo_dir, state_dir) = init_repo("workflow-phase-blocked-preflight");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-blocked-preflight";
    let decision_path = state
        .join("session-entry")
        .join("using-featureforge")
        .join(session_key);

    install_full_contract_ready_artifacts(repo);
    write_file(&decision_path, "enabled\n");
    write_file(&repo.join(".git/MERGE_HEAD"), "deadbeef\n");

    let phase_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "phase", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow phase for blocked-preflight routing fixture",
        ),
        "workflow phase for blocked-preflight routing fixture",
    );
    assert_eq!(phase_json["phase"], "execution_preflight");
    assert_eq!(phase_json["next_action"], "execution preflight");

    let doctor_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "doctor", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow doctor for blocked-preflight routing fixture",
        ),
        "workflow doctor for blocked-preflight routing fixture",
    );
    assert_eq!(doctor_json["phase"], "execution_preflight");
    assert_eq!(doctor_json["execution_status"]["execution_started"], "no");
    assert_eq!(doctor_json["preflight"]["allowed"], false);
    assert_eq!(
        doctor_json["preflight"]["failure_class"],
        "WorkspaceNotSafe"
    );
    assert!(
        doctor_json["preflight"]["reason_codes"]
            .as_array()
            .expect("reason_codes should stay an array")
            .iter()
            .any(|value| value == &Value::String(String::from("merge_in_progress")))
    );
    assert_eq!(doctor_json["gate_review"], Value::Null);
    assert_eq!(doctor_json["gate_finish"], Value::Null);

    let handoff_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "handoff", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow handoff for blocked-preflight routing fixture",
        ),
        "workflow handoff for blocked-preflight routing fixture",
    );
    assert_eq!(handoff_json["phase"], "execution_preflight");
    assert_eq!(handoff_json["execution_started"], "no");
    assert_eq!(handoff_json["next_action"], "execution preflight");
    assert_eq!(handoff_json["recommended_skill"], "");
    assert_eq!(handoff_json["recommendation"], Value::Null);
    assert_eq!(handoff_json["recommendation_reason"], "");
}

#[test]
fn canonical_workflow_routes_dirty_worktree_back_to_execution_handoff() {
    let (repo_dir, state_dir) = init_repo("workflow-phase-dirty-preflight");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-dirty-preflight";
    let decision_path = state
        .join("session-entry")
        .join("using-featureforge")
        .join(session_key);

    install_full_contract_ready_artifacts(repo);
    write_file(&decision_path, "enabled\n");
    write_file(
        &repo.join("README.md"),
        "# workflow-phase-dirty-preflight\ntracked change before execution\n",
    );

    let phase_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "phase", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow phase for dirty-worktree preflight routing fixture",
        ),
        "workflow phase for dirty-worktree preflight routing fixture",
    );
    assert_eq!(phase_json["phase"], "execution_preflight");
    assert_eq!(phase_json["next_action"], "execution preflight");

    let doctor_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "doctor", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow doctor for dirty-worktree preflight routing fixture",
        ),
        "workflow doctor for dirty-worktree preflight routing fixture",
    );
    assert_eq!(doctor_json["phase"], "execution_preflight");
    assert_eq!(doctor_json["execution_status"]["execution_started"], "no");
    assert_eq!(doctor_json["preflight"]["allowed"], false);
    assert_eq!(
        doctor_json["preflight"]["failure_class"],
        "WorkspaceNotSafe"
    );
    assert!(
        doctor_json["preflight"]["reason_codes"]
            .as_array()
            .expect("reason_codes should stay an array")
            .iter()
            .any(|value| value == &Value::String(String::from("tracked_worktree_dirty")))
    );
    assert_eq!(doctor_json["gate_review"], Value::Null);
    assert_eq!(doctor_json["gate_finish"], Value::Null);
}

#[test]
fn canonical_workflow_handoff_rejects_legacy_pre_harness_cutover_state() {
    let (repo_dir, state_dir) = init_repo("workflow-handoff-legacy-pre-harness-cutover");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-handoff-legacy-pre-harness-cutover";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let cutover_message = "Legacy pre-harness execution evidence is no longer accepted; regenerate execution evidence using the harness v2 format.";
    let decision_path = state
        .join("session-entry")
        .join("using-featureforge")
        .join(session_key);

    install_full_contract_ready_artifacts(repo);
    enable_session_decision(state, session_key);
    assert_eq!(
        fs::read_to_string(&decision_path).expect("session decision should be readable"),
        "enabled\n"
    );
    assert!(
        fs::read_to_string(repo.join(plan_rel))
            .expect("approved plan fixture should be readable")
            .contains("**Workflow State:** Engineering Approved"),
        "fixture should keep an engineering-approved plan for workflow handoff routing"
    );

    let execution_status = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status before legacy pre-harness cutover handoff fixture",
    );
    let evidence_path = repo.join(
        execution_status["evidence_path"]
            .as_str()
            .expect("execution status should expose evidence_path"),
    );
    write_file(&repo.join("docs/example-output.md"), "legacy output\n");
    write_file(
        &evidence_path,
        &format!(
            "# Execution Evidence: 2026-03-17-example-execution-plan\n\n**Plan Path:** {plan_rel}\n**Plan Revision:** 1\n\n## Step Evidence\n\n### Task 1 Step 1\n#### Attempt 1\n**Status:** Completed\n**Recorded At:** 2026-03-17T14:22:31Z\n**Execution Source:** featureforge:executing-plans\n**Claim:** Prepared the workspace for execution.\n**Files:**\n- docs/example-output.md\n**Verification:**\n- Manual verification recorded in fixture setup.\n**Invalidation Reason:** N/A\n"
        ),
    );
    assert!(
        fs::read_to_string(&evidence_path)
            .expect("legacy execution evidence fixture should be readable")
            .contains("## Step Evidence"),
        "fixture should inject legacy pre-harness execution evidence"
    );

    let handoff_output = run_rust_featureforge_with_env(
        repo,
        state,
        &["workflow", "handoff", "--json"],
        &[("FEATUREFORGE_SESSION_KEY", session_key)],
        "workflow handoff for legacy pre-harness cutover fixture",
    );
    assert!(
        !handoff_output.status.success(),
        "workflow handoff --json should fail closed for legacy pre-harness cutover state, got {:?}\nstdout:\n{}\nstderr:\n{}",
        handoff_output.status,
        String::from_utf8_lossy(&handoff_output.stdout),
        String::from_utf8_lossy(&handoff_output.stderr)
    );
    let stderr = String::from_utf8_lossy(&handoff_output.stderr);
    assert!(
        stderr.contains("MalformedExecutionState"),
        "workflow handoff should report malformed legacy execution evidence, stderr:\n{stderr}"
    );
    assert!(
        stderr.contains(cutover_message),
        "workflow handoff should explain legacy pre-harness cutover rejection, stderr:\n{stderr}"
    );
}

#[test]
fn canonical_workflow_routes_accepted_preflight_from_harness_state_even_when_workspace_becomes_dirty()
 {
    let (repo_dir, state_dir) = init_repo("workflow-phase-accepted-preflight-dirty");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-accepted-preflight-dirty";
    let decision_path = state
        .join("session-entry")
        .join("using-featureforge")
        .join(session_key);
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    install_full_contract_ready_artifacts(repo);
    write_file(&decision_path, "enabled\n");
    prepare_preflight_acceptance_workspace(repo, "workflow-phase-accepted-preflight-dirty");

    let preflight_json = run_internal_plan_execution_preflight_json(
        repo,
        state,
        plan_rel,
        "explicit plan execution preflight acceptance before dirty workspace routing fixture",
    );
    assert_eq!(preflight_json["allowed"], true);

    let status_after_preflight = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status after explicit plan execution preflight acceptance before dirty workspace routing fixture",
    );
    assert!(
        status_after_preflight["execution_run_id"]
            .as_str()
            .is_some_and(|value| !value.is_empty()),
        "explicit plan execution preflight should persist execution_run_id before workspace becomes dirty"
    );

    write_file(
        &repo.join("README.md"),
        "# workflow-phase-accepted-preflight-dirty\ntracked change after execution preflight acceptance\n",
    );

    let status_after_workspace_dirty = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "status after workspace dirties following explicit plan execution preflight acceptance",
    );
    assert!(
        status_after_workspace_dirty["execution_run_id"]
            .as_str()
            .is_some_and(|value| !value.is_empty()),
        "accepted preflight should keep plan execution status.execution_run_id non-empty after workspace dirties"
    );
    assert_eq!(
        status_after_workspace_dirty["harness_phase"], "execution_preflight",
        "accepted preflight should keep plan execution status.harness_phase at execution_preflight after workspace dirties"
    );

    let phase_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "phase", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow phase for accepted-preflight dirty-workspace routing fixture",
        ),
        "workflow phase for accepted-preflight dirty-workspace routing fixture",
    );
    assert_eq!(phase_json["phase"], "execution_preflight");
    assert_eq!(phase_json["next_action"], "execution preflight");

    let handoff_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "handoff", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow handoff for accepted-preflight dirty-workspace routing fixture",
        ),
        "workflow handoff for accepted-preflight dirty-workspace routing fixture",
    );
    assert_eq!(handoff_json["phase"], "execution_preflight");
    assert_eq!(handoff_json["next_action"], "execution preflight");

    let doctor_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "doctor", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow doctor for accepted-preflight dirty-workspace routing fixture",
        ),
        "workflow doctor for accepted-preflight dirty-workspace routing fixture",
    );
    assert!(
        doctor_json["execution_status"]["execution_run_id"]
            .as_str()
            .is_some_and(|value| !value.is_empty()),
        "accepted preflight should keep doctor.execution_status.execution_run_id non-empty after workspace dirties"
    );
}

#[test]
fn canonical_workflow_doctor_uses_accepted_preflight_truth_after_workspace_dirties() {
    let (repo_dir, state_dir) = init_repo("workflow-doctor-accepted-preflight-dirty");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-doctor-accepted-preflight-dirty";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    install_full_contract_ready_artifacts(repo);
    enable_session_decision(state, session_key);
    prepare_preflight_acceptance_workspace(repo, "workflow-doctor-accepted-preflight-dirty");

    let preflight_json = run_internal_plan_execution_preflight_json(
        repo,
        state,
        plan_rel,
        "explicit plan execution preflight acceptance before doctor dirty-workspace fixture",
    );
    assert_eq!(preflight_json["allowed"], true);

    write_file(
        &repo.join("README.md"),
        "# workflow-doctor-accepted-preflight-dirty\ntracked change after execution preflight acceptance\n",
    );
    assert!(
        discover_repository(repo)
            .expect("workspace dirtiness helper should discover repository")
            .is_dirty()
            .expect("workspace dirtiness helper should compute dirtiness"),
        "workspace should be dirty after introducing tracked change post-preflight acceptance"
    );

    let doctor_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "doctor", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow doctor for accepted-preflight truth after dirty-workspace fixture",
        ),
        "workflow doctor for accepted-preflight truth after dirty-workspace fixture",
    );
    assert_eq!(doctor_json["phase"], "execution_preflight");
    assert!(
        doctor_json["execution_status"]["execution_run_id"]
            .as_str()
            .is_some_and(|value| !value.is_empty()),
        "doctor.execution_status.execution_run_id should stay non-empty after accepted preflight even when workspace becomes dirty"
    );

    assert_ne!(
        doctor_json["preflight"]["failure_class"], "WorkspaceNotSafe",
        "workflow doctor should not surface a fresh WorkspaceNotSafe preflight failure after preflight was already accepted"
    );
    assert_ne!(
        doctor_json["preflight"]["allowed"], false,
        "workflow doctor should not report preflight.allowed=false once accepted preflight state exists"
    );
}

#[test]
fn canonical_workflow_gate_review_rejects_stale_authoritative_late_gate_truth() {
    let (repo_dir, state_dir) = init_repo("workflow-phase-gate-review-stale-authoritative-truth");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-gate-review-stale-authoritative-truth";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    complete_workflow_fixture_execution_with_qa_requirement(repo, state, plan_rel, "required");
    let branch = current_branch_name(repo);
    let expected_base_branch = expected_release_base_branch(repo);
    let test_plan_path = write_branch_test_plan_artifact(repo, state, plan_rel, "yes");
    let review_path = write_branch_review_artifact(repo, state, plan_rel, &expected_base_branch);
    let qa_path = write_branch_qa_artifact(repo, state, plan_rel, &test_plan_path);
    let release_path = write_branch_release_artifact(repo, state, plan_rel, &expected_base_branch);
    enable_session_decision(state, session_key);

    let authoritative_review_source = fs::read_to_string(&review_path)
        .expect("source review artifact should be readable for stale-authoritative fixture");
    let authoritative_review_fingerprint = sha256_hex(authoritative_review_source.as_bytes());
    write_file(
        &harness_authoritative_artifact_path(
            state,
            &repo_slug(repo),
            &branch,
            &format!("final-review-{authoritative_review_fingerprint}.md"),
        ),
        &authoritative_review_source,
    );

    let authoritative_test_plan_source = fs::read_to_string(&test_plan_path)
        .expect("source test-plan artifact should be readable for stale-authoritative fixture");
    let authoritative_test_plan_fingerprint = sha256_hex(authoritative_test_plan_source.as_bytes());
    let authoritative_test_plan_path = harness_authoritative_artifact_path(
        state,
        &repo_slug(repo),
        &branch,
        &format!("test-plan-{authoritative_test_plan_fingerprint}.md"),
    );
    write_file(
        &authoritative_test_plan_path,
        &authoritative_test_plan_source,
    );

    let authoritative_qa_source = rewrite_source_test_plan_header(
        &fs::read_to_string(&qa_path)
            .expect("source QA artifact should be readable for stale-authoritative fixture"),
        &authoritative_test_plan_path,
    );
    let authoritative_qa_fingerprint = sha256_hex(authoritative_qa_source.as_bytes());
    write_file(
        &harness_authoritative_artifact_path(
            state,
            &repo_slug(repo),
            &branch,
            &format!("browser-qa-{authoritative_qa_fingerprint}.md"),
        ),
        &authoritative_qa_source,
    );

    let authoritative_release_source = fs::read_to_string(&release_path)
        .expect("source release artifact should be readable for stale-authoritative fixture");
    let authoritative_release_fingerprint = sha256_hex(authoritative_release_source.as_bytes());
    write_file(
        &harness_authoritative_artifact_path(
            state,
            &repo_slug(repo),
            &branch,
            &format!("release-docs-{authoritative_release_fingerprint}.md"),
        ),
        &authoritative_release_source,
    );

    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            ("schema_version", Value::from(2)),
            ("harness_phase", Value::from("final_review_pending")),
            ("latest_authoritative_sequence", Value::from(17)),
            ("dependency_index_state", Value::from("stale")),
            ("final_review_state", Value::from("stale")),
            ("browser_qa_state", Value::from("stale")),
            ("release_docs_state", Value::from("stale")),
            (
                "last_final_review_artifact_fingerprint",
                Value::from(authoritative_review_fingerprint),
            ),
            (
                "last_browser_qa_artifact_fingerprint",
                Value::from(authoritative_qa_fingerprint.clone()),
            ),
            (
                "last_release_docs_artifact_fingerprint",
                Value::from(authoritative_release_fingerprint),
            ),
        ],
    );

    let doctor_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "doctor", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow doctor for stale authoritative late-gate truth",
        ),
        "workflow doctor for stale authoritative late-gate truth",
    );
    let gate_review_json = parse_json(
        &run_workflow_gate_review_output(
            repo,
            state,
            plan_rel,
            "workflow gate review for stale authoritative late-gate truth",
        ),
        "workflow gate review for stale authoritative late-gate truth",
    );

    assert_eq!(
        gate_review_json["allowed"], false,
        "workflow gate review should load authoritative late-gate truth before trusting v2 evidence; got {gate_review_json:?}"
    );
    assert_eq!(
        doctor_json["gate_review"]["allowed"], false,
        "workflow doctor should report review gate blocked when authoritative late-gate truth is stale; got {doctor_json:?}"
    );
}

#[test]
fn canonical_workflow_gate_review_fail_closes_on_malformed_authoritative_late_gate_truth_values() {
    let (repo_dir, state_dir) =
        init_repo("workflow-phase-gate-review-malformed-authoritative-truth");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-gate-review-malformed-authoritative-truth";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    complete_workflow_fixture_execution_with_qa_requirement(repo, state, plan_rel, "required");
    enable_session_decision(state, session_key);

    let branch = current_branch_name(repo);
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            ("dependency_index_state", Value::from("fresh")),
            ("final_review_state", Value::from("malformed")),
            ("browser_qa_state", Value::from("malformed")),
            ("release_docs_state", Value::from("malformed")),
        ],
    );

    let gate_review_json = parse_json(
        &run_workflow_gate_review_output(
            repo,
            state,
            plan_rel,
            "workflow gate review for malformed authoritative late-gate truth",
        ),
        "workflow gate review for malformed authoritative late-gate truth",
    );

    assert_eq!(gate_review_json["allowed"], false, "{gate_review_json:?}");
    assert_eq!(
        gate_review_json["failure_class"], "StaleProvenance",
        "{gate_review_json:?}"
    );
    assert!(
        gate_review_json["reason_codes"]
            .as_array()
            .is_some_and(|codes| {
                codes
                    .iter()
                    .any(|code| code == "final_review_state_missing")
                    && codes.iter().any(|code| code == "browser_qa_state_missing")
                    && codes
                        .iter()
                        .any(|code| code == "release_docs_state_missing")
            }),
        "gate-review should fail closed with malformed authoritative late-gate reason codes; got {gate_review_json:?}"
    );
}

#[test]
fn canonical_workflow_phase_requires_authoritative_review_truth_before_ready_for_branch_completion()
{
    let (repo_dir, state_dir) =
        init_repo("workflow-phase-ready-guard-stale-authoritative-late-gate-truth");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-ready-guard-stale-authoritative-late-gate-truth";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let base_branch = expected_release_base_branch(repo);

    complete_workflow_fixture_execution_with_qa_requirement(repo, state, plan_rel, "required");
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_dispatched_branch_review_artifact(repo, state, plan_rel, &base_branch);
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    let branch = current_branch_name(repo);
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            ("harness_phase", Value::from("final_review_pending")),
            ("latest_authoritative_sequence", Value::from(17)),
            ("dependency_index_state", Value::from("fresh")),
            ("final_review_state", Value::from("stale")),
            ("browser_qa_state", Value::from("not_required")),
            ("release_docs_state", Value::from("stale")),
        ],
    );
    enable_session_decision(state, session_key);

    let phase_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "phase", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow phase for ready guard stale-authoritative late-gate truth fixture",
        ),
        "workflow phase for ready guard stale-authoritative late-gate truth fixture",
    );
    let handoff_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "handoff", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow handoff for ready guard stale-authoritative late-gate truth fixture",
        ),
        "workflow handoff for ready guard stale-authoritative late-gate truth fixture",
    );
    let gate_review_json = parse_json(
        &run_workflow_gate_review_output(
            repo,
            state,
            plan_rel,
            "workflow gate review for ready guard stale-authoritative late-gate truth fixture",
        ),
        "workflow gate review for ready guard stale-authoritative late-gate truth fixture",
    );
    let gate_finish_json = parse_json(
        &run_workflow_gate_finish_output(
            repo,
            state,
            plan_rel,
            "workflow gate finish for ready guard stale-authoritative late-gate truth fixture",
        ),
        "workflow gate finish for ready guard stale-authoritative late-gate truth fixture",
    );
    assert_eq!(gate_review_json["allowed"], false, "{gate_review_json:?}");
    assert_eq!(
        gate_finish_json["allowed"], false,
        "gate-finish must consume the same authoritative late-gate truth as gate-review; got {gate_finish_json:?}"
    );
    assert!(
        gate_finish_json["reason_codes"]
            .as_array()
            .is_some_and(|codes| {
                codes.iter().any(|code| code == "qa_artifact_missing")
                    && codes.iter().any(|code| code == "browser_qa_state_missing")
            }),
        "gate-finish should expose event-authoritative late-gate blockers and ignore projection-only stale fields; got {gate_finish_json:?}"
    );
    assert_eq!(
        phase_json["phase"], "qa_pending",
        "projection-only stale late-gate fields must not override typed milestone records, got {phase_json:?}"
    );
    assert_eq!(
        handoff_json["recommended_skill"], "featureforge:qa-only",
        "typed final-review truth with required QA should route to QA, got {handoff_json:?}"
    );
}

#[test]
fn canonical_workflow_doctor_and_gate_finish_prefer_recorded_authoritative_final_review_over_newer_branch_decoy()
 {
    let (repo_dir, state_dir) = init_repo("workflow-phase-authoritative-final-review-provenance");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-authoritative-final-review-provenance";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    complete_workflow_fixture_execution_with_qa_requirement(repo, state, plan_rel, "required");
    let branch = current_branch_name(repo);
    let expected_base_branch = expected_release_base_branch(repo);
    let test_plan_path = write_branch_test_plan_artifact(repo, state, plan_rel, "yes");
    let review_path =
        write_dispatched_branch_review_artifact(repo, state, plan_rel, &expected_base_branch);
    let qa_path = write_branch_qa_artifact(repo, state, plan_rel, &test_plan_path);
    write_branch_release_artifact(repo, state, plan_rel, &expected_base_branch);
    enable_session_decision(state, session_key);
    let _ = review_path;
    let _ = publish_authoritative_qa_truth(
        repo,
        state,
        plan_rel,
        &test_plan_path,
        &qa_path,
        &expected_base_branch,
    );

    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            ("schema_version", Value::from(2)),
            ("harness_phase", Value::from("ready_for_branch_completion")),
            ("latest_authoritative_sequence", Value::from(17)),
        ],
    );

    write_file(
        &project_artifact_dir(repo, state).join(format!(
            "tester-{}-code-review-99999999-999999.md",
            branch_storage_key(&branch)
        )),
        &format!(
            "# Code Review Result\n**Source Plan:** `{plan_rel}`\n**Source Plan Revision:** 1\n**Branch:** {branch}\n**Repo:** {}\n**Base Branch:** {branch}\n**Head SHA:** 0000000000000000000000000000000000000000\n**Result:** pass\n**Generated By:** featureforge:requesting-code-review\n**Generated At:** 2026-03-24T23:59:59Z\n\n## Summary\n- newer same-branch decoy should not override recorded authoritative final-review provenance.\n",
            repo_slug(repo)
        ),
    );

    let doctor_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "doctor", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow doctor for authoritative final-review provenance override fixture",
        ),
        "workflow doctor for authoritative final-review provenance override fixture",
    );
    let gate_finish_json = parse_json(
        &run_workflow_gate_finish_output(
            repo,
            state,
            plan_rel,
            "workflow gate finish for authoritative final-review provenance override fixture",
        ),
        "workflow gate finish for authoritative final-review provenance override fixture",
    );

    assert_eq!(
        gate_finish_json["allowed"], true,
        "workflow gate finish should resolve final-review freshness from recorded authoritative provenance instead of scanning the newest branch artifact; got {gate_finish_json:?}"
    );
    assert_eq!(
        doctor_json["gate_finish"]["allowed"], true,
        "workflow doctor should report final-review freshness from recorded authoritative provenance instead of scanning the newest branch artifact; got {doctor_json:?}"
    );
}

#[test]
fn canonical_workflow_doctor_and_gate_finish_prefer_recorded_authoritative_release_docs_over_newer_branch_decoy()
 {
    let (repo_dir, state_dir) = init_repo("workflow-phase-authoritative-release-docs-provenance");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-authoritative-release-docs-provenance";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    complete_workflow_fixture_execution_with_qa_requirement(repo, state, plan_rel, "required");
    let branch = current_branch_name(repo);
    let expected_base_branch = expected_release_base_branch(repo);
    let test_plan_path = write_branch_test_plan_artifact(repo, state, plan_rel, "yes");
    let review_path =
        write_dispatched_branch_review_artifact(repo, state, plan_rel, &expected_base_branch);
    let qa_path = write_branch_qa_artifact(repo, state, plan_rel, &test_plan_path);
    let release_path = write_branch_release_artifact(repo, state, plan_rel, &expected_base_branch);
    enable_session_decision(state, session_key);
    let _ = review_path;
    let _ = release_path;
    let _ = publish_authoritative_qa_truth(
        repo,
        state,
        plan_rel,
        &test_plan_path,
        &qa_path,
        &expected_base_branch,
    );

    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            ("schema_version", Value::from(2)),
            ("harness_phase", Value::from("ready_for_branch_completion")),
            ("latest_authoritative_sequence", Value::from(17)),
        ],
    );

    write_file(
        &project_artifact_dir(repo, state).join(format!(
            "tester-{}-release-readiness-99999999-999999.md",
            branch_storage_key(&branch)
        )),
        &format!(
            "# Release Readiness Result\n**Source Plan:** `{plan_rel}`\n**Source Plan Revision:** 1\n**Branch:** {branch}\n**Repo:** {}\n**Base Branch:** {expected_base_branch}\n**Head SHA:** 0000000000000000000000000000000000000000\n**Result:** pass\n**Generated By:** featureforge:document-release\n**Generated At:** 2026-03-24T23:59:59Z\n\n## Summary\n- newer same-branch decoy should not override recorded authoritative downstream release-doc provenance.\n",
            repo_slug(repo)
        ),
    );

    let doctor_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "doctor", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow doctor for authoritative release-doc provenance override fixture",
        ),
        "workflow doctor for authoritative release-doc provenance override fixture",
    );
    let gate_finish_json = parse_json(
        &run_workflow_gate_finish_output(
            repo,
            state,
            plan_rel,
            "workflow gate finish for authoritative release-doc provenance override fixture",
        ),
        "workflow gate finish for authoritative release-doc provenance override fixture",
    );

    assert_eq!(
        gate_finish_json["allowed"], true,
        "workflow gate finish should resolve release-doc freshness from recorded authoritative downstream provenance instead of scanning the newest branch artifact; got {gate_finish_json:?}"
    );
    assert_eq!(
        doctor_json["gate_finish"]["allowed"], true,
        "workflow doctor should report release-doc freshness from recorded authoritative downstream provenance instead of scanning the newest branch artifact; got {doctor_json:?}"
    );
}

#[test]
fn canonical_workflow_operator_pins_authoritative_contract_drafting_phase_in_public_surfaces() {
    let (repo_dir, state_dir) = init_repo("workflow-phase-authoritative-contract-drafting");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-authoritative-contract-drafting";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    install_full_contract_ready_artifacts(repo);
    enable_session_decision(state, session_key);
    let branch = current_branch_name(repo);
    write_file(
        &harness_state_path(state, &repo_slug(repo), &branch),
        &serde_json::to_string(&json!({
            "schema_version": 1,
            "run_identity": {
                "execution_run_id": "run-contract-drafting-fixture",
                "source_plan_path": plan_rel,
                "source_plan_revision": 1
            },
            "active_worktree_lease_fingerprints": [],
            "active_worktree_lease_bindings": []
        }))
        .expect("contract_drafting fixture harness state should serialize"),
    );

    let expected_phase = public_harness_phase_from_spec("contract_drafting");
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            ("harness_phase", Value::from(expected_phase.clone())),
            ("latest_authoritative_sequence", Value::from(17)),
        ],
    );

    let phase_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "phase", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow phase should pin authoritative contract_drafting phase",
        ),
        "workflow phase should pin authoritative contract_drafting phase",
    );
    let handoff_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "handoff", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow handoff should pin authoritative contract_drafting phase",
        ),
        "workflow handoff should pin authoritative contract_drafting phase",
    );

    assert_eq!(phase_json["route_status"], "implementation_ready");
    assert_eq!(
        phase_json["phase"], "pivot_required",
        "phase JSON should preserve authoritative contract-drafting pivot route, got {phase_json}"
    );
    assert_eq!(handoff_json["route_status"], "implementation_ready");
    assert_eq!(handoff_json["phase"], "pivot_required");
}

#[test]
fn canonical_workflow_operator_surfaces_pivot_required_plan_revision_block_phase_and_next_action() {
    let (repo_dir, state_dir) = init_repo("workflow-phase-authoritative-pivot-required");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-authoritative-pivot-required";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    complete_workflow_fixture_execution(repo, state, plan_rel);
    enable_session_decision(state, session_key);

    let authoritative_state_path =
        harness_state_path(state, &repo_slug(repo), &current_branch_name(repo));
    write_file(
        &authoritative_state_path,
        r#"{"harness_phase":"pivot_required","latest_authoritative_sequence":23,"reason_codes":["blocked_on_plan_revision"]}"#,
    );

    let phase_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "phase", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow phase should surface authoritative pivot_required plan-revision blocks",
        ),
        "workflow phase should surface authoritative pivot_required plan-revision blocks",
    );
    let doctor_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "doctor", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow doctor should surface authoritative pivot_required plan-revision blocks",
        ),
        "workflow doctor should surface authoritative pivot_required plan-revision blocks",
    );
    let handoff_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "handoff", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow handoff should surface authoritative pivot_required plan-revision blocks",
        ),
        "workflow handoff should surface authoritative pivot_required plan-revision blocks",
    );

    let expected_phase = public_harness_phase_from_spec("pivot_required");

    assert_eq!(
        doctor_json["execution_status"]["harness_phase"],
        Value::String(expected_phase.clone())
    );
    assert_eq!(phase_json["phase"], expected_phase);
    assert_eq!(doctor_json["phase"], expected_phase);
    assert_eq!(handoff_json["phase"], expected_phase);
    assert_eq!(phase_json["next_action"], "pivot / return to planning");
    assert_eq!(doctor_json["next_action"], "pivot / return to planning");
    assert_eq!(handoff_json["next_action"], "pivot / return to planning");
    assert_ne!(
        handoff_json["recommended_skill"], doctor_json["execution_status"]["execution_mode"],
        "pivot-required plan-revision blocks should not keep recommending the active execution mode"
    );
    assert_eq!(
        handoff_json["recommendation_reason"],
        "Execution is blocked pending an approved plan revision."
    );
}

#[test]
fn canonical_workflow_routes_gate_review_evidence_failures_back_to_execution() {
    let (repo_dir, state_dir) = init_repo("workflow-phase-gate-review-evidence-failure");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-gate-review-evidence-failure";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    complete_workflow_fixture_execution(repo, state, plan_rel);
    enable_session_decision(state, session_key);
    let branch = current_branch_name(repo);
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            ("dependency_index_state", Value::from("fresh")),
            ("final_review_state", Value::from("fresh")),
            ("browser_qa_state", Value::from("fresh")),
            ("release_docs_state", Value::from("fresh")),
        ],
    );

    let execution_status = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "plan execution status for workflow gate-review evidence failure fixture",
    );
    let evidence_path = repo.join(
        execution_status["evidence_path"]
            .as_str()
            .expect("execution status should expose evidence_path"),
    );
    replace_in_file(
        &evidence_path,
        "**Plan Fingerprint:** ",
        "**Plan Fingerprint:** stale-",
    );

    let phase_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "phase", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow phase for gate-review evidence failure fixture",
        ),
        "workflow phase for gate-review evidence failure fixture",
    );
    let handoff_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "handoff", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow handoff for gate-review evidence failure fixture",
        ),
        "workflow handoff for gate-review evidence failure fixture",
    );
    let doctor_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "doctor", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow doctor for gate-review evidence failure fixture",
        ),
        "workflow doctor for gate-review evidence failure fixture",
    );
    if let Some(gate_review) = doctor_json["gate_review"].as_object() {
        assert_eq!(gate_review.get("allowed"), Some(&Value::Bool(false)));
        assert_eq!(
            gate_review.get("failure_class"),
            Some(&Value::from("StaleProvenance"))
        );
        assert!(
            gate_review
                .get("reason_codes")
                .and_then(Value::as_array)
                .is_some_and(|codes| {
                    codes
                        .iter()
                        .any(|code| code.as_str() == Some("plan_fingerprint_mismatch"))
                }),
            "workflow doctor should surface projection provenance diagnostics without making them route authority, got {doctor_json:?}"
        );
    }
    assert_eq!(phase_json["phase"], "document_release_pending");
    assert_eq!(phase_json["next_action"], "advance late stage");
    assert_eq!(handoff_json["phase"], "document_release_pending");
    assert_eq!(handoff_json["next_action"], "advance late stage");
    assert_eq!(
        handoff_json["recommended_skill"],
        "featureforge:document-release"
    );
}

#[test]
fn canonical_workflow_phase_routes_missing_test_plan_back_to_plan_eng_review() {
    let (repo_dir, state_dir) = init_repo("workflow-phase-test-plan-missing");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-test-plan-missing";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let base_branch = expected_release_base_branch(repo);

    complete_workflow_fixture_execution_with_qa_requirement(repo, state, plan_rel, "required");
    write_dispatched_branch_review_artifact(repo, state, plan_rel, &base_branch);
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    enable_session_decision(state, session_key);

    let handoff_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "handoff", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow handoff for missing-test-plan routing fixture",
        ),
        "workflow handoff for missing-test-plan routing fixture",
    );
    let gate_finish_json = parse_json(
        &run_workflow_gate_finish_output(
            repo,
            state,
            plan_rel,
            "workflow gate finish for missing-test-plan routing fixture",
        ),
        "workflow gate finish for missing-test-plan routing fixture",
    );
    assert_eq!(handoff_json["phase"], "qa_pending");
    assert_eq!(handoff_json["next_action"], "run QA");
    assert_eq!(handoff_json["recommended_skill"], "featureforge:qa-only");
    assert_eq!(gate_finish_json["allowed"], false);
    assert_eq!(gate_finish_json["failure_class"], "QaArtifactNotFresh");
    assert_eq!(gate_finish_json["reason_codes"][0], "qa_artifact_missing");
}

#[test]
fn canonical_workflow_phase_prioritizes_test_plan_prerequisite_over_failed_current_qa_result() {
    let (repo_dir, state_dir) = init_repo("workflow-phase-test-plan-prereq-over-failed-qa");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-test-plan-prereq-over-failed-qa";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let base_branch = expected_release_base_branch(repo);
    let branch = current_branch_name(repo);

    complete_workflow_fixture_execution_with_qa_requirement(repo, state, plan_rel, "required");
    write_dispatched_branch_review_artifact(repo, state, plan_rel, &base_branch);
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            ("harness_phase", Value::from("qa_pending")),
            (
                "current_branch_closure_id",
                Value::from("branch-release-closure"),
            ),
            ("current_release_readiness_result", Value::from("ready")),
            ("release_docs_state", Value::from("fresh")),
            ("final_review_state", Value::from("fresh")),
            ("browser_qa_state", Value::from("missing")),
            (
                "current_qa_branch_closure_id",
                Value::from("branch-release-closure"),
            ),
            ("current_qa_result", Value::from("fail")),
        ],
    );
    enable_session_decision(state, session_key);

    let handoff_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "handoff", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow handoff for test-plan-prereq-over-failed-qa fixture",
        ),
        "workflow handoff for test-plan-prereq-over-failed-qa fixture",
    );

    assert_eq!(handoff_json["phase"], "qa_pending");
    assert_eq!(handoff_json["next_action"], "run QA");
    assert_eq!(handoff_json["recommended_skill"], "featureforge:qa-only");
}

#[test]
fn canonical_workflow_phase_routes_malformed_test_plan_back_to_plan_eng_review() {
    let (repo_dir, state_dir) = init_repo("workflow-phase-test-plan-malformed");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-test-plan-malformed";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let base_branch = expected_release_base_branch(repo);

    complete_workflow_fixture_execution_with_qa_requirement(repo, state, plan_rel, "required");
    let test_plan_path = write_branch_test_plan_artifact(repo, state, plan_rel, "yes");
    write_dispatched_branch_review_artifact(repo, state, plan_rel, &base_branch);
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    enable_session_decision(state, session_key);
    replace_in_file(&test_plan_path, "# Test Plan", "# Not A Test Plan");

    let handoff_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "handoff", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow handoff for malformed-test-plan routing fixture",
        ),
        "workflow handoff for malformed-test-plan routing fixture",
    );
    let gate_finish_json = parse_json(
        &run_workflow_gate_finish_output(
            repo,
            state,
            plan_rel,
            "workflow gate finish for malformed-test-plan routing fixture",
        ),
        "workflow gate finish for malformed-test-plan routing fixture",
    );

    assert_eq!(handoff_json["phase"], "qa_pending");
    assert_eq!(handoff_json["next_action"], "run QA");
    assert_eq!(handoff_json["recommended_skill"], "featureforge:qa-only");
    assert_eq!(gate_finish_json["allowed"], false);
    assert_eq!(gate_finish_json["failure_class"], "QaArtifactNotFresh");
    assert_eq!(gate_finish_json["reason_codes"][0], "qa_artifact_missing");
}

#[test]
fn canonical_workflow_phase_ignores_test_plan_generator_drift_for_routing() {
    let (repo_dir, state_dir) = init_repo("workflow-phase-test-plan-generator-mismatch");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-test-plan-generator-mismatch";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let base_branch = expected_release_base_branch(repo);

    complete_workflow_fixture_execution_with_qa_requirement(repo, state, plan_rel, "required");
    let test_plan_path = write_branch_test_plan_artifact(repo, state, plan_rel, "yes");
    write_dispatched_branch_review_artifact(repo, state, plan_rel, &base_branch);
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    enable_session_decision(state, session_key);
    replace_in_file(
        &test_plan_path,
        "**Generated By:** featureforge:plan-eng-review",
        "**Generated By:** manual-test-plan-edit",
    );

    let handoff_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "handoff", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow handoff for test-plan generator mismatch routing fixture",
        ),
        "workflow handoff for test-plan generator mismatch routing fixture",
    );
    let gate_finish_json = parse_json(
        &run_workflow_gate_finish_output(
            repo,
            state,
            plan_rel,
            "workflow gate finish for test-plan generator mismatch routing fixture",
        ),
        "workflow gate finish for test-plan generator mismatch routing fixture",
    );

    assert_eq!(handoff_json["phase"], "qa_pending");
    assert_eq!(handoff_json["next_action"], "run QA");
    assert_eq!(handoff_json["recommended_skill"], "featureforge:qa-only");
    assert_eq!(gate_finish_json["allowed"], false);
    assert_eq!(gate_finish_json["failure_class"], "QaArtifactNotFresh");
    assert_eq!(gate_finish_json["reason_codes"][0], "qa_artifact_missing");
}

#[test]
fn canonical_workflow_phase_routes_stale_test_plan_back_to_plan_eng_review() {
    let (repo_dir, state_dir) = init_repo("workflow-phase-test-plan-stale");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-test-plan-stale";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let base_branch = expected_release_base_branch(repo);

    complete_workflow_fixture_execution_with_qa_requirement(repo, state, plan_rel, "required");
    let test_plan_path = write_branch_test_plan_artifact(repo, state, plan_rel, "yes");
    write_dispatched_branch_review_artifact(repo, state, plan_rel, &base_branch);
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    enable_session_decision(state, session_key);
    replace_in_file(
        &test_plan_path,
        &format!("**Head SHA:** {}", current_head_sha(repo)),
        "**Head SHA:** 0000000000000000000000000000000000000000",
    );

    let handoff_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "handoff", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow handoff for stale-test-plan routing fixture",
        ),
        "workflow handoff for stale-test-plan routing fixture",
    );
    let gate_finish_json = parse_json(
        &run_workflow_gate_finish_output(
            repo,
            state,
            plan_rel,
            "workflow gate finish for stale-test-plan routing fixture",
        ),
        "workflow gate finish for stale-test-plan routing fixture",
    );

    assert_eq!(handoff_json["phase"], "qa_pending");
    assert_eq!(handoff_json["next_action"], "run QA");
    assert_eq!(handoff_json["recommended_skill"], "featureforge:qa-only");
    assert_eq!(gate_finish_json["allowed"], false);
    assert_eq!(gate_finish_json["failure_class"], "QaArtifactNotFresh");
    assert_eq!(gate_finish_json["reason_codes"][0], "qa_artifact_missing");
}

#[test]
fn canonical_workflow_phase_routes_authoritative_qa_provenance_invalid_to_qa_pending() {
    let (repo_dir, state_dir) = init_repo("workflow-phase-qa-authoritative-provenance-invalid");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-qa-authoritative-provenance-invalid";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let base_branch = expected_release_base_branch(repo);

    complete_workflow_fixture_execution_with_qa_requirement(repo, state, plan_rel, "required");
    let test_plan_path = write_branch_test_plan_artifact(repo, state, plan_rel, "yes");
    write_dispatched_branch_review_artifact(repo, state, plan_rel, &base_branch);
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    let qa_path = write_branch_qa_artifact(repo, state, plan_rel, &test_plan_path);
    let _ = publish_authoritative_qa_truth(
        repo,
        state,
        plan_rel,
        &test_plan_path,
        &qa_path,
        &base_branch,
    );
    enable_session_decision(state, session_key);

    let branch = current_branch_name(repo);
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            ("schema_version", Value::from(2)),
            ("harness_phase", Value::from("qa_pending")),
            ("browser_qa_state", Value::from("fresh")),
        ],
    );
    update_current_history_record_field(
        repo,
        state,
        "browser_qa_record_history",
        "current_qa_record_id",
        "browser_qa_fingerprint",
        Value::from("not-a-fingerprint"),
    );

    let handoff_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "handoff", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow handoff for authoritative-qa-provenance-invalid routing fixture",
        ),
        "workflow handoff for authoritative-qa-provenance-invalid routing fixture",
    );
    let gate_finish_json = parse_json(
        &run_workflow_gate_finish_output(
            repo,
            state,
            plan_rel,
            "workflow gate finish for authoritative-qa-provenance-invalid routing fixture",
        ),
        "workflow gate finish for authoritative-qa-provenance-invalid routing fixture",
    );

    assert_eq!(gate_finish_json["allowed"], true);
    assert_eq!(
        handoff_json["phase"], "ready_for_branch_completion",
        "{gate_finish_json:?}"
    );
    assert_eq!(
        handoff_json["next_action"], "finish branch",
        "{handoff_json:?}"
    );
    assert_eq!(
        handoff_json["recommended_skill"],
        "featureforge:finishing-a-development-branch"
    );
}

#[test]
fn canonical_workflow_phase_routes_authoritative_test_plan_provenance_invalid_to_plan_eng_review() {
    let (repo_dir, state_dir) =
        init_repo("workflow-phase-test-plan-authoritative-provenance-invalid");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-test-plan-authoritative-provenance-invalid";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let base_branch = expected_release_base_branch(repo);

    complete_workflow_fixture_execution_with_qa_requirement(repo, state, plan_rel, "required");
    let test_plan_path = write_branch_test_plan_artifact(repo, state, plan_rel, "yes");
    write_dispatched_branch_review_artifact(repo, state, plan_rel, &base_branch);
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    let qa_path = write_branch_qa_artifact(repo, state, plan_rel, &test_plan_path);
    enable_session_decision(state, session_key);

    let branch = current_branch_name(repo);
    let (authoritative_test_plan_fingerprint, authoritative_qa_fingerprint) =
        publish_authoritative_qa_truth(
            repo,
            state,
            plan_rel,
            &test_plan_path,
            &qa_path,
            &base_branch,
        );
    fs::remove_file(harness_authoritative_artifact_path(
        state,
        &repo_slug(repo),
        &branch,
        &format!("test-plan-{authoritative_test_plan_fingerprint}.md"),
    ))
    .expect(
        "authoritative test-plan provenance fixture should remove the current test-plan artifact",
    );
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            ("schema_version", Value::from(2)),
            ("harness_phase", Value::from("qa_pending")),
            ("browser_qa_state", Value::from("fresh")),
        ],
    );
    update_current_history_record_field(
        repo,
        state,
        "browser_qa_record_history",
        "current_qa_record_id",
        "browser_qa_fingerprint",
        Value::from(authoritative_qa_fingerprint),
    );

    let handoff_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "handoff", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow handoff for authoritative-test-plan-provenance-invalid routing fixture",
        ),
        "workflow handoff for authoritative-test-plan-provenance-invalid routing fixture",
    );
    let gate_finish_json = parse_json(
        &run_workflow_gate_finish_output(
            repo,
            state,
            plan_rel,
            "workflow gate finish for authoritative-test-plan-provenance-invalid routing fixture",
        ),
        "workflow gate finish for authoritative-test-plan-provenance-invalid routing fixture",
    );

    assert_eq!(gate_finish_json["allowed"], true);
    assert_eq!(
        handoff_json["phase"], "ready_for_branch_completion",
        "{gate_finish_json:?}"
    );
    assert_eq!(
        handoff_json["next_action"], "finish branch",
        "{handoff_json:?}"
    );
    assert_eq!(
        handoff_json["recommended_skill"],
        "featureforge:finishing-a-development-branch"
    );
}

fn assert_authoritative_qa_source_test_plan_header_failure(
    fixture_name: &str,
    qa_source_transform: fn(&str) -> String,
) {
    let (repo_dir, state_dir) = init_repo(fixture_name);
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = fixture_name;
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let base_branch = expected_release_base_branch(repo);

    complete_workflow_fixture_execution_with_qa_requirement(repo, state, plan_rel, "required");
    let test_plan_path = write_branch_test_plan_artifact(repo, state, plan_rel, "yes");
    write_dispatched_branch_review_artifact(repo, state, plan_rel, &base_branch);
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    let qa_path = write_branch_qa_artifact(repo, state, plan_rel, &test_plan_path);
    enable_session_decision(state, session_key);

    let branch = current_branch_name(repo);
    let _ = publish_authoritative_qa_truth(
        repo,
        state,
        plan_rel,
        &test_plan_path,
        &qa_path,
        &base_branch,
    );
    let authoritative_qa_source = qa_source_transform(
        &fs::read_to_string(&qa_path)
            .expect("source QA artifact should be readable for authoritative provenance fixture"),
    );
    let authoritative_qa_fingerprint = sha256_hex(authoritative_qa_source.as_bytes());
    write_file(
        &harness_authoritative_artifact_path(
            state,
            &repo_slug(repo),
            &branch,
            &format!("browser-qa-{authoritative_qa_fingerprint}.md"),
        ),
        &authoritative_qa_source,
    );
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            ("schema_version", Value::from(2)),
            ("harness_phase", Value::from("qa_pending")),
            ("browser_qa_state", Value::from("fresh")),
            (
                "last_browser_qa_artifact_fingerprint",
                Value::from(authoritative_qa_fingerprint.clone()),
            ),
        ],
    );
    update_current_history_record_field(
        repo,
        state,
        "browser_qa_record_history",
        "current_qa_record_id",
        "browser_qa_fingerprint",
        Value::from(authoritative_qa_fingerprint),
    );

    let handoff_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "handoff", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow handoff for malformed authoritative QA->test-plan provenance fixture",
        ),
        "workflow handoff for malformed authoritative QA->test-plan provenance fixture",
    );
    let gate_finish_json = parse_json(
        &run_workflow_gate_finish_output(
            repo,
            state,
            plan_rel,
            "workflow gate finish for malformed authoritative QA->test-plan provenance fixture",
        ),
        "workflow gate finish for malformed authoritative QA->test-plan provenance fixture",
    );

    assert_eq!(gate_finish_json["allowed"], true);
    assert_eq!(
        handoff_json["phase"], "ready_for_branch_completion",
        "{gate_finish_json:?}"
    );
    assert_eq!(
        handoff_json["next_action"], "finish branch",
        "{handoff_json:?}"
    );
    assert_eq!(
        handoff_json["recommended_skill"],
        "featureforge:finishing-a-development-branch"
    );
}

#[test]
fn canonical_workflow_phase_routes_missing_authoritative_qa_source_test_plan_header_to_plan_eng_review()
 {
    assert_authoritative_qa_source_test_plan_header_failure(
        "workflow-phase-test-plan-header-missing",
        remove_source_test_plan_header,
    );
}

#[test]
fn canonical_workflow_phase_routes_blank_authoritative_qa_source_test_plan_header_to_plan_eng_review()
 {
    assert_authoritative_qa_source_test_plan_header_failure(
        "workflow-phase-test-plan-header-blank",
        blank_source_test_plan_header,
    );
}

#[test]
fn canonical_workflow_phase_routes_authoritative_release_provenance_invalid_to_document_release() {
    let (repo_dir, state_dir) =
        init_repo("workflow-phase-release-authoritative-provenance-invalid");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-release-authoritative-provenance-invalid";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let base_branch = expected_release_base_branch(repo);

    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_branch_review_artifact(repo, state, plan_rel, &base_branch);
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    enable_session_decision(state, session_key);

    let branch = current_branch_name(repo);
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            ("schema_version", Value::from(2)),
            ("dependency_index_state", Value::from("fresh")),
            ("final_review_state", Value::from("not_required")),
            ("browser_qa_state", Value::from("not_required")),
            ("release_docs_state", Value::from("fresh")),
        ],
    );
    update_current_history_record_field(
        repo,
        state,
        "release_readiness_record_history",
        "current_release_readiness_record_id",
        "release_docs_fingerprint",
        Value::from("not-a-fingerprint"),
    );

    let handoff_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "handoff", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow handoff for authoritative-release-provenance-invalid routing fixture",
        ),
        "workflow handoff for authoritative-release-provenance-invalid routing fixture",
    );
    let gate_finish_json = parse_json(
        &run_workflow_gate_finish_output(
            repo,
            state,
            plan_rel,
            "workflow gate finish for authoritative-release-provenance-invalid routing fixture",
        ),
        "workflow gate finish for authoritative-release-provenance-invalid routing fixture",
    );

    assert_eq!(gate_finish_json["allowed"], false);
    assert!(
        gate_finish_json["reason_codes"]
            .as_array()
            .is_some_and(|codes| { codes.iter().any(|code| code == "review_artifact_missing") }),
        "gate-finish should fail at final-review freshness when release receipt markdown is no longer authoritative, got {gate_finish_json:?}"
    );
    assert_eq!(
        handoff_json["phase"], "final_review_pending",
        "{gate_finish_json:?}"
    );
    assert_eq!(
        handoff_json["next_action"], "request final review",
        "{handoff_json:?}"
    );
    assert_eq!(
        handoff_json["recommended_skill"],
        "featureforge:requesting-code-review"
    );
    assert_eq!(handoff_json["reason_family"], "final_review_freshness");
    assert!(
        handoff_json["diagnostic_reason_codes"]
            .as_array()
            .is_some_and(|codes| { codes.iter().any(|code| code == "review_artifact_missing") }),
        "handoff observability should expose authoritative final-review freshness diagnostics, got {handoff_json:?}"
    );
}

#[test]
fn canonical_workflow_phase_routes_release_and_review_unresolved_to_document_release_pending() {
    let (repo_dir, state_dir) = init_repo("workflow-phase-release-and-review-unresolved");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-release-and-review-unresolved";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    enable_session_decision(state, session_key);

    let handoff_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "handoff", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow handoff for release+review-unresolved routing fixture",
        ),
        "workflow handoff for release+review-unresolved routing fixture",
    );
    let gate_finish_json = parse_json(
        &run_workflow_gate_finish_output(
            repo,
            state,
            plan_rel,
            "workflow gate finish for release+review-unresolved routing fixture",
        ),
        "workflow gate finish for release+review-unresolved routing fixture",
    );

    assert_eq!(
        handoff_json["phase"], "document_release_pending",
        "{gate_finish_json:?}"
    );
    assert_eq!(handoff_json["next_action"], "advance late stage");
    assert_eq!(
        handoff_json["recommended_skill"],
        "featureforge:document-release"
    );
    assert_eq!(gate_finish_json["allowed"], false);
    assert!(
        gate_finish_json["reason_codes"]
            .as_array()
            .is_some_and(|codes| codes
                .iter()
                .any(|code| code == "release_docs_state_missing")),
        "{gate_finish_json:?}"
    );
}

#[test]
fn canonical_workflow_phase_routes_mixed_stale_matrix() {
    let (repo_dir, state_dir) = init_repo("workflow-phase-matrix-shared");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let base_branch = expected_release_base_branch(repo);
    let cases = [
        (
            "all_missing",
            false,
            false,
            true,
            "document_release_pending",
            "advance late stage",
            "featureforge:document-release",
            "release_readiness",
        ),
        (
            "release_review_missing_qa_fresh",
            false,
            false,
            false,
            "document_release_pending",
            "advance late stage",
            "featureforge:document-release",
            "release_readiness",
        ),
        (
            "release_missing_review_fresh_qa_missing",
            false,
            true,
            true,
            "qa_pending",
            "run QA",
            "featureforge:qa-only",
            "qa_freshness",
        ),
        (
            "release_missing_review_qa_fresh",
            false,
            true,
            false,
            "ready_for_branch_completion",
            "finish branch",
            "featureforge:finishing-a-development-branch",
            "all_fresh",
        ),
        (
            "release_fresh_review_qa_missing",
            true,
            false,
            true,
            "final_review_pending",
            "request final review",
            "featureforge:requesting-code-review",
            "final_review_freshness",
        ),
        (
            "release_qa_fresh_review_missing",
            true,
            false,
            false,
            "final_review_pending",
            "request final review",
            "featureforge:requesting-code-review",
            "final_review_freshness",
        ),
        (
            "release_review_fresh_qa_missing",
            true,
            true,
            true,
            "qa_pending",
            "run QA",
            "featureforge:qa-only",
            "qa_freshness",
        ),
        (
            "all_fresh",
            true,
            true,
            false,
            "ready_for_branch_completion",
            "finish branch",
            "featureforge:finishing-a-development-branch",
            "all_fresh",
        ),
    ];

    for (
        case_id,
        release_fresh,
        review_fresh,
        qa_blocked,
        expected_phase,
        expected_next_action,
        expected_skill,
        expected_reason_family,
    ) in cases
    {
        let fixture_name = format!("workflow-phase-matrix-{case_id}");
        let session_key = fixture_name.as_str();
        let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

        complete_workflow_fixture_execution_with_qa_requirement(
            repo,
            state,
            plan_rel,
            if qa_blocked {
                "required"
            } else {
                "not-required"
            },
        );
        write_branch_test_plan_artifact(
            repo,
            state,
            plan_rel,
            if qa_blocked { "yes" } else { "no" },
        );
        if review_fresh {
            write_dispatched_branch_review_artifact(repo, state, plan_rel, &base_branch);
        }
        if release_fresh {
            write_branch_release_artifact(repo, state, plan_rel, &base_branch);
        }
        let branch = current_branch_name(repo);
        update_authoritative_harness_state(
            repo,
            state,
            &branch,
            plan_rel,
            1,
            &[
                ("dependency_index_state", Value::from("fresh")),
                (
                    "final_review_state",
                    Value::from(if review_fresh { "fresh" } else { "missing" }),
                ),
                (
                    "browser_qa_state",
                    Value::from(if qa_blocked {
                        "missing"
                    } else {
                        "not_required"
                    }),
                ),
                (
                    "release_docs_state",
                    Value::from(if release_fresh { "fresh" } else { "missing" }),
                ),
            ],
        );
        enable_session_decision(state, session_key);
        let runtime =
            discover_execution_runtime(repo, state, "workflow_runtime mixed stale matrix fixture");
        let handoff_json =
            workflow_handoff_json(&runtime, "workflow_runtime mixed stale matrix fixture");
        let status_json = plan_execution_status_json(
            &runtime,
            plan_rel,
            false,
            "workflow_runtime mixed stale matrix fixture",
        );

        assert_eq!(
            handoff_json["phase"], expected_phase,
            "matrix case {case_id} expected phase {expected_phase}, got handoff payload: {handoff_json:?}"
        );
        assert_eq!(
            handoff_json["next_action"], expected_next_action,
            "matrix case {case_id} expected next_action {expected_next_action}, got handoff payload: {handoff_json:?}"
        );
        assert_eq!(
            handoff_json["recommended_skill"], expected_skill,
            "matrix case {case_id} expected recommended_skill {expected_skill}, got handoff payload: {handoff_json:?}"
        );
        assert_eq!(
            handoff_json["reason_family"], expected_reason_family,
            "matrix case {case_id} expected handoff reason_family {expected_reason_family}, got handoff payload: {handoff_json:?}"
        );
        assert_eq!(
            status_json["harness_phase"], expected_phase,
            "matrix case {case_id} expected harness_phase {expected_phase}, got status payload: {status_json:?}; handoff payload: {handoff_json:?}"
        );
    }
}

#[test]
fn canonical_workflow_harness_operator_precedence_parity_dual_unresolved() {
    let (repo_dir, state_dir) = init_repo("workflow-harness-operator-parity-dual-unresolved");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-harness-operator-parity-dual-unresolved";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let branch = current_branch_name(repo);

    complete_workflow_fixture_execution_with_qa_requirement(repo, state, plan_rel, "required");
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            ("harness_phase", Value::from("final_review_pending")),
            ("latest_authoritative_sequence", Value::from(17)),
            ("dependency_index_state", Value::from("fresh")),
            ("final_review_state", Value::from("missing")),
            ("browser_qa_state", Value::from("missing")),
            ("release_docs_state", Value::from("missing")),
        ],
    );
    enable_session_decision(state, session_key);

    let runtime = discover_execution_runtime(
        repo,
        state,
        "workflow_runtime harness/operator parity dual-unresolved fixture",
    );
    let phase_json = workflow_phase_json(
        &runtime,
        "workflow_runtime harness/operator parity dual-unresolved fixture",
    );
    let status_json = plan_execution_status_json(
        &runtime,
        plan_rel,
        false,
        "workflow_runtime harness/operator parity dual-unresolved fixture",
    );

    assert_eq!(phase_json["phase"], "document_release_pending");
    assert_eq!(
        status_json["harness_phase"], "document_release_pending",
        "authoritative harness phase should match operator precedence output for dual-unresolved late-stage state; phase payload: {phase_json:?}; status payload: {status_json:?}"
    );
}

#[test]
fn canonical_workflow_harness_operator_parity_unclassified_finish_failure_fails_closed() {
    let (repo_dir, state_dir) = init_repo("workflow-harness-operator-parity-unclassified-finish");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-harness-operator-parity-unclassified-finish";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_dispatched_branch_review_artifact(repo, state, plan_rel, &base_branch);
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    write_file(
        &harness_authoritative_artifact_path(
            state,
            &repo_slug(repo),
            &current_branch_name(repo),
            "execution-topology-downgrade-malformed.json",
        ),
        "{ malformed topology downgrade record",
    );
    enable_session_decision(state, session_key);

    let runtime = discover_execution_runtime(
        repo,
        state,
        "workflow_runtime unclassified-finish parity fixture",
    );
    let phase_json = workflow_phase_json(
        &runtime,
        "workflow_runtime unclassified-finish parity fixture",
    );
    let status_json = plan_execution_status_json(
        &runtime,
        plan_rel,
        false,
        "workflow_runtime unclassified-finish parity fixture",
    );
    let gate_finish_json = parse_json(
        &run_workflow_gate_finish_output(
            repo,
            state,
            plan_rel,
            "workflow gate finish for unclassified-finish parity fixture",
        ),
        "workflow gate finish for unclassified-finish parity fixture",
    );

    assert_eq!(gate_finish_json["allowed"], true, "{gate_finish_json:?}");
    assert_eq!(
        status_json["harness_phase"], "ready_for_branch_completion",
        "status should preserve authoritative late-stage readiness when receipt markdown is no longer gate authority; status payload: {status_json:?}; gate_finish payload: {gate_finish_json:?}"
    );
    assert_eq!(
        phase_json["phase"], "ready_for_branch_completion",
        "operator phase should preserve parity with status for unclassified legacy receipt failures; phase payload: {phase_json:?}; status payload: {status_json:?}; gate_finish payload: {gate_finish_json:?}"
    );
}

#[test]
fn canonical_workflow_phase_routes_review_resolved_browser_qa_to_qa_only() {
    let (repo_dir, state_dir) = init_repo("workflow-phase-qa-pending");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-qa-pending";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let base_branch = expected_release_base_branch(repo);

    complete_workflow_fixture_execution_with_qa_requirement(repo, state, plan_rel, "required");
    write_branch_test_plan_artifact(repo, state, plan_rel, "yes");
    write_dispatched_branch_review_artifact(repo, state, plan_rel, &base_branch);
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    enable_session_decision(state, session_key);

    let handoff_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "handoff", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow handoff for qa-pending routing fixture",
        ),
        "workflow handoff for qa-pending routing fixture",
    );

    assert_eq!(handoff_json["phase"], "qa_pending");
    assert_eq!(handoff_json["next_action"], "run QA");
    assert_eq!(handoff_json["recommended_skill"], "featureforge:qa-only");
    assert_eq!(
        handoff_json["recommendation_reason"],
        "Finish readiness requires a current QA milestone for the current branch closure."
    );
}

#[test]
fn canonical_workflow_phase_routes_review_resolved_to_document_release_pending() {
    let (repo_dir, state_dir) = init_repo("workflow-phase-release-pending");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-release-pending";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let base_branch = expected_release_base_branch(repo);

    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    let review_path = write_branch_review_artifact(repo, state, plan_rel, &base_branch);
    publish_authoritative_final_review_truth(repo, state, plan_rel, &review_path);
    enable_session_decision(state, session_key);

    let handoff_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "handoff", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow handoff for release-pending routing fixture",
        ),
        "workflow handoff for release-pending routing fixture",
    );

    assert_eq!(handoff_json["phase"], "document_release_pending");
    assert_eq!(handoff_json["next_action"], "advance late stage");
    assert_eq!(
        handoff_json["recommended_skill"],
        "featureforge:document-release"
    );
    assert_eq!(
        handoff_json["recommendation_reason"],
        "Finish readiness requires a current release-readiness milestone for the current branch closure."
    );
}

#[test]
fn canonical_workflow_phase_routes_fully_ready_branch_to_finish() {
    let (repo_dir, state_dir) = init_repo("workflow-phase-ready-for-finish");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let session_key = "workflow-phase-ready-for-finish";
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let base_branch = expected_release_base_branch(repo);

    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_dispatched_branch_review_artifact(repo, state, plan_rel, &base_branch);
    write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    enable_session_decision(state, session_key);

    let doctor_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "doctor", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow doctor for ready-for-finish routing fixture",
        ),
        "workflow doctor for ready-for-finish routing fixture",
    );
    let handoff_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "handoff", "--json"],
            &[("FEATUREFORGE_SESSION_KEY", session_key)],
            "workflow handoff for ready-for-finish routing fixture",
        ),
        "workflow handoff for ready-for-finish routing fixture",
    );

    assert_eq!(doctor_json["gate_finish"]["allowed"], true);
    assert_eq!(handoff_json["phase"], "ready_for_branch_completion");
    assert_eq!(handoff_json["next_action"], "finish branch");
    assert_eq!(
        handoff_json["recommended_skill"],
        "featureforge:finishing-a-development-branch"
    );
    assert_eq!(
        handoff_json["recommendation_reason"],
        "All required late-stage artifacts are fresh for the current HEAD."
    );
}

#[test]
fn compiled_cli_route_parity_probe_for_completed_late_stage_fixture() {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs07-parity");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    complete_workflow_fixture_execution(repo, state, plan_rel);

    let mut runtime_management_commands = 0usize;
    runtime_management_commands += 1;
    let operator_json = parse_json(
        &run_rust_featureforge_with_env_real_cli(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "FS-07 workflow operator route parity fixture",
        ),
        "FS-07 workflow operator route parity fixture",
    );
    runtime_management_commands += 1;
    let status_json = parse_json(
        &run_rust_featureforge_with_env_real_cli(
            repo,
            state,
            &["plan", "execution", "status", "--plan", plan_rel],
            &[],
            "FS-07 plan execution status route parity fixture",
        ),
        "FS-07 plan execution status route parity fixture",
    );
    assert_public_route_parity(&operator_json, &status_json, None);
    assert_parity_probe_budget("PARITY-PROBE-CLEAN", runtime_management_commands, 2);
}

#[test]
fn runtime_remediation_fs01_shared_route_parity_for_missing_current_closure() {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs01-workflow-runtime");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let base_branch = expected_release_base_branch(repo);
    let branch = current_branch_name(repo);

    complete_workflow_fixture_execution(repo, state, plan_rel);
    let test_plan = write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_dispatched_branch_review_artifact(repo, state, plan_rel, &base_branch);
    let release_path = write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    publish_authoritative_release_truth(repo, state, plan_rel, &release_path, &base_branch);
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            ("current_branch_closure_id", Value::Null),
            ("current_branch_closure_reviewed_state_id", Value::Null),
            ("current_branch_closure_contract_identity", Value::Null),
            (
                "review_state_repair_follow_up",
                Value::from("record_branch_closure"),
            ),
        ],
    );

    let operator_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "FS-01 workflow operator shared-runtime parity fixture",
        ),
        "FS-01 workflow operator shared-runtime parity fixture",
    );
    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-01 plan execution status shared-runtime parity fixture",
    );
    let doctor_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "doctor", "--json"],
            &[],
            "FS-01 workflow doctor shared-runtime parity fixture",
        ),
        "FS-01 workflow doctor shared-runtime parity fixture",
    );

    assert_public_route_parity(&operator_json, &status_json, Some(&doctor_json));

    let repair_json = run_plan_execution_json(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "FS-01 repair-review-state shared-runtime parity fixture",
    );
    let operator_after_repair = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "FS-01 workflow operator after repair fixture",
        ),
        "FS-01 workflow operator after repair fixture",
    );
    if repair_json["action"] == "already_current" {
        assert_ne!(
            operator_after_repair["review_state_status"],
            Value::from("missing_current_closure"),
            "FS-01 shared truth must not leave a missing-current-closure blocker active when repair says already_current"
        );
    }
    assert!(
        test_plan.exists(),
        "FS-01 fixture sanity check should preserve staged projection setup"
    );
}

#[test]
fn runtime_remediation_fs04_repair_returns_route_consumed_by_operator() {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs04-workflow-runtime");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-04-03-runtime-fs04-repair-route-runtime.md";
    setup_runtime_fs14_fs16_task_boundary_fixture(repo, state, plan_rel);

    let operator_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &[
                "workflow",
                "operator",
                "--plan",
                plan_rel,
                "--external-review-result-ready",
                "--json",
            ],
            &[],
            "FS-04 workflow operator shared-route parity fixture",
        ),
        "FS-04 workflow operator shared-route parity fixture",
    );
    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-04 plan execution status shared-route parity fixture",
    );
    assert_public_route_parity(&operator_json, &status_json, None);

    let repair_json = run_plan_execution_json(
        repo,
        state,
        &[
            "repair-review-state",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
        ],
        "FS-04 repair-review-state shared-runtime fixture",
    );
    assert_eq!(
        repair_json["action"],
        Value::from("blocked"),
        "FS-04 repair should surface the shared closure-recording blocker instead of claiming repair is already current"
    );
    assert_eq!(
        repair_json["phase_detail"],
        Value::from("task_closure_recording_ready"),
        "FS-04 repair should expose task_closure_recording_ready shared-routing detail, got {repair_json:?}"
    );
    assert!(
        repair_json["required_follow_up"].is_null(),
        "FS-04 closure-baseline recovery should not require a stale follow-up category, got {repair_json:?}"
    );
    assert_eq!(
        repair_json["recommended_command"], operator_json["recommended_command"],
        "FS-04 repair and operator must expose the same concrete command target"
    );
    let recommended_command = repair_json["recommended_command"]
        .as_str()
        .expect("FS-04 repair output should expose recommended command");
    assert!(
        recommended_command.starts_with("featureforge plan execution close-current-task --plan "),
        "FS-04 closure-baseline route should stay on close-current-task guidance, got {recommended_command}"
    );
    let routed_follow_up = run_recommended_plan_execution_command(
        repo,
        state,
        recommended_command,
        "FS-04 run operator-recommended command directly",
    );
    assert_follow_up_blocker_parity_with_operator(
        &operator_json,
        &routed_follow_up,
        "FS-04 command-follow parity",
    );
    assert!(
        matches!(
            routed_follow_up["action"].as_str(),
            Some("recorded" | "already_current")
        ),
        "FS-04 routed command must be immediately runnable when repair reports already_current, got {routed_follow_up:?}"
    );
}

#[test]
fn runtime_remediation_fs08_resume_overlay_does_not_hide_stale_blocker() {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs08-workflow-runtime");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-04-03-runtime-fs08-stale-blocker.md";
    setup_runtime_fs14_fs16_task_boundary_fixture(repo, state, plan_rel);
    let branch = current_branch_name(repo);
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            ("current_branch_closure_id", Value::Null),
            ("current_branch_closure_reviewed_state_id", Value::Null),
            ("current_branch_closure_contract_identity", Value::Null),
            ("resume_task", Value::from(1_u64)),
            ("resume_step", Value::from(1_u64)),
            (
                "review_state_repair_follow_up",
                Value::from("execution_reentry"),
            ),
        ],
    );

    let operator_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "FS-08 workflow operator stale-blocker visibility fixture",
        ),
        "FS-08 workflow operator stale-blocker visibility fixture",
    );
    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-08 plan execution status stale-blocker visibility fixture",
    );
    assert_public_route_parity(&operator_json, &status_json, None);
    assert_eq!(
        operator_json["phase_detail"],
        Value::from("task_closure_recording_ready"),
        "FS-08 stale blocker should remain visible as task_closure_recording_ready"
    );
    assert_eq!(
        status_json["phase_detail"],
        Value::from("task_closure_recording_ready")
    );
    assert_eq!(operator_json["blocking_scope"], Value::from("task"));
    assert_eq!(status_json["blocking_scope"], Value::from("task"));
    assert_eq!(operator_json["blocking_task"], Value::from(1_u64));
    assert_eq!(status_json["blocking_task"], Value::from(1_u64));
    let mut operator_reason_codes = operator_json["blocking_reason_codes"]
        .as_array()
        .expect("FS-08 operator should expose blocking_reason_codes as an array")
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mut status_reason_codes = status_json["blocking_reason_codes"]
        .as_array()
        .expect("FS-08 status should expose blocking_reason_codes as an array")
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    operator_reason_codes.sort();
    status_reason_codes.sort();
    let expected_reason_codes = vec![
        String::from("prior_task_current_closure_missing"),
        String::from("task_closure_baseline_repair_candidate"),
    ];
    assert_eq!(
        operator_reason_codes, expected_reason_codes,
        "FS-08 operator should expose the exact stale-blocker reason-code set for this fixture"
    );
    assert_eq!(
        status_reason_codes, expected_reason_codes,
        "FS-08 status should expose the exact stale-blocker reason-code set for this fixture"
    );
    let recommended_command = operator_json["recommended_command"]
        .as_str()
        .expect("FS-08 operator should expose recommended command");
    assert!(
        recommended_command.starts_with("featureforge plan execution close-current-task --plan "),
        "FS-08 operator should expose a runnable plan-execution command, got {recommended_command}"
    );
    assert!(
        recommended_command.contains("--task 1"),
        "FS-08 operator should route stale-blocker recovery to Task 1 close-current-task, got {recommended_command}"
    );
    let routed_follow_up = run_recommended_plan_execution_command(
        repo,
        state,
        recommended_command,
        "FS-08 run operator-routed command directly",
    );
    assert_follow_up_blocker_parity_with_operator(
        &operator_json,
        &routed_follow_up,
        "FS-08 command-follow parity",
    );
    if routed_follow_up["action"].as_str() != Some("blocked") {
        assert!(
            matches!(
                routed_follow_up["action"].as_str(),
                Some("recorded" | "already_current")
            ),
            "FS-08 routed command should either record closure or remain already_current, got {routed_follow_up:?}"
        );
    }
}

#[test]
fn runtime_remediation_fs04_compiled_cli_repair_returns_route_consumed_by_operator() {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs04-workflow-runtime-real-cli");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel =
        "docs/featureforge/plans/2026-04-03-runtime-fs04-repair-route-runtime-real-cli.md";
    setup_runtime_fs14_fs16_task_boundary_fixture(repo, state, plan_rel);

    let operator_json = parse_json(
        &run_rust_featureforge_with_env_real_cli(
            repo,
            state,
            &[
                "workflow",
                "operator",
                "--plan",
                plan_rel,
                "--external-review-result-ready",
                "--json",
            ],
            &[],
            "FS-04 compiled-cli workflow operator shared-route parity fixture",
        ),
        "FS-04 compiled-cli workflow operator shared-route parity fixture",
    );
    let status_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-04 compiled-cli plan execution status shared-route parity fixture",
    );
    assert_public_route_parity(&operator_json, &status_json, None);

    let repair_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &[
            "repair-review-state",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
        ],
        "FS-04 compiled-cli repair-review-state shared-runtime fixture",
    );
    assert_eq!(
        repair_json["action"],
        Value::from("blocked"),
        "FS-04 compiled-cli repair should surface the shared closure-recording blocker instead of claiming repair is already current"
    );
    assert_eq!(
        repair_json["phase_detail"],
        Value::from("task_closure_recording_ready"),
        "FS-04 compiled-cli repair should expose task_closure_recording_ready shared-routing detail, got {repair_json:?}"
    );
    assert!(
        repair_json["required_follow_up"].is_null(),
        "FS-04 compiled-cli closure-baseline recovery should not require a stale follow-up category, got {repair_json:?}"
    );
    assert_eq!(
        repair_json["recommended_command"], operator_json["recommended_command"],
        "FS-04 compiled-cli repair and operator must expose the same concrete command target"
    );
    let recommended_command = repair_json["recommended_command"]
        .as_str()
        .expect("FS-04 compiled-cli repair should expose recommended command");
    assert!(
        recommended_command.starts_with("featureforge plan execution close-current-task --plan "),
        "FS-04 compiled-cli closure-baseline route should stay on close-current-task guidance, got {recommended_command}"
    );
    let routed_follow_up = run_recommended_plan_execution_command_real_cli(
        repo,
        state,
        recommended_command,
        "FS-04 run compiled-cli operator-recommended command directly",
    );
    assert_follow_up_blocker_parity_with_operator(
        &operator_json,
        &routed_follow_up,
        "FS-04 compiled-cli command-follow parity",
    );
    assert!(
        matches!(
            routed_follow_up["action"].as_str(),
            Some("recorded" | "already_current")
        ),
        "FS-04 compiled-cli routed command must be immediately runnable when repair reports already_current, got {routed_follow_up:?}"
    );
}

#[test]
fn runtime_remediation_fs08_compiled_cli_resume_overlay_does_not_hide_stale_blocker() {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs08-workflow-runtime-real-cli");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-04-03-runtime-fs08-stale-blocker-real-cli.md";
    setup_runtime_fs14_fs16_task_boundary_fixture(repo, state, plan_rel);
    let branch = current_branch_name(repo);
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            ("current_branch_closure_id", Value::Null),
            ("current_branch_closure_reviewed_state_id", Value::Null),
            ("current_branch_closure_contract_identity", Value::Null),
            ("resume_task", Value::from(1_u64)),
            ("resume_step", Value::from(1_u64)),
            (
                "review_state_repair_follow_up",
                Value::from("execution_reentry"),
            ),
        ],
    );

    let operator_json = parse_json(
        &run_rust_featureforge_with_env_real_cli(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "FS-08 compiled-cli workflow operator stale-blocker visibility fixture",
        ),
        "FS-08 compiled-cli workflow operator stale-blocker visibility fixture",
    );
    let status_json = run_plan_execution_json_real_cli(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-08 compiled-cli plan execution status stale-blocker visibility fixture",
    );
    assert_public_route_parity(&operator_json, &status_json, None);
    assert_eq!(
        operator_json["phase_detail"],
        Value::from("task_closure_recording_ready")
    );
    assert_eq!(
        status_json["phase_detail"],
        Value::from("task_closure_recording_ready")
    );
    assert_eq!(operator_json["blocking_scope"], Value::from("task"));
    assert_eq!(status_json["blocking_scope"], Value::from("task"));
    assert_eq!(operator_json["blocking_task"], Value::from(1_u64));
    assert_eq!(status_json["blocking_task"], Value::from(1_u64));
    let mut operator_reason_codes = operator_json["blocking_reason_codes"]
        .as_array()
        .expect("FS-08 compiled-cli operator should expose blocking_reason_codes as an array")
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mut status_reason_codes = status_json["blocking_reason_codes"]
        .as_array()
        .expect("FS-08 compiled-cli status should expose blocking_reason_codes as an array")
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    operator_reason_codes.sort();
    status_reason_codes.sort();
    let expected_reason_codes = vec![
        String::from("prior_task_current_closure_missing"),
        String::from("task_closure_baseline_repair_candidate"),
    ];
    assert_eq!(
        operator_reason_codes, expected_reason_codes,
        "FS-08 compiled-cli operator should expose the exact stale-blocker reason-code set for this fixture"
    );
    assert_eq!(
        status_reason_codes, expected_reason_codes,
        "FS-08 compiled-cli status should expose the exact stale-blocker reason-code set for this fixture"
    );
    let recommended_command = operator_json["recommended_command"]
        .as_str()
        .expect("FS-08 compiled-cli operator should expose recommended command");
    assert!(
        recommended_command.starts_with("featureforge plan execution close-current-task --plan "),
        "FS-08 compiled-cli operator should expose a runnable plan-execution command, got {recommended_command}"
    );
    assert!(
        recommended_command.contains("--task 1"),
        "FS-08 compiled-cli operator should route stale-blocker recovery to Task 1 close-current-task, got {recommended_command}"
    );
    let routed_follow_up = run_recommended_plan_execution_command_real_cli(
        repo,
        state,
        recommended_command,
        "FS-08 run compiled-cli operator-routed command directly",
    );
    assert_follow_up_blocker_parity_with_operator(
        &operator_json,
        &routed_follow_up,
        "FS-08 compiled-cli command-follow parity",
    );
    if routed_follow_up["action"].as_str() != Some("blocked") {
        assert!(
            matches!(
                routed_follow_up["action"].as_str(),
                Some("recorded" | "already_current")
            ),
            "FS-08 compiled-cli routed command should either record closure or remain already_current, got {routed_follow_up:?}"
        );
    }
}
fn setup_runtime_fs11_next_action_fixture(repo: &Path, state: &Path, plan_rel: &str) {
    install_full_contract_ready_artifacts(repo);
    write_runtime_remediation_fs11_plan(repo, plan_rel, FULL_CONTRACT_READY_SPEC_REL);
    prepare_preflight_acceptance_workspace(repo, "runtime-remediation-fs11-next-action");
    let branch = current_branch_name(repo);

    let preflight = run_internal_plan_execution_preflight_json(
        repo,
        state,
        plan_rel,
        "FS-11 preflight before shared-next-action contradiction fixture",
    );
    assert_eq!(
        preflight["allowed"],
        Value::Bool(true),
        "FS-11 fixture preflight should allow execution",
    );
    let status_before_begin = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-11 status before execution-start bootstrap begin",
    );
    let begin = run_plan_execution_json(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "2",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_before_begin["execution_fingerprint"]
                .as_str()
                .expect("FS-11 status should expose execution fingerprint before begin"),
        ],
        "FS-11 execution-start bootstrap begin",
    );
    run_plan_execution_json(
        repo,
        state,
        &[
            "complete",
            "--plan",
            plan_rel,
            "--task",
            "2",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "FS-11 bootstrap complete",
            "--manual-verify-summary",
            "FS-11 bootstrap complete summary",
            "--file",
            "tests/workflow_runtime.rs",
            "--expect-execution-fingerprint",
            begin["execution_fingerprint"]
                .as_str()
                .expect("FS-11 begin should expose execution fingerprint before complete"),
        ],
        "FS-11 execution-start bootstrap complete",
    );

    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            (
                "task_closure_record_history",
                json!({
                    "task-2-stale": {
                        "closure_record_id": "task-2-stale",
                        "task": 2,
                        "source_plan_path": plan_rel,
                        "source_plan_revision": 1,
                        "record_sequence": 10,
                        "closure_status": "stale_unreviewed",
                        "effective_reviewed_surface_paths": ["README.md"]
                    }
                }),
            ),
            (
                "current_open_step_state",
                json!({
                    "task": 3,
                    "step": 6,
                    "note_state": "Interrupted",
                    "note_summary": "FS-11 forward reentry overlay must not outrank stale Task 2 boundary",
                    "source_plan_path": plan_rel,
                    "source_plan_revision": 1,
                    "authoritative_sequence": 30
                }),
            ),
            ("active_task", Value::Null),
            ("active_step", Value::Null),
            ("resume_task", Value::from(3_u64)),
            ("resume_step", Value::from(6_u64)),
        ],
    );
}

fn setup_runtime_fs11_fs15_next_action_fixture(repo: &Path, state: &Path, plan_rel: &str) {
    install_full_contract_ready_artifacts(repo);
    write_runtime_remediation_fs15_plan(repo, plan_rel, FULL_CONTRACT_READY_SPEC_REL);
    prepare_preflight_acceptance_workspace(repo, "runtime-remediation-fs11-fs15-next-action");
    let branch = current_branch_name(repo);

    let preflight = run_internal_plan_execution_preflight_json(
        repo,
        state,
        plan_rel,
        "FS-11/FS-15 preflight before stale-boundary selection fixture",
    );
    assert_eq!(
        preflight["allowed"],
        Value::Bool(true),
        "FS-11/FS-15 fixture preflight should allow execution",
    );
    let status_before_begin = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-11/FS-15 status before execution-start bootstrap begin",
    );
    let begin = run_plan_execution_json(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_before_begin["execution_fingerprint"]
                .as_str()
                .expect("FS-11/FS-15 status should expose execution fingerprint before begin"),
        ],
        "FS-11/FS-15 execution-start bootstrap task 1 begin",
    );
    run_plan_execution_json(
        repo,
        state,
        &[
            "complete",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "FS-11/FS-15 bootstrap task 1 complete",
            "--manual-verify-summary",
            "FS-11/FS-15 bootstrap task 1 complete summary",
            "--file",
            "tests/workflow_runtime.rs",
            "--expect-execution-fingerprint",
            begin["execution_fingerprint"].as_str().expect(
                "FS-11/FS-15 begin should expose execution fingerprint before task 1 complete",
            ),
        ],
        "FS-11/FS-15 execution-start bootstrap task 1 complete",
    );
    let repair_task_1 = run_plan_execution_json(
        repo,
        state,
        &[
            "repair-review-state",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
        ],
        "FS-11/FS-15 task 1 repair bridge",
    );
    let repair_task_1_command = repair_task_1["recommended_command"]
        .as_str()
        .expect("FS-11/FS-15 task 1 repair bridge should expose a public follow-up command");
    assert!(
        repair_task_1_command.contains("close-current-task"),
        "FS-11/FS-15 task 1 repair bridge should route through close-current-task, got {repair_task_1_command}"
    );
    let close_task_1 = run_recommended_plan_execution_command(
        repo,
        state,
        repair_task_1_command,
        "FS-11/FS-15 task 1 repair follow-up",
    );
    assert!(
        matches!(
            close_task_1["action"].as_str(),
            Some("recorded" | "already_current")
        ),
        "FS-11/FS-15 task 1 repair follow-up should leave task 1 closure current, got {close_task_1:?}"
    );
    let status_after_task_1_repair = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-11/FS-15 status after task 1 repair",
    );
    let begin = run_plan_execution_json(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "2",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_after_task_1_repair["execution_fingerprint"]
                .as_str()
                .expect(
                    "FS-11/FS-15 status after task 1 repair should expose execution fingerprint before task 2 begin"
                ),
        ],
        "FS-11/FS-15 execution-start bootstrap task 2 begin",
    );
    run_plan_execution_json(
        repo,
        state,
        &[
            "complete",
            "--plan",
            plan_rel,
            "--task",
            "2",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "FS-11/FS-15 bootstrap complete",
            "--manual-verify-summary",
            "FS-11/FS-15 bootstrap complete summary",
            "--file",
            "tests/workflow_runtime.rs",
            "--expect-execution-fingerprint",
            begin["execution_fingerprint"].as_str().expect(
                "FS-11/FS-15 begin should expose execution fingerprint before task 2 complete",
            ),
        ],
        "FS-11/FS-15 execution-start bootstrap task 2 complete",
    );
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            (
                "task_closure_record_history",
                json!({
                    "task-2-stale": {
                        "closure_record_id": "task-2-stale",
                        "task": 2,
                        "source_plan_path": plan_rel,
                        "source_plan_revision": 1,
                        "record_sequence": 10,
                        "closure_status": "stale_unreviewed",
                        "effective_reviewed_surface_paths": ["README.md"]
                    },
                    "task-6-stale": {
                        "closure_record_id": "task-6-stale",
                        "task": 6,
                        "source_plan_path": plan_rel,
                        "source_plan_revision": 1,
                        "record_sequence": 20,
                        "closure_status": "stale_unreviewed",
                        "effective_reviewed_surface_paths": ["README.md"]
                    }
                }),
            ),
            (
                "current_open_step_state",
                json!({
                    "task": 6,
                    "step": 1,
                    "note_state": "Interrupted",
                    "note_summary": "FS-11/FS-15 later resume overlay should not outrank earlier stale boundary",
                    "source_plan_path": plan_rel,
                    "source_plan_revision": 1,
                    "authoritative_sequence": 30
                }),
            ),
            ("active_task", Value::Null),
            ("active_step", Value::Null),
            ("resume_task", Value::from(6_u64)),
            ("resume_step", Value::from(1_u64)),
        ],
    );
}

fn setup_runtime_fs17_fs21_fs22_stale_task1_bridge_fixture(
    repo: &Path,
    state: &Path,
    plan_rel: &str,
    resume_overlay: Option<(u32, u32)>,
) {
    let dispatch_id = setup_runtime_fs14_fs16_task_boundary_fixture(repo, state, plan_rel);
    let status_after_fixture = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-17/FS-21/FS-22 fixture status before stale history injection",
    );
    let execution_run_id = status_after_fixture["execution_run_id"]
        .as_str()
        .expect("FS-17/FS-21/FS-22 fixture status should expose execution_run_id")
        .to_owned();
    let branch = current_branch_name(repo);
    let mut updates = vec![(
        "task_closure_record_history",
        json!({
            "task-1-stale-history": {
                "closure_record_id": "task-1-stale-history",
                "task": 1,
                "source_plan_path": plan_rel,
                "source_plan_revision": 1,
                "record_sequence": 7,
                "record_status": "stale_unreviewed",
                "closure_status": "stale_unreviewed",
                "dispatch_id": dispatch_id,
                "execution_run_id": execution_run_id,
                "effective_reviewed_surface_paths": ["README.md"]
            }
        }),
    )];
    if let Some((resume_task, resume_step)) = resume_overlay {
        updates.extend([
            (
                "current_open_step_state",
                json!({
                    "task": resume_task,
                    "step": resume_step,
                    "note_state": "Interrupted",
                    "note_summary": "FS-21 interrupted downstream step should be preempted by stale Task 1 closure bridge.",
                    "source_plan_path": plan_rel,
                    "source_plan_revision": 1,
                    "authoritative_sequence": 42
                }),
            ),
            ("resume_task", Value::from(u64::from(resume_task))),
            ("resume_step", Value::from(u64::from(resume_step))),
            ("active_task", Value::Null),
            ("active_step", Value::Null),
        ]);
    }
    update_authoritative_harness_state(repo, state, &branch, plan_rel, 1, &updates);
}

#[test]
fn runtime_remediation_fs15_earliest_stale_boundary_beats_latest_overlay_target() {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs15-earliest-stale-boundary");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-04-01-runtime-fs15-earliest-stale-boundary.md";
    setup_runtime_fs11_fs15_next_action_fixture(repo, state, plan_rel);

    let operator_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "FS-15 workflow operator stale-boundary targeting fixture",
        ),
        "FS-15 workflow operator stale-boundary targeting fixture",
    );
    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-15 plan execution status stale-boundary targeting fixture",
    );
    assert_public_route_parity(&operator_json, &status_json, None);
    assert_eq!(
        operator_json["blocking_scope"],
        Value::from("task"),
        "FS-15 should keep stale-boundary targeting task-scoped, got {operator_json:?}"
    );
    assert_eq!(
        operator_json["blocking_task"],
        Value::from(2_u64),
        "FS-15 earliest unresolved stale boundary must target Task 2, not Task 6; operator payload: {operator_json:?}"
    );
    let recommended_command = operator_json["recommended_command"]
        .as_str()
        .expect("FS-15 workflow operator should return a concrete follow-up command");
    assert!(
        recommended_command.contains("--task 2"),
        "FS-15 workflow operator must target Task 2 while it is the earliest unresolved stale boundary, got {recommended_command}"
    );
    assert!(
        !recommended_command.contains("--task 6"),
        "FS-15 workflow operator must not prefer stale Task 6 while Task 2 remains unresolved, got {recommended_command}"
    );
    assert!(
        recommended_command.starts_with("featureforge plan execution "),
        "FS-15 workflow operator should return an executable plan-execution command, got {recommended_command}"
    );
    let routed_follow_up = run_recommended_plan_execution_command(
        repo,
        state,
        recommended_command,
        "FS-15 run operator-routed public command for concrete parity check",
    );
    assert_follow_up_blocker_parity_with_operator(
        &operator_json,
        &routed_follow_up,
        "FS-15 command-follow parity",
    );
    if routed_follow_up["action"].as_str() == Some("blocked") {
        assert_eq!(
            routed_follow_up["required_follow_up"],
            Value::from("execution_reentry"),
            "FS-15 blocked command-follow parity must keep execution_reentry as the concrete follow-up lane"
        );
    } else {
        let reopened_task_2 = routed_follow_up["resume_task"].as_u64() == Some(2_u64)
            && routed_follow_up["resume_step"].as_u64() == Some(1_u64);
        assert!(
            reopened_task_2,
            "FS-15 command-follow parity must reopen Task 2 Step 1 when the routed command is not blocked, got {routed_follow_up:?}"
        );
    }
}

#[test]
fn runtime_remediation_fs11_prestart_operator_status_begin_share_first_unchecked_step() {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs11-prestart-shared-next-action");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = FULL_CONTRACT_READY_PLAN_REL;

    install_full_contract_ready_artifacts(repo);
    prepare_preflight_acceptance_workspace(
        repo,
        "runtime-remediation-fs11-prestart-shared-next-action",
    );

    let status_before_begin = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-11 prestart status shared next-action fixture",
    );
    assert_eq!(
        status_before_begin["execution_started"],
        Value::from("no"),
        "FS-11 prestart fixture should remain pre-start before begin parity checks"
    );
    let preflight_before_begin = run_internal_plan_execution_preflight_json(
        repo,
        state,
        plan_rel,
        "FS-11 prestart preflight before shared begin parity",
    );
    assert_eq!(
        preflight_before_begin["allowed"],
        Value::Bool(true),
        "FS-11 prestart preflight should allow begin parity fixture"
    );
    let operator_before_begin = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "FS-11 prestart operator shared next-action fixture",
        ),
        "FS-11 prestart operator shared next-action fixture",
    );
    assert_public_route_parity(&operator_before_begin, &status_before_begin, None);
    assert_eq!(
        operator_before_begin["recommended_command"], status_before_begin["recommended_command"],
        "FS-11 prestart operator and status must expose the same shared begin command target",
    );
    let recommended_command = operator_before_begin["recommended_command"]
        .as_str()
        .expect("FS-11 prestart operator should expose recommended command");
    assert!(
        recommended_command.starts_with("featureforge plan execution begin --plan "),
        "FS-11 prestart route should recommend begin from the shared engine, got {recommended_command}",
    );
    assert!(
        recommended_command.contains("--task 1") && recommended_command.contains("--step 1"),
        "FS-11 prestart shared begin target should point to Task 1 Step 1, got {recommended_command}",
    );
    let recommended_parts = recommended_command.split_whitespace().collect::<Vec<_>>();
    let recommended_task = recommended_parts
        .windows(2)
        .find(|window| window[0] == "--task")
        .map(|window| window[1])
        .expect("FS-11 prestart recommended begin should include --task");
    let recommended_step = recommended_parts
        .windows(2)
        .find(|window| window[0] == "--step")
        .map(|window| window[1])
        .expect("FS-11 prestart recommended begin should include --step");
    let begin_from_operator = run_plan_execution_json(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            recommended_task,
            "--step",
            recommended_step,
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_before_begin["execution_fingerprint"]
                .as_str()
                .expect("FS-11 prestart status should expose execution fingerprint"),
        ],
        "FS-11 prestart begin parity command from operator/status shared decision",
    );
    assert_eq!(
        begin_from_operator["active_task"],
        Value::from(1_u64),
        "FS-11 prestart shared begin route should activate Task 1"
    );
    assert_eq!(
        begin_from_operator["active_step"],
        Value::from(1_u64),
        "FS-11 prestart shared begin route should activate Step 1"
    );
}

#[test]
fn runtime_remediation_fs11_late_stage_reroute_does_not_outrank_earliest_stale_boundary() {
    let (repo_dir, state_dir) =
        init_repo("runtime-remediation-fs11-late-stage-reroute-stale-boundary-priority");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-04-04-runtime-fs11-late-stage-reroute-priority.md";
    setup_runtime_fs11_fs15_next_action_fixture(repo, state, plan_rel);

    let branch = current_branch_name(repo);
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            (
                "review_state_repair_follow_up",
                Value::from("record_branch_closure"),
            ),
            ("current_branch_closure_id", Value::Null),
            ("current_branch_closure_reviewed_state_id", Value::Null),
            ("current_branch_closure_contract_identity", Value::Null),
            ("harness_phase", Value::from("document_release_pending")),
        ],
    );

    let operator_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "FS-11 late-stage reroute priority operator fixture",
        ),
        "FS-11 late-stage reroute priority operator fixture",
    );
    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-11 late-stage reroute priority status fixture",
    );
    assert_public_route_parity(&operator_json, &status_json, None);
    if let Some(blocking_task) = operator_json["blocking_task"].as_u64() {
        assert_eq!(
            blocking_task, 2_u64,
            "FS-11 stale-boundary priority must keep Task 2 as the blocker even when persisted late-stage reroute metadata exists: {operator_json:?}"
        );
    } else {
        assert_eq!(
            operator_json["execution_command_context"]["task_number"],
            Value::from(2_u64),
            "FS-11 stale-boundary priority must keep Task 2 in execution command context even when persisted late-stage reroute metadata exists: {operator_json:?}"
        );
    }
    let recommended_command = operator_json["recommended_command"]
        .as_str()
        .expect("FS-11 stale-boundary priority operator should expose recommended command");
    let actionable_command = if recommended_command
        .contains("featureforge plan execution repair-review-state --plan ")
    {
        let repair_json = run_plan_execution_json(
            repo,
            state,
            &["repair-review-state", "--plan", plan_rel],
            "FS-11 stale-boundary priority repair follow-up fixture",
        );
        repair_json["recommended_command"]
            .as_str()
            .expect("FS-11 stale-boundary priority repair should expose recommended_command")
            .to_owned()
    } else {
        recommended_command.to_owned()
    };
    assert!(
        actionable_command.contains("--task 2"),
        "FS-11 stale-boundary priority route should target Task 2, got {actionable_command}",
    );
    assert!(
        !actionable_command.contains("--task 6"),
        "FS-11 stale-boundary priority route must not drift to later stale overlays, got {actionable_command}",
    );
    assert!(
        !actionable_command.contains("advance-late-stage"),
        "FS-11 stale-boundary priority route must not jump straight to late-stage recording while an earlier stale task boundary remains unresolved, got {actionable_command}",
    );
    let routed_follow_up = run_recommended_plan_execution_command(
        repo,
        state,
        &actionable_command,
        "FS-11 stale-boundary priority run operator-surfaced public command target",
    );
    if routed_follow_up["action"].as_str() == Some("blocked") {
        assert_follow_up_blocker_parity_with_operator(
            &operator_json,
            &routed_follow_up,
            "FS-11 stale-boundary priority command-follow parity",
        );
    } else {
        let progressed_task_2 = (routed_follow_up["resume_task"].as_u64() == Some(2_u64)
            && routed_follow_up["resume_step"].as_u64() == Some(1_u64))
            || (routed_follow_up["active_task"].as_u64() == Some(2_u64)
                && routed_follow_up["active_step"].as_u64() == Some(1_u64));
        assert!(
            progressed_task_2,
            "FS-11 stale-boundary priority command-follow parity must keep execution on Task 2 Step 1 when the routed command is not blocked, got {routed_follow_up:?}"
        );
    }
}

#[test]
fn runtime_remediation_fs11_operator_begin_repair_share_one_next_action_engine() {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs11-shared-next-action-engine");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-04-02-runtime-fs11-shared-next-action.md";
    setup_runtime_fs11_next_action_fixture(repo, state, plan_rel);

    let operator_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "FS-11 workflow operator stale-boundary fixture",
        ),
        "FS-11 workflow operator stale-boundary fixture",
    );
    let doctor_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "doctor", "--plan", plan_rel, "--json"],
            &[],
            "FS-11 workflow doctor stale-boundary fixture",
        ),
        "FS-11 workflow doctor stale-boundary fixture",
    );
    assert!(
        doctor_json["phase_detail"]
            .as_str()
            .is_some_and(|phase_detail| {
                matches!(
                    phase_detail,
                    "execution_reentry_required" | "planning_reentry_required"
                )
            }),
        "FS-11 workflow doctor should keep stale-boundary reentry detail when the stale Task 2 boundary is authoritative, got {doctor_json:?}"
    );
    assert!(
        doctor_json["next_step"]
            .as_str()
            .is_some_and(|next_step| next_step.contains("Task 2")),
        "FS-11 workflow doctor should provide task-targeted next_step guidance for the stale Task 2 boundary, got {doctor_json:?}"
    );
    assert!(
        doctor_json["next_step"].as_str().is_some_and(|next_step| {
            !next_step.contains("Return to the current execution flow for the approved plan")
        }),
        "FS-11 workflow doctor should not fall back to generic execution guidance when Task 2 is the authoritative blocker, got {doctor_json:?}"
    );
    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-11 plan execution status stale-boundary fixture",
    );
    assert_public_route_parity(&operator_json, &status_json, None);
    assert_eq!(operator_json["blocking_scope"], Value::from("task"));
    assert_eq!(
        operator_json["blocking_task"],
        Value::from(2_u64),
        "FS-11 operator must surface Task 2 as the blocker when the forward Task 3 Step 6 overlay is present"
    );
    let recommended_command = operator_json["recommended_command"]
        .as_str()
        .expect("FS-11 operator should expose recommended command");
    assert!(
        recommended_command.contains("--task 2"),
        "FS-11 operator should keep Task 2 as the authoritative blocker target, got {recommended_command}"
    );
    assert!(
        !recommended_command.contains("--task 3"),
        "FS-11 operator must not drift to Task 3 while Task 2 remains stale, got {recommended_command}"
    );

    let repair_json = run_plan_execution_json(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "FS-11 repair-review-state stale-boundary fixture",
    );
    assert_eq!(repair_json["action"], Value::from("blocked"));
    assert_eq!(
        repair_json["required_follow_up"],
        Value::from("execution_reentry"),
        "FS-11 repair-review-state should expose the same shared follow-up as operator routing"
    );
    assert!(
        repair_json["recommended_command"]
            .as_str()
            .is_some_and(|command| {
                command.starts_with("featureforge plan execution ")
                    && command.contains("--task 2")
                    && command.contains("--step 1")
                    && !command.contains("--task 3")
            }),
        "FS-11 repair-review-state should provide the exact shared executable command targeting Task 2 Step 1 when execution reentry is required: {repair_json}",
    );

    let begin_output = run_rust_featureforge_with_env(
        repo,
        state,
        &[
            "plan",
            "execution",
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "3",
            "--step",
            "6",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_json["execution_fingerprint"]
                .as_str()
                .expect("FS-11 status should expose execution fingerprint for begin rejection"),
        ],
        &[],
        "FS-11 begin should fail closed when it diverges from shared next-action target",
    );
    assert!(
        !begin_output.status.success(),
        "FS-11 begin should fail when requesting Task 3 Step 6 while the shared engine targets Task 2"
    );
    let failure_payload = if begin_output.stderr.is_empty() {
        &begin_output.stdout
    } else {
        &begin_output.stderr
    };
    let failure_json: Value = serde_json::from_slice(failure_payload)
        .expect("FS-11 begin failure payload should be valid json");
    let failure_error_class = failure_json["error_class"]
        .as_str()
        .expect("FS-11 begin failure payload should expose error_class");
    assert!(
        failure_error_class == "InvalidStepTransition"
            || failure_error_class == "ExecutionStateNotReady",
        "FS-11 begin failure should remain a closed begin-time rejection class, got {failure_json:?}",
    );
    let message = failure_json["message"]
        .as_str()
        .expect("FS-11 begin failure payload should expose message text");
    if failure_error_class == "InvalidStepTransition" {
        assert!(
            message.contains("next legal action is"),
            "FS-11 invalid-step rejection should explain shared next-action mismatch, got {failure_json:?}"
        );
    }
    assert!(
        message.contains("task Some(2)") || message.contains("Task 2"),
        "FS-11 begin failure should preserve the authoritative Task 2 blocker target, got {failure_json:?}"
    );

    let recommended_parts = recommended_command.split_whitespace().collect::<Vec<_>>();
    assert!(
        recommended_parts.len() >= 4,
        "FS-11 operator command should remain directly executable, got {recommended_command}"
    );
    let routed_follow_up = run_plan_execution_json(
        repo,
        state,
        &recommended_parts[3..],
        "FS-11 run operator-routed command directly for parity",
    );
    assert_follow_up_blocker_parity_with_operator(
        &operator_json,
        &routed_follow_up,
        "FS-11 command-follow parity",
    );
    if routed_follow_up["action"].as_str() == Some("blocked") {
        assert_eq!(
            routed_follow_up["required_follow_up"],
            Value::from("execution_reentry"),
            "FS-11 operator follow-up should preserve execution-reentry blocker parity"
        );
    }
}

#[test]
fn runtime_remediation_fs11_repair_returns_same_action_as_operator_and_begin() {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs11-repair-shared-action");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-04-02-runtime-fs11-repair-shared-action.md";
    setup_runtime_fs11_next_action_fixture(repo, state, plan_rel);

    let operator_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "FS-11 repair shared-action workflow operator fixture",
        ),
        "FS-11 repair shared-action workflow operator fixture",
    );
    let repair_json = run_plan_execution_json(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "FS-11 repair shared-action repair-review-state fixture",
    );
    assert_eq!(
        repair_json["action"],
        Value::from("blocked"),
        "FS-11 repair shared-action fixture should return a concrete blocker"
    );
    assert_eq!(
        repair_json["required_follow_up"],
        Value::from("execution_reentry"),
        "FS-11 repair shared-action fixture should surface execution reentry"
    );
    assert_eq!(
        repair_json["recommended_command"], operator_json["recommended_command"],
        "FS-11 repair shared-action fixture should expose the same next command target as workflow operator"
    );
    let recommended_command = repair_json["recommended_command"]
        .as_str()
        .expect("FS-11 repair shared-action fixture should expose recommended_command");
    assert!(
        recommended_command.contains("--task 2"),
        "FS-11 repair shared-action fixture should keep Task 2 as the earliest stale-boundary target, got {recommended_command}"
    );
    assert!(
        !recommended_command.contains("--task 3"),
        "FS-11 repair shared-action fixture must not drift to Task 3 Step 6 while Task 2 remains unresolved, got {recommended_command}"
    );
    let routed_follow_up = run_recommended_plan_execution_command(
        repo,
        state,
        recommended_command,
        "FS-11 repair shared-action command-follow parity",
    );
    assert_follow_up_blocker_parity_with_operator(
        &operator_json,
        &routed_follow_up,
        "FS-11 repair shared-action command-follow parity",
    );
    if routed_follow_up["action"].as_str() == Some("blocked") {
        assert_eq!(
            routed_follow_up["required_follow_up"], repair_json["required_follow_up"],
            "FS-11 repair shared-action follow-up should preserve repair-required_follow_up parity when blocked"
        );
    }
}

#[test]
fn runtime_remediation_fs14_missing_task_closure_baseline_routes_to_close_current_task_not_execution_reentry()
 {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs14-workflow-runtime");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-04-03-runtime-fs14-workflow-runtime.md";
    setup_runtime_fs14_fs16_task_boundary_fixture(repo, state, plan_rel);

    let operator_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &[
                "workflow",
                "operator",
                "--plan",
                plan_rel,
                "--external-review-result-ready",
                "--json",
            ],
            &[],
            "FS-14 workflow operator closure-baseline repair routing fixture",
        ),
        "FS-14 workflow operator closure-baseline repair routing fixture",
    );
    assert_eq!(
        operator_json["phase"],
        Value::from("task_closure_pending"),
        "FS-14 operator should stay in task-closure pending when closure baseline is missing"
    );
    assert_eq!(
        operator_json["phase_detail"],
        Value::from("task_closure_recording_ready"),
        "FS-14 operator should route missing closure baselines directly to task_closure_recording_ready"
    );
    assert_eq!(
        operator_json["next_action"],
        Value::from("close current task"),
        "FS-14 operator should route to close-current-task instead of execution reentry"
    );
    let recommended_command = operator_json["recommended_command"]
        .as_str()
        .expect("FS-14 operator should expose recommended close-current-task command");
    assert!(
        recommended_command.contains("close-current-task"),
        "FS-14 operator should recommend close-current-task, got {recommended_command}"
    );
    assert!(
        !recommended_command.contains("preflight")
            && !recommended_command.contains("record-review-dispatch")
            && !recommended_command.contains("gate-review")
            && !recommended_command.contains("rebuild-evidence"),
        "FS-14 normal recovery should not require hidden helper commands, got {recommended_command}"
    );
    let close_json = run_recommended_plan_execution_command(
        repo,
        state,
        recommended_command,
        "FS-14 execute workflow/operator routed command target directly",
    );
    assert_follow_up_blocker_parity_with_operator(
        &operator_json,
        &close_json,
        "FS-14 command-follow parity",
    );
    if close_json["action"].as_str() != Some("blocked") {
        assert!(
            matches!(
                close_json["action"].as_str(),
                Some("recorded" | "already_current")
            ),
            "FS-14 command-follow parity should let close-current-task record or preserve a current closure baseline, got {close_json:?}"
        );
    }
}

#[test]
fn runtime_remediation_fs14_repair_routes_missing_task_closure_baseline_to_close_current_task() {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs14-repair-routing");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-04-03-runtime-fs14-repair-routing.md";
    setup_runtime_fs14_fs16_task_boundary_fixture(repo, state, plan_rel);

    let operator_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &[
                "workflow",
                "operator",
                "--plan",
                plan_rel,
                "--external-review-result-ready",
                "--json",
            ],
            &[],
            "FS-14 repair parity workflow operator fixture",
        ),
        "FS-14 repair parity workflow operator fixture",
    );
    assert_eq!(
        operator_json["phase_detail"],
        Value::from("task_closure_recording_ready"),
        "FS-14 repair parity fixture should expose task_closure_recording_ready in workflow/operator"
    );
    let operator_command = operator_json["recommended_command"]
        .as_str()
        .expect("FS-14 repair parity fixture should expose workflow/operator command")
        .to_owned();
    assert!(
        operator_command.contains("close-current-task"),
        "FS-14 repair parity workflow/operator should recommend close-current-task, got {operator_command}"
    );

    let repair_json = run_plan_execution_json(
        repo,
        state,
        &[
            "repair-review-state",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
        ],
        "FS-14 repair parity fixture",
    );
    assert_eq!(
        repair_json["action"],
        Value::from("blocked"),
        "FS-14 repair parity fixture should surface closure recording as the shared blocker"
    );
    assert_eq!(
        repair_json["phase_detail"],
        Value::from("task_closure_recording_ready"),
        "FS-14 repair parity fixture should surface task_closure_recording_ready"
    );
    assert_eq!(
        repair_json["required_follow_up"],
        Value::Null,
        "FS-14 repair parity fixture should avoid execution-reentry follow-up when closure recording is the next action"
    );
    assert_eq!(
        repair_json["recommended_command"],
        Value::from(operator_command.clone()),
        "FS-14 repair parity fixture should expose the same public command target as workflow/operator"
    );
    assert_eq!(
        repair_json["authoritative_next_action"],
        Value::from(operator_command.clone()),
        "FS-14 repair parity fixture should mirror workflow/operator through authoritative_next_action"
    );
    let routed_follow_up = run_recommended_plan_execution_command(
        repo,
        state,
        &operator_command,
        "FS-14 repair parity run workflow/operator-surfaced command target",
    );
    if routed_follow_up["action"].as_str() == Some("blocked") {
        assert_follow_up_blocker_parity_with_operator(
            &operator_json,
            &routed_follow_up,
            "FS-14 repair command-follow parity",
        );
    } else {
        let action = routed_follow_up["action"].as_str();
        assert!(
            action == Some("recorded") || action == Some("already_current"),
            "FS-14 repair command-follow parity should either record closure state or stay already current, got {routed_follow_up:?}"
        );
    }
}

#[test]
fn runtime_remediation_fs14_operator_repair_parity_without_external_review_ready_flag() {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs14-repair-routing-no-ext-ready");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-04-03-runtime-fs14-repair-routing-no-ext-ready.md";
    setup_runtime_fs14_fs16_task_boundary_fixture(repo, state, plan_rel);

    let operator_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "FS-14 no-external-ready parity workflow operator fixture",
        ),
        "FS-14 no-external-ready parity workflow operator fixture",
    );
    assert_eq!(
        operator_json["phase"],
        Value::from("task_closure_pending"),
        "FS-14 no-external-ready parity should remain on task closure pending"
    );
    assert_eq!(
        operator_json["phase_detail"],
        Value::from("task_closure_recording_ready"),
        "FS-14 no-external-ready parity should route directly to task_closure_recording_ready"
    );
    let operator_command = operator_json["recommended_command"]
        .as_str()
        .expect("FS-14 no-external-ready parity should expose workflow/operator command")
        .to_owned();
    assert!(
        operator_command.contains("close-current-task"),
        "FS-14 no-external-ready parity should recommend close-current-task, got {operator_command}"
    );

    let repair_json = run_plan_execution_json(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "FS-14 no-external-ready repair parity fixture",
    );
    assert_eq!(
        repair_json["action"],
        Value::from("blocked"),
        "FS-14 no-external-ready repair parity should surface closure recording as the shared blocker"
    );
    assert_eq!(
        repair_json["phase_detail"],
        Value::from("task_closure_recording_ready"),
        "FS-14 no-external-ready repair parity should keep closure recording as the next action"
    );
    assert_eq!(
        repair_json["required_follow_up"],
        Value::Null,
        "FS-14 no-external-ready repair parity should not regress to execution reentry"
    );
    assert_eq!(
        repair_json["recommended_command"],
        Value::from(operator_command.clone()),
        "FS-14 no-external-ready repair parity should expose the same public command as workflow/operator"
    );
    assert_eq!(
        repair_json["authoritative_next_action"],
        Value::from(operator_command.clone()),
        "FS-14 no-external-ready repair parity should mirror workflow/operator through authoritative_next_action"
    );
    let routed_follow_up = run_recommended_plan_execution_command(
        repo,
        state,
        &operator_command,
        "FS-14 no-external-ready repair parity run workflow/operator-surfaced command target",
    );
    if routed_follow_up["action"].as_str() == Some("blocked") {
        assert_follow_up_blocker_parity_with_operator(
            &operator_json,
            &routed_follow_up,
            "FS-14 no-external-ready repair command-follow parity",
        );
    } else {
        let action = routed_follow_up["action"].as_str();
        assert!(
            action == Some("recorded") || action == Some("already_current"),
            "FS-14 no-external-ready command-follow parity should either record closure state or stay already current, got {routed_follow_up:?}"
        );
    }
}

#[test]
fn runtime_remediation_fs15_repair_never_jumps_to_later_task_when_earlier_boundary_exists() {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs15-repair-target-parity");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-04-04-runtime-fs15-repair-target-parity.md";
    setup_runtime_fs11_fs15_next_action_fixture(repo, state, plan_rel);
    let operator_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "FS-15 repair target parity workflow operator fixture",
        ),
        "FS-15 repair target parity workflow operator fixture",
    );

    let repair_json = run_plan_execution_json(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "FS-15 repair target parity fixture",
    );
    assert_eq!(repair_json["action"], Value::from("blocked"));
    assert_eq!(
        repair_json["required_follow_up"],
        Value::from("execution_reentry"),
        "FS-15 repair should keep stale-boundary follow-up in execution_reentry lane"
    );
    let recommended_command = repair_json["recommended_command"]
        .as_str()
        .expect("FS-15 repair target parity fixture should expose recommended command");
    assert_eq!(
        repair_json["recommended_command"], operator_json["recommended_command"],
        "FS-15 repair target parity fixture should expose the same public command as workflow/operator"
    );
    assert!(
        recommended_command.starts_with("featureforge plan execution "),
        "FS-15 repair target parity should return an executable plan-execution command, got {recommended_command}"
    );
    assert!(
        recommended_command.contains("--task 2"),
        "FS-15 repair must keep Task 2 as the earliest stale boundary target, got {recommended_command}"
    );
    assert!(
        !recommended_command.contains("--task 6"),
        "FS-15 repair must not jump to Task 6 while Task 2 remains unresolved, got {recommended_command}"
    );
    let routed_follow_up = run_recommended_plan_execution_command(
        repo,
        state,
        recommended_command,
        "FS-15 repair target parity run repair-surfaced public command target",
    );
    if routed_follow_up["action"].as_str() == Some("blocked") {
        assert_follow_up_blocker_parity_with_operator(
            &operator_json,
            &routed_follow_up,
            "FS-15 repair command-follow parity",
        );
        assert_eq!(
            routed_follow_up["required_follow_up"],
            Value::from("execution_reentry"),
            "FS-15 blocked repair command-follow parity must keep execution_reentry as the concrete follow-up lane"
        );
    } else {
        let reopened_task_2 = routed_follow_up["resume_task"].as_u64() == Some(2_u64)
            && routed_follow_up["resume_step"].as_u64() == Some(1_u64);
        assert!(
            reopened_task_2,
            "FS-15 repair command-follow parity must reopen Task 2 Step 1 when the routed command is not blocked, got {routed_follow_up:?}"
        );
    }
}

#[test]
fn fs17_stale_unreviewed_truthful_replay_promotes_to_task_closure_recording_ready() {
    let (repo_dir, state_dir) =
        init_repo("runtime-remediation-fs17-stale-unreviewed-bridge-routing");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-04-08-runtime-fs17-stale-unreviewed-bridge.md";
    setup_runtime_fs17_fs21_fs22_stale_task1_bridge_fixture(repo, state, plan_rel, None);

    let operator_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "FS-17 workflow operator truthful replay convergence fixture",
        ),
        "FS-17 workflow operator truthful replay convergence fixture",
    );
    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-17 status truthful replay convergence fixture",
    );
    assert_public_route_parity(&operator_json, &status_json, None);
    assert_eq!(
        status_json["review_state_status"],
        Value::from("stale_unreviewed"),
        "FS-17 fixture must stay in stale_unreviewed while stale task-closure history is unresolved: {status_json:?}"
    );
    assert!(
        status_json["reason_codes"].as_array().is_some_and(|codes| {
            codes
                .iter()
                .any(|code| code == &Value::from("prior_task_current_closure_missing"))
                && codes
                    .iter()
                    .any(|code| code == &Value::from("task_closure_baseline_repair_candidate"))
        }),
        "FS-17 fixture should expose missing-current-closure baseline bridge reason codes: {status_json:?}"
    );
    assert_eq!(
        status_json["phase_detail"],
        Value::from("task_closure_recording_ready"),
        "FS-17 truthful replay recovery must route to task_closure_recording_ready instead of generic execution reentry: {status_json:?}"
    );
    let recommended_command = status_json["recommended_command"]
        .as_str()
        .expect("FS-17 status should expose a concrete closure-recording command");
    assert!(
        recommended_command.contains("close-current-task")
            && recommended_command.contains("--task 1"),
        "FS-17 truthful replay recovery must route through close-current-task --task 1, got {recommended_command}"
    );
    assert_ne!(
        status_json["phase_detail"],
        Value::from("execution_reentry_required"),
        "FS-17 truthful replay recovery must not fall back to execution_reentry_required"
    );
}

#[test]
fn fs18_cycle_break_binding_is_task_scoped_not_global() {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs18-cycle-break-task-scoped");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-04-09-runtime-fs18-cycle-break-task-scoped.md";
    setup_runtime_fs11_fs15_next_action_fixture(repo, state, plan_rel);
    let branch = current_branch_name(repo);
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            ("strategy_state", Value::from("cycle_breaking")),
            ("strategy_checkpoint_kind", Value::from("cycle_break")),
            ("strategy_cycle_break_task", Value::from(1_u64)),
            ("strategy_cycle_break_step", Value::from(1_u64)),
            (
                "strategy_cycle_break_checkpoint_fingerprint",
                Value::from("fs18-cycle-break-task-1"),
            ),
        ],
    );

    let operator_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "FS-18 workflow operator cycle-break task binding fixture",
        ),
        "FS-18 workflow operator cycle-break task binding fixture",
    );
    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-18 status cycle-break task binding fixture",
    );
    assert_public_route_parity(&operator_json, &status_json, None);
    assert_eq!(
        operator_json["blocking_task"],
        Value::from(2_u64),
        "FS-18 Task-1 cycle-break binding must not globally block later Task-2 stale-boundary routing: {operator_json:?}"
    );
    let recommended_command = operator_json["recommended_command"]
        .as_str()
        .expect("FS-18 operator should expose recommended command");
    assert!(
        recommended_command.contains("--task 2"),
        "FS-18 cycle-break routing should still target Task 2 after Task 1 is already repaired, got {recommended_command}"
    );
    assert!(
        !recommended_command.contains("--task 1"),
        "FS-18 cycle-break binding must not drag routing back to Task 1 once Task 1 is repaired, got {recommended_command}"
    );
}

#[test]
fn fs19_superseded_stale_historical_task_closure_is_not_an_unresolved_stale_boundary() {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs19-workflow-runtime");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-04-06-runtime-fs19-workflow-runtime.md";
    setup_runtime_fs11_fs15_next_action_fixture(repo, state, plan_rel);
    let branch = current_branch_name(repo);
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            (
                "task_closure_record_history",
                json!({
                    "task-1-stale": {
                        "closure_record_id": "task-1-stale",
                        "task": 1,
                        "source_plan_path": plan_rel,
                        "source_plan_revision": 1,
                        "record_sequence": 8,
                        "record_status": "stale_unreviewed",
                        "closure_status": "stale_unreviewed",
                        "effective_reviewed_surface_paths": ["README.md"]
                    },
                    "task-1-current": {
                        "closure_record_id": "task-1-current",
                        "task": 1,
                        "source_plan_path": plan_rel,
                        "source_plan_revision": 1,
                        "record_sequence": 22,
                        "record_status": "current",
                        "closure_status": "current",
                        "effective_reviewed_surface_paths": ["README.md"]
                    },
                    "task-2-stale": {
                        "closure_record_id": "task-2-stale",
                        "task": 2,
                        "source_plan_path": plan_rel,
                        "source_plan_revision": 1,
                        "record_sequence": 10,
                        "record_status": "stale_unreviewed",
                        "closure_status": "stale_unreviewed",
                        "effective_reviewed_surface_paths": ["README.md"]
                    },
                    "task-6-stale": {
                        "closure_record_id": "task-6-stale",
                        "task": 6,
                        "source_plan_path": plan_rel,
                        "source_plan_revision": 1,
                        "record_sequence": 20,
                        "record_status": "stale_unreviewed",
                        "closure_status": "stale_unreviewed",
                        "effective_reviewed_surface_paths": ["README.md"]
                    }
                }),
            ),
            ("superseded_task_closure_ids", json!(["task-1-stale"])),
            (
                "current_open_step_state",
                json!({
                    "task": 6,
                    "step": 1,
                    "note_state": "Interrupted",
                    "note_summary": "FS-19 stale task 1 history should be superseded by current task 1 closure.",
                    "source_plan_path": plan_rel,
                    "source_plan_revision": 1,
                    "authoritative_sequence": 30
                }),
            ),
            ("resume_task", Value::from(6_u64)),
            ("resume_step", Value::from(1_u64)),
        ],
    );

    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-19 status should ignore superseded stale task 1 history",
    );
    assert_eq!(
        status_json["blocking_task"],
        Value::from(2_u64),
        "FS-19 earliest unresolved stale task should move past superseded stale task 1 history"
    );
    assert_ne!(
        status_json["blocking_task"],
        Value::from(1_u64),
        "FS-19 superseded stale task 1 history must not remain an unresolved stale boundary"
    );

    let operator_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "FS-19 operator stale-boundary target fixture",
        ),
        "FS-19 operator stale-boundary target fixture",
    );
    let recommended_command = operator_json["recommended_command"]
        .as_str()
        .expect("FS-19 operator should expose recommended command");
    assert!(
        recommended_command.contains("--task 2"),
        "FS-19 operator should route to Task 2 after stale task 1 history is superseded, got {recommended_command}"
    );
    assert!(
        !recommended_command.contains("--task 1"),
        "FS-19 operator must not route back to superseded stale task 1 history, got {recommended_command}"
    );
}

#[test]
fn fs20_runtime_owned_plan_and_execution_evidence_changes_do_not_stale_current_task_closure() {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs20-task-closure-freshness");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-04-07-runtime-fs20-task-closure-freshness.md";
    let task1_dispatch_id = setup_runtime_fs14_fs16_task_boundary_fixture(repo, state, plan_rel);
    let review_summary_path = repo.join("docs/featureforge/execution-evidence/fs20-review.md");
    let verification_summary_path =
        repo.join("docs/featureforge/execution-evidence/fs20-verification.md");
    write_file(
        &review_summary_path,
        "FS-20 current closure should remain fresh when only runtime-owned paths changed.\n",
    );
    write_file(&verification_summary_path, "FS-20 verification summary.\n");
    let close_task1 = run_plan_execution_json(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--dispatch-id",
            &task1_dispatch_id,
            "--review-result",
            "pass",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("FS-20 review summary path should be utf-8"),
            "--verification-result",
            "pass",
            "--verification-summary-file",
            verification_summary_path
                .to_str()
                .expect("FS-20 verification summary path should be utf-8"),
        ],
        "FS-20 close-current-task should establish a current Task 1 closure",
    );
    assert_eq!(close_task1["action"], Value::from("recorded"));

    let baseline_status = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-20 baseline status before runtime-owned churn",
    );
    let evidence_path = repo.join(
        baseline_status["evidence_path"]
            .as_str()
            .expect("FS-20 baseline status should expose evidence_path"),
    );
    let plan_path = repo.join(plan_rel);
    let plan_source = fs::read_to_string(&plan_path).expect("FS-20 plan should be readable");
    write_file(
        &plan_path,
        &format!("{plan_source}\n<!-- fs20 runtime-owned plan mutation -->\n"),
    );
    let evidence_source =
        fs::read_to_string(&evidence_path).expect("FS-20 evidence should be readable");
    write_file(
        &evidence_path,
        &format!("{evidence_source}\n<!-- fs20 runtime-owned evidence mutation -->\n"),
    );

    let status_after_churn = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-20 status after runtime-owned churn",
    );
    assert!(
        !status_after_churn["reason_codes"]
            .as_array()
            .expect("FS-20 status reason_codes should remain an array")
            .iter()
            .any(|code| code == &Value::from("prior_task_current_closure_stale")),
        "FS-20 runtime-owned plan/evidence-only churn must not stale the current Task 1 closure: {status_after_churn:?}"
    );
    assert_ne!(
        status_after_churn["blocking_task"],
        Value::from(1_u64),
        "FS-20 runtime-owned plan/evidence-only churn must not reblock Task 1 closure freshness"
    );
}

#[test]
fn fs20_runtime_owned_plan_and_execution_evidence_changes_do_not_null_current_branch_closure() {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs20-branch-closure-freshness");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_dispatched_branch_review_artifact(repo, state, plan_rel, &base_branch);
    let release_path = write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    publish_authoritative_release_truth(repo, state, plan_rel, &release_path, &base_branch);

    let baseline_status = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-20 baseline branch status before runtime-owned churn",
    );
    let baseline_branch_closure_id = baseline_status["current_branch_closure_id"]
        .as_str()
        .expect("FS-20 baseline status should expose current_branch_closure_id")
        .to_owned();
    let evidence_path = repo.join(
        baseline_status["evidence_path"]
            .as_str()
            .expect("FS-20 baseline status should expose evidence_path"),
    );
    let plan_path = repo.join(plan_rel);
    let plan_source = fs::read_to_string(&plan_path).expect("FS-20 plan should be readable");
    write_file(
        &plan_path,
        &format!("{plan_source}\n<!-- fs20 branch runtime-owned plan mutation -->\n"),
    );
    let evidence_source =
        fs::read_to_string(&evidence_path).expect("FS-20 evidence should be readable");
    write_file(
        &evidence_path,
        &format!("{evidence_source}\n<!-- fs20 branch runtime-owned evidence mutation -->\n"),
    );

    let status_after_churn = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-20 status should retain current branch closure after runtime-owned churn",
    );
    assert_eq!(
        status_after_churn["current_branch_closure_id"],
        Value::from(baseline_branch_closure_id),
        "FS-20 runtime-owned plan/evidence churn must not null current_branch_closure_id"
    );
    assert!(
        !status_after_churn["current_release_readiness_state"].is_null(),
        "FS-20 runtime-owned plan/evidence churn must not drop release-readiness state"
    );
}

#[test]
fn fs20_branch_closure_remains_current_when_only_runtime_owned_plan_and_execution_evidence_paths_changed()
 {
    let (repo_dir, state_dir) =
        init_repo("runtime-remediation-fs20-branch-closure-remains-current");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let base_branch = expected_release_base_branch(repo);
    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_dispatched_branch_review_artifact(repo, state, plan_rel, &base_branch);
    let release_path = write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    publish_authoritative_release_truth(repo, state, plan_rel, &release_path, &base_branch);

    let baseline_status = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-20 baseline branch-closure-current status",
    );
    let baseline_branch_closure_id = baseline_status["current_branch_closure_id"]
        .as_str()
        .expect("FS-20 baseline status should expose branch closure id")
        .to_owned();
    let baseline_release_state = baseline_status["current_release_readiness_state"].clone();
    let baseline_final_state = baseline_status["current_final_review_state"].clone();
    let baseline_qa_state = baseline_status["current_qa_state"].clone();
    let evidence_path = repo.join(
        baseline_status["evidence_path"]
            .as_str()
            .expect("FS-20 baseline status should expose evidence path"),
    );
    let plan_path = repo.join(plan_rel);
    let plan_source = fs::read_to_string(&plan_path).expect("FS-20 plan should be readable");
    write_file(
        &plan_path,
        &format!("{plan_source}\n<!-- fs20 branch-current runtime-owned plan mutation -->\n"),
    );
    let evidence_source =
        fs::read_to_string(&evidence_path).expect("FS-20 evidence should be readable");
    write_file(
        &evidence_path,
        &format!(
            "{evidence_source}\n<!-- fs20 branch-current runtime-owned evidence mutation -->\n"
        ),
    );

    let status_after_churn = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-20 branch closure should remain current when only runtime-owned paths changed",
    );
    assert_eq!(
        status_after_churn["current_branch_closure_id"],
        Value::from(baseline_branch_closure_id),
        "FS-20 branch closure should remain current when only runtime-owned plan/evidence paths changed"
    );
    assert_eq!(
        status_after_churn["current_release_readiness_state"], baseline_release_state,
        "FS-20 runtime-owned plan/evidence churn must not alter release-readiness state"
    );
    assert_eq!(
        status_after_churn["current_final_review_state"], baseline_final_state,
        "FS-20 runtime-owned plan/evidence churn must not alter final-review state"
    );
    assert_eq!(
        status_after_churn["current_qa_state"], baseline_qa_state,
        "FS-20 runtime-owned plan/evidence churn must not alter QA state"
    );
    assert_ne!(
        status_after_churn["review_state_status"],
        Value::from("missing_current_closure"),
        "FS-20 runtime-owned plan/evidence churn must not produce missing_current_closure reroute"
    );
}

#[test]
fn fs21_operator_status_and_exact_command_all_agree_on_close_current_task_when_bridge_is_ready() {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs21-route-parity-close-current");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-04-10-runtime-fs21-route-parity-close-current.md";
    setup_runtime_fs17_fs21_fs22_stale_task1_bridge_fixture(repo, state, plan_rel, Some((2, 1)));

    let operator_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "FS-21 workflow operator bridge-preempts-resume fixture",
        ),
        "FS-21 workflow operator bridge-preempts-resume fixture",
    );
    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-21 status bridge-preempts-resume fixture",
    );
    let repair_json = run_plan_execution_json(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "FS-21 repair-review-state bridge-preempts-resume fixture",
    );
    assert_public_route_parity(&operator_json, &status_json, None);
    assert_eq!(
        status_json["phase_detail"],
        Value::from("task_closure_recording_ready"),
        "FS-21 stale-boundary bridge fixture should route to task_closure_recording_ready: {status_json:?}"
    );
    assert!(status_json["resume_task"].is_null());
    assert!(status_json["resume_step"].is_null());
    let operator_command = operator_json["recommended_command"]
        .as_str()
        .expect("FS-21 operator should expose recommended command")
        .to_owned();
    assert!(
        operator_command.contains("close-current-task") && operator_command.contains("--task 1"),
        "FS-21 operator should route to close-current-task --task 1 when bridge preempts resume, got {operator_command}"
    );
    assert_eq!(
        repair_json["phase_detail"],
        Value::from("task_closure_recording_ready")
    );
    assert_eq!(
        repair_json["recommended_command"],
        Value::from(operator_command),
        "FS-21 operator/status/repair surfaces must agree on the exact closure-recording command"
    );
}

#[test]
fn fs22_repair_review_state_prefers_non_destructive_closure_bridge_over_reentry_cleanup() {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs22-bridge-first-repair");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-04-11-runtime-fs22-bridge-first-repair.md";
    setup_runtime_fs17_fs21_fs22_stale_task1_bridge_fixture(repo, state, plan_rel, None);

    let repair_json = run_plan_execution_json(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "FS-22 repair-review-state bridge-first non-destructive fixture",
    );
    assert_eq!(repair_json["action"], Value::from("blocked"));
    assert_eq!(
        repair_json["phase_detail"],
        Value::from("task_closure_recording_ready"),
        "FS-22 repair-review-state must promote to task_closure_recording_ready when closure bridge is available: {repair_json:?}"
    );
    assert_eq!(
        repair_json["required_follow_up"],
        Value::Null,
        "FS-22 closure bridge lane must not keep an execution_reentry follow-up"
    );
    let recommended_command = repair_json["recommended_command"]
        .as_str()
        .expect("FS-22 repair should expose recommended closure-recording command");
    assert!(
        recommended_command.contains("close-current-task")
            && recommended_command.contains("--task 1"),
        "FS-22 repair should route directly to close-current-task --task 1, got {recommended_command}"
    );
    let actions = repair_json["actions_performed"]
        .as_array()
        .expect("FS-22 repair should expose actions_performed array");
    assert!(
        actions.iter().all(|action| {
            action.as_str().is_some_and(|action| {
                !action.starts_with("cleared_task_review_dispatch_lineage")
                    && !action.starts_with("cleared_current_task_closure_scope")
                    && !action.starts_with("cleared_current_task_closure_task")
            })
        }),
        "FS-22 bridge-first repair must not run destructive task-scope/dispatch-lineage cleanup actions: {repair_json:?}"
    );
}

#[test]
fn runtime_remediation_fs16_current_positive_task_closure_allows_next_task_begin_even_if_receipts_need_projection_refresh()
 {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs16-workflow-runtime");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-04-05-runtime-fs16-workflow-runtime.md";
    let dispatch_id = setup_runtime_fs14_fs16_task_boundary_fixture(repo, state, plan_rel);

    let review_summary_path =
        repo.join("docs/featureforge/execution-evidence/fs16-review-summary.md");
    let verification_summary_path =
        repo.join("docs/featureforge/execution-evidence/fs16-verification-summary.md");
    write_file(
        &review_summary_path,
        "FS-16 close-current-task review summary fixture.\n",
    );
    write_file(
        &verification_summary_path,
        "FS-16 close-current-task verification summary fixture.\n",
    );
    let close_json = run_plan_execution_json(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--dispatch-id",
            &dispatch_id,
            "--review-result",
            "pass",
            "--review-summary-file",
            review_summary_path
                .to_str()
                .expect("FS-16 review summary path should be utf-8"),
            "--verification-result",
            "pass",
            "--verification-summary-file",
            verification_summary_path
                .to_str()
                .expect("FS-16 verification summary path should be utf-8"),
        ],
        "FS-16 close-current-task should record current positive closure before projection drift",
    );
    assert_eq!(
        close_json["action"],
        Value::from("recorded"),
        "FS-16 fixture should establish a current positive task closure before projection drift"
    );
    let status_after_close = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-16 status after recording current positive closure",
    );
    let execution_run_id = status_after_close["execution_run_id"]
        .as_str()
        .expect("FS-16 status should expose execution_run_id after close-current-task")
        .to_owned();

    let branch = current_branch_name(repo);
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[("strategy_review_dispatch_lineage", json!({}))],
    );

    for step in [1_u32, 2_u32] {
        let unit_review_receipt = harness_authoritative_artifact_path(
            state,
            &repo_slug(repo),
            &branch,
            &format!("unit-review-{execution_run_id}-task-1-step-{step}.md"),
        );
        if unit_review_receipt.is_file() {
            fs::remove_file(&unit_review_receipt).unwrap_or_else(|error| {
                panic!(
                    "FS-16 should be able to remove stale unit-review projection `{}`: {error}",
                    unit_review_receipt.display()
                )
            });
        }
    }
    let verification_receipt = harness_authoritative_artifact_path(
        state,
        &repo_slug(repo),
        &branch,
        &format!("task-verification-{execution_run_id}-task-1.md"),
    );
    if verification_receipt.is_file() {
        fs::remove_file(&verification_receipt).unwrap_or_else(|error| {
            panic!(
                "FS-16 should be able to remove stale verification projection `{}`: {error}",
                verification_receipt.display()
            )
        });
    }

    let status_after_projection_drift = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-16 status after dispatch/receipt projection drift",
    );
    let begin_task2 = run_plan_execution_json(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "2",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_after_projection_drift["execution_fingerprint"]
                .as_str()
                .expect("FS-16 status should expose execution fingerprint before begin"),
        ],
        "FS-16 begin task 2 should remain legal when Task 1 has a current positive closure despite dispatch/receipt projection drift",
    );
    assert_eq!(begin_task2["active_task"], Value::from(2_u64));
    assert_eq!(begin_task2["active_step"], Value::from(1_u64));
}

#[test]
fn runtime_remediation_fs14_projection_refresh_only_routes_to_close_current_task() {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs14-projection-refresh-route");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-04-03-runtime-fs14-projection-refresh-route.md";
    let task1_dispatch_id = setup_runtime_fs14_fs16_task_boundary_fixture(repo, state, plan_rel);

    let task1_review_summary_path =
        repo.join("docs/featureforge/execution-evidence/fs14-projection-task1-review.md");
    let task1_verification_summary_path =
        repo.join("docs/featureforge/execution-evidence/fs14-projection-task1-verification.md");
    write_file(
        &task1_review_summary_path,
        "FS-14 projection-refresh fixture task 1 review summary.\n",
    );
    write_file(
        &task1_verification_summary_path,
        "FS-14 projection-refresh fixture task 1 verification summary.\n",
    );
    let close_task1 = run_plan_execution_json(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--dispatch-id",
            &task1_dispatch_id,
            "--review-result",
            "pass",
            "--review-summary-file",
            task1_review_summary_path
                .to_str()
                .expect("FS-14 projection-refresh fixture task 1 review summary path"),
            "--verification-result",
            "pass",
            "--verification-summary-file",
            task1_verification_summary_path
                .to_str()
                .expect("FS-14 projection-refresh fixture task 1 verification summary path"),
        ],
        "FS-14 projection-refresh fixture close task 1",
    );
    assert_eq!(close_task1["action"], Value::from("recorded"));

    let status_before_task2_begin = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-14 projection-refresh fixture status before task 2 begin",
    );
    let begin_task2_step1 = run_plan_execution_json(
        repo,
        state,
        &[
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "2",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_before_task2_begin["execution_fingerprint"]
                .as_str()
                .expect("FS-14 projection-refresh fixture status should expose fingerprint"),
        ],
        "FS-14 projection-refresh fixture begin task 2 step 1",
    );
    let complete_task2_step1 = run_plan_execution_json(
        repo,
        state,
        &[
            "complete",
            "--plan",
            plan_rel,
            "--task",
            "2",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "FS-14 projection-refresh fixture completed task 2 step 1.",
            "--manual-verify-summary",
            "FS-14 projection-refresh fixture task 2 step 1 verification summary.",
            "--file",
            "tests/workflow_runtime.rs",
            "--expect-execution-fingerprint",
            begin_task2_step1["execution_fingerprint"]
                .as_str()
                .expect("FS-14 projection-refresh fixture begin task 2 should expose fingerprint"),
        ],
        "FS-14 projection-refresh fixture complete task 2 step 1",
    );
    let dispatch_task2 = run_runtime_review_dispatch_authority_json(
        repo,
        state,
        plan_rel,
        ReviewDispatchScopeArg::Task,
        Some(2),
        "FS-14 projection-refresh fixture record task 2 dispatch",
    );
    assert_eq!(dispatch_task2["allowed"], Value::Bool(true));
    let task2_dispatch_id = dispatch_task2["dispatch_id"]
        .as_str()
        .expect("FS-14 projection-refresh fixture task 2 dispatch should expose id")
        .to_owned();
    let task2_review_summary_path =
        repo.join("docs/featureforge/execution-evidence/fs14-projection-task2-review.md");
    let task2_verification_summary_path =
        repo.join("docs/featureforge/execution-evidence/fs14-projection-task2-verification.md");
    write_file(
        &task2_review_summary_path,
        "FS-14 projection-refresh fixture task 2 review summary.\n",
    );
    write_file(
        &task2_verification_summary_path,
        "FS-14 projection-refresh fixture task 2 verification summary.\n",
    );
    let close_task2 = run_plan_execution_json(
        repo,
        state,
        &[
            "close-current-task",
            "--plan",
            plan_rel,
            "--task",
            "2",
            "--dispatch-id",
            &task2_dispatch_id,
            "--review-result",
            "pass",
            "--review-summary-file",
            task2_review_summary_path
                .to_str()
                .expect("FS-14 projection-refresh fixture task 2 review summary path"),
            "--verification-result",
            "pass",
            "--verification-summary-file",
            task2_verification_summary_path
                .to_str()
                .expect("FS-14 projection-refresh fixture task 2 verification summary path"),
        ],
        "FS-14 projection-refresh fixture close task 2 before projection drift",
    );
    assert_eq!(
        close_task2["action"],
        Value::from("recorded"),
        "FS-14 projection-refresh fixture close task 2 before projection drift should record a current closure, got {close_task2:?}"
    );

    let execution_run_id = complete_task2_step1["execution_run_id"]
        .as_str()
        .expect("FS-14 projection-refresh fixture complete task 2 should expose run id")
        .to_owned();
    let branch = current_branch_name(repo);
    let unit_review_receipt = harness_authoritative_artifact_path(
        state,
        &repo_slug(repo),
        &branch,
        &format!("unit-review-{execution_run_id}-task-2-step-1.md"),
    );
    if unit_review_receipt.is_file() {
        fs::remove_file(&unit_review_receipt).unwrap_or_else(|error| {
            panic!(
                "FS-14 projection-refresh fixture should remove task 2 unit review receipt `{}`: {error}",
                unit_review_receipt.display()
            )
        });
    }
    let verification_receipt = harness_authoritative_artifact_path(
        state,
        &repo_slug(repo),
        &branch,
        &format!("task-verification-{execution_run_id}-task-2.md"),
    );
    if verification_receipt.is_file() {
        fs::remove_file(&verification_receipt).unwrap_or_else(|error| {
            panic!(
                "FS-14 projection-refresh fixture should remove task 2 verification receipt `{}`: {error}",
                verification_receipt.display()
            )
        });
    }

    let operator_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &[
                "workflow",
                "operator",
                "--plan",
                plan_rel,
                "--external-review-result-ready",
                "--json",
            ],
            &[],
            "FS-14 projection-refresh fixture workflow operator",
        ),
        "FS-14 projection-refresh fixture workflow operator",
    );
    let status_json = run_plan_execution_json(
        repo,
        state,
        &[
            "status",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
        ],
        "FS-14 projection-refresh fixture status",
    );
    assert_public_route_parity(&operator_json, &status_json, None);
    assert_eq!(
        operator_json["phase"],
        Value::from("task_closure_pending"),
        "FS-14 projection-refresh fixture should stay in task_closure_pending when projection-only refresh is required, got {operator_json:?}"
    );
    assert_eq!(
        operator_json["phase_detail"],
        Value::from("task_closure_recording_ready")
    );
    assert_eq!(
        operator_json["next_action"],
        Value::from("close current task")
    );
    assert_eq!(operator_json["blocking_task"], Value::from(2_u64));
    let recommended_command = operator_json["recommended_command"]
        .as_str()
        .expect("FS-14 projection-refresh fixture should expose recommended command");
    assert!(
        recommended_command.contains("close-current-task")
            && recommended_command.contains("--task 2"),
        "FS-14 projection-refresh fixture should route receipt projection repair through close-current-task for Task 2, got {recommended_command}"
    );

    let repair_json = run_plan_execution_json(
        repo,
        state,
        &[
            "repair-review-state",
            "--plan",
            plan_rel,
            "--external-review-result-ready",
        ],
        "FS-14 projection-refresh fixture repair-review-state",
    );
    assert_eq!(repair_json["action"], Value::from("blocked"));
    assert_eq!(
        repair_json["phase_detail"],
        Value::from("task_closure_recording_ready")
    );
    assert_eq!(repair_json["required_follow_up"], Value::Null);
    assert_eq!(
        repair_json["recommended_command"],
        operator_json["recommended_command"]
    );
    let routed_follow_up = run_recommended_plan_execution_command(
        repo,
        state,
        recommended_command,
        "FS-14 projection-refresh fixture execute operator-routed command",
    );
    if routed_follow_up["action"].as_str() == Some("blocked") {
        assert_follow_up_blocker_parity_with_operator(
            &operator_json,
            &routed_follow_up,
            "FS-14 projection-refresh command-follow parity",
        );
    } else {
        assert!(
            matches!(
                routed_follow_up["action"].as_str(),
                Some("recorded" | "already_current")
            ),
            "FS-14 projection-refresh routed close-current-task should regenerate missing projections without hidden helpers, got {routed_follow_up:?}"
        );
    }
}

#[test]
fn runtime_remediation_fs09_repair_exposes_next_blocker_immediately() {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs09-workflow-runtime");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let branch = current_branch_name(repo);

    complete_workflow_fixture_execution(repo, state, plan_rel);
    seed_current_branch_closure_truth(repo, state, plan_rel, 1);
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            (
                "current_branch_closure_reviewed_state_id",
                Value::from("git_tree:not-a-tree"),
            ),
            (
                "review_state_repair_follow_up",
                Value::from("execution_reentry"),
            ),
        ],
    );
    update_current_history_record_field(
        repo,
        state,
        "branch_closure_records",
        "current_branch_closure_id",
        "reviewed_state_id",
        Value::from("git_tree:not-a-tree"),
    );

    let repair_json = run_plan_execution_json(
        repo,
        state,
        &["repair-review-state", "--plan", plan_rel],
        "FS-09 repair-review-state post-stale-blocker fixture",
    );
    let repair_action = repair_json["action"]
        .as_str()
        .expect("FS-09 repair-review-state should expose action")
        .to_owned();
    let operator_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "FS-09 workflow operator post-repair fixture",
        ),
        "FS-09 workflow operator post-repair fixture",
    );
    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-09 plan execution status post-repair fixture",
    );
    assert_public_route_parity(&operator_json, &status_json, None);
    if repair_action == "blocked" {
        let operator_recommended_command = operator_json["recommended_command"]
            .as_str()
            .expect("FS-09 workflow operator should expose recommended_command");
        if let Some(required_follow_up) = repair_json["required_follow_up"].as_str() {
            let routed_follow_up = run_recommended_plan_execution_command(
                repo,
                state,
                operator_recommended_command,
                "FS-09 run workflow-operator recommended command directly",
            );
            assert_follow_up_blocker_parity_with_operator(
                &operator_json,
                &routed_follow_up,
                "FS-09 command-follow parity",
            );
            if routed_follow_up["action"].as_str() == Some("blocked") {
                let routed_required_follow_up = routed_follow_up["required_follow_up"]
                    .as_str()
                    .expect("FS-09 blocked routed command should expose required_follow_up");
                assert_eq!(
                    required_follow_up, routed_required_follow_up,
                    "FS-09 blocked repair should surface the exact required_follow_up lane returned by the routed public command"
                );
            } else {
                assert!(
                    !required_follow_up.trim().is_empty(),
                    "FS-09 blocked repair should always expose a concrete required_follow_up lane"
                );
            }
        } else {
            assert_eq!(
                repair_json["phase_detail"],
                Value::from("task_closure_recording_ready"),
                "FS-09 blocked repair may omit required_follow_up only when it has already advanced to the direct public task-closure route: {repair_json:?}"
            );
            assert!(
                operator_recommended_command.contains("plan execution close-current-task --plan "),
                "FS-09 blocked repair without required_follow_up must expose the direct close-current-task command through workflow/operator: {operator_json:?}"
            );
        }
    } else {
        assert_eq!(
            repair_action, "reconciled",
            "FS-09 repair-review-state should either fail closed with a follow-up or reconcile in-place, got {repair_json:?}"
        );
        assert_eq!(
            repair_json["required_follow_up"],
            Value::Null,
            "FS-09 reconciled repair-review-state should not emit a required_follow_up lane"
        );
        assert!(
            repair_json["actions_performed"]
                .as_array()
                .is_some_and(|actions| !actions.is_empty()),
            "FS-09 reconciled repair-review-state should report the reconciliation work it performed"
        );
    }
}

#[test]
fn runtime_remediation_fs10_stale_follow_up_is_ignored_when_truth_is_current() {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs10-workflow-runtime");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    let base_branch = expected_release_base_branch(repo);
    let branch = current_branch_name(repo);

    complete_workflow_fixture_execution(repo, state, plan_rel);
    write_branch_test_plan_artifact(repo, state, plan_rel, "no");
    write_dispatched_branch_review_artifact(repo, state, plan_rel, &base_branch);
    let release_path = write_branch_release_artifact(repo, state, plan_rel, &base_branch);
    publish_authoritative_release_truth(repo, state, plan_rel, &release_path, &base_branch);
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[
            (
                "review_state_repair_follow_up",
                Value::from("execution_reentry"),
            ),
            ("harness_phase", Value::from("ready_for_branch_completion")),
        ],
    );

    let operator_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "FS-10 workflow operator stale-follow-up ignore fixture",
        ),
        "FS-10 workflow operator stale-follow-up ignore fixture",
    );
    let status_json = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-10 plan execution status stale-follow-up ignore fixture",
    );
    assert_public_route_parity(&operator_json, &status_json, None);
    assert_eq!(
        operator_json["phase"],
        Value::from("ready_for_branch_completion"),
        "FS-10 stale persisted follow-up must not override live current truth"
    );
    assert_eq!(
        operator_json["next_action"],
        Value::from("finish branch"),
        "FS-10 stale persisted follow-up must not reroute away from ready-for-branch-completion"
    );
}

#[test]
fn runtime_remediation_fs12_authoritative_run_identity_beats_preflight_for_begin_and_operator() {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs12-workflow-runtime");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    complete_workflow_fixture_execution(repo, state, plan_rel);
    let preflight_path = preflight_acceptance_state_path(repo, state);
    assert!(
        preflight_path.is_file(),
        "FS-12 fixture should have preflight acceptance before deleting it"
    );
    fs::remove_file(&preflight_path)
        .expect("FS-12 should be able to remove preflight acceptance fixture state");
    write_file(
        &preflight_path,
        "{ malformed preflight acceptance fixture for FS-12",
    );

    let status_before_reopen = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-12 status before reopen after deleting preflight acceptance",
    );
    let reopened = run_plan_execution_json(
        repo,
        state,
        &[
            "reopen",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--reason",
            "FS-12 reopen to force a begin path after preflight deletion.",
            "--expect-execution-fingerprint",
            status_before_reopen["execution_fingerprint"]
                .as_str()
                .expect("FS-12 status should expose execution fingerprint before reopen"),
        ],
        "FS-12 reopen should succeed after deleting preflight acceptance",
    );
    assert!(
        reopened["execution_run_id"].as_str().is_some(),
        "FS-12 reopen status should still surface authoritative execution_run_id: {reopened}"
    );

    let operator_json = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "FS-12 workflow operator after deleting preflight acceptance",
        ),
        "FS-12 workflow operator after deleting preflight acceptance",
    );
    assert_ne!(
        operator_json["next_action"],
        Value::from("execution preflight"),
        "FS-12 operator should not regress to execution preflight when authoritative run identity exists"
    );
    let recommended_command = operator_json["recommended_command"]
        .as_str()
        .expect("FS-12 workflow operator should expose recommended command");
    assert!(
        recommended_command.starts_with("featureforge plan execution begin --plan "),
        "FS-12 workflow operator should surface a begin command when authoritative run identity exists, got {recommended_command}"
    );

    let resumed = run_recommended_plan_execution_command(
        repo,
        state,
        recommended_command,
        "FS-12 run workflow/operator-surfaced begin command after deleting preflight acceptance",
    );
    if resumed["action"].as_str() == Some("blocked") {
        assert_follow_up_blocker_parity_with_operator(
            &operator_json,
            &resumed,
            "FS-12 operator command-follow parity after deleting preflight acceptance",
        );
    } else {
        assert_eq!(resumed["active_task"], Value::from(1_u64));
        assert_eq!(resumed["active_step"], Value::from(1_u64));
        assert!(
            resumed["execution_run_id"].as_str().is_some(),
            "FS-12 begin status should preserve authoritative execution_run_id"
        );
    }
}

#[test]
fn runtime_remediation_fs13_markdown_note_is_projection_not_authority() {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs13-workflow-runtime");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-04-02-runtime-fs13-workflow-runtime.md";
    setup_runtime_fs11_fs15_next_action_fixture(repo, state, plan_rel);

    let plan_path = repo.join(plan_rel);
    replace_in_file(
        &plan_path,
        "- [ ] **Step 1: Execute task 6 baseline step**",
        "- [ ] **Step 1: Execute task 6 baseline step**\n  **Execution Note:** Interrupted - FS-13 projection baseline",
    );
    let operator_before_tamper = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "FS-13 operator before manual markdown note tamper",
        ),
        "FS-13 operator before manual markdown note tamper",
    );
    if let Some(blocking_task) = operator_before_tamper["blocking_task"].as_u64() {
        assert_eq!(
            blocking_task, 2_u64,
            "FS-13 operator should expose Task 2 as the earliest stale boundary before note tamper: {operator_before_tamper:?}"
        );
    } else {
        assert_eq!(
            operator_before_tamper["execution_command_context"]["task_number"],
            Value::from(2_u64),
            "FS-13 operator should expose Task 2 through execution command context before note tamper: {operator_before_tamper:?}"
        );
    }

    let status_before_tamper = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-13 status before manual markdown note tamper",
    );
    assert_eq!(
        status_before_tamper["blocking_task"],
        Value::from(2_u64),
        "FS-13 status should expose Task 2 as the earliest stale boundary before note tamper"
    );

    replace_in_file(
        &plan_path,
        "  **Execution Note:** Interrupted - FS-13 projection baseline",
        "  **Execution Note:** Blocked - FS-13 manual markdown edit should be projection-only.",
    );
    let status_after_tamper = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-13 status after manual markdown note tamper",
    );
    assert_eq!(
        status_after_tamper["blocking_task"],
        Value::from(2_u64),
        "FS-13 status must keep Task 2 as the earliest stale boundary even when markdown note is edited"
    );

    let operator_after_tamper = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "FS-13 operator after manual markdown note tamper",
        ),
        "FS-13 operator after manual markdown note tamper",
    );
    assert_eq!(
        operator_after_tamper["phase_detail"], operator_before_tamper["phase_detail"],
        "FS-13 operator phase-detail routing must remain authoritative when markdown note text is tampered"
    );
    if let Some(blocking_task) = operator_after_tamper["blocking_task"].as_u64() {
        assert_eq!(
            blocking_task, 2_u64,
            "FS-13 operator should keep Task 2 as the earliest stale boundary after note tamper: {operator_after_tamper:?}"
        );
    } else {
        assert_eq!(
            operator_after_tamper["execution_command_context"]["task_number"],
            Value::from(2_u64),
            "FS-13 operator should keep Task 2 in execution command context after note tamper: {operator_after_tamper:?}"
        );
    }

    replace_in_file(
        &plan_path,
        "  **Execution Note:** Blocked - FS-13 manual markdown edit should be projection-only.",
        "",
    );
    let status_after_projection_delete = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-13 status after deleting markdown note projection",
    );
    assert_eq!(
        status_after_projection_delete["blocking_task"],
        Value::from(2_u64),
        "FS-13 status must preserve Task 2 stale-boundary targeting even when markdown note projection is deleted"
    );

    let operator_after_projection_delete = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "FS-13 operator after deleting markdown note projection",
        ),
        "FS-13 operator after deleting markdown note projection",
    );
    assert_eq!(
        operator_after_projection_delete["phase_detail"], operator_before_tamper["phase_detail"],
        "FS-13 operator phase-detail routing must remain authoritative when markdown note projection is deleted"
    );
    if let Some(blocking_task) = operator_after_projection_delete["blocking_task"].as_u64() {
        assert_eq!(
            blocking_task, 2_u64,
            "FS-13 operator should keep Task 2 as the earliest stale boundary after projection deletion: {operator_after_projection_delete:?}"
        );
    } else {
        assert_eq!(
            operator_after_projection_delete["execution_command_context"]["task_number"],
            Value::from(2_u64),
            "FS-13 operator should keep Task 2 in execution command context after projection deletion: {operator_after_projection_delete:?}"
        );
    }

    let authoritative_state_path =
        harness_state_path(state, &repo_slug(repo), &current_branch_name(repo));
    let authoritative_state: Value = serde_json::from_str(
        &fs::read_to_string(authoritative_state_path)
            .expect("FS-13 authoritative harness state should remain readable"),
    )
    .expect("FS-13 authoritative harness state should remain valid json");
    assert_eq!(
        authoritative_state["current_open_step_state"]["task"],
        Value::from(6_u64)
    );
    assert_eq!(
        authoritative_state["current_open_step_state"]["step"],
        Value::from(1_u64)
    );
    assert_eq!(
        authoritative_state["current_open_step_state"]["note_state"],
        Value::from("Interrupted")
    );
}

#[test]
fn runtime_remediation_fs13_hidden_gates_do_not_materialize_legacy_open_step_state_when_blocked() {
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs13-hidden-gate-materialization");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    complete_workflow_fixture_execution(repo, state, plan_rel);

    let status_before_reopen = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-13 hidden-gate materialization status before reopen baseline",
    );
    run_plan_execution_json(
        repo,
        state,
        &[
            "reopen",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--reason",
            "FS-13 hidden-gate migration baseline",
            "--expect-execution-fingerprint",
            status_before_reopen["execution_fingerprint"]
                .as_str()
                .expect("FS-13 hidden-gate baseline should expose execution fingerprint"),
        ],
        "FS-13 hidden-gate materialization baseline reopen",
    );

    let branch = current_branch_name(repo);
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[("current_open_step_state", Value::Null)],
    );
    let status_without_authoritative_open_step = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-13 status should ignore legacy markdown open-step note when authoritative current_open_step_state is absent",
    );
    assert_eq!(
        status_without_authoritative_open_step["active_task"],
        Value::Null,
        "FS-13 status must not derive an active task from raw markdown when authoritative current_open_step_state is absent",
    );
    assert_eq!(
        status_without_authoritative_open_step["active_step"],
        Value::Null,
        "FS-13 status must not derive an active step from raw markdown when authoritative current_open_step_state is absent",
    );
    assert_eq!(
        status_without_authoritative_open_step["resume_task"],
        Value::Null,
        "FS-13 status must not derive a resume task from raw markdown when authoritative current_open_step_state is absent",
    );
    assert_eq!(
        status_without_authoritative_open_step["resume_step"],
        Value::Null,
        "FS-13 status must not derive a resume step from raw markdown when authoritative current_open_step_state is absent",
    );

    let operator_without_authoritative_open_step = parse_json(
        &run_rust_featureforge_with_env(
            repo,
            state,
            &["workflow", "operator", "--plan", plan_rel, "--json"],
            &[],
            "FS-13 operator should ignore legacy markdown open-step note when authoritative state is absent",
        ),
        "FS-13 operator should ignore legacy markdown open-step note when authoritative state is absent",
    );
    let recommended_without_authority =
        operator_without_authoritative_open_step["recommended_command"]
            .as_str()
            .unwrap_or("");
    assert!(
        !recommended_without_authority.contains("--task 1 --step 1"),
        "FS-13 operator must not surface Task 1 Step 1 solely from raw markdown when authoritative current_open_step_state is absent: {operator_without_authoritative_open_step:?}",
    );

    let preflight = run_internal_plan_execution_preflight_json(
        repo,
        state,
        plan_rel,
        "FS-13 hidden-gate preflight blocked lane",
    );
    assert_eq!(preflight["allowed"], Value::Bool(true));
    let authoritative_state_path = harness_state_path(state, &repo_slug(repo), &branch);
    let preflight_state: Value = serde_json::from_str(
        &fs::read_to_string(&authoritative_state_path)
            .expect("FS-13 hidden-gate preflight state should remain readable"),
    )
    .expect("FS-13 hidden-gate preflight state should remain valid json");
    assert!(
        preflight_state["current_open_step_state"].is_null(),
        "FS-13 hidden-gate preflight must not recreate current_open_step_state from raw markdown notes: {preflight_state:?}"
    );

    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[("current_open_step_state", Value::Null)],
    );

    let gate_review = run_internal_plan_execution_gate_review_json(
        repo,
        state,
        plan_rel,
        "FS-13 hidden-gate gate-review blocked lane",
    );
    assert_eq!(gate_review["allowed"], Value::Bool(false));
    let gate_review_state: Value = serde_json::from_str(
        &fs::read_to_string(&authoritative_state_path)
            .expect("FS-13 hidden-gate gate-review state should remain readable"),
    )
    .expect("FS-13 hidden-gate gate-review state should remain valid json");
    assert!(
        gate_review_state["current_open_step_state"].is_null(),
        "FS-13 hidden-gate gate-review must not recreate current_open_step_state from raw markdown notes: {gate_review_state:?}"
    );

    fs::remove_file(&authoritative_state_path).expect(
        "FS-13 hidden-gate missing-state preflight setup should remove authoritative state",
    );
    let preflight_missing_state = run_internal_plan_execution_preflight_json(
        repo,
        state,
        plan_rel,
        "FS-13 hidden-gate preflight blocked lane with missing authoritative state",
    );
    assert_eq!(preflight_missing_state["allowed"], Value::Bool(true));
    let restored_after_preflight: Value =
        serde_json::from_str(&fs::read_to_string(&authoritative_state_path).expect(
            "FS-13 hidden-gate preflight may recreate authoritative state for accepted preflight",
        ))
        .expect("FS-13 hidden-gate preflight state should remain valid json");
    assert!(
        restored_after_preflight["current_open_step_state"].is_null(),
        "FS-13 hidden-gate preflight must not recreate current_open_step_state from raw markdown notes when bootstrapping missing authoritative state: {restored_after_preflight:?}"
    );

    if let Err(error) = fs::remove_file(&authoritative_state_path) {
        assert_eq!(
            error.kind(),
            std::io::ErrorKind::NotFound,
            "FS-13 hidden-gate missing-state gate-review setup should only tolerate an already-missing authoritative state",
        );
    }
    let gate_review_missing_state = run_internal_plan_execution_gate_review_json(
        repo,
        state,
        plan_rel,
        "FS-13 hidden-gate gate-review blocked lane with missing authoritative state",
    );
    assert_eq!(gate_review_missing_state["allowed"], Value::Bool(false));
    if authoritative_state_path.exists() {
        let restored_after_gate_review: Value = serde_json::from_str(
            &fs::read_to_string(&authoritative_state_path)
                .expect("FS-13 hidden-gate gate-review recreated state should remain readable"),
        )
        .expect("FS-13 hidden-gate gate-review recreated state should remain valid json");
        assert!(
            restored_after_gate_review["current_open_step_state"].is_null(),
            "FS-13 hidden-gate gate-review must not recreate current_open_step_state from raw markdown notes when bootstrapping missing authoritative state: {restored_after_gate_review:?}"
        );
    }
}

#[test]
fn runtime_remediation_fs13_hidden_gates_fail_closed_on_malformed_authoritative_harness_state() {
    let (repo_dir, state_dir) =
        init_repo("runtime-remediation-fs13-hidden-gates-malformed-harness-state");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    complete_workflow_fixture_execution(repo, state, plan_rel);

    let branch = current_branch_name(repo);
    let authoritative_state_path = harness_state_path(state, &repo_slug(repo), &branch);
    write_file(&authoritative_state_path, "{ this is not valid json }");

    for (output, context) in [
        (
            run_internal_plan_execution_output(
                plan_execution_direct_support::run_runtime_preflight_gate_json(
                    repo,
                    state,
                    &PlanExecutionStatusArgs {
                        plan: plan_rel.into(),
                        external_review_result_ready: false,
                    },
                ),
            ),
            "FS-13 malformed authoritative harness state should fail closed in hidden preflight",
        ),
        (
            run_internal_plan_execution_output(
                plan_execution_direct_support::run_runtime_review_gate_json(
                    repo,
                    state,
                    &PlanExecutionStatusArgs {
                        plan: plan_rel.into(),
                        external_review_result_ready: false,
                    },
                ),
            ),
            "FS-13 malformed authoritative harness state should fail closed in hidden gate-review",
        ),
    ] {
        assert!(
            !output.status.success(),
            "{context} should not succeed when authoritative harness state is malformed"
        );
        let failure_payload = if output.stderr.is_empty() {
            &output.stdout
        } else {
            &output.stderr
        };
        let failure_json: Value =
            serde_json::from_slice(failure_payload).expect("failure payload should be valid json");
        assert_eq!(failure_json["error_class"], "MalformedExecutionState");
        assert!(
            failure_json["message"].as_str().is_some_and(|message| {
                message.contains("Authoritative harness state is malformed")
            }),
            "{context} failure should mention malformed authoritative harness state, got {failure_json:?}"
        );
    }
}

#[test]
fn runtime_remediation_fs13_mutation_fails_closed_on_malformed_authoritative_open_step_state() {
    let (repo_dir, state_dir) =
        init_repo("runtime-remediation-fs13-malformed-authoritative-open-step-state");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    complete_workflow_fixture_execution(repo, state, plan_rel);

    let status_before_reopen = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-13 malformed authoritative open-step baseline status before reopen",
    );
    let reopened = run_plan_execution_json(
        repo,
        state,
        &[
            "reopen",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--reason",
            "FS-13 malformed authoritative open-step baseline",
            "--expect-execution-fingerprint",
            status_before_reopen["execution_fingerprint"]
                .as_str()
                .expect("FS-13 malformed open-step baseline should expose execution fingerprint"),
        ],
        "FS-13 malformed authoritative open-step baseline reopen",
    );
    let branch = current_branch_name(repo);
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[(
            "current_open_step_state",
            serde_json::json!({
                "task": 1,
                "step": 1,
                "note_state": "not-a-state",
                "note_summary": "FS-13 malformed authoritative open-step state",
                "source_plan_path": plan_rel,
                "source_plan_revision": 1,
                "authoritative_sequence": 2
            }),
        )],
    );

    let output = run_rust_featureforge_with_env(
        repo,
        state,
        &[
            "plan",
            "execution",
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            reopened["execution_fingerprint"]
                .as_str()
                .expect("FS-13 malformed open-step reopen should expose execution fingerprint"),
        ],
        &[],
        "FS-13 begin should fail closed on malformed authoritative current_open_step_state",
    );
    assert!(
        !output.status.success(),
        "FS-13 begin must fail closed when authoritative current_open_step_state is malformed"
    );
    let failure_payload = if output.stderr.is_empty() {
        &output.stdout
    } else {
        &output.stderr
    };
    let failure_json: Value = serde_json::from_slice(failure_payload).expect(
        "FS-13 malformed authoritative current_open_step_state failure payload should be valid json",
    );
    assert_eq!(failure_json["error_class"], "MalformedExecutionState");
    assert!(
        failure_json["message"]
            .as_str()
            .is_some_and(|message| message.contains("current_open_step_state")),
        "FS-13 malformed authoritative current_open_step_state failure should mention current_open_step_state, got {failure_json:?}"
    );
}

#[test]
fn runtime_remediation_fs13_status_and_hidden_gates_fail_closed_on_malformed_authoritative_open_step_state()
 {
    let (repo_dir, state_dir) =
        init_repo("runtime-remediation-fs13-malformed-authoritative-open-step-read-paths");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    complete_workflow_fixture_execution(repo, state, plan_rel);

    let status_before_reopen = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-13 malformed open-step read-path baseline status before reopen",
    );
    run_plan_execution_json(
        repo,
        state,
        &[
            "reopen",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--reason",
            "FS-13 malformed read-path baseline",
            "--expect-execution-fingerprint",
            status_before_reopen["execution_fingerprint"]
                .as_str()
                .expect("FS-13 malformed read-path baseline should expose execution fingerprint"),
        ],
        "FS-13 malformed open-step read-path baseline reopen",
    );
    let branch = current_branch_name(repo);
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[(
            "current_open_step_state",
            serde_json::json!({
                "task": 1,
                "step": 1,
                "note_state": "not-a-state",
                "note_summary": "FS-13 malformed read-path authoritative open-step state",
                "source_plan_path": plan_rel,
                "source_plan_revision": 1,
                "authoritative_sequence": 2
            }),
        )],
    );

    for (output, context) in [
        (
            run_rust_featureforge_with_env(
                repo,
                state,
                &["plan", "execution", "status", "--plan", plan_rel],
                &[],
                "FS-13 malformed authoritative current_open_step_state status should fail closed",
            ),
            "FS-13 malformed authoritative current_open_step_state status should fail closed",
        ),
        (
            run_internal_plan_execution_output(
                plan_execution_direct_support::run_runtime_preflight_gate_json(
                    repo,
                    state,
                    &PlanExecutionStatusArgs {
                        plan: plan_rel.into(),
                        external_review_result_ready: false,
                    },
                ),
            ),
            "FS-13 malformed authoritative current_open_step_state preflight should fail closed",
        ),
        (
            run_internal_plan_execution_output(
                plan_execution_direct_support::run_runtime_review_gate_json(
                    repo,
                    state,
                    &PlanExecutionStatusArgs {
                        plan: plan_rel.into(),
                        external_review_result_ready: false,
                    },
                ),
            ),
            "FS-13 malformed authoritative current_open_step_state gate-review should fail closed",
        ),
    ] {
        assert!(
            !output.status.success(),
            "{context} should not succeed when authoritative current_open_step_state is malformed"
        );
        let failure_payload = if output.stderr.is_empty() {
            &output.stdout
        } else {
            &output.stderr
        };
        let failure_json: Value =
            serde_json::from_slice(failure_payload).expect("failure payload should be valid json");
        assert_eq!(failure_json["error_class"], "MalformedExecutionState");
        assert!(
            failure_json["message"]
                .as_str()
                .is_some_and(|message| message.contains("current_open_step_state")),
            "{context} failure should mention current_open_step_state, got {failure_json:?}"
        );
    }
}

#[test]
fn runtime_remediation_fs13_status_fails_closed_on_authoritative_open_step_plan_revision_mismatch()
{
    let (repo_dir, state_dir) =
        init_repo("runtime-remediation-fs13-open-step-plan-revision-mismatch");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    complete_workflow_fixture_execution(repo, state, plan_rel);

    let status_before_reopen = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-13 open-step plan-revision mismatch baseline status before reopen",
    );
    run_plan_execution_json(
        repo,
        state,
        &[
            "reopen",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--reason",
            "FS-13 open-step plan-revision mismatch baseline",
            "--expect-execution-fingerprint",
            status_before_reopen["execution_fingerprint"]
                .as_str()
                .expect("FS-13 open-step mismatch baseline should expose execution fingerprint"),
        ],
        "FS-13 open-step plan-revision mismatch baseline reopen",
    );

    let branch = current_branch_name(repo);
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[(
            "current_open_step_state",
            serde_json::json!({
                "task": 1,
                "step": 1,
                "note_state": "Interrupted",
                "note_summary": "FS-13 plan revision mismatch",
                "source_plan_path": plan_rel,
                "source_plan_revision": 99,
                "authoritative_sequence": 2
            }),
        )],
    );

    let output = run_rust_featureforge_with_env(
        repo,
        state,
        &["plan", "execution", "status", "--plan", plan_rel],
        &[],
        "FS-13 status should fail closed on authoritative open-step source_plan_revision mismatch",
    );
    assert!(
        !output.status.success(),
        "FS-13 status must fail closed when authoritative current_open_step_state source_plan_revision mismatches"
    );
    let failure_payload = if output.stderr.is_empty() {
        &output.stdout
    } else {
        &output.stderr
    };
    let failure_json: Value = serde_json::from_slice(failure_payload)
        .expect("FS-13 open-step mismatch status failure payload should be valid json");
    assert_eq!(failure_json["error_class"], "MalformedExecutionState");
    assert!(
        failure_json["message"]
            .as_str()
            .is_some_and(|message| message.contains("source_plan_revision")),
        "FS-13 open-step mismatch status failure should mention source_plan_revision, got {failure_json:?}"
    );
}

#[test]
fn runtime_remediation_fs13_mutation_fails_closed_on_multiple_legacy_open_step_notes() {
    let (repo_dir, state_dir) =
        init_repo("runtime-remediation-fs13-multiple-legacy-open-step-notes");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";

    complete_workflow_fixture_execution(repo, state, plan_rel);
    let status_before_reopen = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-13 multiple-legacy-note status before reopen baseline",
    );
    let reopened = run_plan_execution_json(
        repo,
        state,
        &[
            "reopen",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--reason",
            "FS-13 multiple legacy note baseline",
            "--expect-execution-fingerprint",
            status_before_reopen["execution_fingerprint"]
                .as_str()
                .expect("FS-13 multiple-legacy-note reopen baseline should expose execution fingerprint"),
        ],
        "FS-13 multiple-legacy-note baseline reopen",
    );

    let plan_path = repo.join(plan_rel);
    insert_step_with_execution_note_after_step(
        &plan_path,
        1,
        1,
        2,
        "FS-13 injected secondary step",
        "Blocked - FS-13 injected secondary legacy note",
    );

    let branch = current_branch_name(repo);
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[("current_open_step_state", Value::Null)],
    );

    let status_with_multiple_notes = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-13 multiple-legacy-note status before mutation failure assertion",
    );
    assert_eq!(
        reopened["resume_task"],
        Value::from(1_u64),
        "FS-13 multiple-legacy-note reopen baseline should expose resume target"
    );
    let output = run_rust_featureforge_with_env(
        repo,
        state,
        &[
            "plan",
            "execution",
            "note",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--state",
            "blocked",
            "--message",
            "FS-13 multiple legacy note fail-closed check",
            "--expect-execution-fingerprint",
            status_with_multiple_notes["execution_fingerprint"]
                .as_str()
                .expect("FS-13 multiple-legacy-note status should expose execution fingerprint"),
        ],
        &[],
        "FS-13 note should fail closed when multiple legacy open-step notes exist",
    );
    assert!(
        !output.status.success(),
        "FS-13 note must fail closed when multiple legacy open-step notes are present"
    );
    let failure_payload = if output.stderr.is_empty() {
        &output.stdout
    } else {
        &output.stderr
    };
    let failure_json: Value = serde_json::from_slice(failure_payload)
        .expect("FS-13 multiple legacy open-step note failure payload should be valid json");
    assert_eq!(failure_json["error_class"], "InvalidCommandInput");
    assert!(
        failure_json["message"]
            .as_str()
            .is_some_and(|message| message.contains("unrecognized subcommand 'note'")),
        "FS-13 multiple legacy open-step note failure should reflect the removed public command surface, got {failure_json:?}"
    );
}

#[test]
fn runtime_remediation_fs13_mutation_fails_closed_without_persisting_checked_legacy_open_step_note()
{
    let (repo_dir, state_dir) = init_repo("runtime-remediation-fs13-checked-legacy-open-step-note");
    let repo = repo_dir.path();
    let state = state_dir.path();
    let plan_rel = "docs/featureforge/plans/2026-03-22-runtime-integration-hardening.md";
    complete_workflow_fixture_execution(repo, state, plan_rel);

    let status_before_reopen = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-13 checked-note status before reopen baseline",
    );
    run_plan_execution_json(
        repo,
        state,
        &[
            "reopen",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--reason",
            "FS-13 checked-note baseline",
            "--expect-execution-fingerprint",
            status_before_reopen["execution_fingerprint"]
                .as_str()
                .expect("FS-13 checked-note reopen baseline should expose execution fingerprint"),
        ],
        "FS-13 checked-note baseline reopen",
    );

    let plan_path = repo.join(plan_rel);
    replace_in_file(
        &plan_path,
        "- [ ] **Step 1: Validate full approved-plan readiness**",
        "- [x] **Step 1: Validate full approved-plan readiness**",
    );

    let branch = current_branch_name(repo);
    update_authoritative_harness_state(
        repo,
        state,
        &branch,
        plan_rel,
        1,
        &[("current_open_step_state", Value::Null)],
    );

    let status_before_begin = run_plan_execution_json(
        repo,
        state,
        &["status", "--plan", plan_rel],
        "FS-13 checked-note status before begin failure assertion",
    );
    let output = run_rust_featureforge_with_env(
        repo,
        state,
        &[
            "plan",
            "execution",
            "begin",
            "--plan",
            plan_rel,
            "--task",
            "1",
            "--step",
            "1",
            "--execution-mode",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            status_before_begin["execution_fingerprint"]
                .as_str()
                .expect("FS-13 checked-note status should expose execution fingerprint"),
        ],
        &[],
        "FS-13 begin should fail closed on checked legacy open-step note candidate",
    );
    assert!(
        !output.status.success(),
        "FS-13 begin must fail closed when legacy open-step note points at a completed step"
    );
    let failure_payload = if output.stderr.is_empty() {
        &output.stdout
    } else {
        &output.stderr
    };
    let failure_json: Value = serde_json::from_slice(failure_payload)
        .expect("FS-13 checked-note failure payload should be valid json");
    assert_eq!(failure_json["error_class"], "InvalidStepTransition");
    assert!(
        failure_json["message"].as_str().is_some_and(|message| {
            message.contains("next legal action")
                && message.contains("featureforge plan execution")
                && !message.contains("legacy open-step")
        }),
        "FS-13 checked-note failure should come from the shared next-action guard without materializing markdown-note truth, got {failure_json:?}"
    );

    let authoritative_state_path = harness_state_path(state, &repo_slug(repo), &branch);
    let authoritative_state: Value = serde_json::from_str(
        &fs::read_to_string(&authoritative_state_path)
            .expect("FS-13 checked-note authoritative state should be readable"),
    )
    .expect("FS-13 checked-note authoritative state should remain valid json");
    assert!(
        authoritative_state["current_open_step_state"].is_null(),
        "FS-13 checked-note migration must fail before mutating current_open_step_state"
    );
}

#[test]
fn runtime_remediation_inventory_maps_fs_regressions_to_workflow_runtime() {
    let inventory = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/runtime-remediation/README.md"),
    )
    .expect("runtime-remediation inventory should be readable");
    assert!(
        inventory.contains("## Function-Level Traceability"),
        "runtime-remediation inventory should retain function-level traceability coverage"
    );
    assert!(
        inventory.contains("## Detailed Failure Shapes (Mandatory)"),
        "runtime-remediation inventory should include the mandatory detailed failure-shape section"
    );
    for scenario in [
        "FS-01", "FS-02", "FS-03", "FS-04", "FS-05", "FS-06", "FS-07", "FS-08", "FS-09", "FS-10",
        "FS-11", "FS-12", "FS-13", "FS-14", "FS-15", "FS-16",
    ] {
        assert!(
            inventory.contains(scenario),
            "runtime-remediation inventory should include {scenario}"
        );
    }
    for (scenario, detail_anchor) in [
        ("FS-01", "branch-closure mutation says repair is required"),
        (
            "FS-02",
            "late-stage writes re-stale execution and loop between late-stage refresh and execution reentry",
        ),
        (
            "FS-03",
            "begin requires prior-task redispatch, but dispatch recorder rejects that target",
        ),
        (
            "FS-04",
            "repair mutates state and still leaves the wrong route visible",
        ),
        (
            "FS-05",
            "unsupported-field CLI paths mutate authoritative state before returning an error",
        ),
        (
            "FS-06",
            "helper-backed tests pass but compiled CLI behavior differs",
        ),
        (
            "FS-07",
            "status points to the right blocker, operator still recommends execution reentry / begin",
        ),
        (
            "FS-08",
            "later resume overlays suppress the real stale prerequisite",
        ),
        (
            "FS-09",
            "repair removes one stale layer but fails to surface the next blocker and still says begin",
        ),
        (
            "FS-10",
            "persisted repair follow-up remains stuck on execution reentry or branch refresh",
        ),
        (
            "FS-11",
            "rebased consumer-style fixture with forward reentry overlay pointing at Task 3",
        ),
        (
            "FS-12",
            "authoritative state contains `run_identity.execution_run_id`",
        ),
        (
            "FS-13",
            "authoritative open-step state or legacy markdown note on a later task",
        ),
        (
            "FS-14",
            "completed task with no current task closure baseline",
        ),
        (
            "FS-15",
            "stale tasks 2 and 6 present after earlier repair cleanup",
        ),
        (
            "FS-16",
            "remove or stale receipt projections without changing the reviewed state that closure binds to",
        ),
    ] {
        assert!(
            inventory.contains(detail_anchor),
            "runtime-remediation inventory should retain detailed failure-shape text for {scenario}"
        );
    }
    let required_function_traceability_anchors: [(&str, &[&str]); 16] = [
        (
            "FS-01",
            &[
                "tests/workflow_runtime.rs::runtime_remediation_fs01_shared_route_parity_for_missing_current_closure",
                "tests/workflow_shell_smoke.rs::compiled_cli_route_parity_probe_for_late_stage_refresh_fixture",
                "tests/workflow_shell_smoke.rs::plan_execution_record_release_readiness_primitive_uses_shared_routing_when_stale",
                "tests/workflow_shell_smoke.rs::runtime_remediation_fs01_compiled_cli_repair_and_branch_closure_do_not_disagree",
            ],
        ),
        (
            "FS-02",
            &[
                "tests/workflow_runtime_final_review.rs::fs02_late_stage_drift_routes_consistently_across_operator_and_status",
                "tests/workflow_entry_shell_smoke.rs::fs02_entry_route_surfaces_share_parity_and_budget",
            ],
        ),
        (
            "FS-03",
            &[
                "tests/workflow_runtime.rs::workflow_phase_routes_task_boundary_blocked",
                "tests/plan_execution.rs::runtime_remediation_fs03_compiled_cli_dispatch_target_acceptance_and_mismatch",
                "tests/workflow_shell_smoke.rs::plan_execution_record_review_dispatch_prefers_task_boundary_target_over_interrupted_note_state",
                "tests/contracts_execution_runtime_boundaries.rs::runtime_remediation_fs03_dispatch_target_acceptance_and_mismatch_stay_aligned_between_direct_and_compiled_cli",
            ],
        ),
        (
            "FS-04",
            &[
                "tests/workflow_runtime.rs::runtime_remediation_fs04_repair_returns_route_consumed_by_operator",
                "tests/workflow_runtime.rs::runtime_remediation_fs04_compiled_cli_repair_returns_route_consumed_by_operator",
                "tests/plan_execution.rs::runtime_remediation_fs04_rebuild_evidence_preserves_authoritative_state_digest",
                "tests/contracts_execution_runtime_boundaries.rs::runtime_remediation_fs04_repair_route_visibility_stays_aligned_between_direct_and_compiled_cli",
                "tests/contracts_execution_runtime_boundaries.rs::runtime_remediation_fs04_repair_review_state_accepts_external_review_ready_flag_without_irrelevant_route_drift",
            ],
        ),
        (
            "FS-05",
            &[
                "tests/plan_execution.rs::record_review_dispatch_task_target_mismatch_fails_before_authoritative_mutation",
                "tests/plan_execution.rs::record_review_dispatch_final_review_scope_rejects_task_field_before_authoritative_mutation",
                "tests/plan_execution.rs::record_final_review_rejects_unapproved_reviewer_source_before_mutation",
                "tests/contracts_execution_runtime_boundaries.rs::runtime_remediation_fs05_unsupported_field_fails_before_mutation_on_compatibility_aliases",
            ],
        ),
        (
            "FS-06",
            &[
                "tests/workflow_shell_smoke.rs::fs06_helper_and_compiled_cli_target_mismatch_stay_in_parity",
            ],
        ),
        (
            "FS-07",
            &[
                "tests/execution_query.rs::runtime_remediation_fs07_query_surface_parity_for_task_review_dispatch_blocked",
                "tests/workflow_shell_smoke.rs::fs07_task_review_dispatch_route_parity_in_compiled_cli_surfaces",
            ],
        ),
        (
            "FS-08",
            &[
                "tests/workflow_runtime.rs::runtime_remediation_fs08_resume_overlay_does_not_hide_stale_blocker",
                "tests/workflow_runtime.rs::runtime_remediation_fs08_compiled_cli_resume_overlay_does_not_hide_stale_blocker",
                "tests/contracts_execution_runtime_boundaries.rs::runtime_remediation_fs08_stale_blocker_visibility_stays_aligned_between_direct_and_compiled_cli",
            ],
        ),
        (
            "FS-09",
            &[
                "tests/workflow_runtime.rs::runtime_remediation_fs09_repair_exposes_next_blocker_immediately",
                "tests/workflow_entry_shell_smoke.rs::fs09_repair_surfaces_post_repair_next_blocker_in_entry_cli",
            ],
        ),
        (
            "FS-10",
            &[
                "tests/workflow_runtime.rs::runtime_remediation_fs10_stale_follow_up_is_ignored_when_truth_is_current",
                "tests/workflow_shell_smoke.rs::prerelease_branch_closure_refresh_ignores_stale_execution_reentry_follow_up",
            ],
        ),
        (
            "FS-11",
            &[
                "tests/workflow_runtime.rs::runtime_remediation_fs11_operator_begin_repair_share_one_next_action_engine",
                "tests/workflow_runtime.rs::runtime_remediation_fs11_repair_returns_same_action_as_operator_and_begin",
                "tests/workflow_shell_smoke.rs::fs11_operator_and_begin_target_parity_after_rebase_resume",
                "tests/workflow_shell_smoke.rs::fs11_repair_output_matches_following_public_command_without_hidden_helper",
                "tests/workflow_shell_smoke.rs::fs11_rebase_resume_recovery_budget_is_capped_without_hidden_helpers",
            ],
        ),
        (
            "FS-12",
            &[
                "tests/workflow_runtime.rs::runtime_remediation_fs12_authoritative_run_identity_beats_preflight_for_begin_and_operator",
                "tests/plan_execution.rs::runtime_remediation_fs12_close_current_task_uses_authoritative_run_identity_without_hidden_preflight",
                "tests/workflow_shell_smoke.rs::fs12_recovery_path_does_not_require_hidden_preflight_when_run_identity_exists",
            ],
        ),
        (
            "FS-13",
            &[
                "tests/workflow_runtime.rs::runtime_remediation_fs13_markdown_note_is_projection_not_authority",
                "tests/workflow_runtime.rs::runtime_remediation_fs13_hidden_gates_materialize_legacy_open_step_state_when_blocked",
                "tests/plan_execution.rs::runtime_remediation_fs13_reopen_and_begin_update_authoritative_open_step_state",
                "tests/workflow_shell_smoke.rs::fs13_normal_recovery_never_requires_manual_plan_note_edit",
                "tests/contracts_execution_runtime_boundaries.rs::runtime_remediation_fs13_authoritative_open_step_state_survives_compiled_cli_round_trip",
            ],
        ),
        (
            "FS-14",
            &[
                "tests/workflow_runtime.rs::runtime_remediation_fs14_missing_task_closure_baseline_routes_to_close_current_task_not_execution_reentry",
                "tests/workflow_runtime.rs::runtime_remediation_fs14_repair_routes_missing_task_closure_baseline_to_close_current_task",
                "tests/plan_execution.rs::runtime_remediation_fs14_close_current_task_rebuilds_missing_current_closure_baseline_without_hidden_dispatch",
                "tests/workflow_shell_smoke.rs::fs14_recovery_to_close_current_task_uses_only_public_intent_commands",
            ],
        ),
        (
            "FS-15",
            &[
                "tests/workflow_runtime.rs::runtime_remediation_fs15_earliest_stale_boundary_beats_latest_overlay_target",
                "tests/workflow_runtime.rs::runtime_remediation_fs15_repair_never_jumps_to_later_task_when_earlier_boundary_exists",
                "tests/contracts_execution_runtime_boundaries.rs::runtime_remediation_fs15_compiled_cli_never_prefers_later_stale_task",
            ],
        ),
        (
            "FS-16",
            &[
                "tests/workflow_runtime.rs::runtime_remediation_fs16_current_positive_task_closure_allows_next_task_begin_even_if_receipts_need_projection_refresh",
                "tests/plan_execution.rs::runtime_remediation_fs16_begin_no_longer_reads_prior_task_dispatch_or_receipts",
            ],
        ),
    ];
    for (scenario, anchors) in required_function_traceability_anchors {
        for anchor in anchors {
            assert!(
                inventory.contains(anchor),
                "runtime-remediation inventory should map {scenario} to function-level anchor `{anchor}`"
            );
        }
    }
}
