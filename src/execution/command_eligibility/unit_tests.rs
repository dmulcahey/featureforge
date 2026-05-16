use super::*;
use crate::execution::harness::{
    AggregateEvaluationState, ChunkId, DownstreamFreshnessState, HarnessPhase,
};
use crate::execution::route_plan::{STATE_KIND_PLANNING_REENTRY_REQUIRED, STATE_KIND_TERMINAL};
use crate::execution::status::{PlanExecutionStatus, PublicRepairTarget};

fn bound_inputs(values: &[(&str, &str)]) -> PublicCommandInputValues {
    values
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect()
}

fn late_stage_mutation_status(
    phase_detail: &str,
    recommended_public_command: PublicCommand,
) -> PlanExecutionStatus {
    PlanExecutionStatus {
        schema_version: 3,
        plan_revision: 1,
        execution_run_id: None,
        workspace_state_id: String::from("semantic_tree:test"),
        current_branch_reviewed_state_id: Some(String::from("git_tree:test")),
        current_branch_closure_id: Some(String::from("branch-closure-test")),
        current_branch_meaningful_drift: false,
        current_task_closures: Vec::new(),
        superseded_closures_summary: Vec::new(),
        stale_unreviewed_closures: Vec::new(),
        current_release_readiness_state: Some(String::from("ready")),
        current_final_review_state: String::from("fresh"),
        current_qa_state: String::from("not_required"),
        current_final_review_branch_closure_id: None,
        current_final_review_result: None,
        current_qa_branch_closure_id: None,
        current_qa_result: None,
        qa_requirement: None,
        latest_authoritative_sequence: 1,
        phase: Some(String::from(HarnessPhase::QaPending.as_str())),
        harness_phase: HarnessPhase::QaPending,
        chunk_id: ChunkId(String::from("chunk-test")),
        chunking_strategy: None,
        evaluator_policy: None,
        reset_policy: None,
        review_stack: None,
        active_contract_path: None,
        active_contract_fingerprint: None,
        required_evaluator_kinds: Vec::new(),
        completed_evaluator_kinds: Vec::new(),
        pending_evaluator_kinds: Vec::new(),
        non_passing_evaluator_kinds: Vec::new(),
        aggregate_evaluation_state: AggregateEvaluationState::Pending,
        last_evaluation_report_path: None,
        last_evaluation_report_fingerprint: None,
        last_evaluation_evaluator_kind: None,
        last_evaluation_verdict: None,
        current_chunk_retry_count: 0,
        current_chunk_retry_budget: 0,
        current_chunk_pivot_threshold: 0,
        handoff_required: false,
        open_failed_criteria: Vec::new(),
        write_authority_state: String::from("idle"),
        write_authority_holder: None,
        write_authority_worktree: None,
        repo_state_baseline_head_sha: None,
        repo_state_baseline_worktree_fingerprint: None,
        repo_state_drift_state: String::from("clean"),
        dependency_index_state: String::from("fresh"),
        final_review_state: DownstreamFreshnessState::Fresh,
        browser_qa_state: DownstreamFreshnessState::NotRequired,
        release_docs_state: DownstreamFreshnessState::Fresh,
        last_final_review_artifact_fingerprint: None,
        last_browser_qa_artifact_fingerprint: None,
        last_release_docs_artifact_fingerprint: None,
        strategy_state: String::from("ready"),
        last_strategy_checkpoint_fingerprint: None,
        strategy_checkpoint_kind: String::from("review_remediation"),
        strategy_reset_required: false,
        phase_detail: phase_detail.to_owned(),
        review_state_status: String::from("clean"),
        recording_context: None,
        execution_command_context: None,
        execution_reentry_target_source: None,
        public_repair_targets: Vec::new(),
        blocking_records: Vec::new(),
        blocking_scope: Some(String::from("branch")),
        external_wait_state: None,
        blocking_reason_codes: Vec::new(),
        projection_diagnostics: Vec::new(),
        state_kind: String::from("actionable_public_command"),
        next_public_action: None,
        blockers: Vec::new(),
        runtime_provenance: None,
        semantic_workspace_tree_id: String::from("semantic_tree:test"),
        raw_workspace_tree_id: Some(String::from("git_tree:test")),
        next_action: String::from("advance late stage"),
        recommended_public_command: Some(recommended_public_command),
        recommended_public_command_argv: None,
        recommended_public_command_template: None,
        required_inputs: Vec::new(),
        recommended_command: None,
        finish_review_gate_pass_branch_closure_id: None,
        reason_codes: Vec::new(),
        execution_mode: String::from("featureforge:executing-plans"),
        execution_fingerprint: String::from("fingerprint"),
        evidence_path: String::from("docs/featureforge/execution-evidence/plan-r1-evidence.md"),
        projection_mode: String::from("state_dir_only"),
        state_dir_projection_paths: Vec::new(),
        tracked_projection_paths: Vec::new(),
        tracked_projections_current: false,
        execution_started: String::from("yes"),
        warning_codes: Vec::new(),
        active_task: None,
        active_step: None,
        blocking_task: None,
        blocking_step: None,
        resume_task: None,
        resume_step: None,
    }
}

