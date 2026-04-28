use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

const INTERNAL_RUNTIME_HELPER_HEADER: &str = "//! INTERNAL_RUNTIME_HELPER_TEST: this file intentionally exercises unavailable runtime internals.";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn read_repo_file(rel: &str) -> String {
    fs::read_to_string(repo_root().join(rel))
        .unwrap_or_else(|error| panic!("{rel} should be readable: {error}"))
}

fn hidden_literal(parts: &[&str]) -> String {
    parts.concat()
}

fn concat_source_expr(parts: &[&str]) -> String {
    let quoted_parts = parts
        .iter()
        .map(|part| format!("{part:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("concat!({quoted_parts})")
}

#[test]
fn internal_plan_execution_helpers_are_explicitly_quarantined() {
    let source = read_repo_file("tests/support/plan_execution_direct.rs");

    assert!(
        source.starts_with(INTERNAL_RUNTIME_HELPER_HEADER),
        "plan_execution_direct.rs must start with the internal-only quarantine contract"
    );
    assert!(
        !source.contains("pub fn internal_test_"),
        "plan_execution_direct.rs must not expose ambiguous internal_test_* helpers"
    );
    assert!(
        source.contains("pub fn internal_only_"),
        "plan_execution_direct.rs should keep internal helpers visibly prefixed"
    );
}

#[test]
fn public_cli_json_helper_uses_the_compiled_binary_only() {
    let source = read_repo_file("tests/support/featureforge.rs");
    let helper_start = source
        .find("pub fn run_public_featureforge_cli_json")
        .expect("public CLI JSON helper should exist");
    let helper_body = &source[helper_start
        ..source[helper_start..]
            .find("\n}\n\n")
            .map(|offset| helper_start + offset + 3)
            .expect("public CLI JSON helper body should be bounded")];

    assert!(
        helper_body.contains("run_featureforge_with_env_control_real_cli"),
        "public CLI helper must invoke the compiled featureforge binary"
    );
    for forbidden in denied_helper_calls() {
        assert!(
            !helper_body.contains(&forbidden),
            "public CLI helper must not use internal helper `{forbidden}`"
        );
    }
}

#[test]
fn public_test_files_do_not_use_internal_helpers_or_hidden_commands() {
    let mut violations = Vec::new();
    for file in rust_test_files(&repo_root().join("tests")) {
        let rel = repo_relative(&file);
        let source = fs::read_to_string(&file)
            .unwrap_or_else(|error| panic!("{} should be readable: {error}", file.display()));
        violations.extend(scan_source_for_public_flow_violations(&rel, &source));
    }

    assert!(
        violations.is_empty(),
        "public-flow tests must not use internal helpers or hidden command literals:\n{}",
        violations.join("\n")
    );
}

#[test]
fn scanner_rejects_public_internal_helper_and_hidden_command_fixtures() {
    let helper = hidden_literal(&[
        "internal_only_try_run_",
        "plan_execution_output_direct(repo, state, args, context);",
    ]);
    let helper_name = hidden_literal(&["internal_only_try_run_", "plan_execution_output_direct"]);
    let root_helper = hidden_literal(&["internal_only_try_run_", "root_output_direct"]);
    let wrapper_helper = hidden_literal(&[
        "internal_only_runtime_",
        "pre",
        "flight_gate_json(repo, state, plan, context);",
    ]);
    let split_wrapper_helper =
        hidden_literal(&["internal_only_runtime_", "pre", "flight_gate_json"]);
    let internal_fixture_call = hidden_literal(&["internal_only_", "plan_execution_fixture_json"]);
    let hidden_command = hidden_literal(&["record", "-review-dispatch"]);
    let hidden_flag = hidden_literal(&["--dispatch", "-id"]);
    let concat_hidden_command = concat_source_expr(&["record", "-review-dispatch"]);
    let concat_hidden_flag = concat_source_expr(&["--dispatch", "-id"]);
    let fixture = format!(
        "use crate::support::plan_execution_direct::{helper_name} as aliased_direct_helper;\nuse crate::support::plan_execution_direct::{{\n    {helper_name} as multiline_aliased_direct_helper,\n}};\nconst HIDDEN_COMMANDS: &[&str] = &[\n    \"{hidden_command}\",\n    {concat_hidden_command},\n];\nconst CMD_ALIAS: &str = {concat_hidden_command};\nconst MULTILINE_CMD_ALIAS: &str =\n    {concat_hidden_command};\nlet hidden_flags = [\"{hidden_flag}\", {concat_hidden_flag}];\n\nfn public_wrapper(repo: &Path, state: &Path, plan: &str, context: &str) {{\n    {wrapper_helper}\n}}\n\npub fn\npublic_split_signature_wrapper(repo: &Path, state: &Path, plan: &str, context: &str) {{\n    {wrapper_helper}\n}}\n\nfn public_split_wrapper(repo: &Path, state: &Path, plan: &str, context: &str) {{\n    {split_wrapper_helper}\n        (repo, state, plan, context);\n}}\n\n#[test]\nfn public_fixture() {{\n    {helper}\n    {root_helper}(repo, state, args, context);\n    let direct_helper_alias = {helper_name};\n    let split_direct_helper_alias =\n        {helper_name};\n    aliased_direct_helper(repo, state, args, context);\n    multiline_aliased_direct_helper(repo, state, args, context);\n    direct_helper_alias(repo, state, args, context);\n    split_direct_helper_alias(repo, state, args, context);\n    let plain_hidden_literal = [\"{hidden_command}\"];\n    let dispatch_flag = {concat_hidden_flag};\n    let args_alias = [CMD_ALIAS, dispatch_flag];\n    let multiline_args_alias = [\n        MULTILINE_CMD_ALIAS,\n        {concat_hidden_flag},\n    ];\n    let split_args_alias =\n        [\n            CMD_ALIAS,\n            {concat_hidden_flag},\n        ];\n    let mut pushed_args = Vec::new();\n    pushed_args.push(CMD_ALIAS);\n    pushed_args.extend([{concat_hidden_command}]);\n    pushed_args.extend_from_slice(&multiline_args_alias);\n    let _ = plain_hidden_literal;\n    let _ = run_featureforge(repo, state, &[\n        \"{hidden_command}\",\n        \"{hidden_flag}\",\n    ], context);\n    let _ = run_featureforge(repo, state, &[\n        {concat_hidden_command},\n        {concat_hidden_flag},\n    ], context);\n    let _ = run_featureforge(repo, state, &args_alias, context);\n    let _ = run_featureforge(repo, state, &multiline_args_alias, context);\n    let _ = run_featureforge(repo, state, &split_args_alias, context);\n    let _ = run_featureforge(repo, state, &pushed_args, context);\n    let _child = Command::new(\"featureforge\").arg(CMD_ALIAS).arg(dispatch_flag);\n    let _ = {internal_fixture_call}(repo, state, &[\n        \"{hidden_command}\",\n    ], context);\n}}\n"
    );
    let violations = scan_source_for_public_flow_violations("tests/public_fixture.rs", &fixture);

    assert!(
        violations
            .iter()
            .any(|violation| violation
                .contains(concat!("try_run_", "plan_execution_output_direct("))),
        "scanner should reject public direct-helper calls, got {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("public_wrapper")),
        "scanner should reject public wrappers around internal helpers, got {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("public_split_wrapper")),
        "scanner should reject public wrappers around split-line internal helper calls, got {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("public_split_signature_wrapper")),
        "scanner should reject public split-signature wrappers around internal helpers, got {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("aliased_direct_helper")),
        "scanner should reject use-alias calls to internal helpers, got {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("multiline_aliased_direct_helper")),
        "scanner should reject multiline use-alias calls to internal helpers, got {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("direct_helper_alias")),
        "scanner should reject function-pointer aliases to internal helpers, got {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("split_direct_helper_alias")),
        "scanner should reject split-line function-pointer aliases to internal helpers, got {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains(&hidden_command)),
        "scanner should reject public hidden command invocations, got {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains(&hidden_flag)),
        "scanner should reject public hidden flags, got {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("CMD_ALIAS")),
        "scanner should reject public hidden command aliases, got {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("MULTILINE_CMD_ALIAS")),
        "scanner should reject public multiline hidden command aliases, got {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("args_alias")),
        "scanner should reject public prebuilt hidden arg arrays, got {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("multiline_args_alias")),
        "scanner should reject public multiline prebuilt hidden arg arrays, got {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("split_args_alias")),
        "scanner should reject public split-line hidden arg arrays, got {violations:?}"
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains("pushed_args")),
        "scanner should reject push/extend-built hidden arg arrays, got {violations:?}"
    );
}

#[test]
fn scanner_rejects_public_direct_runtime_surface_wrapper_fixture() {
    let operator_marker = hidden_literal(&["operator_for_", "runtime("]);
    let status_marker = hidden_literal(&[".status", "("]);
    let review_gate_marker = hidden_literal(&[".review_gate", "("]);
    let finish_gate_marker = hidden_literal(&[".finish_gate", "("]);
    let fixture = format!(
        "
fn status_value(runtime: &ExecutionRuntime, args: &StatusArgs) {{
    runtime{status_marker}args);
}}

fn plan_execution_status_json(runtime: &ExecutionRuntime, plan: &str) {{
    runtime{status_marker}&StatusArgs {{ plan: plan.into(), external_review_result_ready: false }});
}}

fn workflow_operator_json(runtime: &ExecutionRuntime, args: &OperatorArgs) {{
    operator::{operator_marker}runtime, args);
}}

