//! Regenerates checked-in schema artifacts from runtime writers.
use std::path::Path;

use featureforge::contracts::packet::write_contract_schemas;
use featureforge::execution::state::write_plan_execution_schema;
use featureforge::repo_safety::write_repo_safety_schema;
use featureforge::runtime_root::write_runtime_root_schema;
use featureforge::update_check::write_update_check_schema;
use featureforge::workflow::status::write_workflow_schemas;

fn main() {
    if let Err(error) = regenerate(Path::new("schemas")) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn regenerate(output_dir: &Path) -> Result<(), String> {
    write_contract_schemas(output_dir).map_err(|error| error.message().to_owned())?;
    write_plan_execution_schema(output_dir).map_err(|error| error.message)?;
    write_repo_safety_schema(output_dir).map_err(|error| error.message().to_owned())?;
    write_runtime_root_schema(output_dir).map_err(|error| error.message().to_owned())?;
    write_update_check_schema(output_dir).map_err(|error| error.message().to_owned())?;
    write_workflow_schemas(output_dir).map_err(|error| error.message().to_owned())?;
    Ok(())
}