#[test]
fn public_command_shapes_round_trip_and_drive_mutation_requests() {
    let commands = [
        PublicCommand::WorkflowOperator {
            plan: String::from("docs/plan.md"),
            external_review_result_ready: false,
            json: false,
        },
        PublicCommand::WorkflowOperator {
            plan: String::from("docs/plan.md"),
            external_review_result_ready: true,
            json: true,
        },
        PublicCommand::Status {
            plan: String::from("docs/plan.md"),
        },
        PublicCommand::RepairReviewState {
            plan: String::from("docs/plan.md"),
        },
        PublicCommand::Begin {
            plan: String::from("docs/plan.md"),
            task: 1,
            step: 2,
            execution_mode: Some(String::from("featureforge:executing-plans")),
            fingerprint: Some(String::from("fingerprint")),
        },
        PublicCommand::Complete {
            plan: String::from("docs/plan.md"),
            task: 1,
            step: 2,
            source: Some(String::from("featureforge:executing-plans")),
            fingerprint: Some(String::from("fingerprint")),
        },
        PublicCommand::Reopen {
            plan: String::from("docs/plan.md"),
            task: 1,
            step: 2,
            source: Some(String::from("featureforge:executing-plans")),
            reason: Some(String::from("repair")),
            fingerprint: Some(String::from("fingerprint")),
        },
        PublicCommand::TransferRepairStep {
            plan: String::from("docs/plan.md"),
            task: 1,
            step: 2,
            fingerprint: Some(String::from("fingerprint")),
        },
        PublicCommand::TransferHandoff {
            plan: String::from("docs/plan.md"),
            scope: String::from("task"),
        },
        PublicCommand::CloseCurrentTask {
            plan: String::from("docs/plan.md"),
            task: Some(1),
            result_inputs_required: true,
        },
        PublicCommand::AdvanceLateStage {
            plan: String::from("docs/plan.md"),
            mode: PublicAdvanceLateStageMode::Basic,
        },
        PublicCommand::MaterializeProjectionsStateDirOnly {
            plan: String::from("docs/plan.md"),
            scope: None,
        },
    ];

    for command in commands {
        let display = command.to_display_command();
        if command.to_invocation().is_some() {
            let parsed = PublicCommand::parse_display_command(&display)
                .unwrap_or_else(|| panic!("typed command should parse from `{display}`"));
            assert_eq!(parsed, command, "round trip failed for `{display}`");
            assert!(command_is_legal_public_command(&display));
        } else {
            assert!(
                PublicCommand::parse_display_command(&display).is_none(),
                "missing-input display should not parse as an exact public command: `{display}`"
            );
            assert!(
                !command_is_legal_public_command(&display),
                "missing-input display should not be mutation authority: `{display}`"
            );
        }
    }
}

#[test]
fn malformed_command_suffixes_do_not_pass_public_shape_parsing() {
    let commands = [
        "featureforge plan execution begin --plan docs/plan.md --task 1 --step 2 --expect-execution-fingerprint fp --unexpected",
        "featureforge plan execution close-current-task --plan docs/plan.md --task 1 --review-result pass --review-summary-file review.md --verification-result pass --unexpected",
    ];

    for command in commands {
        assert!(!command_is_legal_public_command(command));
        assert!(public_mutation_request_from_command(command).is_none());
    }
}

#[test]
fn hidden_and_debug_commands_are_unrepresentable_as_typed_public_commands() {
    let commands = vec![
        format!(
            "featureforge plan execution {} --plan docs/plan.md --scope task --task 1",
            ["record", "review", "dispatch"].join("-")
        ),
        format!(
            "featureforge plan execution {} --plan docs/plan.md",
            ["gate", "review"].join("-")
        ),
        format!(
            "featureforge plan execution {} --plan docs/plan.md",
            ["gate", "finish"].join("-")
        ),
        format!(
            "featureforge plan execution {} --plan docs/plan.md",
            ["rebuild", "evidence"].join("-")
        ),
        format!(
            "featureforge {} --plan docs/plan.md",
            ["plan", "execution", "preflight"].join(" ")
        ),
        format!(
            "featureforge plan execution internal {} --plan docs/plan.md",
            ["record", "branch", "closure"].join("-")
        ),
        format!(
            "featureforge {} --plan docs/plan.md",
            ["plan", "execution", "recommend"].join(" ")
        ),
        format!(
            "featureforge plan execution {} --plan docs/plan.md",
            ["reconcile", "review", "state"].join("-")
        ),
        format!(
            "featureforge {} --plan docs/plan.md",
            ["workflow", "preflight"].join(" ")
        ),
        format!(
            "featureforge {} --plan docs/plan.md",
            ["workflow", "recommend"].join(" ")
        ),
    ];

    for command in &commands {
        assert!(
            PublicCommand::parse_display_command(command).is_none(),
            "hidden/debug command must not parse as typed public command: {command}"
        );
        assert!(!command_is_legal_public_command(command));
    }
}