fn workflow_gate_review_json(runtime: &ExecutionRuntime, args: &StatusArgs) {{
    runtime{review_gate_marker}args);
}}

fn workflow_gate_finish_json(runtime: &ExecutionRuntime, args: &StatusArgs) {{
    runtime{finish_gate_marker}args);
}}
"
    );
    let violations =
        scan_source_for_public_flow_violations("tests/support/runtime_surfaces.rs", &fixture);

    for wrapper in [
        "status_value",
        "plan_execution_status_json",
        "workflow_operator_json",
        "workflow_gate_review_json",
        "workflow_gate_finish_json",
    ] {
        assert!(
            violations
                .iter()
                .any(|violation| violation.contains(wrapper)
                    && violation.contains("direct runtime surface")),
            "scanner should reject public direct runtime surface wrapper `{wrapper}`, got {violations:?}"
        );
    }
}

#[test]
fn scanner_rejects_public_flow_header_bypass_fixture() {
    let helper = hidden_literal(&[
        "internal_only_try_run_",
        "plan_execution_output_direct(repo, state, args, context);",
    ]);
    let fixture = format!(
        "{INTERNAL_RUNTIME_HELPER_HEADER}\n\n#[test]\nfn public_replay_fixture() {{\n    {helper}\n}}\n"
    );
    let violations =
        scan_source_for_public_flow_violations("tests/public_replay_churn.rs", &fixture);

    assert!(
        violations
            .iter()
            .any(|violation| violation
                .contains("must not use the internal helper quarantine header")),
        "scanner should reject quarantine headers on protected public-flow files, got {violations:?}"
    );
}

#[test]
fn scanner_allows_explicitly_quarantined_internal_file_fixture() {
    let helper = hidden_literal(&[
        "internal_only_try_run_",
        "plan_execution_output_direct(repo, state, args, context);",
    ]);
    let hidden_command = hidden_literal(&["record", "-review-dispatch"]);
    let fixture = format!(
        "{INTERNAL_RUNTIME_HELPER_HEADER}\n\n#[test]\nfn internal_{}_fixture() {{\n    {helper}\n    let _ = &[\"{hidden_command}\"];\n}}\n",
        "only"
    );

    assert!(
        scan_source_for_public_flow_violations("tests/internal_fixture.rs", &fixture).is_empty(),
        "quarantined internal files should be allowed to exercise internal helpers"
    );
}

