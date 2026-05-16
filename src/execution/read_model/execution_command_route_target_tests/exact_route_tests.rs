use super::*;

#[test]
fn public_execution_route_validation_fails_closed_without_finalized_route_projection() {
    let (_repo_dir, context, _plan_rel) = unresolved_execution_context();
    let mut status =
        status_from_context(&context).expect("status should derive for exact-route test");
    status.execution_started = String::from("yes");
    status.review_state_status = String::from("clean");
    status.phase_detail = String::from(phase::DETAIL_EXECUTION_IN_PROGRESS);
    status.harness_phase = HarnessPhase::Executing;
    status.execution_mode = String::from("featureforge:executing-plans");
    status.recommended_public_command_argv = Some(vec![
        String::from("featureforge"),
        String::from("plan"),
        String::from("execution"),
        String::from("begin"),
        String::from("--plan"),
        context.plan_rel.clone(),
        String::from("--task"),
        String::from("1"),
        String::from("--step"),
        String::from("1"),
    ]);

    let failure =
        crate::execution::status_assembly::require_public_execution_command_route_target(&status)
            .expect_err("missing finalized route fields must fail closed");

    assert_eq!(failure.error_class, "MalformedExecutionState");
    assert!(
        failure
            .message
            .contains("Finalized public execution route projection is missing"),
        "failure should identify the finalized projection boundary, got: {}",
        failure.message
    );
    assert!(
        failure.message.contains("workflow/operator JSON"),
        "failure should point agents back to typed operator routing, got: {}",
        failure.message
    );
}

#[test]
fn public_execution_route_validation_rejects_display_only_command_projection() {
    let (_repo_dir, context, _plan_rel) = unresolved_execution_context();
    let mut status =
        status_from_context(&context).expect("status should derive for exact-route test");
    status.execution_started = String::from("yes");
    status.review_state_status = String::from("clean");
    status.phase_detail = String::from(phase::DETAIL_EXECUTION_IN_PROGRESS);
    status.harness_phase = HarnessPhase::Executing;
    status.execution_mode = String::from("featureforge:executing-plans");
    status.execution_command_context =
        Some(crate::execution::status::PublicExecutionCommandContext {
            command_kind: String::from("begin"),
            task_number: Some(1),
            step_id: Some(1),
        });
    status.recommended_command = Some(String::from(
        "featureforge plan execution begin --plan docs/featureforge/plans/example.md --task 1 --step 1",
    ));

    let failure =
        crate::execution::status_assembly::require_public_execution_command_route_target(&status)
            .expect_err("display-only recommended_command must not satisfy route validation");

    assert_eq!(failure.error_class, "MalformedExecutionState");
    assert!(
        failure
            .message
            .contains("recommended_public_command_argv or recommended_public_command_template"),
        "failure should require typed public command surfaces, got: {}",
        failure.message
    );
}

#[test]
fn public_execution_route_validation_rejects_mismatched_typed_argv_projection() {
    let (_repo_dir, context, _plan_rel) = unresolved_execution_context();
    let mut status =
        status_from_context(&context).expect("status should derive for exact-route test");
    status.execution_started = String::from("yes");
    status.review_state_status = String::from("clean");
    status.phase_detail = String::from(phase::DETAIL_EXECUTION_IN_PROGRESS);
    status.harness_phase = HarnessPhase::Executing;
    status.execution_mode = String::from("featureforge:executing-plans");
    status.execution_command_context =
        Some(crate::execution::status::PublicExecutionCommandContext {
            command_kind: String::from("begin"),
            task_number: Some(1),
            step_id: Some(1),
        });
    status.recommended_public_command_argv = Some(vec![
        String::from("featureforge"),
        String::from("plan"),
        String::from("execution"),
        String::from("complete"),
        String::from("--plan"),
        context.plan_rel.clone(),
        String::from("--task"),
        String::from("1"),
        String::from("--step"),
        String::from("1"),
    ]);

    let failure =
        crate::execution::status_assembly::require_public_execution_command_route_target(&status)
            .expect_err("typed argv must match execution_command_context");

    assert_eq!(failure.error_class, "MalformedExecutionState");
    assert!(
        failure.message.contains("recommended_public_command_argv"),
        "failure should identify the mismatched typed argv field, got: {}",
        failure.message
    );
}