#[test]
fn close_current_task_public_command_accepts_concrete_result_flags() {
    let command = "featureforge plan execution close-current-task --plan docs/plan.md --task 1 --review-result pass --review-summary-file review.md --verification-result pass --verification-summary-file verification.md";

    assert!(command_is_legal_public_command(command));
    assert_eq!(
        public_mutation_request_from_command(command)
            .expect("concrete command should map to public close-current-task mutation")
            .kind,
        PublicCommandKind::CloseCurrentTask
    );
}

#[test]
fn missing_input_commands_do_not_emit_executable_argv() {
    let command = PublicCommand::CloseCurrentTask {
        plan: String::from("docs/plan.md"),
        task: Some(1),
        result_inputs_required: true,
    };

    assert_eq!(
        command.to_display_command(),
        "featureforge plan execution close-current-task --plan docs/plan.md --task 1; requires review and verification inputs"
    );
    assert_eq!(
        recommended_public_command_argv(Some(&command)),
        None,
        "commands with unresolved result inputs must not emit executable argv"
    );
    let template = recommended_public_command_template(Some(&command))
        .expect("input-required commands should expose a non-executable command template");
    assert_eq!(template.command_kind, "close_current_task");
    assert_eq!(
        template.base_argv,
        vec![
            "featureforge",
            "plan",
            "execution",
            "close-current-task",
            "--plan",
            "docs/plan.md",
            "--task",
            "1"
        ]
    );
    assert_eq!(
        template.required_input_names,
        vec![
            "review_result",
            "review_summary_file",
            "verification_result",
            "verification_summary_file"
        ]
    );
    assert_eq!(
        required_inputs_for_public_command(Some(&command))
            .into_iter()
            .map(|input| input.name)
            .collect::<Vec<_>>(),
        vec![
            "review_result",
            "review_summary_file",
            "verification_result",
            "verification_summary_file"
        ]
    );
}

#[test]
fn workflow_operator_recommendation_display_uses_placeholder_while_argv_keeps_plan() {
    let command = PublicCommand::WorkflowOperator {
        plan: String::from("docs/featureforge/plans/plan with spaces.md"),
        external_review_result_ready: false,
        json: true,
    };

    assert_eq!(
        recommended_public_command_display(Some(&command)).as_deref(),
        Some("featureforge workflow operator --plan <approved-plan-path> --json")
    );
    assert_eq!(
        recommended_public_command_argv(Some(&command)),
        Some(vec![
            String::from("featureforge"),
            String::from("workflow"),
            String::from("operator"),
            String::from("--plan"),
            String::from("docs/featureforge/plans/plan with spaces.md"),
            String::from("--json"),
        ])
    );
}

#[test]
fn all_input_required_public_commands_have_explicit_template_bindings() {
    let commands = [
        PublicCommand::Begin {
            plan: String::from("docs/plan.md"),
            task: 1,
            step: 1,
            execution_mode: Some(String::from("featureforge:executing-plans")),
            fingerprint: None,
        },
        PublicCommand::Complete {
            plan: String::from("docs/plan.md"),
            task: 1,
            step: 1,
            source: None,
            fingerprint: None,
        },
        PublicCommand::Reopen {
            plan: String::from("docs/plan.md"),
            task: 1,
            step: 1,
            source: None,
            reason: None,
            fingerprint: None,
        },
        PublicCommand::TransferRepairStep {
            plan: String::from("docs/plan.md"),
            task: 1,
            step: 1,
            fingerprint: None,
        },
        PublicCommand::TransferHandoff {
            plan: String::from("docs/plan.md"),
            scope: String::from("task|branch"),
        },
        PublicCommand::CloseCurrentTask {
            plan: String::from("docs/plan.md"),
            task: None,
            result_inputs_required: false,
        },
        PublicCommand::CloseCurrentTask {
            plan: String::from("docs/plan.md"),
            task: Some(1),
            result_inputs_required: true,
        },
        PublicCommand::AdvanceLateStage {
            plan: String::from("docs/plan.md"),
            mode: PublicAdvanceLateStageMode::ReleaseReadiness,
        },
        PublicCommand::AdvanceLateStage {
            plan: String::from("docs/plan.md"),
            mode: PublicAdvanceLateStageMode::Qa,
        },
        PublicCommand::AdvanceLateStage {
            plan: String::from("docs/plan.md"),
            mode: PublicAdvanceLateStageMode::FinalReview,
        },
    ];

    for command in commands {
        let template = command
            .to_input_template()
            .unwrap_or_else(|| panic!("{command:?} should expose an input template"));
        assert_eq!(
            template.required_input_names,
            template
                .input_bindings
                .iter()
                .map(|input| input.name.clone())
                .collect::<Vec<_>>(),
            "template metadata must cover every required input exactly for {command:?}"
        );
        for input in &template.input_bindings {
            match input.binding.kind {
                PublicCommandInputBindingKind::Flag => {
                    let flag = input
                        .binding
                        .flag
                        .as_deref()
                        .unwrap_or_else(|| panic!("{input:?} must bind to an explicit flag"));
                    assert!(
                        flag.starts_with("--"),
                        "{input:?} must bind to a CLI flag, not guessed display text"
                    );
                }
                PublicCommandInputBindingKind::Virtual => {
                    assert_eq!(
                        input.binding.flag, None,
                        "{input:?} virtual inputs must not emit argv"
                    );
                }
            }
        }
    }
}