#[test]
fn scanner_allows_explicit_internal_only_test_quarantine() {
    let helper = hidden_literal(&[
        "internal_only_try_run_",
        "plan_execution_output_direct(repo, state, args, context);",
    ]);
    let hidden_command = hidden_literal(&["record", "-review-dispatch"]);
    let fixture = format!(
        "#[test]\nfn internal_only_compatibility_fixture() {{\n    {helper}\n    let _ = &[\"{hidden_command}\"];\n}}\n"
    );

    assert!(
        scan_source_for_public_flow_violations("tests/public_fixture.rs", &fixture).is_empty(),
        "internal_only_* test scopes are explicit test-level quarantines"
    );
}

fn scan_source_for_public_flow_violations(rel: &str, source: &str) -> Vec<String> {
    let mut violations = Vec::new();
    if file_name_is_internal_quarantine(rel) {
        return Vec::new();
    }
    if has_internal_runtime_helper_header(source) {
        if is_protected_public_flow_file(rel) {
            violations.push(format!(
                "{rel}:1 must not use the internal helper quarantine header on a protected public-flow test surface"
            ));
        } else {
            return Vec::new();
        }
    }

    let denied_helper_calls = denied_helper_calls();
    let mut denied_helper_names = denied_helper_names(&denied_helper_calls);
    denied_helper_names.extend(helper_alias_names(source, &denied_helper_names));
    denied_helper_names.sort();
    denied_helper_names.dedup();
    let denied_hidden_literals = denied_hidden_literals();
    let mut concat_collector = ConcatLiteralCollector::default();
    let mut hidden_string_bindings = HashSet::new();
    let mut hidden_arg_bindings = HashSet::new();
    let mut pending_assignment = None::<PendingAssignment>;
    let mut current_fn = None::<String>;
    let mut current_fn_brace_depth = 0usize;
    let mut current_fn_body_started = false;
    let mut inside_command_invocation = false;
    let mut inside_command_args_array = false;
    let tainted_functions = tainted_runtime_helper_wrappers(source, &denied_helper_names);
    for (line, function_name) in
        public_tainted_runtime_helper_wrappers(source, &denied_helper_names)
    {
        violations.push(format!(
            "{rel}:{line} defines public wrapper `{function_name}` around an internal runtime helper outside an internal-only quarantine or test"
        ));
    }
    for (line, function_name, marker) in public_direct_runtime_surface_wrappers(source) {
        violations.push(format!(
            "{rel}:{line} defines public direct runtime surface wrapper `{function_name}` using `{marker}` outside an internal-only quarantine or test"
        ));
    }
    for (index, line) in source.lines().enumerate() {
        let trimmed = line.trim();
        if let Some(name) = rust_fn_name(trimmed) {
            current_fn = Some(name);
            current_fn_brace_depth = 0;
            current_fn_body_started = false;
        }
        let saw_body_open = update_fn_brace_depth(&mut current_fn_brace_depth, line);
        current_fn_body_started |= saw_body_open;
        if starts_command_invocation(trimmed) {
            inside_command_invocation = true;
            inside_command_args_array =
                starts_command_args_array(trimmed) || contains_inline_command_args_array(trimmed);
        } else if inside_command_invocation && starts_command_args_array(trimmed) {
            inside_command_args_array = true;
        }
        if current_fn
            .as_deref()
            .is_some_and(|name| name.starts_with("internal_only_"))
        {
            if current_fn_body_started && current_fn_brace_depth == 0 {
                current_fn = None;
                current_fn_body_started = false;
            }
            continue;
        }
        let candidate_literals = candidate_string_literals(trimmed, &mut concat_collector);
        let hidden_literal_hits = hidden_literal_hits(&candidate_literals, &denied_hidden_literals);
        let hidden_identifiers = hidden_identifier_hits(trimmed, &hidden_string_bindings);
        let hidden_arg_identifiers = hidden_identifier_hits(trimmed, &hidden_arg_bindings);
        let line_assignment = (assignment_can_bind_hidden_data(trimmed))
            .then(|| assignment_binding_name(trimmed))
            .flatten()
            .map(|binding| PendingAssignment {
                binding,
                start_line: index + 1,
                is_arg_collection: assignment_binds_arg_collection(trimmed),
                saw_hidden_value: false,
                saw_hidden_arg_collection: false,
            });
        let mut assignment_to_finalize = None;
        if pending_assignment.is_none() {
            pending_assignment = line_assignment;
        }
        if let Some(assignment) = pending_assignment.as_mut() {
            assignment.observe_line(
                trimmed,
                !hidden_literal_hits.is_empty() || !hidden_identifiers.is_empty(),
                !hidden_arg_identifiers.is_empty(),
            );
            if assignment_ends(trimmed) {
                assignment_to_finalize = pending_assignment.take();
            }
        }
        if let Some(assignment) = assignment_to_finalize {
            finalize_assignment(
                rel,
                current_fn.as_deref().unwrap_or("<module>"),
                &assignment,
                &mut hidden_string_bindings,
                &mut hidden_arg_bindings,
                &mut violations,
            );
        }
        if let Some(binding) = arg_collection_mutation_binding(trimmed)
            && (!hidden_literal_hits.is_empty()
                || !hidden_identifiers.is_empty()
                || !hidden_arg_identifiers.is_empty())
        {
            hidden_arg_bindings.insert(binding.clone());
            violations.push(format!(
                "{rel}:{} mutates hidden command or flag data into arg collection `{binding}` outside an internal-only quarantine or test in `{}`",
                index + 1,
                current_fn.as_deref().unwrap_or("<module>")
            ));
        }
        for forbidden in &denied_helper_names {
            if calls_named_function(trimmed, forbidden) {
                violations.push(format!(
                    "{rel}:{} uses internal helper `{forbidden}(` outside an internal-only quarantine or test in `{}`",
                    index + 1,
                    current_fn.as_deref().unwrap_or("<module>")
                ));
            }
        }
        for helper_name in &tainted_functions {
            if current_fn.as_ref() != Some(helper_name)
                && calls_named_function(trimmed, helper_name)
            {
                violations.push(format!(
                    "{rel}:{} calls tainted internal runtime helper wrapper `{helper_name}` outside an internal-only quarantine or test in `{}`",
                    index + 1,
                    current_fn.as_deref().unwrap_or("<module>")
                ));
            }
        }
        let hidden_literals_are_executable_args =
            inside_command_invocation || inside_command_args_array;
        for hit in &hidden_literal_hits {
            if hit.always_hidden || hidden_literals_are_executable_args {
                violations.push(format!(
                    "{rel}:{} exposes hidden command or flag literal `{}` outside an internal-only quarantine or test in `{}`",
                    index + 1,
                    hit.literal,
                    current_fn.as_deref().unwrap_or("<module>")
                ));
            }
        }
        if hidden_literals_are_executable_args {
            for identifier in hidden_identifiers {
                violations.push(format!(
                    "{rel}:{} passes hidden command or flag alias `{identifier}` to an executable command outside an internal-only quarantine or test in `{}`",
                    index + 1,
                    current_fn.as_deref().unwrap_or("<module>")
                ));
            }
            for identifier in hidden_arg_identifiers {
                violations.push(format!(
                    "{rel}:{} passes hidden command arg collection `{identifier}` to an executable command outside an internal-only quarantine or test in `{}`",
                    index + 1,
                    current_fn.as_deref().unwrap_or("<module>")
                ));
            }
        }
        if inside_command_args_array && ends_command_args_array(trimmed) {
            inside_command_args_array = false;
        }
        if inside_command_invocation && trimmed.ends_with(");") {
            inside_command_invocation = false;
            inside_command_args_array = false;
        }
        if current_fn_body_started && current_fn_brace_depth == 0 {
            current_fn = None;
            current_fn_body_started = false;
        }
    }
    violations
}