#[test]
fn public_execution_route_validation_rejects_malformed_typed_argv_projection() {
    let (_repo_dir, context, _plan_rel) = unresolved_execution_context();
    let mut status =
        status_from_context(&context).expect("status should derive for exact-route test");
    status.execution_started = String::from("yes");
    status.review_state_status = String::from("clean");
    status.phase_detail = String::from(phase::DETAIL_EXECUTION_IN_PROGRESS);
    status.harness_phase = HarnessPhase::Executing;
    status.execution_mode = String::from("featureforge:executing-plans");
    status.execution_command_context =
        Some(crate::execution::status::PublicExecutionCommandContext {
            command_kind: String::from("begin"),
            task_number: Some(1),
            step_id: Some(1),
        });
    status.recommended_public_command_argv = Some(vec![
        String::from("featureforge"),
        String::from("plan"),
        String::from("execution"),
        String::from("begin"),
        String::from("--plan"),
        context.plan_rel.clone(),
        String::from("--task"),
        String::from("1"),
    ]);

    let failure =
        crate::execution::status_assembly::require_public_execution_command_route_target(&status)
            .expect_err("malformed typed argv must fail closed");

    assert_eq!(failure.error_class, "MalformedExecutionState");
    assert!(
        failure.message.contains("recommended_public_command_argv"),
        "failure should identify the malformed typed argv field, got: {}",
        failure.message
    );
}

#[test]
fn public_execution_route_validation_rejects_non_executable_typed_argv_projection() {
    let (_repo_dir, context, _plan_rel) = unresolved_execution_context();
    let mut status =
        status_from_context(&context).expect("status should derive for exact-route test");
    status.execution_started = String::from("yes");
    status.review_state_status = String::from("clean");
    status.phase_detail = String::from(phase::DETAIL_EXECUTION_IN_PROGRESS);
    status.harness_phase = HarnessPhase::Executing;
    status.execution_mode = String::from("featureforge:executing-plans");
    status.execution_command_context =
        Some(crate::execution::status::PublicExecutionCommandContext {
            command_kind: String::from("begin"),
            task_number: Some(1),
            step_id: Some(1),
        });
    status.recommended_public_command_argv = Some(vec![
        String::from("featureforge"),
        String::from("plan"),
        String::from("execution"),
        String::from("begin"),
        String::from("--plan"),
        context.plan_rel.clone(),
        String::from("--task"),
        String::from("1"),
        String::from("--step"),
        String::from("1"),
    ]);

    let failure =
        crate::execution::status_assembly::require_public_execution_command_route_target(&status)
            .expect_err("typed argv must include command-specific executable inputs");

    assert_eq!(failure.error_class, "MalformedExecutionState");
    assert!(
        failure.message.contains("recommended_public_command_argv"),
        "failure should identify the non-executable typed argv field, got: {}",
        failure.message
    );
}

#[test]
fn public_execution_route_validation_rejects_mixed_complete_verification_argv_projection() {
    let (_repo_dir, context, _plan_rel) = unresolved_execution_context();
    let mut status =
        status_from_context(&context).expect("status should derive for exact-route test");
    status.execution_started = String::from("yes");
    status.review_state_status = String::from("clean");
    status.phase_detail = String::from(phase::DETAIL_EXECUTION_IN_PROGRESS);
    status.harness_phase = HarnessPhase::Executing;
    status.execution_mode = String::from("featureforge:executing-plans");
    status.execution_command_context =
        Some(crate::execution::status::PublicExecutionCommandContext {
            command_kind: String::from("complete"),
            task_number: Some(1),
            step_id: Some(1),
        });
    status.recommended_public_command_argv = Some(vec![
        String::from("featureforge"),
        String::from("plan"),
        String::from("execution"),
        String::from("complete"),
        String::from("--plan"),
        context.plan_rel.clone(),
        String::from("--task"),
        String::from("1"),
        String::from("--step"),
        String::from("1"),
        String::from("--source"),
        String::from("featureforge:executing-plans"),
        String::from("--claim"),
        String::from("implemented"),
        String::from("--manual-verify-summary"),
        String::from("checked manually"),
        String::from("--verify-command"),
        String::from("cargo test"),
        String::from("--expect-execution-fingerprint"),
        String::from("fingerprint-123"),
    ]);

    let failure =
        crate::execution::status_assembly::require_public_execution_command_route_target(&status)
            .expect_err("mixed complete verification modes must fail closed");

    assert_eq!(failure.error_class, "MalformedExecutionState");
    assert!(
        failure.message.contains("recommended_public_command_argv"),
        "failure should identify the CLI-invalid typed argv field, got: {}",
        failure.message
    );
}