#[test]
fn close_current_task_template_materializes_review_and_verification_argv() {
    let command = PublicCommand::CloseCurrentTask {
        plan: String::from("docs/plan.md"),
        task: Some(1),
        result_inputs_required: true,
    };
    let template = command
        .to_input_template()
        .expect("close-current-task should expose an input template");

    assert_eq!(
        template
            .input_bindings
            .iter()
            .map(|input| (input.name.as_str(), input.binding.flag.as_deref()))
            .collect::<Vec<_>>(),
        vec![
            ("review_result", Some("--review-result")),
            ("review_summary_file", Some("--review-summary-file")),
            ("verification_result", Some("--verification-result")),
            (
                "verification_summary_file",
                Some("--verification-summary-file")
            ),
        ]
    );
    let argv = materialize_public_command_argv(
        &template,
        &bound_inputs(&[
            ("review_result", "pass"),
            ("review_summary_file", "review.md"),
            ("verification_result", "not-run"),
        ]),
    )
    .expect("not-run verification should not require a verification summary file");

    assert_eq!(
        argv,
        vec![
            "featureforge",
            "plan",
            "execution",
            "close-current-task",
            "--plan",
            "docs/plan.md",
            "--task",
            "1",
            "--review-result",
            "pass",
            "--review-summary-file",
            "review.md",
            "--verification-result",
            "not-run",
        ]
    );
}

#[test]
fn complete_template_materializes_claim_and_manual_verification_argv() {
    let command = PublicCommand::Complete {
        plan: String::from("docs/plan.md"),
        task: 2,
        step: 3,
        source: None,
        fingerprint: None,
    };
    let template = command
        .to_input_template()
        .expect("complete should expose an input template");

    let argv = materialize_public_command_argv(
        &template,
        &bound_inputs(&[
            ("source", "featureforge:executing-plans"),
            ("claim", "implemented parser"),
            ("verification_mode", "manual_summary"),
            ("manual_verify_summary", "cargo test passed"),
            ("expect_execution_fingerprint", "fingerprint-123"),
        ]),
    )
    .expect("manual verification input set should bind into complete argv");

    assert_eq!(
        argv,
        vec![
            "featureforge",
            "plan",
            "execution",
            "complete",
            "--plan",
            "docs/plan.md",
            "--task",
            "2",
            "--step",
            "3",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "implemented parser",
            "--manual-verify-summary",
            "cargo test passed",
            "--expect-execution-fingerprint",
            "fingerprint-123",
        ]
    );
}

#[test]
fn complete_template_materializes_command_verification_argv() {
    let command = PublicCommand::Complete {
        plan: String::from("docs/plan.md"),
        task: 2,
        step: 3,
        source: Some(String::from("featureforge:subagent-driven-development")),
        fingerprint: Some(String::from("fingerprint-123")),
    };
    let template = command
        .to_input_template()
        .expect("complete should still require claim and verification inputs");

    let argv = materialize_public_command_argv(
        &template,
        &bound_inputs(&[
            ("claim", "implemented parser"),
            ("verification_mode", "command_result"),
            ("verify_command", "cargo test parser"),
            ("verify_result", "pass"),
        ]),
    )
    .expect("command verification input set should bind into complete argv");

    assert_eq!(
        argv,
        vec![
            "featureforge",
            "plan",
            "execution",
            "complete",
            "--plan",
            "docs/plan.md",
            "--task",
            "2",
            "--step",
            "3",
            "--source",
            "featureforge:subagent-driven-development",
            "--expect-execution-fingerprint",
            "fingerprint-123",
            "--claim",
            "implemented parser",
            "--verify-command",
            "cargo test parser",
            "--verify-result",
            "pass",
        ]
    );
}