fn file_name_is_internal_quarantine(rel: &str) -> bool {
    let file_name = Path::new(rel)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    file_name.starts_with("internal_")
}

fn has_internal_runtime_helper_header(source: &str) -> bool {
    source.lines().next() == Some(INTERNAL_RUNTIME_HELPER_HEADER)
}

fn is_protected_public_flow_file(rel: &str) -> bool {
    if rel.starts_with("tests/support/") {
        return false;
    }
    let file_name = Path::new(rel)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    matches!(
        file_name,
        "contracts_execution_runtime_boundaries.rs"
            | "execution_harness_state.rs"
            | "execution_query.rs"
            | "liveness_model_checker.rs"
            | "plan_execution.rs"
            | "plan_execution_topology.rs"
            | "public_replay_churn.rs"
            | "workflow_entry_shell_smoke.rs"
            | "workflow_runtime.rs"
            | "workflow_runtime_final_review.rs"
            | "workflow_shell_smoke.rs"
    )
}

fn rust_fn_name(trimmed: &str) -> Option<String> {
    rust_fn_name_from_signature(trimmed)
}

#[derive(Debug)]
struct RustFunctionBody<'a> {
    name: String,
    start_line: usize,
    lines: Vec<&'a str>,
}

fn tainted_runtime_helper_wrappers(
    source: &str,
    denied_helper_names: &[String],
) -> HashSet<String> {
    let functions = rust_function_bodies(source);
    let mut tainted = functions
        .iter()
        .filter(|function| {
            let body = function.lines.join("\n");
            denied_helper_names
                .iter()
                .any(|helper_name| calls_named_function_in_source(&body, helper_name))
        })
        .map(|function| function.name.clone())
        .collect::<HashSet<_>>();

    let mut changed = true;
    while changed {
        changed = false;
        for function in &functions {
            if tainted.contains(&function.name) {
                continue;
            }
            let body = function.lines.join("\n");
            if tainted
                .iter()
                .any(|tainted_function| calls_named_function_in_source(&body, tainted_function))
            {
                changed |= tainted.insert(function.name.clone());
            }
        }
    }

    tainted
}

fn public_tainted_runtime_helper_wrappers(
    source: &str,
    denied_helper_names: &[String],
) -> Vec<(usize, String)> {
    rust_function_bodies(source)
        .into_iter()
        .filter(|function| !function.name.starts_with("internal_only_"))
        .filter(|function| {
            let body = function.lines.join("\n");
            denied_helper_names
                .iter()
                .any(|helper_name| calls_named_function_in_source(&body, helper_name))
        })
        .map(|function| (function.start_line, function.name))
        .collect()
}

fn public_direct_runtime_surface_wrappers(source: &str) -> Vec<(usize, String, String)> {
    rust_function_bodies(source)
        .into_iter()
        .filter(|function| !function.name.starts_with("internal_only_"))
        .filter_map(|function| {
            let body = line_without_string_literals(&function.lines.join("\n"));
            direct_runtime_surface_markers()
                .into_iter()
                .find(|marker| body.contains(marker.as_str()))
                .map(|marker| (function.start_line, function.name, marker))
        })
        .collect()
}

fn direct_runtime_surface_markers() -> Vec<String> {
    vec![
        "operator_for_runtime(".to_owned(),
        ".status(".to_owned(),
        ".review_gate(".to_owned(),
        ".finish_gate(".to_owned(),
    ]
}