#[test]
fn public_execution_route_validation_rejects_mismatched_typed_template_projection() {
    let (_repo_dir, context, _plan_rel) = unresolved_execution_context();
    let mut status =
        status_from_context(&context).expect("status should derive for exact-route test");
    status.execution_started = String::from("yes");
    status.review_state_status = String::from("clean");
    status.phase_detail = String::from(phase::DETAIL_EXECUTION_IN_PROGRESS);
    status.harness_phase = HarnessPhase::Executing;
    status.execution_mode = String::from("featureforge:executing-plans");
    status.execution_command_context =
        Some(crate::execution::status::PublicExecutionCommandContext {
            command_kind: String::from("begin"),
            task_number: Some(1),
            step_id: Some(1),
        });
    status.recommended_public_command_template =
        Some(crate::execution::public_command_types::PublicCommandTemplate {
            command_kind: String::from("complete"),
            base_argv: vec![
                String::from("featureforge"),
                String::from("plan"),
                String::from("execution"),
                String::from("complete"),
                String::from("--plan"),
                context.plan_rel.clone(),
                String::from("--task"),
                String::from("1"),
                String::from("--step"),
                String::from("1"),
            ],
            required_input_names: vec![String::from("claim")],
            input_bindings: vec![
                crate::execution::public_command_types::PublicCommandTemplateInput {
                    name: String::from("claim"),
                    kind: crate::execution::public_command_types::PublicCommandInputKind::Text,
                    binding: crate::execution::public_command_types::PublicCommandInputBinding {
                        kind: crate::execution::public_command_types::PublicCommandInputBindingKind::Flag,
                        flag: Some(String::from("--claim")),
                    },
                    values: Vec::new(),
                    must_exist: false,
                    required_when: None,
                    shell_escape_by_caller: false,
                },
            ],
        });

    let failure =
        crate::execution::status_assembly::require_public_execution_command_route_target(&status)
            .expect_err("typed template must match execution_command_context");

    assert_eq!(failure.error_class, "MalformedExecutionState");
    assert!(
        failure
            .message
            .contains("recommended_public_command_template"),
        "failure should identify the mismatched typed template field, got: {}",
        failure.message
    );
}

#[test]
fn public_execution_route_validation_rejects_mixed_complete_verification_template_projection() {
    let (_repo_dir, context, _plan_rel) = unresolved_execution_context();
    let mut status =
        status_from_context(&context).expect("status should derive for exact-route test");
    status.execution_started = String::from("yes");
    status.review_state_status = String::from("clean");
    status.phase_detail = String::from(phase::DETAIL_EXECUTION_IN_PROGRESS);
    status.harness_phase = HarnessPhase::Executing;
    status.execution_mode = String::from("featureforge:executing-plans");
    status.execution_command_context =
        Some(crate::execution::status::PublicExecutionCommandContext {
            command_kind: String::from("complete"),
            task_number: Some(1),
            step_id: Some(1),
        });
    let mut template = crate::execution::command_eligibility::PublicCommand::Complete {
        plan: context.plan_rel.clone(),
        task: 1,
        step: 1,
        source: None,
        fingerprint: None,
    }
    .to_input_template()
    .expect("complete should expose a bindable template");
    template
        .input_bindings
        .retain(|input| input.name != "verification_mode" && input.name != "verify_result");
    template
        .required_input_names
        .retain(|input| input != "verification_mode" && input != "verify_result");
    for input in &mut template.input_bindings {
        if matches!(
            input.name.as_str(),
            "manual_verify_summary" | "verify_command"
        ) {
            input.required_when = None;
        }
    }
    status.recommended_public_command_template = Some(template);

    let failure =
        crate::execution::status_assembly::require_public_execution_command_route_target(&status)
            .expect_err("mixed/partial complete verification template must fail closed");

    assert_eq!(failure.error_class, "MalformedExecutionState");
    assert!(
        failure
            .message
            .contains("recommended_public_command_template.input_bindings"),
        "failure should identify invalid complete template bindings, got: {}",
        failure.message
    );
}