#[test]
fn execution_argv_and_template_policy_agree_for_execution_commands() {
    let executable_cases = [
        (
            PublicCommandKind::Begin,
            vec![
                "featureforge",
                "plan",
                "execution",
                "begin",
                "--plan",
                "docs/plan.md",
                "--task",
                "1",
                "--step",
                "1",
                "--expect-execution-fingerprint",
                "fingerprint",
            ],
        ),
        (
            PublicCommandKind::Complete,
            vec![
                "featureforge",
                "plan",
                "execution",
                "complete",
                "--plan",
                "docs/plan.md",
                "--task",
                "1",
                "--step",
                "1",
                "--source",
                "featureforge:executing-plans",
                "--claim",
                "implemented task",
                "--manual-verify-summary",
                "cargo test passed",
                "--expect-execution-fingerprint",
                "fingerprint",
            ],
        ),
        (
            PublicCommandKind::Complete,
            vec![
                "featureforge",
                "plan",
                "execution",
                "complete",
                "--plan",
                "docs/plan.md",
                "--task",
                "1",
                "--step",
                "1",
                "--source",
                "featureforge:executing-plans",
                "--claim",
                "implemented task",
                "--verify-command",
                "cargo test",
                "--verify-result",
                "pass",
                "--expect-execution-fingerprint",
                "fingerprint",
            ],
        ),
        (
            PublicCommandKind::Reopen,
            vec![
                "featureforge",
                "plan",
                "execution",
                "reopen",
                "--plan",
                "docs/plan.md",
                "--task",
                "1",
                "--step",
                "1",
                "--source",
                "featureforge:executing-plans",
                "--reason",
                "repair",
                "--expect-execution-fingerprint",
                "fingerprint",
            ],
        ),
    ];

    for (expected_kind, argv) in executable_cases {
        let argv = argv.into_iter().map(String::from).collect::<Vec<_>>();
        let target = PublicCommandKind::execution_target_from_public_argv(&argv)
            .unwrap_or_else(|| panic!("{expected_kind:?} argv should be executable: {argv:?}"));
        assert_eq!(target.kind, expected_kind);
        assert_eq!(target.task, 1);
        assert_eq!(target.step, 1);
    }

    let missing_required_arg_cases = [
        vec![
            "featureforge",
            "plan",
            "execution",
            "begin",
            "--plan",
            "docs/plan.md",
            "--task",
            "1",
            "--step",
            "1",
        ],
        vec![
            "featureforge",
            "plan",
            "execution",
            "complete",
            "--plan",
            "docs/plan.md",
            "--task",
            "1",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--claim",
            "implemented task",
            "--expect-execution-fingerprint",
            "fingerprint",
        ],
        vec![
            "featureforge",
            "plan",
            "execution",
            "reopen",
            "--plan",
            "docs/plan.md",
            "--task",
            "1",
            "--step",
            "1",
            "--source",
            "featureforge:executing-plans",
            "--expect-execution-fingerprint",
            "fingerprint",
        ],
    ];

    for argv in missing_required_arg_cases {
        let argv = argv.into_iter().map(String::from).collect::<Vec<_>>();
        assert!(
            PublicCommandKind::execution_target_from_public_argv(&argv).is_none(),
            "incomplete execution argv must not satisfy executable route policy: {argv:?}"
        );
    }

    let begin_template = recommended_public_command_template(Some(&PublicCommand::Begin {
        plan: String::from("docs/plan.md"),
        task: 1,
        step: 1,
        execution_mode: Some(String::from("featureforge:executing-plans")),
        fingerprint: None,
    }))
    .expect("missing begin fingerprint should expose a bindable template");
    let complete_template = recommended_public_command_template(Some(&PublicCommand::Complete {
        plan: String::from("docs/plan.md"),
        task: 1,
        step: 1,
        source: None,
        fingerprint: None,
    }))
    .expect("missing complete inputs should expose a bindable template");
    let reopen_template = recommended_public_command_template(Some(&PublicCommand::Reopen {
        plan: String::from("docs/plan.md"),
        task: 1,
        step: 1,
        source: None,
        reason: None,
        fingerprint: None,
    }))
    .expect("missing reopen inputs should expose a bindable template");

    assert!(execution_template_inputs_are_bindable(
        &begin_template,
        PublicCommandKind::Begin
    ));
    assert!(execution_template_inputs_are_bindable(
        &complete_template,
        PublicCommandKind::Complete
    ));
    assert!(execution_template_inputs_are_bindable(
        &reopen_template,
        PublicCommandKind::Reopen
    ));

    let mut begin_without_binding = begin_template.clone();
    begin_without_binding
        .input_bindings
        .retain(|input| input.name != "expect_execution_fingerprint");
    assert!(!execution_template_inputs_are_bindable(
        &begin_without_binding,
        PublicCommandKind::Begin
    ));

    let mut complete_with_concrete_verification_flag = complete_template.clone();
    complete_with_concrete_verification_flag.base_argv.extend([
        String::from("--manual-verify-summary"),
        String::from("passed"),
    ]);
    assert!(!execution_template_inputs_are_bindable(
        &complete_with_concrete_verification_flag,
        PublicCommandKind::Complete
    ));
}