fn helper_alias_names(source: &str, denied_helper_names: &[String]) -> Vec<String> {
    let mut aliases = Vec::new();
    for statement in use_statements(source) {
        let code = line_without_string_literals(&statement);
        for helper_name in denied_helper_names {
            aliases.extend(use_alias_names(&code, helper_name));
        }
    }
    for (_, statement) in assignment_statements(source) {
        let code = line_without_string_literals(&statement);
        for helper_name in denied_helper_names {
            if let Some(alias) = function_pointer_alias_name(&code, helper_name) {
                aliases.push(alias);
            }
        }
    }
    aliases
}

fn use_statements(source: &str) -> Vec<String> {
    collect_multiline_statements(source, |trimmed| trimmed.starts_with("use "))
        .into_iter()
        .map(|(_, statement)| statement)
        .collect()
}

fn assignment_statements(source: &str) -> Vec<(usize, String)> {
    collect_multiline_statements(source, |trimmed| {
        assignment_binding_name(trimmed).is_some()
            || matches!(
                trimmed.split_whitespace().next(),
                Some("let" | "const" | "static")
            )
    })
}

fn collect_multiline_statements(
    source: &str,
    starts_statement: impl Fn(&str) -> bool,
) -> Vec<(usize, String)> {
    let mut statements = Vec::new();
    let mut pending = None::<(usize, String)>;
    for (index, line) in source.lines().enumerate() {
        let trimmed = line.trim();
        if pending.is_none() && starts_statement(trimmed) {
            pending = Some((index + 1, String::new()));
        }
        if let Some((start_line, statement)) = pending.as_mut() {
            statement.push_str(line);
            statement.push('\n');
            if trimmed.ends_with(';') {
                statements.push((*start_line, std::mem::take(statement)));
                pending = None;
            }
        }
    }
    statements
}

fn use_alias_names(code: &str, helper_name: &str) -> Vec<String> {
    let trimmed = code.trim();
    if !trimmed.starts_with("use ") || !contains_identifier(trimmed, helper_name) {
        return Vec::new();
    }
    let mut aliases = Vec::new();
    let mut offset = 0usize;
    while let Some(relative_start) = trimmed[offset..].find(helper_name) {
        let start = offset + relative_start;
        let end = start + helper_name.len();
        if !identifier_at(trimmed, start, helper_name) {
            offset = end;
            continue;
        }
        let rest = &trimmed[end..];
        let statement_segment_end = rest.find([',', ';', '}']).unwrap_or(rest.len());
        let statement_segment = &rest[..statement_segment_end];
        if let Some((_, alias)) = statement_segment.split_once(" as ")
            && let Some(alias) = first_identifier(alias)
        {
            aliases.push(alias.to_owned());
        }
        offset = end;
    }
    aliases
}

fn function_pointer_alias_name(code: &str, helper_name: &str) -> Option<String> {
    let trimmed = code.trim();
    let (left, right) = trimmed.split_once('=')?;
    if !contains_identifier(right, helper_name)
        || calls_named_function_in_source(right, helper_name)
    {
        return None;
    }
    assignment_binding_name(left.trim()).or_else(|| assignment_binding_name(trimmed))
}

fn contains_identifier(source: &str, identifier: &str) -> bool {
    let mut offset = 0usize;
    while let Some(relative_start) = source[offset..].find(identifier) {
        let start = offset + relative_start;
        let end = start + identifier.len();
        if identifier_at(source, start, identifier) {
            return true;
        }
        offset = end;
    }
    false
}

fn identifier_at(source: &str, start: usize, identifier: &str) -> bool {
    let end = start + identifier.len();
    let previous_is_identifier = start > 0
        && source[..start]
            .chars()
            .next_back()
            .is_some_and(is_rust_identifier_character);
    let next_is_identifier = source[end..]
        .chars()
        .next()
        .is_some_and(is_rust_identifier_character);
    !previous_is_identifier && !next_is_identifier
}

fn first_identifier(source: &str) -> Option<&str> {
    identifier_tokens(source).next()
}

fn denied_helper_names(denied_helper_calls: &[String]) -> Vec<String> {
    denied_helper_calls
        .iter()
        .map(|call| call.trim_end_matches('(').to_owned())
        .collect()
}

#[derive(Debug)]
struct PendingAssignment {
    binding: String,
    start_line: usize,
    is_arg_collection: bool,
    saw_hidden_value: bool,
    saw_hidden_arg_collection: bool,
}

impl PendingAssignment {
    fn observe_line(
        &mut self,
        trimmed: &str,
        saw_hidden_value: bool,
        saw_hidden_arg_collection: bool,
    ) {
        self.is_arg_collection |= assignment_binds_arg_collection(trimmed);
        self.saw_hidden_value |= saw_hidden_value;
        self.saw_hidden_arg_collection |= saw_hidden_arg_collection;
    }
}

fn finalize_assignment(
    rel: &str,
    current_scope: &str,
    assignment: &PendingAssignment,
    hidden_string_bindings: &mut HashSet<String>,
    hidden_arg_bindings: &mut HashSet<String>,
    violations: &mut Vec<String>,
) {
    if assignment.saw_hidden_value {
        if assignment.is_arg_collection {
            hidden_arg_bindings.insert(assignment.binding.clone());
        } else {
            hidden_string_bindings.insert(assignment.binding.clone());
        }
        violations.push(format!(
            "{rel}:{} binds hidden command or flag data to `{}` outside an internal-only quarantine or test in `{current_scope}`",
            assignment.start_line, assignment.binding
        ));
    } else if assignment.is_arg_collection && assignment.saw_hidden_arg_collection {
        hidden_arg_bindings.insert(assignment.binding.clone());
        violations.push(format!(
            "{rel}:{} aliases hidden command arg collection as `{}` outside an internal-only quarantine or test in `{current_scope}`",
            assignment.start_line, assignment.binding
        ));
    }
}

fn assignment_ends(trimmed: &str) -> bool {
    trimmed.ends_with(';')
}