#[test]
fn public_execution_route_validation_rejects_concrete_complete_template_with_virtual_verification_mode()
 {
    let (_repo_dir, context, _plan_rel) = unresolved_execution_context();
    let mut status =
        status_from_context(&context).expect("status should derive for exact-route test");
    status.execution_started = String::from("yes");
    status.review_state_status = String::from("clean");
    status.phase_detail = String::from(phase::DETAIL_EXECUTION_IN_PROGRESS);
    status.harness_phase = HarnessPhase::Executing;
    status.execution_mode = String::from("featureforge:executing-plans");
    status.execution_command_context =
        Some(crate::execution::status::PublicExecutionCommandContext {
            command_kind: String::from("complete"),
            task_number: Some(1),
            step_id: Some(1),
        });
    let mut template = crate::execution::command_eligibility::PublicCommand::Complete {
        plan: context.plan_rel.clone(),
        task: 1,
        step: 1,
        source: Some(String::from("featureforge:executing-plans")),
        fingerprint: Some(String::from("fingerprint-123")),
    }
    .to_input_template()
    .expect("complete should expose a claim/verification template");
    template.base_argv.extend([
        String::from("--manual-verify-summary"),
        String::from("checked manually"),
    ]);
    status.recommended_public_command_template = Some(template);

    let failure =
        crate::execution::status_assembly::require_public_execution_command_route_target(&status)
            .expect_err(
                "concrete complete template must not also request virtual verification mode",
            );

    assert_eq!(failure.error_class, "MalformedExecutionState");
    assert!(
        failure
            .message
            .contains("recommended_public_command_template.input_bindings"),
        "failure should identify invalid complete template verification binding, got: {}",
        failure.message
    );
}

#[test]
fn public_execution_route_validation_rejects_equals_form_complete_template_verification_argv() {
    let (_repo_dir, context, _plan_rel) = unresolved_execution_context();
    let mut status =
        status_from_context(&context).expect("status should derive for exact-route test");
    status.execution_started = String::from("yes");
    status.review_state_status = String::from("clean");
    status.phase_detail = String::from(phase::DETAIL_EXECUTION_IN_PROGRESS);
    status.harness_phase = HarnessPhase::Executing;
    status.execution_mode = String::from("featureforge:executing-plans");
    status.execution_command_context =
        Some(crate::execution::status::PublicExecutionCommandContext {
            command_kind: String::from("complete"),
            task_number: Some(1),
            step_id: Some(1),
        });
    let mut template = crate::execution::command_eligibility::PublicCommand::Complete {
        plan: context.plan_rel.clone(),
        task: 1,
        step: 1,
        source: Some(String::from("featureforge:executing-plans")),
        fingerprint: Some(String::from("fingerprint-123")),
    }
    .to_input_template()
    .expect("complete should expose a claim/verification template");
    template
        .base_argv
        .push(String::from("--manual-verify-summary=checked manually"));
    status.recommended_public_command_template = Some(template);

    let failure =
        crate::execution::status_assembly::require_public_execution_command_route_target(&status)
            .expect_err(
                "equals-form complete verification argv must not bypass virtual verification_mode",
            );

    assert_eq!(failure.error_class, "MalformedExecutionState");
    assert!(
        failure
            .message
            .contains("recommended_public_command_template.input_bindings"),
        "failure should identify equals-form complete verification argv, got: {}",
        failure.message
    );
}