#[test]
fn late_stage_templates_materialize_result_summary_and_reviewer_argv() {
    let release_template = PublicCommand::AdvanceLateStage {
        plan: String::from("docs/plan.md"),
        mode: PublicAdvanceLateStageMode::ReleaseReadiness,
    }
    .to_input_template()
    .expect("release readiness should expose result and summary inputs");
    assert_eq!(
        materialize_public_command_argv(
            &release_template,
            &bound_inputs(&[("result", "ready"), ("summary_file", "release.md")]),
        )
        .expect("release readiness inputs should bind"),
        vec![
            "featureforge",
            "plan",
            "execution",
            "advance-late-stage",
            "--plan",
            "docs/plan.md",
            "--result",
            "ready",
            "--summary-file",
            "release.md",
        ]
    );

    let final_review_template = PublicCommand::AdvanceLateStage {
        plan: String::from("docs/plan.md"),
        mode: PublicAdvanceLateStageMode::FinalReview,
    }
    .to_input_template()
    .expect("final review should expose reviewer and summary inputs");
    assert_eq!(
        materialize_public_command_argv(
            &final_review_template,
            &bound_inputs(&[
                ("reviewer_source", "fresh-context-subagent"),
                ("reviewer_id", "019e-reviewer"),
                ("result", "pass"),
                ("summary_file", "final-review.md"),
            ]),
        )
        .expect("final-review inputs should bind"),
        vec![
            "featureforge",
            "plan",
            "execution",
            "advance-late-stage",
            "--plan",
            "docs/plan.md",
            "--reviewer-source",
            "fresh-context-subagent",
            "--reviewer-id",
            "019e-reviewer",
            "--result",
            "pass",
            "--summary-file",
            "final-review.md",
        ]
    );
}

#[test]
fn advance_late_stage_mutation_requests_are_mode_bound() {
    let route_request = PublicCommand::AdvanceLateStage {
        plan: String::from("docs/plan.md"),
        mode: PublicAdvanceLateStageMode::Qa,
    }
    .to_mutation_request()
    .expect("advance-late-stage should map to a mutation request");
    assert_eq!(
        route_request.advance_late_stage_mode,
        Some(PublicAdvanceLateStageMode::Qa)
    );

    let matching_request = PublicMutationRequest {
        kind: PublicCommandKind::AdvanceLateStage,
        task: None,
        step: None,
        expect_execution_fingerprint: None,
        transfer_mode: None,
        transfer_scope: None,
        advance_late_stage_mode: Some(PublicAdvanceLateStageMode::Qa),
    };
    assert!(public_mutation_requests_match(
        &route_request,
        &matching_request
    ));

    let wrong_mode_request = PublicMutationRequest {
        advance_late_stage_mode: Some(PublicAdvanceLateStageMode::ReleaseReadiness),
        ..matching_request
    };
    assert!(
        !public_mutation_requests_match(&route_request, &wrong_mode_request),
        "advance-late-stage mutation matching must reject a route/request operation mismatch"
    );
}

#[test]
fn advance_late_stage_mutation_rejects_phase_detail_route_mode_divergence() {
    let status = late_stage_mutation_status(
        crate::execution::phase::DETAIL_QA_RECORDING_REQUIRED,
        PublicCommand::AdvanceLateStage {
            plan: String::from("docs/plan.md"),
            mode: PublicAdvanceLateStageMode::ReleaseReadiness,
        },
    );
    let decision = decide_public_mutation(
        &status,
        &PublicMutationRequest::advance_late_stage(PublicAdvanceLateStageMode::ReleaseReadiness),
    );
    assert!(
        !decision.allowed,
        "advance-late-stage mutation must fail closed when phase_detail and typed public route mode diverge"
    );
    assert_eq!(decision.reason_code, "mutation_not_route_authorized");
}

#[test]
fn blocking_state_kind_rejects_stale_exact_public_routes() {
    for (state_kind, phase_detail) in [
        (
            STATE_KIND_PLANNING_REENTRY_REQUIRED,
            crate::execution::phase::DETAIL_PLANNING_REENTRY_REQUIRED,
        ),
        (
            STATE_KIND_TERMINAL,
            crate::execution::phase::DETAIL_FINISH_COMPLETION_GATE_READY,
        ),
    ] {
        let mut status = late_stage_mutation_status(
            phase_detail,
            PublicCommand::Begin {
                plan: String::from("docs/plan.md"),
                task: 1,
                step: 2,
                execution_mode: Some(String::from("featureforge:executing-plans")),
                fingerprint: Some(String::from("fingerprint")),
            },
        );
        status.state_kind = String::from(state_kind);

        let decision = decide_public_mutation(&status, &PublicMutationRequest::begin(1, 2, None));

        assert!(
            !decision.allowed,
            "{state_kind} must reject stale exact public route data"
        );
        assert_eq!(
            decision.reason_code, "mutation_state_kind_blocks_local_mutation",
            "{state_kind} should fail through the shared state-kind mutation block"
        );
    }
}