fn rust_function_bodies(source: &str) -> Vec<RustFunctionBody<'_>> {
    let mut functions = Vec::new();
    let mut current_name = None::<String>;
    let mut current_start_line = 0usize;
    let mut current_lines = Vec::new();
    let mut brace_depth = 0usize;
    let mut body_started = false;
    let mut pending_signature = None::<PendingFunctionSignature>;

    for (index, line) in source.lines().enumerate() {
        if current_name.is_none() {
            let code = line_without_string_literals(line);
            if pending_signature.is_none() && contains_fn_keyword(&code) {
                pending_signature = Some(PendingFunctionSignature {
                    start_line: index + 1,
                    lines: Vec::new(),
                });
            }
            if let Some(signature) = pending_signature.as_mut() {
                signature.lines.push(line);
                let signature_source = signature.lines.join("\n");
                if signature_source.contains(';') && !signature_source.contains('{') {
                    pending_signature = None;
                    continue;
                }
                if signature_source.contains('{')
                    && let Some(name) = rust_fn_name_from_signature(&signature_source)
                {
                    current_name = Some(name);
                    current_start_line = signature.start_line;
                    current_lines = std::mem::take(&mut signature.lines);
                    pending_signature = None;
                    brace_depth = 0;
                    body_started = false;
                    for function_line in &current_lines {
                        body_started |= update_fn_brace_depth(&mut brace_depth, function_line);
                    }
                }
            }
        }

        if current_name.is_some() {
            if pending_signature.is_none()
                && current_lines
                    .last()
                    .is_none_or(|last_line| !std::ptr::eq(*last_line, line))
            {
                current_lines.push(line);
                body_started |= update_fn_brace_depth(&mut brace_depth, line);
            }
            if body_started && brace_depth == 0 {
                functions.push(RustFunctionBody {
                    name: current_name
                        .take()
                        .expect("current function should be present"),
                    start_line: current_start_line,
                    lines: std::mem::take(&mut current_lines),
                });
                body_started = false;
            }
        }
    }

    functions
}

#[derive(Debug)]
struct PendingFunctionSignature<'a> {
    start_line: usize,
    lines: Vec<&'a str>,
}

fn contains_fn_keyword(source: &str) -> bool {
    identifier_tokens(source).any(|token| token == "fn")
}

fn rust_fn_name_from_signature(signature: &str) -> Option<String> {
    let code = line_without_string_literals(signature);
    let bytes = code.as_bytes();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        let relative_start = code[cursor..].find("fn")?;
        let start = cursor + relative_start;
        let end = start + "fn".len();
        let previous_is_identifier = start > 0
            && code[..start]
                .chars()
                .next_back()
                .is_some_and(is_rust_identifier_character);
        let next_is_identifier = code[end..]
            .chars()
            .next()
            .is_some_and(is_rust_identifier_character);
        if previous_is_identifier || next_is_identifier {
            cursor = end;
            continue;
        }

        let mut name_start = end;
        while code[name_start..]
            .chars()
            .next()
            .is_some_and(char::is_whitespace)
        {
            name_start += code[name_start..]
                .chars()
                .next()
                .expect("checked next char should exist")
                .len_utf8();
        }
        let first = code[name_start..].chars().next()?;
        if !(first == '_' || first.is_ascii_alphabetic()) {
            cursor = end;
            continue;
        }
        let mut name_end = name_start + first.len_utf8();
        while code[name_end..]
            .chars()
            .next()
            .is_some_and(is_rust_identifier_character)
        {
            name_end += code[name_end..]
                .chars()
                .next()
                .expect("checked next char should exist")
                .len_utf8();
        }
        let name = &code[name_start..name_end];
        let after_name = code[name_end..].trim_start();
        if after_name.starts_with('(') || after_name.starts_with('<') {
            return Some(name.to_owned());
        }
        cursor = name_end;
    }
    None
}

fn update_fn_brace_depth(depth: &mut usize, line: &str) -> bool {
    let opens = line.chars().filter(|character| *character == '{').count();
    let closes = line.chars().filter(|character| *character == '}').count();
    *depth = depth.saturating_add(opens).saturating_sub(closes);
    opens > 0
}

#[derive(Debug)]
struct HiddenLiteralHit {
    literal: String,
    always_hidden: bool,
}

fn hidden_literal_hits(
    candidate_literals: &[CandidateLiteral],
    denied_hidden_literals: &[String],
) -> Vec<HiddenLiteralHit> {
    let mut hits = Vec::new();
    for literal in denied_hidden_literals {
        for candidate in candidate_literals {
            if candidate.value == *literal {
                hits.push(HiddenLiteralHit {
                    literal: literal.clone(),
                    always_hidden: candidate.kind == CandidateLiteralKind::Raw,
                });
                break;
            }
            if shell_command_mentions_hidden_literal(&candidate.value, literal)
                || literal.starts_with("--")
                    && shell_words(&candidate.value).any(|word| word == literal)
            {
                hits.push(HiddenLiteralHit {
                    literal: literal.clone(),
                    always_hidden: candidate.kind == CandidateLiteralKind::Raw,
                });
                break;
            }
        }
    }
    hits
}

fn hidden_identifier_hits(line: &str, hidden_bindings: &HashSet<String>) -> Vec<String> {
    identifier_tokens(line)
        .filter(|identifier| hidden_bindings.contains(*identifier))
        .map(str::to_owned)
        .collect()
}

fn identifier_tokens(line: &str) -> impl Iterator<Item = &str> {
    line.split(|character: char| !is_rust_identifier_character(character))
        .filter(|token| {
            !token.is_empty() && !token.chars().all(|character| character.is_ascii_digit())
        })
}

fn is_rust_identifier_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