#[test]
fn public_execution_route_validation_rejects_concrete_complete_template_without_virtual_verification_mode()
 {
    let (_repo_dir, context, _plan_rel) = unresolved_execution_context();
    let mut status =
        status_from_context(&context).expect("status should derive for exact-route test");
    status.execution_started = String::from("yes");
    status.review_state_status = String::from("clean");
    status.phase_detail = String::from(phase::DETAIL_EXECUTION_IN_PROGRESS);
    status.harness_phase = HarnessPhase::Executing;
    status.execution_mode = String::from("featureforge:executing-plans");
    status.execution_command_context =
        Some(crate::execution::status::PublicExecutionCommandContext {
            command_kind: String::from("complete"),
            task_number: Some(1),
            step_id: Some(1),
        });
    let mut template = crate::execution::command_eligibility::PublicCommand::Complete {
        plan: context.plan_rel.clone(),
        task: 1,
        step: 1,
        source: Some(String::from("featureforge:executing-plans")),
        fingerprint: Some(String::from("fingerprint-123")),
    }
    .to_input_template()
    .expect("complete should expose a claim/verification template");
    template.input_bindings.retain(|input| {
        !matches!(
            input.name.as_str(),
            "verification_mode" | "manual_verify_summary" | "verify_command" | "verify_result"
        )
    });
    template.required_input_names.retain(|input| {
        !matches!(
            input.as_str(),
            "verification_mode" | "manual_verify_summary" | "verify_command" | "verify_result"
        )
    });
    template.base_argv.extend([
        String::from("--manual-verify-summary"),
        String::from("checked manually"),
    ]);
    status.recommended_public_command_template = Some(template);

    let failure = crate::execution::status_assembly::require_public_execution_command_route_target(
        &status,
    )
    .expect_err(
        "complete templates must use virtual verification_mode, not concrete verification argv",
    );

    assert_eq!(failure.error_class, "MalformedExecutionState");
    assert!(
        failure
            .message
            .contains("recommended_public_command_template.input_bindings"),
        "failure should identify concrete complete template verification mode, got: {}",
        failure.message
    );
}

#[test]
fn public_execution_route_validation_rejects_hidden_complete_template_bindings() {
    let (_repo_dir, context, _plan_rel) = unresolved_execution_context();
    let mut status =
        status_from_context(&context).expect("status should derive for exact-route test");
    status.execution_started = String::from("yes");
    status.review_state_status = String::from("clean");
    status.phase_detail = String::from(phase::DETAIL_EXECUTION_IN_PROGRESS);
    status.harness_phase = HarnessPhase::Executing;
    status.execution_mode = String::from("featureforge:executing-plans");
    status.execution_command_context =
        Some(crate::execution::status::PublicExecutionCommandContext {
            command_kind: String::from("complete"),
            task_number: Some(1),
            step_id: Some(1),
        });
    let mut template = crate::execution::command_eligibility::PublicCommand::Complete {
        plan: context.plan_rel.clone(),
        task: 1,
        step: 1,
        source: None,
        fingerprint: None,
    }
    .to_input_template()
    .expect("complete should expose a bindable template");
    template.required_input_names = vec![String::from("claim")];
    status.recommended_public_command_template = Some(template);

    let failure =
        crate::execution::status_assembly::require_public_execution_command_route_target(&status)
            .expect_err("templates must not hide required bindings from required_input_names");

    assert_eq!(failure.error_class, "MalformedExecutionState");
    assert!(
        failure
            .message
            .contains("recommended_public_command_template.input_bindings"),
        "failure should identify hidden complete template bindings, got: {}",
        failure.message
    );
}

#[test]
fn public_execution_route_validation_rejects_duplicate_template_input_aliasing() {
    let (_repo_dir, context, _plan_rel) = unresolved_execution_context();
    let mut status =
        status_from_context(&context).expect("status should derive for exact-route test");
    status.execution_started = String::from("yes");
    status.review_state_status = String::from("clean");
    status.phase_detail = String::from(phase::DETAIL_EXECUTION_IN_PROGRESS);
    status.harness_phase = HarnessPhase::Executing;
    status.execution_mode = String::from("featureforge:executing-plans");
    status.execution_command_context =
        Some(crate::execution::status::PublicExecutionCommandContext {
            command_kind: String::from("complete"),
            task_number: Some(1),
            step_id: Some(1),
        });
    let mut template = crate::execution::command_eligibility::PublicCommand::Complete {
        plan: context.plan_rel.clone(),
        task: 1,
        step: 1,
        source: None,
        fingerprint: None,
    }
    .to_input_template()
    .expect("complete should expose a bindable template");
    for required_name in &mut template.required_input_names {
        if required_name == "source" {
            *required_name = String::from("claim");
        }
    }
    for input in &mut template.input_bindings {
        if input.name == "source" {
            input.name = String::from("claim");
        }
    }
    status.recommended_public_command_template = Some(template);

    let failure =
        crate::execution::status_assembly::require_public_execution_command_route_target(&status)
            .expect_err("templates must not hide bindings through duplicate input aliases");

    assert_eq!(failure.error_class, "MalformedExecutionState");
    assert!(
        failure
            .message
            .contains("recommended_public_command_template.input_bindings"),
        "failure should identify duplicate complete template input aliases, got: {}",
        failure.message
    );
}