#[test]
fn execution_command_context_authorizes_begin_mutation() {
    let mut status = late_stage_mutation_status(
        crate::execution::phase::DETAIL_EXECUTION_IN_PROGRESS,
        PublicCommand::Begin {
            plan: String::from("docs/plan.md"),
            task: 1,
            step: 2,
            execution_mode: Some(String::from("featureforge:executing-plans")),
            fingerprint: Some(String::from("fingerprint")),
        },
    );
    status.execution_command_context =
        Some(crate::execution::status::PublicExecutionCommandContext {
            command_kind: String::from("begin"),
            task_number: Some(1),
            step_id: Some(2),
        });

    let decision = decide_public_mutation(
        &status,
        &PublicMutationRequest::begin(1, 2, Some(String::from("fingerprint"))),
    );

    assert!(
        decision.allowed,
        "matching execution command context should authorize begin mutation: {decision:?}"
    );
    assert_eq!(
        decision.reason_code, "mutation_exact_route_authorized",
        "typed route authority should remain the begin source"
    );
}

#[test]
fn resume_fields_without_exact_begin_route_do_not_authorize_begin_mutation() {
    let mut status = late_stage_mutation_status(
        crate::execution::phase::DETAIL_EXECUTION_IN_PROGRESS,
        PublicCommand::Status {
            plan: String::from("docs/plan.md"),
        },
    );
    status.recommended_public_command = None;
    status.execution_started = String::from("yes");
    status.active_task = None;
    status.active_step = None;
    status.resume_task = Some(1);
    status.resume_step = Some(2);

    let decision = decide_public_mutation(
        &status,
        &PublicMutationRequest::begin(1, 2, Some(String::from("fingerprint"))),
    );

    assert!(
        !decision.allowed,
        "resume_task/resume_step are diagnostic and must not authorize begin without typed route authority"
    );
    assert_eq!(
        decision.reason_code, "mutation_not_route_authorized",
        "resume-field-only begin should fall through to the normal route-authority rejection"
    );
}

#[test]
fn blocking_state_kind_rejects_stale_explicit_repair_targets() {
    for (state_kind, phase_detail) in [
        (
            STATE_KIND_PLANNING_REENTRY_REQUIRED,
            crate::execution::phase::DETAIL_PLANNING_REENTRY_REQUIRED,
        ),
        (
            STATE_KIND_TERMINAL,
            crate::execution::phase::DETAIL_FINISH_COMPLETION_GATE_READY,
        ),
    ] {
        let mut status = late_stage_mutation_status(
            phase_detail,
            PublicCommand::Status {
                plan: String::from("docs/plan.md"),
            },
        );
        status.state_kind = String::from(state_kind);
        status.recommended_public_command = None;
        status.public_repair_targets = vec![PublicRepairTarget {
            command_kind: String::from("begin"),
            task: Some(1),
            step: Some(2),
            reason_code: String::from("test_stale_repair_target"),
            source_record_id: Some(String::from("test")),
            expires_when_fingerprint_changes: true,
        }];

        let decision = decide_public_mutation(&status, &PublicMutationRequest::begin(1, 2, None));

        assert!(
            !decision.allowed,
            "{state_kind} must reject stale explicit repair target data"
        );
        assert_eq!(
            decision.reason_code, "mutation_state_kind_blocks_local_mutation",
            "{state_kind} should fail before explicit repair target authorization"
        );
    }
}

#[test]
fn public_mutation_request_constructors_derive_command_names_from_kind() {
    let cases = [
        PublicMutationRequest::repair_review_state(),
        PublicMutationRequest::begin(1, 1, None),
        PublicMutationRequest::complete(1, 1, None),
        PublicMutationRequest::reopen(1, 1, None),
        PublicMutationRequest::transfer_repair_step(1, 1, None),
        PublicMutationRequest::transfer_handoff(Some(String::from("task"))),
        PublicMutationRequest::close_current_task(Some(1)),
        PublicMutationRequest::advance_late_stage(PublicAdvanceLateStageMode::Qa),
    ];
    for request in cases {
        assert_eq!(
            request.public_command_name(),
            request.kind.public_mutation_name(),
            "{request:?}"
        );
        assert_eq!(
            request.command_name_for_diagnostics(),
            request.kind.public_mutation_token(),
            "{request:?}"
        );
    }
}