fn assignment_binding_name(trimmed: &str) -> Option<String> {
    let rest = trimmed
        .strip_prefix("const ")
        .or_else(|| trimmed.strip_prefix("static "))
        .or_else(|| trimmed.strip_prefix("let "))?;
    let rest = rest.strip_prefix("mut ").unwrap_or(rest);
    let name = rest.split([':', '=', '[', ' ', '\t', ',', ';']).next()?;
    if name.is_empty() || !name.chars().all(is_rust_identifier_character) {
        return None;
    }
    Some(name.to_owned())
}

fn assignment_can_bind_hidden_data(trimmed: &str) -> bool {
    let Some((left, right)) = trimmed.split_once('=') else {
        return false;
    };
    let right = right.trim_start();
    if left.contains("&str") || left.contains("String") {
        return true;
    }
    right.is_empty()
        || right.starts_with('"')
        || right.starts_with("concat!(")
        || right.starts_with('[')
        || right.starts_with("&[")
        || right.starts_with("vec![")
}

fn assignment_binds_arg_collection(trimmed: &str) -> bool {
    trimmed.contains('[') || trimmed.contains("vec![")
}

fn arg_collection_mutation_binding(trimmed: &str) -> Option<String> {
    [".push(", ".extend(", ".extend_from_slice("]
        .iter()
        .find_map(|needle| {
            let (receiver, _) = trimmed.split_once(needle)?;
            receiver
                .split(|character: char| !is_rust_identifier_character(character))
                .rfind(|token| !token.is_empty())
                .map(str::to_owned)
        })
}

fn shell_command_mentions_hidden_literal(string_literal: &str, literal: &str) -> bool {
    [
        format!("featureforge plan execution {literal}"),
        format!("plan execution {literal}"),
        format!("featureforge workflow {literal}"),
        format!("workflow {literal}"),
    ]
    .iter()
    .any(|needle| string_literal.contains(needle))
}

fn shell_words(string_literal: &str) -> impl Iterator<Item = &str> {
    string_literal.split_whitespace().map(|word| {
        word.trim_matches(|character: char| {
            matches!(
                character,
                ',' | ';' | ':' | ')' | '(' | '[' | ']' | '{' | '}' | '\'' | '"' | '`'
            )
        })
    })
}

#[derive(Default)]
struct ConcatLiteralCollector {
    active: bool,
    depth: usize,
    value: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CandidateLiteralKind {
    Raw,
    Concat,
}

#[derive(Clone, Debug)]
struct CandidateLiteral {
    value: String,
    kind: CandidateLiteralKind,
}

fn candidate_string_literals(
    line: &str,
    concat_collector: &mut ConcatLiteralCollector,
) -> Vec<CandidateLiteral> {
    let mut candidates = string_literals(line)
        .into_iter()
        .map(|value| CandidateLiteral {
            value,
            kind: CandidateLiteralKind::Raw,
        })
        .collect::<Vec<_>>();
    candidates.extend(
        concat_collector
            .collect(line)
            .into_iter()
            .map(|value| CandidateLiteral {
                value,
                kind: CandidateLiteralKind::Concat,
            }),
    );
    candidates
}

impl ConcatLiteralCollector {
    fn collect(&mut self, line: &str) -> Vec<String> {
        let mut values = Vec::new();
        let mut cursor = 0;
        while cursor < line.len() {
            if self.active {
                cursor = self.collect_active(line, cursor, &mut values);
                continue;
            }
            let Some(relative_start) = line[cursor..].find("concat!(") else {
                break;
            };
            let start = cursor + relative_start;
            self.active = true;
            self.depth = 1;
            self.value.clear();
            cursor = self.collect_active(line, start + "concat!(".len(), &mut values);
        }
        values
    }

    fn collect_active(&mut self, line: &str, mut cursor: usize, values: &mut Vec<String>) -> usize {
        let bytes = line.as_bytes();
        while cursor < bytes.len() {
            match bytes[cursor] {
                b'"' => {
                    cursor += 1;
                    while cursor < bytes.len() {
                        match bytes[cursor] {
                            b'\\' => {
                                if cursor + 1 < bytes.len() {
                                    self.value.push(bytes[cursor + 1] as char);
                                    cursor += 2;
                                } else {
                                    cursor += 1;
                                }
                            }
                            b'"' => {
                                cursor += 1;
                                break;
                            }
                            byte => {
                                self.value.push(byte as char);
                                cursor += 1;
                            }
                        }
                    }
                }
                b'(' => {
                    self.depth += 1;
                    cursor += 1;
                }
                b')' => {
                    self.depth = self.depth.saturating_sub(1);
                    cursor += 1;
                    if self.depth == 0 {
                        values.push(self.value.clone());
                        self.active = false;
                        self.value.clear();
                        return cursor;
                    }
                }
                _ => {
                    cursor += 1;
                }
            }
        }
        cursor
    }
}

fn calls_named_function(trimmed: &str, function_name: &str) -> bool {
    if trimmed.starts_with("fn ") || trimmed.starts_with("mod ") || trimmed.contains("concat!(") {
        return false;
    }
    calls_named_function_in_source(trimmed, function_name)
}

fn calls_named_function_in_source(source: &str, function_name: &str) -> bool {
    let code = line_without_string_literals(source);
    let mut offset = 0usize;
    while let Some(relative_start) = code[offset..].find(function_name) {
        let start = offset + relative_start;
        if start > 0 {
            let previous = code[..start]
                .chars()
                .next_back()
                .expect("start > 0 should have previous char");
            if previous.is_ascii_alphanumeric() || previous == '_' {
                offset = start + function_name.len();
                continue;
            }
        }
        let rest = &code[start + function_name.len()..];
        if rest
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphanumeric() || character == '_')
        {
            offset = start + function_name.len();
            continue;
        }
        if rest.trim_start().starts_with('(') {
            return true;
        }
        offset = start + function_name.len();
    }
    false
}