#[test]
fn public_execution_route_validation_rejects_route_identity_template_bindings() {
    let (_repo_dir, context, _plan_rel) = unresolved_execution_context();
    let mut status =
        status_from_context(&context).expect("status should derive for exact-route test");
    status.execution_started = String::from("yes");
    status.review_state_status = String::from("clean");
    status.phase_detail = String::from(phase::DETAIL_EXECUTION_IN_PROGRESS);
    status.harness_phase = HarnessPhase::Executing;
    status.execution_mode = String::from("featureforge:executing-plans");
    status.execution_command_context =
        Some(crate::execution::status::PublicExecutionCommandContext {
            command_kind: String::from("begin"),
            task_number: Some(1),
            step_id: Some(1),
        });
    let mut template = crate::execution::command_eligibility::PublicCommand::Begin {
        plan: context.plan_rel.clone(),
        task: 1,
        step: 1,
        execution_mode: Some(String::from("featureforge:executing-plans")),
        fingerprint: None,
    }
    .to_input_template()
    .expect("begin should expose a fingerprint template");
    template
        .required_input_names
        .push(String::from("target_task_override"));
    template.input_bindings.push(
        crate::execution::public_command_types::PublicCommandTemplateInput {
            name: String::from("target_task_override"),
            kind: crate::execution::public_command_types::PublicCommandInputKind::Text,
            binding: crate::execution::public_command_types::PublicCommandInputBinding {
                kind: crate::execution::public_command_types::PublicCommandInputBindingKind::Flag,
                flag: Some(String::from("--task")),
            },
            values: Vec::new(),
            must_exist: false,
            required_when: None,
            shell_escape_by_caller: false,
        },
    );
    status.recommended_public_command_template = Some(template);

    let failure =
        crate::execution::status_assembly::require_public_execution_command_route_target(&status)
            .expect_err("template bindings must not override base route identity flags");

    assert_eq!(failure.error_class, "MalformedExecutionState");
    assert!(
        failure
            .message
            .contains("recommended_public_command_template.input_bindings"),
        "failure should identify route identity override bindings, got: {}",
        failure.message
    );
}

#[test]
fn public_execution_route_validation_rejects_conditional_required_template_flags() {
    let (_repo_dir, context, _plan_rel) = unresolved_execution_context();
    let mut status =
        status_from_context(&context).expect("status should derive for exact-route test");
    status.execution_started = String::from("yes");
    status.review_state_status = String::from("clean");
    status.phase_detail = String::from(phase::DETAIL_EXECUTION_IN_PROGRESS);
    status.harness_phase = HarnessPhase::Executing;
    status.execution_mode = String::from("featureforge:executing-plans");
    status.execution_command_context =
        Some(crate::execution::status::PublicExecutionCommandContext {
            command_kind: String::from("complete"),
            task_number: Some(1),
            step_id: Some(1),
        });
    let mut template = crate::execution::command_eligibility::PublicCommand::Complete {
        plan: context.plan_rel.clone(),
        task: 1,
        step: 1,
        source: None,
        fingerprint: None,
    }
    .to_input_template()
    .expect("complete should expose a bindable template");
    for input in &mut template.input_bindings {
        if input.binding.flag.as_deref() == Some("--source") {
            input.required_when = Some(String::from("verification_mode=manual_summary"));
        }
    }
    status.recommended_public_command_template = Some(template);

    let failure =
        crate::execution::status_assembly::require_public_execution_command_route_target(&status)
            .expect_err("required command flags must not be hidden behind required_when");

    assert_eq!(failure.error_class, "MalformedExecutionState");
    assert!(
        failure
            .message
            .contains("recommended_public_command_template.input_bindings"),
        "failure should identify conditional required template flags, got: {}",
        failure.message
    );
}