#[test]
fn advance_late_stage_invocation_mode_prefers_args_then_route_detail() {
    let qa_route = PublicCommand::AdvanceLateStage {
        plan: String::from("docs/plan.md"),
        mode: PublicAdvanceLateStageMode::Qa,
    };
    assert_eq!(
        public_advance_late_stage_mode_for_invocation(
            crate::execution::phase::DETAIL_QA_RECORDING_REQUIRED,
            Some(&qa_route),
            Some("pass"),
            false,
        ),
        PublicAdvanceLateStageMode::Qa
    );

    let final_review_route = PublicCommand::AdvanceLateStage {
        plan: String::from("docs/plan.md"),
        mode: PublicAdvanceLateStageMode::FinalReview,
    };
    assert_eq!(
        public_advance_late_stage_mode_for_invocation(
            crate::execution::phase::DETAIL_FINAL_REVIEW_RECORDING_READY,
            Some(&final_review_route),
            Some("pass"),
            false,
        ),
        PublicAdvanceLateStageMode::FinalReview
    );

    assert_eq!(
        public_advance_late_stage_mode_for_invocation(
            crate::execution::phase::DETAIL_BRANCH_CLOSURE_RECORDING_REQUIRED_FOR_RELEASE_READINESS,
            None,
            Some("blocked"),
            false,
        ),
        PublicAdvanceLateStageMode::ReleaseReadiness
    );
    assert_eq!(
        public_advance_late_stage_mode_for_invocation(
            crate::execution::phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED,
            None,
            None,
            false,
        ),
        PublicAdvanceLateStageMode::FinalReviewDispatch
    );
    let dispatch_route = PublicCommand::AdvanceLateStage {
        plan: String::from("docs/plan.md"),
        mode: PublicAdvanceLateStageMode::FinalReviewDispatch,
    };
    assert_eq!(
        public_advance_late_stage_mode_for_invocation(
            crate::execution::phase::DETAIL_FINAL_REVIEW_DISPATCH_REQUIRED,
            Some(&dispatch_route),
            Some("pass"),
            true,
        ),
        PublicAdvanceLateStageMode::FinalReviewDispatch,
        "final-review inputs may legally bootstrap the exact final-review dispatch route"
    );
    assert_eq!(
        public_advance_late_stage_mode_for_invocation(
            crate::execution::phase::DETAIL_QA_RECORDING_REQUIRED,
            Some(&qa_route),
            None,
            true,
        ),
        PublicAdvanceLateStageMode::FinalReview,
        "explicit final-review inputs should not be accepted as a QA mutation"
    );
}

#[test]
fn template_materializer_rejects_invalid_enum_values() {
    let template = PublicCommand::AdvanceLateStage {
        plan: String::from("docs/plan.md"),
        mode: PublicAdvanceLateStageMode::Qa,
    }
    .to_input_template()
    .expect("QA should expose result and summary inputs");

    assert!(matches!(
        materialize_public_command_argv(
            &template,
            &bound_inputs(&[("result", "ready"), ("summary_file", "qa.md")]),
        ),
        Err(PublicCommandTemplateBindingError::InvalidEnumValue { name, .. })
            if name == "result"
    ));
}

#[test]
fn placeholder_handoff_scope_is_typed_required_input_not_argv() {
    let command = PublicCommand::TransferHandoff {
        plan: String::from("docs/plan.md"),
        scope: String::from("task|branch"),
    };

    assert_eq!(
        recommended_public_command_argv(Some(&command)),
        None,
        "commands with unresolved handoff scope must not emit executable argv"
    );
    let required_inputs = required_inputs_for_public_command(Some(&command));
    assert_eq!(
        required_inputs
            .iter()
            .map(|input| input.name.as_str())
            .collect::<Vec<_>>(),
        vec!["scope", "owner", "reason"]
    );
    assert_eq!(
        required_inputs[0]
            .values
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["task", "branch"],
        "unresolved handoff scope should expose concrete enum values"
    );
    let template = recommended_public_command_template(Some(&command))
        .expect("unresolved handoff command should expose its typed input contract");
    assert_eq!(template.command_kind, "transfer");
    assert_eq!(
        template.required_input_names,
        vec!["scope", "owner", "reason"],
        "template should name every concrete input needed to bind the handoff"
    );
    assert!(
        !template.base_argv.iter().any(|arg| arg == "--scope"),
        "unresolved handoff scope must not leave an unbound --scope flag in template argv: {:?}",
        template.base_argv
    );
    assert!(
        !template.base_argv.iter().any(|arg| arg == "task|branch"),
        "unresolved handoff scope must not leak placeholder values into template argv: {:?}",
        template.base_argv
    );
}

#[test]
fn bound_argv_allows_literal_template_punctuation_in_plan_paths() {
    let plan = "docs/featureforge/plans/[release]|candidate plan.md";
    let argv = recommended_public_command_argv(Some(&PublicCommand::Begin {
        plan: plan.to_owned(),
        task: 1,
        step: 1,
        execution_mode: Some(String::from("featureforge:executing-plans")),
        fingerprint: Some(String::from("fingerprint")),
    }))
    .expect("fully bound argv should remain executable despite literal path punctuation");

    assert!(
        argv.windows(2)
            .any(|window| window[0] == "--plan" && window[1] == plan),
        "bound plan path should be preserved as a single executable argv element: {argv:?}"
    );
    assert!(!public_argv_has_template_tokens(&argv));
}
