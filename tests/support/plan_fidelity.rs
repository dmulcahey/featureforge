use std::fs;
use std::path::{Path, PathBuf};

use featureforge::contracts::plan::{PLAN_FIDELITY_REQUIRED_SURFACES, parse_plan_file};
use featureforge::contracts::spec::parse_spec_file;
use featureforge::git::sha256_hex;

pub fn write_current_pass_plan_fidelity_review_artifact(
    repo: &Path,
    artifact_rel: impl AsRef<Path>,
    plan_rel: &str,
    spec_rel: &str,
) {
    let artifact_path = repo.join(artifact_rel);
    let plan = parse_plan_file(repo.join(plan_rel)).expect("plan fixture should parse");
    let spec = parse_spec_file(repo.join(spec_rel)).expect("spec fixture should parse");
    let plan_fingerprint =
        sha256_hex(&fs::read(repo.join(plan_rel)).expect("plan fixture should be readable"));
    let spec_fingerprint =
        sha256_hex(&fs::read(repo.join(spec_rel)).expect("spec fixture should be readable"));
    let verified_requirement_ids = spec
        .requirements
        .iter()
        .map(|requirement| requirement.id.clone())
        .collect::<Vec<_>>();

    if let Some(parent) = artifact_path.parent() {
        fs::create_dir_all(parent)
            .expect("plan-fidelity artifact parent directory should be creatable");
    }
    fs::write(
        artifact_path,
        format!(
            "## Plan Fidelity Review Summary\n\n**Review Stage:** featureforge:plan-fidelity-review\n**Review Verdict:** pass\n**Reviewed Plan:** `{plan_rel}`\n**Reviewed Plan Revision:** {}\n**Reviewed Plan Fingerprint:** {plan_fingerprint}\n**Reviewed Spec:** `{spec_rel}`\n**Reviewed Spec Revision:** {}\n**Reviewed Spec Fingerprint:** {spec_fingerprint}\n**Reviewer Source:** fresh-context-subagent\n**Reviewer ID:** fixture-plan-fidelity-reviewer\n**Distinct From Stages:** featureforge:writing-plans, featureforge:plan-eng-review\n**Verified Surfaces:** {}\n**Verified Requirement IDs:** {}\n",
            plan.plan_revision,
            spec.spec_revision,
            PLAN_FIDELITY_REQUIRED_SURFACES.join(", "),
            verified_requirement_ids.join(", "),
        ),
    )
    .expect("plan-fidelity artifact should write");
}

pub fn write_current_pass_plan_fidelity_review_artifact_for_plan(repo: &Path, plan_rel: &str) {
    let plan = parse_plan_file(repo.join(plan_rel)).expect("plan fixture should parse");
    let spec_rel = plan.source_spec_path.clone();
    let artifact_rel = default_plan_fidelity_artifact_rel(plan_rel);
    write_current_pass_plan_fidelity_review_artifact(repo, &artifact_rel, plan_rel, &spec_rel);
}

pub fn default_plan_fidelity_artifact_rel(plan_rel: &str) -> PathBuf {
    let plan_stem = Path::new(plan_rel)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("plan");
    PathBuf::from(".featureforge")
        .join("reviews")
        .join(format!("{plan_stem}-plan-fidelity.md"))
}
