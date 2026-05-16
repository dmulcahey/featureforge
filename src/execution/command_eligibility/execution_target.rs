use std::collections::BTreeSet;

use crate::execution::public_command_types::{
    PublicCommandInputBindingKind, PublicCommandTemplate, PublicCommandTemplateInput,
};

use super::command_kind::PublicCommandKind;

const BASE_OWNED_EXECUTION_ROUTE_FLAGS: &[&str] = &["--plan", "--task", "--step"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PublicExecutionCommandTarget {
    pub(crate) kind: PublicCommandKind,
    pub(crate) task: u32,
    pub(crate) step: u32,
}

pub(crate) fn execution_mutation_name_from_public_argv(argv: &[String]) -> Option<&str> {
    let command_kind = execution_mutation_name_from_public_argv_prefix(argv)?;
    PublicCommandKind::from_execution_mutation_name(command_kind).map(|_| command_kind)
}

pub(crate) fn execution_target_from_public_argv(
    argv: &[String],
) -> Option<PublicExecutionCommandTarget> {
    execution_target_from_public_argv_with_shape(argv, true)
}

pub(crate) fn execution_target_from_public_template_base_argv(
    argv: &[String],
) -> Option<PublicExecutionCommandTarget> {
    execution_target_from_public_argv_with_shape(argv, false)
}

// Command eligibility owns execution-template bindability policy. Presentation
// and status assembly code must call this boundary instead of reinterpreting
// template input-binding DTOs.
pub(crate) fn execution_template_inputs_are_bindable(
    template: &PublicCommandTemplate,
    kind: PublicCommandKind,
) -> bool {
    template_bindings_are_materializable(template)
        && template_execution_args_are_bindable(template, kind)
}

fn execution_target_from_public_argv_with_shape(
    argv: &[String],
    require_executable_args: bool,
) -> Option<PublicExecutionCommandTarget> {
    let command_kind = execution_mutation_name_from_public_argv_prefix(argv)?;
    let args = argv.get(4..)?;
    let kind = PublicCommandKind::from_execution_mutation_name(command_kind)?;
    concrete_flag_value(args, "--plan")?;
    if require_executable_args {
        require_executable_execution_args(kind, args)?;
    }
    Some(PublicExecutionCommandTarget {
        kind,
        task: concrete_flag_value(args, "--task")?.parse().ok()?,
        step: concrete_flag_value(args, "--step")?.parse().ok()?,
    })
}

fn execution_mutation_name_from_public_argv_prefix(argv: &[String]) -> Option<&str> {
    let [program, plan, execution, command_kind, ..] = argv else {
        return None;
    };
    if program == "featureforge" && plan == "plan" && execution == "execution" {
        Some(command_kind.as_str())
    } else {
        None
    }
}

fn require_executable_execution_args(kind: PublicCommandKind, args: &[String]) -> Option<()> {
    match kind {
        PublicCommandKind::Begin => {
            concrete_flag_value(args, "--expect-execution-fingerprint")?;
        }
        PublicCommandKind::Complete => {
            concrete_flag_value(args, "--source")?;
            concrete_flag_value(args, "--claim")?;
            concrete_flag_value(args, "--expect-execution-fingerprint")?;
            require_exactly_one_complete_verification_mode(args)?;
        }
        PublicCommandKind::Reopen => {
            concrete_flag_value(args, "--source")?;
            concrete_flag_value(args, "--reason")?;
            concrete_flag_value(args, "--expect-execution-fingerprint")?;
        }
        _ => return None,
    }
    Some(())
}

fn require_exactly_one_complete_verification_mode(args: &[String]) -> Option<()> {
    let has_manual_summary = concrete_flag_value(args, "--manual-verify-summary").is_some();
    let has_verify_command = concrete_flag_value(args, "--verify-command").is_some();
    let has_verify_result = concrete_flag_value(args, "--verify-result").is_some();
    (has_manual_summary && !has_verify_command && !has_verify_result
        || !has_manual_summary && has_verify_command && has_verify_result)
        .then_some(())
}

fn template_bindings_are_materializable(template: &PublicCommandTemplate) -> bool {
    !template.required_input_names.is_empty()
        && template_input_names_match_bindings(template)
        && template.required_input_names.iter().all(|required_name| {
            template
                .input_bindings
                .iter()
                .any(|input| input.name == *required_name)
        })
        && template.input_bindings.iter().all(|input| {
            !input.name.trim().is_empty()
                && input_binding_has_materializable_shape(input)
                && !input_binding_overrides_base_execution_route_target(input)
        })
}

fn input_binding_has_materializable_shape(input: &PublicCommandTemplateInput) -> bool {
    match input.binding.kind {
        PublicCommandInputBindingKind::Flag => input
            .binding
            .flag
            .as_deref()
            .is_some_and(|flag| flag.starts_with("--") && flag.len() > 2),
        PublicCommandInputBindingKind::Virtual => input.binding.flag.is_none(),
    }
}

fn input_binding_overrides_base_execution_route_target(input: &PublicCommandTemplateInput) -> bool {
    matches!(input.binding.kind, PublicCommandInputBindingKind::Flag)
        && input
            .binding
            .flag
            .as_deref()
            .is_some_and(|flag| BASE_OWNED_EXECUTION_ROUTE_FLAGS.contains(&flag))
}

fn template_input_names_match_bindings(template: &PublicCommandTemplate) -> bool {
    let required_names = template
        .required_input_names
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let binding_names = template
        .input_bindings
        .iter()
        .map(|input| input.name.as_str())
        .collect::<BTreeSet<_>>();
    required_names.len() == template.required_input_names.len()
        && binding_names.len() == template.input_bindings.len()
        && required_names == binding_names
}

fn template_execution_args_are_bindable(
    template: &PublicCommandTemplate,
    kind: PublicCommandKind,
) -> bool {
    match kind {
        PublicCommandKind::Begin => {
            template_supplies_required_flag(template, "--expect-execution-fingerprint")
        }
        PublicCommandKind::Complete => {
            template_supplies_required_flag(template, "--source")
                && template_supplies_required_flag(template, "--claim")
                && template_supplies_required_flag(template, "--expect-execution-fingerprint")
                && complete_template_verification_mode_is_bindable(template)
        }
        PublicCommandKind::Reopen => {
            template_supplies_required_flag(template, "--source")
                && template_supplies_required_flag(template, "--reason")
                && template_supplies_required_flag(template, "--expect-execution-fingerprint")
        }
        _ => false,
    }
}

fn template_supplies_required_flag(template: &PublicCommandTemplate, flag: &str) -> bool {
    argv_has_concrete_flag_value(&template.base_argv, flag)
        || template.input_bindings.iter().any(|input| {
            matches!(input.binding.kind, PublicCommandInputBindingKind::Flag)
                && input.binding.flag.as_deref() == Some(flag)
                && input.required_when.is_none()
        })
}

fn argv_has_concrete_flag_value(argv: &[String], flag: &str) -> bool {
    concrete_flag_value(argv, flag).is_some()
}

fn complete_template_verification_mode_is_bindable(template: &PublicCommandTemplate) -> bool {
    if verification_flags_present_in_base_argv(template) {
        return false;
    }
    template_has_virtual_verification_mode(template)
        && template_has_conditional_flag(
            template,
            "--manual-verify-summary",
            "verification_mode=manual_summary",
        )
        && template_has_conditional_flag(
            template,
            "--verify-command",
            "verification_mode=command_result",
        )
        && template_has_conditional_flag(
            template,
            "--verify-result",
            "verification_mode=command_result",
        )
        && template_verification_flag_bindings_are_expected(template)
}

fn verification_flags_present_in_base_argv(template: &PublicCommandTemplate) -> bool {
    [
        "--manual-verify-summary",
        "--verify-command",
        "--verify-result",
    ]
    .into_iter()
    .any(|flag| base_argv_contains_flag(&template.base_argv, flag))
}

fn base_argv_contains_flag(argv: &[String], flag: &str) -> bool {
    argv.iter().any(|arg| {
        arg == flag
            || arg
                .strip_prefix(flag)
                .is_some_and(|suffix| suffix.starts_with('='))
    })
}

fn template_has_virtual_verification_mode(template: &PublicCommandTemplate) -> bool {
    template.input_bindings.iter().any(|input| {
        input.name == "verification_mode"
            && matches!(input.binding.kind, PublicCommandInputBindingKind::Virtual)
            && input.binding.flag.is_none()
            && input.required_when.is_none()
    })
}

fn template_has_conditional_flag(
    template: &PublicCommandTemplate,
    flag: &str,
    required_when: &str,
) -> bool {
    template.input_bindings.iter().any(|input| {
        matches!(input.binding.kind, PublicCommandInputBindingKind::Flag)
            && input.binding.flag.as_deref() == Some(flag)
            && input.required_when.as_deref() == Some(required_when)
    })
}

fn template_verification_flag_bindings_are_expected(template: &PublicCommandTemplate) -> bool {
    template.input_bindings.iter().all(|input| {
        let Some(flag) = input.binding.flag.as_deref() else {
            return true;
        };
        match flag {
            "--manual-verify-summary" => {
                input.required_when.as_deref() == Some("verification_mode=manual_summary")
            }
            "--verify-command" | "--verify-result" => {
                input.required_when.as_deref() == Some("verification_mode=command_result")
            }
            _ => true,
        }
    })
}

fn concrete_flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    flag_value(args, flag).filter(|value| value_is_concrete(value))
}

fn flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2).find_map(|window| match window {
        [candidate, value] if candidate == flag && !value.starts_with("--") => Some(value.as_str()),
        _ => None,
    })
}

fn value_is_concrete(value: &str) -> bool {
    let trimmed = value.trim();
    !value_is_template_placeholder(trimmed)
}

fn value_is_template_placeholder(value: &str) -> bool {
    value.is_empty()
        || matches!(
            value,
            "pass|fail" | "pass|fail|not-run" | "ready|blocked" | "task|branch"
        )
        || (value.starts_with('<') && value.ends_with('>'))
        || value.contains("[when verification ran]")
}
