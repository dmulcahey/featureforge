pub(crate) const BRANCH_CLOSURE_PROVENANCE_TASK_CLOSURE_LINEAGE: &str = "task_closure_lineage";
pub(crate) const BRANCH_CLOSURE_PROVENANCE_LATE_STAGE_SURFACE_EXEMPTION: &str =
    "task_closure_lineage_plus_late_stage_surface_exemption";

pub(crate) fn branch_closure_provenance_is_late_stage_surface_exemption(
    provenance_basis: &str,
) -> bool {
    provenance_basis == BRANCH_CLOSURE_PROVENANCE_LATE_STAGE_SURFACE_EXEMPTION
}

pub(crate) fn branch_closure_has_empty_lineage_late_stage_surface_exemption(
    provenance_basis: &str,
    source_task_closure_ids: &[String],
) -> bool {
    branch_closure_provenance_is_late_stage_surface_exemption(provenance_basis)
        && source_task_closure_ids.is_empty()
}