fn starts_command_invocation(trimmed: &str) -> bool {
    !trimmed.starts_with("fn ")
        && [
            "run_featureforge(",
            "run_featureforge_json(",
            "run_featureforge_real_cli(",
            "run_featureforge_with_env_json(",
            "run_featureforge_json_real_cli(",
            "run_plan_execution(",
            "run_plan_execution_json(",
            "run_plan_execution_json_real_cli(",
            concat!("internal_only_", "plan_execution_fixture_json("),
            "run_public_featureforge_cli_json(",
            "run_public_cli(",
            "run_shell(",
            "run_shell_json(",
            "run_rust(",
            "run_rust_json(",
            "run_rust_with_env(",
            ".arg(",
            ".args(",
            ".args([",
            ".args(&[",
            ".push(",
            ".extend(",
            ".extend_from_slice(",
        ]
        .iter()
        .any(|needle| trimmed.contains(needle))
}

fn starts_command_args_array(trimmed: &str) -> bool {
    trimmed.starts_with("&[")
        || trimmed.starts_with('[')
        || trimmed.contains(".args([")
        || trimmed.contains(".args(&[")
}

fn contains_inline_command_args_array(trimmed: &str) -> bool {
    trimmed.contains("&[") || trimmed.contains(".args([") || trimmed.contains(".args(&[")
}

fn ends_command_args_array(trimmed: &str) -> bool {
    trimmed.starts_with(']') || trimmed.contains("],") || trimmed.contains("])")
}

fn string_literals(line: &str) -> Vec<String> {
    let mut literals = Vec::new();
    let mut chars = line.char_indices().peekable();
    while let Some((_, character)) = chars.next() {
        if character != '"' {
            continue;
        }
        let mut literal = String::new();
        let mut escaped = false;
        for (_, next) in chars.by_ref() {
            if escaped {
                literal.push(next);
                escaped = false;
                continue;
            }
            match next {
                '\\' => escaped = true,
                '"' => {
                    literals.push(literal);
                    break;
                }
                _ => literal.push(next),
            }
        }
    }
    literals
}

fn line_without_string_literals(line: &str) -> String {
    let mut stripped = String::with_capacity(line.len());
    let mut chars = line.chars();
    let mut in_string = false;
    let mut escaped = false;
    for character in chars.by_ref() {
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
                stripped.push('"');
            } else {
                stripped.push(' ');
            }
            continue;
        }
        if character == '"' {
            in_string = true;
        }
        stripped.push(character);
    }
    stripped
}

fn denied_helper_calls() -> Vec<String> {
    vec![
        concat!("run_", "rust_featureforge(").to_owned(),
        concat!("run_", "rust_featureforge_with_env_control(").to_owned(),
        concat!("try_run_", "plan_execution_output_direct(").to_owned(),
        concat!("try_run_", "root_output_direct(").to_owned(),
        concat!("try_run_", "workflow_output_direct(").to_owned(),
        concat!("internal_only_try_run_", "plan_execution_output_direct(").to_owned(),
        concat!("internal_only_try_run_", "root_output_direct(").to_owned(),
        concat!("internal_only_try_run_", "workflow_output_direct(").to_owned(),
        concat!("internal_only_run_", "featureforge_direct_or_cli(").to_owned(),
        concat!(
            "internal_only_run_",
            "featureforge_with_env_control_direct_or_cli("
        )
        .to_owned(),
        concat!("internal_only_run_", "rust_direct_or_cli(").to_owned(),
        concat!("internal_only_run_", "rust_json_direct_or_cli(").to_owned(),
        concat!("internal_only_run_", "plan_execution_json_direct_or_cli(").to_owned(),
        hidden_literal(&["internal_only_runtime_", "pre", "flight_gate_json("]),
        concat!("internal_only_runtime_", "review_gate_json(").to_owned(),
        concat!("internal_only_runtime_", "finish_gate_json(").to_owned(),
        concat!("internal_only_runtime_", "review_dispatch_authority_json(").to_owned(),
        hidden_literal(&["internal_only_unit_", "plan_execution_pre", "flight_json("]),
        concat!("internal_only_", "plan_execution_fixture_json(").to_owned(),
        hidden_literal(&[
            "internal_only_compatibility_",
            "workflow_pre",
            "flight_json(",
        ]),
        concat!("internal_only_compatibility_", "workflow_gate_review_json(").to_owned(),
        concat!("internal_only_compatibility_", "workflow_gate_finish_json(").to_owned(),
        hidden_literal(&["internal_only_workflow_", "pre", "flight_output("]),
        concat!("internal_only_workflow_", "gate_review_output(").to_owned(),
        concat!("internal_only_workflow_", "gate_finish_output(").to_owned(),
    ]
}

fn denied_hidden_literals() -> Vec<String> {
    vec![
        hidden_literal(&["pre", "flight"]),
        hidden_literal(&["gate", "-review"]),
        hidden_literal(&["gate", "-finish"]),
        hidden_literal(&["record", "-review-dispatch"]),
        hidden_literal(&["record", "-branch-closure"]),
        hidden_literal(&["record", "-release-readiness"]),
        hidden_literal(&["record", "-final-review"]),
        hidden_literal(&["record", "-qa"]),
        hidden_literal(&["rebuild", "-evidence"]),
        hidden_literal(&["explain", "-review-state"]),
        hidden_literal(&["reconcile", "-review-state"]),
        hidden_literal(&["--dispatch", "-id"]),
        hidden_literal(&["--branch", "-closure-id"]),
        hidden_literal(&["FEATUREFORGE", "_ALLOW_INTERNAL_EXECUTION_FLAGS"]),
    ]
}

fn rust_test_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rust_test_files(root, &mut files);
    files.sort();
    files
}

fn collect_rust_test_files(dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap_or_else(|error| {
        panic!("{} should be readable: {error}", dir.display());
    }) {
        let entry = entry.expect("test directory entry should be readable");
        let path = entry.path();
        if path.is_dir() {
            collect_rust_test_files(&path, files);
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
