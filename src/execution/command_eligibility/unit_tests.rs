use super::*;

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
        PublicMutationKind::CloseCurrentTask
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