#[test]
fn public_execution_route_validation_accepts_bindable_complete_template_projection() {
    let (_repo_dir, context, _plan_rel) = unresolved_execution_context();
    let mut status =
        status_from_context(&context).expect("status should derive for exact-route test");
    status.execution_started = String::from("yes");
    status.review_state_status = String::from("clean");
    status.phase_detail = String::from(phase::DETAIL_EXECUTION_IN_PROGRESS);
    status.harness_phase = HarnessPhase::Executing;
    status.execution_mode = String::from("featureforge:executing-plans");
    status.execution_command_context =
        Some(crate::execution::status::PublicExecutionCommandContext {
            command_kind: String::from("complete"),
            task_number: Some(1),
            step_id: Some(1),
        });
    status.recommended_public_command_template =
        crate::execution::command_eligibility::PublicCommand::Complete {
            plan: context.plan_rel.clone(),
            task: 1,
            step: 1,
            source: None,
            fingerprint: None,
        }
        .to_input_template();

    crate::execution::status_assembly::require_public_execution_command_route_target(&status)
        .expect("complete template with virtual verification mode should satisfy validation");
}

#[test]
fn public_execution_route_validation_rejects_inconsistent_command_context() {
    let (_repo_dir, context, _plan_rel) = unresolved_execution_context();
    let mut status =
        status_from_context(&context).expect("status should derive for exact-route test");
    status.execution_started = String::from("yes");
    status.review_state_status = String::from("clean");
    status.phase_detail = String::from(phase::DETAIL_EXECUTION_IN_PROGRESS);
    status.harness_phase = HarnessPhase::Executing;
    status.execution_mode = String::from("featureforge:executing-plans");
    status.execution_command_context =
        Some(crate::execution::status::PublicExecutionCommandContext {
            command_kind: String::from("complete"),
            task_number: Some(1),
            step_id: Some(1),
        });
    status.recommended_public_command = Some(
        crate::execution::command_eligibility::PublicCommand::Begin {
            plan: context.plan_rel.clone(),
            task: 1,
            step: 1,
            execution_mode: Some(String::from("featureforge:executing-plans")),
            fingerprint: None,
        },
    );
    status.recommended_public_command_argv = Some(vec![
        String::from("featureforge"),
        String::from("plan"),
        String::from("execution"),
        String::from("begin"),
        String::from("--plan"),
        context.plan_rel.clone(),
        String::from("--task"),
        String::from("1"),
        String::from("--step"),
        String::from("1"),
    ]);

    let failure =
        crate::execution::status_assembly::require_public_execution_command_route_target(&status)
            .expect_err("inconsistent finalized command/context fields must fail closed");

    assert_eq!(failure.error_class, "MalformedExecutionState");
    assert!(
        failure
            .message
            .contains("inconsistent execution_command_context"),
        "failure should identify inconsistent route fields, got: {}",
        failure.message
    );
}

#[test]
fn public_execution_route_validation_accepts_finalized_typed_route_projection() {
    let (_repo_dir, context, _plan_rel) = unresolved_execution_context();
    let mut status =
        status_from_context(&context).expect("status should derive for exact-route test");
    status.execution_started = String::from("yes");
    status.review_state_status = String::from("clean");
    status.phase_detail = String::from(phase::DETAIL_EXECUTION_IN_PROGRESS);
    status.harness_phase = HarnessPhase::Executing;
    status.execution_mode = String::from("featureforge:executing-plans");
    status.execution_command_context =
        Some(crate::execution::status::PublicExecutionCommandContext {
            command_kind: String::from("begin"),
            task_number: Some(1),
            step_id: Some(1),
        });
    status.recommended_public_command_argv = Some(vec![
        String::from("featureforge"),
        String::from("plan"),
        String::from("execution"),
        String::from("begin"),
        String::from("--plan"),
        context.plan_rel.clone(),
        String::from("--task"),
        String::from("1"),
        String::from("--step"),
        String::from("1"),
        String::from("--expect-execution-fingerprint"),
        String::from("fingerprint-123"),
    ]);

    crate::execution::status_assembly::require_public_execution_command_route_target(&status)
        .expect("finalized typed public route fields should satisfy validation");
}
