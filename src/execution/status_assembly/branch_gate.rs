use crate::execution::branch_closure_provenance::{
    branch_closure_has_empty_lineage_late_stage_surface_exemption,
    branch_closure_provenance_is_late_stage_surface_exemption,
};
use crate::execution::context::ExecutionContext;
use crate::execution::current_closure_projection::still_current_task_closure_records_from_authoritative_state;
use crate::execution::current_truth::{
    branch_source_task_closure_ids as shared_branch_source_task_closure_ids,
    current_late_stage_branch_bindings as shared_current_late_stage_branch_bindings,
    normalized_late_stage_surface, parse_late_stage_surface_only_branch_surface,
    path_matches_late_stage_surface,
};
use crate::execution::semantic_identity::branch_definition_identity_for_context;
use crate::execution::status_support::resolve_branch_closure_reviewed_tree_sha;
use crate::execution::transitions::{
    AuthoritativeTransitionState, load_authoritative_transition_state,
};

pub(crate) fn validated_current_branch_closure_identity(
    context: &ExecutionContext,
) -> Option<crate::execution::transitions::CurrentBranchClosureIdentity> {
    let authoritative_state = load_authoritative_transition_state(context).ok().flatten();
    validated_current_branch_closure_identity_from_authoritative_state(
        context,
        authoritative_state.as_ref(),
    )
}

pub(crate) fn validated_current_branch_closure_identity_from_authoritative_state(
    context: &ExecutionContext,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> Option<crate::execution::transitions::CurrentBranchClosureIdentity> {
    let state = authoritative_state?;
    let identity = state.bound_current_branch_closure_identity()?;
    let record = state.branch_closure_record(&identity.branch_closure_id)?;
    let current_base_branch = context.current_release_base_branch()?;
    let semantic_contract_identity = branch_definition_identity_for_context(context);
    let contract_identity_matches = identity.contract_identity == record.contract_identity
        && normalized_branch_contract_identity_for_current_truth(
            context,
            &current_base_branch,
            &identity.contract_identity,
        )
        .is_some_and(|normalized| normalized == semantic_contract_identity);
    let late_stage_surface =
        if branch_closure_provenance_is_late_stage_surface_exemption(&record.provenance_basis) {
            normalized_late_stage_surface(&context.plan_source).ok()
        } else {
            None
        };
    let expected_source_task_closure_ids = shared_branch_source_task_closure_ids(
        context,
        &still_current_task_closure_records_from_authoritative_state(context, state).ok()?,
        late_stage_surface.as_deref(),
    );
    let mut normalized_record_source_task_closure_ids = record.source_task_closure_ids.clone();
    normalized_record_source_task_closure_ids.sort();
    normalized_record_source_task_closure_ids.dedup();
    (record.source_plan_path == context.plan_rel
        && record.source_plan_revision == context.plan_document.plan_revision
        && record.repo_slug == context.runtime.repo_slug
        && record.branch_name == context.runtime.branch_name
        && record.base_branch == current_base_branch
        && contract_identity_matches
        && record.source_task_closure_ids.len() == normalized_record_source_task_closure_ids.len()
        && normalized_record_source_task_closure_ids == expected_source_task_closure_ids
        && branch_closure_record_matches_plan_exemption(context, &record))
    .then_some(identity)
}

fn normalized_branch_contract_identity_for_current_truth(
    context: &ExecutionContext,
    _base_branch: &str,
    observed_identity: &str,
) -> Option<String> {
    let semantic = branch_definition_identity_for_context(context);
    (observed_identity == semantic).then_some(semantic)
}

pub(crate) fn usable_current_branch_closure_identity(
    context: &ExecutionContext,
) -> Option<crate::execution::transitions::CurrentBranchClosureIdentity> {
    let authoritative_state = load_authoritative_transition_state(context).ok().flatten();
    usable_current_branch_closure_identity_from_authoritative_state(
        context,
        authoritative_state.as_ref(),
    )
}

pub(crate) fn usable_current_branch_closure_identity_from_authoritative_state(
    context: &ExecutionContext,
    authoritative_state: Option<&AuthoritativeTransitionState>,
) -> Option<crate::execution::transitions::CurrentBranchClosureIdentity> {
    let identity = validated_current_branch_closure_identity_from_authoritative_state(
        context,
        authoritative_state,
    )?;
    resolve_branch_closure_reviewed_tree_sha(
        &context.runtime.repo_root,
        &identity.branch_closure_id,
        &identity.reviewed_state_id,
    )
    .ok()?;
    Some(identity)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CurrentBranchGateBindings {
    pub(crate) current_branch_reviewed_state_id: Option<String>,
    pub(crate) current_branch_closure_id: Option<String>,
    pub(crate) finish_review_gate_pass_branch_closure_id: Option<String>,
}

pub(crate) fn current_branch_gate_bindings_from_authoritative_state(
    context: &ExecutionContext,
    authoritative_state: Option<&AuthoritativeTransitionState>,
    gate_allowed: bool,
) -> CurrentBranchGateBindings {
    let current_branch_closure_id =
        validated_current_branch_closure_identity_from_authoritative_state(
            context,
            authoritative_state,
        )
        .map(|identity| identity.branch_closure_id);
    let usable_identity = usable_current_branch_closure_identity_from_authoritative_state(
        context,
        authoritative_state,
    );
    let current_branch_reviewed_state_id = usable_identity
        .as_ref()
        .map(|identity| identity.reviewed_state_id.clone());
    let current_branch_closure_id = current_branch_closure_id.or_else(|| {
        gate_allowed.then(|| {
            usable_identity
                .as_ref()
                .map(|identity| identity.branch_closure_id.clone())
        })?
    });
    let finish_review_gate_pass_branch_closure_id = shared_current_late_stage_branch_bindings(
        authoritative_state,
        current_branch_closure_id.as_deref(),
        current_branch_reviewed_state_id.as_deref(),
    )
    .finish_review_gate_pass_branch_closure_id;

    CurrentBranchGateBindings {
        current_branch_reviewed_state_id,
        current_branch_closure_id,
        finish_review_gate_pass_branch_closure_id,
    }
}

pub(crate) fn branch_closure_record_matches_plan_exemption(
    context: &ExecutionContext,
    record: &crate::execution::transitions::BranchClosureRecord,
) -> bool {
    if !branch_closure_has_empty_lineage_late_stage_surface_exemption(
        &record.provenance_basis,
        &record.source_task_closure_ids,
    ) {
        return true;
    }
    let Ok(late_stage_surface) = normalized_late_stage_surface(&context.plan_source) else {
        return false;
    };
    !late_stage_surface.is_empty()
        && parse_late_stage_surface_only_branch_surface(&record._effective_reviewed_branch_surface)
            .is_some_and(|recorded_surface| {
                !recorded_surface.is_empty()
                    && recorded_surface
                        .iter()
                        .all(|entry| path_matches_late_stage_surface(entry, &late_stage_surface))
            })
}
