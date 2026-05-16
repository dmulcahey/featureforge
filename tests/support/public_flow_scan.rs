#![allow(dead_code)]

use std::collections::HashSet;
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::rust_source_scan;
use syn::visit::{self, Visit};

#[path = "hidden_public_commands.rs"]
mod hidden_public_commands;

pub const INTERNAL_RUNTIME_HELPER_HEADER: &str = "//! INTERNAL_RUNTIME_HELPER_TEST: this file intentionally exercises unavailable runtime internals.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublicFlowExceptionCategory {
    InternalSemanticComparison,
    RemovedCommandRejection,
    ScannerSelfTest,
    SyntheticFixtureSetup,
    FocusedContractCoverage,
}

impl PublicFlowExceptionCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InternalSemanticComparison => "internal_semantic_comparison",
            Self::RemovedCommandRejection => "removed_command_rejection",
            Self::ScannerSelfTest => "scanner_self_test",
            Self::SyntheticFixtureSetup => "synthetic_fixture_setup",
            Self::FocusedContractCoverage => "focused_contract_coverage",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublicRuntimeFlowGateCategory {
    ExecutablePublicFlowProof,
    MixedPublicAndInternalSemantic,
    FocusedPublicContract,
    StaticPublicGuard,
}

impl PublicRuntimeFlowGateCategory {
    pub const ALL: [Self; 4] = [
        Self::ExecutablePublicFlowProof,
        Self::MixedPublicAndInternalSemantic,
        Self::FocusedPublicContract,
        Self::StaticPublicGuard,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ExecutablePublicFlowProof => "executable_public_flow_proof",
            Self::MixedPublicAndInternalSemantic => "mixed_public_and_internal_semantic",
            Self::FocusedPublicContract => "focused_public_contract",
            Self::StaticPublicGuard => "static_public_guard",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PublicRuntimeFlowGateEntry {
    pub binary: &'static str,
    pub category: PublicRuntimeFlowGateCategory,
    pub proof_scope: &'static str,
}

pub const PUBLIC_RUNTIME_FLOW_GATE_ENTRIES: &[PublicRuntimeFlowGateEntry] = &[
    PublicRuntimeFlowGateEntry {
        binary: "public_cli_flow_contracts",
        category: PublicRuntimeFlowGateCategory::StaticPublicGuard,
        proof_scope: "static scanner and public/private command drift guard",
    },
    PublicRuntimeFlowGateEntry {
        binary: "public_replay_churn",
        category: PublicRuntimeFlowGateCategory::ExecutablePublicFlowProof,
        proof_scope: "compiled public-runtime replay coverage for historical churn and reentry dead ends",
    },
    PublicRuntimeFlowGateEntry {
        binary: "runtime_behavior_golden",
        category: PublicRuntimeFlowGateCategory::FocusedPublicContract,
        proof_scope: "focused public JSON/operator route contract capture through the public argv/parser runner",
    },
    PublicRuntimeFlowGateEntry {
        binary: "workflow_shell_smoke",
        category: PublicRuntimeFlowGateCategory::ExecutablePublicFlowProof,
        proof_scope: "compiled CLI/operator smoke coverage for workflow transitions and public command reachability",
    },
    PublicRuntimeFlowGateEntry {
        binary: "workflow_entry_shell_smoke",
        category: PublicRuntimeFlowGateCategory::MixedPublicAndInternalSemantic,
        proof_scope: "mixed compiled entry-route parity and internal semantic boundary coverage",
    },
    PublicRuntimeFlowGateEntry {
        binary: "workflow_runtime",
        category: PublicRuntimeFlowGateCategory::MixedPublicAndInternalSemantic,
        proof_scope: "mixed workflow routing semantics with public route parity assertions",
    },
    PublicRuntimeFlowGateEntry {
        binary: "workflow_runtime_final_review",
        category: PublicRuntimeFlowGateCategory::ExecutablePublicFlowProof,
        proof_scope: "compiled public route coverage for final-review and late-stage progression",
    },
    PublicRuntimeFlowGateEntry {
        binary: "plan_execution",
        category: PublicRuntimeFlowGateCategory::MixedPublicAndInternalSemantic,
        proof_scope: "mixed public command and routing coverage with internal compatibility quarantine",
    },
    PublicRuntimeFlowGateEntry {
        binary: "plan_execution_final_review",
        category: PublicRuntimeFlowGateCategory::FocusedPublicContract,
        proof_scope: "focused final-review public contract coverage and parser realism",
    },
    PublicRuntimeFlowGateEntry {
        binary: "plan_execution_topology",
        category: PublicRuntimeFlowGateCategory::FocusedPublicContract,
        proof_scope: "focused execution-topology public contract coverage",
    },
    PublicRuntimeFlowGateEntry {
        binary: "contracts_execution_runtime_boundaries",
        category: PublicRuntimeFlowGateCategory::MixedPublicAndInternalSemantic,
        proof_scope: "mixed runtime-authority boundary contracts and compiled route assertions",
    },
    PublicRuntimeFlowGateEntry {
        binary: "execution_query",
        category: PublicRuntimeFlowGateCategory::MixedPublicAndInternalSemantic,
        proof_scope: "mixed query/read-model semantics with public route parity assertions",
    },
    PublicRuntimeFlowGateEntry {
        binary: "execution_harness_state",
        category: PublicRuntimeFlowGateCategory::FocusedPublicContract,
        proof_scope: "focused execution-harness state contract coverage",
    },
];

pub fn public_runtime_flow_gate_entries() -> &'static [PublicRuntimeFlowGateEntry] {
    PUBLIC_RUNTIME_FLOW_GATE_ENTRIES
}

pub fn public_runtime_flow_gate_category(binary: &str) -> Option<PublicRuntimeFlowGateCategory> {
    PUBLIC_RUNTIME_FLOW_GATE_ENTRIES
        .iter()
        .find_map(|entry| (entry.binary == binary).then_some(entry.category))
}

pub fn public_runtime_flow_required_test_binaries() -> Vec<String> {
    let mut binaries = PUBLIC_RUNTIME_FLOW_GATE_ENTRIES
        .iter()
        .map(|entry| entry.binary.to_owned())
        .collect::<Vec<_>>();
    binaries.sort();
    binaries.dedup();
    binaries
}

pub fn protected_public_flow_test_files_from_contract() -> &'static HashSet<String> {
    static PROTECTED_PUBLIC_FLOW_TEST_FILES: OnceLock<HashSet<String>> = OnceLock::new();
    PROTECTED_PUBLIC_FLOW_TEST_FILES.get_or_init(|| {
        PUBLIC_RUNTIME_FLOW_GATE_ENTRIES
            .iter()
            .map(|entry| format!("tests/{}.rs", entry.binary))
            .collect()
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PublicFlowException {
    pub category: PublicFlowExceptionCategory,
    pub explanation: &'static str,
}

fn public_flow_exception(
    category: PublicFlowExceptionCategory,
    explanation: &'static str,
) -> PublicFlowException {
    PublicFlowException {
        category,
        explanation,
    }
}

#[derive(Clone, Copy)]
enum ForbiddenTestHelperSymbol {
    Exact(&'static str),
    Prefix(&'static str),
}

impl ForbiddenTestHelperSymbol {
    fn label(self) -> &'static str {
        match self {
            Self::Exact(symbol) | Self::Prefix(symbol) => symbol,
        }
    }

    fn matches(self, symbol: &str) -> bool {
        match self {
            Self::Exact(expected) => symbol == expected,
            Self::Prefix(prefix) => symbol.starts_with(prefix),
        }
    }
}

// Keep this scanner narrow. It protects helper-support files from exposing
// retired public-command shims, but the check is symbol-based so historical
// comments or explanatory strings do not become routing-policy failures.
const PUBLIC_COMMAND_BOUNDARY_FORBIDDEN_TEST_HELPER_SYMBOLS: &[(
    &str,
    &[ForbiddenTestHelperSymbol],
)] = &[
    (
        "tests/support/workflow_direct.rs",
        &[
            ForbiddenTestHelperSymbol::Exact("LegacyWorkflowCli"),
            ForbiddenTestHelperSymbol::Exact("LegacyWorkflowCommand"),
            ForbiddenTestHelperSymbol::Exact("allow_legacy_removed_commands"),
            ForbiddenTestHelperSymbol::Exact("WorkflowPlanFidelityCli"),
            ForbiddenTestHelperSymbol::Exact("record_plan_fidelity_receipt_with_state_dir"),
        ],
    ),
    (
        "tests/support/plan_execution_direct.rs",
        &[
            ForbiddenTestHelperSymbol::Prefix("run_runtime_"),
            ForbiddenTestHelperSymbol::Prefix("run_internal_"),
            ForbiddenTestHelperSymbol::Exact("run_record_plan_fidelity"),
            ForbiddenTestHelperSymbol::Exact("record_plan_fidelity_receipt_with_state_dir"),
        ],
    ),
];

pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

pub fn read_repo_file(rel: &str) -> String {
    fs::read_to_string(repo_root().join(rel))
        .unwrap_or_else(|error| panic!("{rel} should be readable: {error}"))
}

pub fn hidden_literal(parts: &[&str]) -> String {
    parts.concat()
}

pub fn public_flow_hidden_command_or_flag_literals() -> Vec<String> {
    hidden_public_commands::public_flow_hidden_command_or_flag_literals()
}

pub fn low_level_late_stage_recorder_tokens() -> &'static [&'static str] {
    hidden_public_commands::LOW_LEVEL_LATE_STAGE_RECORDER_TOKENS
}

pub fn public_command_boundary_forbidden_test_helper_violations(repo_root: &Path) -> Vec<String> {
    let mut violations = Vec::new();
    for (relative, _) in PUBLIC_COMMAND_BOUNDARY_FORBIDDEN_TEST_HELPER_SYMBOLS {
        let source = fs::read_to_string(repo_root.join(relative))
            .unwrap_or_else(|error| panic!("{relative} should be readable: {error}"));
        violations.extend(
            public_command_boundary_forbidden_test_helper_violations_for_source(relative, &source),
        );
    }
    violations
}

pub fn public_command_boundary_forbidden_test_helper_violations_for_source(
    rel: &str,
    source: &str,
) -> Vec<String> {
    let Some((_, forbidden_symbols)) = PUBLIC_COMMAND_BOUNDARY_FORBIDDEN_TEST_HELPER_SYMBOLS
        .iter()
        .find(|(candidate_rel, _)| *candidate_rel == rel)
    else {
        return Vec::new();
    };

    let source_symbols = public_command_boundary_test_helper_symbols(rel, source);
    let mut violations = Vec::new();
    for forbidden_symbol in *forbidden_symbols {
        if source_symbols
            .iter()
            .any(|source_symbol| forbidden_symbol.matches(source_symbol))
        {
            violations.push(format!(
                "{rel} exposes forbidden helper symbol `{}`",
                forbidden_symbol.label()
            ));
        }
    }
    violations
}

fn public_command_boundary_test_helper_symbols(rel: &str, source: &str) -> HashSet<String> {
    let syntax = rust_source_scan::parse_rust_source(rel, source);
    let mut collector = PublicCommandBoundarySymbolCollector::default();
    collector.visit_file(&syntax);
    collector.symbols
}

#[derive(Default)]
struct PublicCommandBoundarySymbolCollector {
    symbols: HashSet<String>,
}

impl PublicCommandBoundarySymbolCollector {
    fn record_ident(&mut self, ident: &syn::Ident) {
        self.symbols
            .insert(canonical_rust_symbol(&ident.to_string()));
    }
}

impl<'ast> Visit<'ast> for PublicCommandBoundarySymbolCollector {
    fn visit_ident(&mut self, ident: &'ast syn::Ident) {
        self.record_ident(ident);
    }

    fn visit_macro(&mut self, macro_call: &'ast syn::Macro) {
        for raw_path in rust_source_scan::macro_token_path_candidates(macro_call.tokens.clone()) {
            self.symbols
                .extend(symbol_segments(&raw_path).map(str::to_owned));
        }
        visit::visit_macro(self, macro_call);
    }
}

fn symbol_segments(path: &str) -> impl Iterator<Item = &str> {
    path.split("::").filter_map(|segment| {
        let segment = segment.strip_prefix("r#").unwrap_or(segment);
        segment
            .split(|character: char| !(character == '_' || character.is_ascii_alphanumeric()))
            .find(|candidate| !candidate.is_empty() && *candidate != "*")
    })
}

fn canonical_rust_symbol(symbol: &str) -> String {
    symbol.strip_prefix("r#").unwrap_or(symbol).to_owned()
}

pub fn scan_source_for_public_flow_violations(rel: &str, source: &str) -> Vec<String> {
    let mut violations = Vec::new();
    if is_public_flow_scanner_contract_file(rel) {
        return violations;
    }
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
    if is_protected_public_flow_file(rel) {
        for forbidden in forbidden_internal_support_imports(rel, source) {
            violations.push(format!(
                "{rel} imports internal support module `{forbidden}` from a protected public-flow test surface"
            ));
        }
        for forbidden in internal_quarantine_bridge_imports(rel, source) {
            violations.push(format!(
                "{rel} imports internal-only quarantine bridge `{forbidden}` from a protected public-flow test surface"
            ));
        }
    }

    let denied_helper_calls = denied_helper_calls();
    let denied_helper_names = denied_helper_names(source, &denied_helper_calls);
    let denied_hidden_literals = denied_hidden_literals();
    let mut concat_collector = ConcatLiteralCollector::default();
    let mut hidden_string_bindings = HashSet::new();
    let mut hidden_arg_bindings = HashSet::new();
    let mut display_command_bindings = HashSet::new();
    let mut display_command_binding_scope = String::new();
    let mut pending_assignment = None::<PendingAssignment>;
    let mut inside_command_invocation = false;
    let mut inside_command_args_array = false;
    let function_spans = rust_source_scan::function_spans(rel, source);
    let call_hits = rust_source_scan::normalized_call_path_hits(rel, source, &[]);
    let tainted_functions = tainted_runtime_helper_wrappers(rel, source, &denied_helper_names);
    for (line, function_name) in
        public_tainted_runtime_helper_wrappers(rel, source, &denied_helper_names)
    {
        violations.push(format!(
            "{rel}:{line} defines public wrapper `{function_name}` around an internal runtime helper outside an internal-only quarantine or test"
        ));
    }
    for (line, function_name, marker) in public_direct_runtime_surface_wrappers(rel, source) {
        violations.push(format!(
            "{rel}:{line} defines public direct runtime surface wrapper `{function_name}` using `{marker}` outside an internal-only quarantine or test"
        ));
    }
    for (index, line) in source.lines().enumerate() {
        let line_number = index + 1;
        let trimmed = line.trim();
        let current_fn = function_name_for_line(&function_spans, line_number);
        let current_scope = current_fn.unwrap_or("<module>");
        if display_command_binding_scope != current_scope {
            display_command_binding_scope = current_scope.to_owned();
            display_command_bindings.clear();
        }
        if rel != "tests/public_cli_flow_contracts.rs" && is_protected_public_flow_file(rel) {
            if let Some(binding) = display_command_binding_from_recommended_command(trimmed) {
                display_command_bindings.insert(binding);
            }
            if let Some(pattern) = display_command_execution_pattern(trimmed)
                .map(str::to_owned)
                .or_else(|| {
                    display_command_alias_execution_pattern(trimmed, &display_command_bindings)
                })
            {
                violations.push(format!(
                    "{rel}:{line_number} treats display-only recommended_command as executable authority via `{pattern}` in `{current_scope}`"
                ));
            }
            if let Some(binding) =
                copied_display_command_binding(trimmed, &display_command_bindings)
            {
                display_command_bindings.insert(binding);
            }
        }
        if starts_command_invocation(trimmed) {
            inside_command_invocation = true;
            inside_command_args_array =
                starts_command_args_array(trimmed) || contains_inline_command_args_array(trimmed);
        } else if inside_command_invocation && starts_command_args_array(trimmed) {
            inside_command_args_array = true;
        }
        for call in call_hits.iter().filter(|call| call.line == line_number) {
            if is_protected_public_flow_file(rel)
                && let Some(api) = event_log_fixture_authority_call_name(call)
                && event_log_test_api_exception_category(rel, current_scope).is_none()
            {
                violations.push(format!(
                    "{rel}:{} uses event-log fixture authority API `{api}` in `{}` without an explicit synthetic fixture exception",
                    line_number,
                    current_scope
                ));
            }
        }
        if current_fn.is_some_and(|name| function_scope_allows_internal_helpers(rel, name)) {
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
                current_scope,
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
                line_number,
                current_scope
            ));
        }
        for call in call_hits.iter().filter(|call| call.line == line_number) {
            if denied_helper_names
                .iter()
                .any(|forbidden| call_matches_name(call, forbidden))
            {
                let displayed = call_display_name(call);
                violations.push(format!(
                    "{rel}:{} uses internal helper `{displayed}(` outside an internal-only quarantine or test in `{}`",
                    line_number,
                    current_scope
                ));
            }
            if tainted_functions.iter().any(|helper_name| {
                current_fn != Some(helper_name.as_str()) && call_matches_name(call, helper_name)
            }) {
                let displayed = call_display_name(call);
                violations.push(format!(
                    "{rel}:{} calls tainted internal runtime helper wrapper `{displayed}` outside an internal-only quarantine or test in `{}`",
                    line_number,
                    current_scope
                ));
            }
        }
        let hidden_literals_are_executable_args =
            inside_command_invocation || inside_command_args_array;
        for hit in &hidden_literal_hits {
            if hit.always_hidden || hidden_literals_are_executable_args {
                violations.push(format!(
                    "{rel}:{} exposes hidden command or flag literal `{}` outside an internal-only quarantine or test in `{}`",
                    line_number,
                    hit.literal,
                    current_scope
                ));
            }
        }
        if hidden_literals_are_executable_args {
            for identifier in hidden_identifiers {
                violations.push(format!(
                    "{rel}:{} passes hidden command or flag alias `{identifier}` to an executable command outside an internal-only quarantine or test in `{}`",
                    line_number,
                    current_scope
                ));
            }
            for identifier in hidden_arg_identifiers {
                violations.push(format!(
                    "{rel}:{} passes hidden command arg collection `{identifier}` to an executable command outside an internal-only quarantine or test in `{}`",
                    line_number,
                    current_scope
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
    }
    violations
}

pub fn display_command_execution_pattern(trimmed: &str) -> Option<&'static str> {
    [
        "recommended_command.split_whitespace()",
        "shlex::split(recommended_command)",
        "internal_display_compatibility_only_run_recommended_command_json(",
        "internal_display_compatibility_only_run_recommended_plan_execution_command(",
        "internal_display_compatibility_only_run_recommended_plan_execution_command_with_mode(",
        "internal_display_compatibility_only_run_recommended_plan_execution_command_json_real_cli(",
        "run_recommended_plan_execution_command(",
        "run_recommended_plan_execution_command_with_mode(",
        "run_recommended_plan_execution_command_output_real_cli(",
    ]
    .into_iter()
    .find(|pattern| trimmed.contains(pattern))
}

fn display_command_binding_from_recommended_command(trimmed: &str) -> Option<String> {
    if trimmed.contains("recommended_command") {
        display_command_binding_name(trimmed)
    } else {
        None
    }
}

fn copied_display_command_binding(
    trimmed: &str,
    display_command_bindings: &HashSet<String>,
) -> Option<String> {
    let binding = display_command_binding_name(trimmed)?;
    let (_left, right) = trimmed.split_once('=')?;
    display_command_bindings
        .iter()
        .any(|display_binding| identifier_tokens(right).any(|token| token == display_binding))
        .then_some(binding)
}

fn display_command_binding_name(trimmed: &str) -> Option<String> {
    let rest = trimmed.strip_prefix("let ")?;
    let rest = rest.strip_prefix("mut ").unwrap_or(rest);
    let name = rest
        .strip_prefix("Some(")
        .and_then(|pattern| pattern.split_once(')').map(|(name, _)| name))
        .unwrap_or_else(|| {
            rest.split(|character: char| !is_rust_identifier_character(character))
                .next()
                .unwrap_or_default()
        });
    if name.is_empty() || !name.chars().all(is_rust_identifier_character) {
        return None;
    }
    Some(name.to_owned())
}

fn display_command_alias_execution_pattern(
    trimmed: &str,
    display_command_bindings: &HashSet<String>,
) -> Option<String> {
    display_command_bindings.iter().find_map(|binding| {
        for suffix in [".split_whitespace()", ".as_str().split_whitespace()"] {
            if identifier_is_followed_by(trimmed, binding, suffix) {
                return Some(format!("{binding}{suffix}"));
            }
        }
        [
            format!("shlex::split({binding})"),
            format!("shlex::split(&{binding})"),
            format!("shlex::split({binding}.as_str())"),
            format!("shlex::split({binding}.as_ref())"),
        ]
        .into_iter()
        .find(|pattern| trimmed.contains(pattern))
    })
}

fn identifier_is_followed_by(line: &str, identifier: &str, suffix: &str) -> bool {
    line.match_indices(identifier).any(|(start, _)| {
        let identifier_start_is_boundary = line[..start]
            .chars()
            .next_back()
            .is_none_or(|character| !is_rust_identifier_character(character));
        let suffix_start = start + identifier.len();
        identifier_start_is_boundary && line[suffix_start..].starts_with(suffix)
    })
}

pub fn scan_stale_dispatch_public_flow_violations(rel: &str, source: &str) -> Vec<String> {
    if !rel.ends_with(".rs") {
        return Vec::new();
    }
    if is_public_flow_scanner_contract_file(rel) {
        return Vec::new();
    }
    let function_spans = rust_source_scan::function_spans(rel, source);
    let protected_stale_dispatch_public_functions = function_spans
        .iter()
        .filter(|span| {
            span.name.contains("stale_dispatch")
                && !span.name.starts_with("internal_only_")
                && !span.name.starts_with("setup_")
                && !span.name.starts_with("scanner_")
                && span.name != "scan_stale_dispatch_public_flow_violations"
                && span.name != "stale_dispatch_public_flow_test_is_static_guarded"
        })
        .collect::<Vec<_>>();
    if protected_stale_dispatch_public_functions.is_empty() {
        return Vec::new();
    }
    let denied_literals = public_flow_hidden_command_or_flag_literals();
    let mut concat_collector = ConcatLiteralCollector::default();
    let mut violations = Vec::new();
    for (index, line) in source.lines().enumerate() {
        let line_number = index + 1;
        let Some(function_name) = function_name_for_line(&function_spans, line_number) else {
            continue;
        };
        if !protected_stale_dispatch_public_functions
            .iter()
            .any(|span| span.name == function_name)
        {
            continue;
        }
        let trimmed = line.trim();
        if let Some(call) = trimmed
            .split(|character: char| !(character == '_' || character.is_ascii_alphanumeric()))
            .find(|token| token.starts_with("internal_only_"))
        {
            violations.push(format!(
                "{rel}:{line_number} stale-dispatch public-flow test `{function_name}` must not call internal helper `{call}`"
            ));
        }
        for candidate in candidate_string_literals(trimmed, &mut concat_collector) {
            for denied in &denied_literals {
                if candidate.value == *denied {
                    violations.push(format!(
                        "{rel}:{line_number} stale-dispatch public-flow test `{function_name}` must not use hidden command or flag `{denied}`"
                    ));
                }
            }
        }
    }
    violations
}

pub fn file_name_is_internal_quarantine(rel: &str) -> bool {
    let file_name = Path::new(rel)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    file_name.starts_with("internal_")
}

fn has_internal_runtime_helper_header(source: &str) -> bool {
    source.lines().next() == Some(INTERNAL_RUNTIME_HELPER_HEADER)
}

pub fn is_protected_public_flow_file(rel: &str) -> bool {
    if rel.starts_with("tests/support/") {
        return false;
    }
    if is_public_flow_scanner_contract_file(rel) {
        return false;
    }
    if internal_semantic_non_public_flow_exception(rel).is_some() {
        return false;
    }
    protected_public_flow_test_files_from_contract().contains(rel)
}

pub fn is_public_flow_scanner_contract_file(rel: &str) -> bool {
    matches!(
        rel,
        "tests/public_flow_scan_contracts.rs" | "tests/support/hidden_public_commands.rs"
    )
}

pub fn public_flow_scanner_contract_exception(rel: &str) -> Option<PublicFlowException> {
    is_public_flow_scanner_contract_file(rel).then_some(public_flow_exception(
        PublicFlowExceptionCategory::ScannerSelfTest,
        "public-flow scanner fixture validates static guard behavior; it is not shipped-runtime public-flow proof",
    ))
}

pub fn internal_semantic_non_public_flow_exception(rel: &str) -> Option<PublicFlowException> {
    match rel {
        "tests/liveness_model_checker.rs" => Some(public_flow_exception(
            PublicFlowExceptionCategory::InternalSemanticComparison,
            "internal semantic/liveness matrix with targeted compiled-CLI parity; not shipped-runtime public-flow proof",
        )),
        _ => None,
    }
}

pub fn internal_semantic_non_public_flow_reason(rel: &str) -> Option<&'static str> {
    internal_semantic_non_public_flow_exception(rel).map(|exception| exception.explanation)
}

pub fn internal_semantic_non_public_flow_category(
    rel: &str,
) -> Option<PublicFlowExceptionCategory> {
    internal_semantic_non_public_flow_exception(rel).map(|exception| exception.category)
}

pub fn explicit_internal_helper_scope_exception(
    rel: &str,
    function_name: &str,
) -> Option<PublicFlowException> {
    if rel == "tests/support/public_runtime_contract_runner.rs"
        && matches!(
            function_name,
            "run_public_runtime_contract_in_process"
                | "try_run_public_runtime_contract_in_process"
                | "workflow_output"
                | "plan_execution_output"
        )
    {
        return Some(public_flow_exception(
            PublicFlowExceptionCategory::FocusedContractCoverage,
            "focused route-contract support may use production runtime surfaces after public argv parsing; compiled-CLI proof remains in executable public-flow suites",
        ));
    }
    if rel == "tests/workflow_shell_smoke.rs"
        && function_name.starts_with("setup_")
        && function_name.ends_with("_slow")
    {
        return Some(public_flow_exception(
            PublicFlowExceptionCategory::FocusedContractCoverage,
            "explicit slow fixture setup seeds late-stage review artifacts before public compiled-CLI routing assertions",
        ));
    }
    if rel == "tests/workflow_shell_smoke.rs"
        && function_name.starts_with("removed_command_rejection_")
    {
        return Some(public_flow_exception(
            PublicFlowExceptionCategory::RemovedCommandRejection,
            "negative public CLI coverage intentionally executes a removed workflow subcommand to prove the shipped CLI rejects it; not public workflow progress proof",
        ));
    }
    if function_name.starts_with("internal_semantic_")
        && matches!(
            rel,
            "tests/execution_query.rs"
                | "tests/contracts_execution_runtime_boundaries.rs"
                | "tests/workflow_runtime.rs"
                | "tests/workflow_entry_shell_smoke.rs"
        )
    {
        return Some(public_flow_exception(
            PublicFlowExceptionCategory::InternalSemanticComparison,
            "internal_semantic_ test explicitly exercises in-process DTO/query semantics; shipped public flow remains covered by compiled CLI/operator tests",
        ));
    }
    None
}

pub fn explicit_internal_helper_scope_exception_reason(
    rel: &str,
    function_name: &str,
) -> Option<&'static str> {
    explicit_internal_helper_scope_exception(rel, function_name)
        .map(|exception| exception.explanation)
}

pub fn explicit_internal_helper_scope_exception_category(
    rel: &str,
    function_name: &str,
) -> Option<PublicFlowExceptionCategory> {
    explicit_internal_helper_scope_exception(rel, function_name).map(|exception| exception.category)
}

pub fn event_log_fixture_authority_call_name(
    call: &rust_source_scan::RustCallPath,
) -> Option<&'static str> {
    denied_event_log_fixture_authority_names()
        .into_iter()
        .find(|api| call_matches_name(call, api))
}

fn denied_event_log_fixture_authority_names() -> [&'static str; 3] {
    [
        "load_reduced_authoritative_state_for_tests",
        "sync_fixture_event_log_for_tests",
        "synthetic_write_execution_harness_state_file",
    ]
}

pub fn event_log_test_api_exception(rel: &str, function_name: &str) -> Option<PublicFlowException> {
    if function_name.starts_with("synthetic_") && synthetic_event_log_fixture_file(rel) {
        Some(public_flow_exception(
            PublicFlowExceptionCategory::SyntheticFixtureSetup,
            "synthetic_ fixture scope may read or write event authority only to seed impossible historical states before public runtime assertions",
        ))
    } else {
        None
    }
}

fn synthetic_event_log_fixture_file(rel: &str) -> bool {
    matches!(
        rel,
        "tests/contracts_execution_runtime_boundaries.rs"
            | "tests/execution_query.rs"
            | "tests/liveness_model_checker.rs"
            | "tests/plan_execution.rs"
            | "tests/public_replay_churn.rs"
            | "tests/runtime_behavior_golden.rs"
            | "tests/workflow_entry_shell_smoke.rs"
            | "tests/workflow_runtime.rs"
            | "tests/workflow_runtime_final_review.rs"
            | "tests/workflow_shell_smoke.rs"
    )
}

pub fn event_log_test_api_exception_reason(rel: &str, function_name: &str) -> Option<&'static str> {
    event_log_test_api_exception(rel, function_name).map(|exception| exception.explanation)
}

pub fn event_log_test_api_exception_category(
    rel: &str,
    function_name: &str,
) -> Option<PublicFlowExceptionCategory> {
    event_log_test_api_exception(rel, function_name).map(|exception| exception.category)
}

fn function_scope_allows_internal_helpers(rel: &str, function_name: &str) -> bool {
    if is_protected_public_flow_file(rel) {
        return explicit_internal_helper_scope_exception_category(rel, function_name).is_some();
    }
    function_name.starts_with("internal_only_")
        || explicit_internal_helper_scope_exception_category(rel, function_name).is_some()
}

fn forbidden_internal_support_imports(rel: &str, source: &str) -> Vec<String> {
    let syntax = rust_source_scan::parse_rust_source(rel, source);
    let forbidden = forbidden_internal_support_paths();
    syntax
        .items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Mod(module) => Some(module),
            _ => None,
        })
        .flat_map(|module| module.attrs.iter())
        .filter_map(path_attr_value)
        .filter(|path| forbidden.contains(path))
        .filter(|path| {
            !(rel == "tests/runtime_behavior_golden.rs"
                && path == "support/public_runtime_contract_runner.rs")
        })
        .collect()
}

fn internal_quarantine_bridge_imports(rel: &str, source: &str) -> Vec<String> {
    rust_source_scan::parse_rust_source(rel, source)
        .items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Mod(module) => Some(module),
            _ => None,
        })
        .flat_map(|module| module.attrs.iter())
        .filter_map(path_attr_value)
        .filter(|path| path == "support/internal_only_direct_helpers.rs")
        .collect()
}

fn forbidden_internal_support_paths() -> HashSet<String> {
    [
        hidden_literal(&["support/", "featureforge.rs"]),
        hidden_literal(&["support/", "internal_public_runtime_in_process.rs"]),
        hidden_literal(&["support/", "public_runtime_contract_runner.rs"]),
        hidden_literal(&["support/", "internal_runtime_phase_handoff.rs"]),
        hidden_literal(&["support/", "plan_execution_direct.rs"]),
        hidden_literal(&["support/", "workflow_direct.rs"]),
        hidden_literal(&["support/", "internal_runtime_direct.rs"]),
    ]
    .into_iter()
    .collect()
}

fn path_attr_value(attr: &syn::Attribute) -> Option<String> {
    if !attr.path().is_ident("path") {
        return None;
    }
    let syn::Meta::NameValue(name_value) = &attr.meta else {
        return None;
    };
    let syn::Expr::Lit(expr_lit) = &name_value.value else {
        return None;
    };
    let syn::Lit::Str(lit) = &expr_lit.lit else {
        return None;
    };
    Some(lit.value())
}

pub fn public_runtime_flow_test_files() -> &'static HashSet<String> {
    static PUBLIC_RUNTIME_FLOW_TEST_FILES: OnceLock<HashSet<String>> = OnceLock::new();
    PUBLIC_RUNTIME_FLOW_TEST_FILES.get_or_init(|| {
        let script = read_repo_file("scripts/run-public-runtime-flow-tests.sh");
        public_runtime_flow_test_binaries_from_script(&script)
            .into_iter()
            .map(|binary| format!("tests/{binary}.rs"))
            .collect()
    })
}

pub fn public_runtime_flow_test_binaries_from_script(script: &str) -> Vec<String> {
    let mut binaries = Vec::new();
    for command in cargo_nextest_run_commands(script) {
        let mut tokens = command
            .split_whitespace()
            .map(normalized_shell_token)
            .peekable();
        while let Some(token) = tokens.next() {
            if let Some(binary) = token.strip_prefix("--test=") {
                binaries.push(normalized_test_binary_token(binary));
                continue;
            }
            if token == "--test"
                && let Some(binary) = tokens.next()
            {
                binaries.push(normalized_test_binary_token(&binary));
            }
        }
    }
    binaries.sort();
    binaries.dedup();
    binaries
}

fn cargo_nextest_run_commands(script: &str) -> Vec<String> {
    shell_logical_commands(script)
        .into_iter()
        .filter(|command| command_starts_with_cargo_nextest_run(command))
        .collect()
}

fn shell_logical_commands(script: &str) -> Vec<String> {
    let mut commands = Vec::new();
    let mut current = String::new();
    for raw_line in script.lines() {
        let without_comment = strip_shell_comment(raw_line);
        let trimmed = without_comment.trim_end();
        if trimmed.trim().is_empty() {
            continue;
        }
        let continued = trimmed.ends_with('\\');
        let part = if continued {
            trimmed.trim_end_matches('\\').trim_end()
        } else {
            trimmed
        };
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(part.trim());
        if !continued {
            commands.push(std::mem::take(&mut current));
        }
    }
    if !current.trim().is_empty() {
        commands.push(current);
    }
    commands
}

fn strip_shell_comment(line: &str) -> String {
    let mut result = String::new();
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut escaped = false;
    for ch in line.chars() {
        if escaped {
            result.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' && !in_single_quote {
            result.push(ch);
            escaped = true;
            continue;
        }
        match ch {
            '\'' if !in_double_quote => {
                in_single_quote = !in_single_quote;
                result.push(ch);
            }
            '"' if !in_single_quote => {
                in_double_quote = !in_double_quote;
                result.push(ch);
            }
            '#' if !in_single_quote && !in_double_quote => break,
            _ => result.push(ch),
        }
    }
    result
}

fn command_starts_with_cargo_nextest_run(command: &str) -> bool {
    let tokens: Vec<_> = command
        .split_whitespace()
        .map(normalized_shell_token)
        .collect();
    let mut index = 0;
    while tokens
        .get(index)
        .is_some_and(|token| token.contains('=') && !token.starts_with('-'))
    {
        index += 1;
    }
    tokens.get(index).map(String::as_str) == Some("cargo")
        && tokens.get(index + 1).map(String::as_str) == Some("nextest")
        && tokens.get(index + 2).map(String::as_str) == Some("run")
}

fn normalized_shell_token(token: &str) -> String {
    token
        .trim_matches('\\')
        .trim_matches('"')
        .trim_matches('\'')
        .to_owned()
}

fn normalized_test_binary_token(token: &str) -> String {
    normalized_shell_token(token.trim_matches('\\'))
}

pub fn function_name_for_line(
    spans: &[rust_source_scan::RustFunctionSpan],
    line: usize,
) -> Option<&str> {
    spans
        .iter()
        .rev()
        .find(|span| line >= span.start_line && line <= span.end_line)
        .map(|span| span.name.as_str())
}

pub fn call_matches_name(call: &rust_source_scan::RustCallPath, function_name: &str) -> bool {
    let normalized_leaf = call.path.rsplit("::").next().unwrap_or(call.path.as_str());
    let raw_leaf = call
        .raw_path
        .rsplit("::")
        .next()
        .unwrap_or(call.raw_path.as_str());
    normalized_leaf == function_name || raw_leaf == function_name
}

fn call_display_name(call: &rust_source_scan::RustCallPath) -> &str {
    call.raw_path
        .rsplit("::")
        .next()
        .unwrap_or(call.raw_path.as_str())
}

#[derive(Debug)]
pub struct RustFunctionBody<'a> {
    pub name: String,
    pub start_line: usize,
    pub lines: Vec<&'a str>,
}

fn tainted_runtime_helper_wrappers(
    rel: &str,
    source: &str,
    denied_helper_names: &[String],
) -> HashSet<String> {
    let functions = rust_function_bodies(rel, source);
    let call_hits = rust_source_scan::normalized_call_path_hits(rel, source, &[]);
    let mut tainted = functions
        .iter()
        .filter(|function| !function_scope_allows_internal_helpers(rel, &function.name))
        .filter(|function| {
            function_calls_any(&call_hits, function, denied_helper_names)
                || function_is_removed_hidden_command_helper(function)
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
            if function_scope_allows_internal_helpers(rel, &function.name) {
                continue;
            }
            let tainted_names = tainted.iter().cloned().collect::<Vec<_>>();
            if function_calls_any(&call_hits, function, &tainted_names) {
                changed |= tainted.insert(function.name.clone());
            }
        }
    }

    tainted
}

fn function_is_removed_hidden_command_helper(function: &RustFunctionBody<'_>) -> bool {
    if !function.name.contains("removed") {
        return false;
    }
    let denied_hidden_literals = denied_hidden_literals();
    let mut concat_collector = ConcatLiteralCollector::default();
    function.lines.iter().any(|line| {
        let candidate_literals = candidate_string_literals(line.trim(), &mut concat_collector);
        !hidden_literal_hits(&candidate_literals, &denied_hidden_literals).is_empty()
    })
}

fn public_tainted_runtime_helper_wrappers(
    rel: &str,
    source: &str,
    denied_helper_names: &[String],
) -> Vec<(usize, String)> {
    let call_hits = rust_source_scan::normalized_call_path_hits(rel, source, &[]);
    rust_function_bodies(rel, source)
        .into_iter()
        .filter(|function| !function_scope_allows_internal_helpers(rel, &function.name))
        .filter(|function| function_calls_any(&call_hits, function, denied_helper_names))
        .map(|function| (function.start_line, function.name))
        .collect()
}

fn public_direct_runtime_surface_wrappers(rel: &str, source: &str) -> Vec<(usize, String, String)> {
    let call_hits = rust_source_scan::normalized_call_path_hits(rel, source, &[]);
    rust_function_bodies(rel, source)
        .into_iter()
        .filter(|function| !function_scope_allows_internal_helpers(rel, &function.name))
        .filter_map(|function| {
            call_hits
                .iter()
                .filter(|call| {
                    call.line >= function.start_line
                        && call.line < function.start_line + function.lines.len()
                })
                .find_map(direct_runtime_surface_marker)
                .map(|marker| (function.start_line, function.name, marker))
        })
        .collect()
}

fn function_calls_any(
    call_hits: &[rust_source_scan::RustCallPath],
    function: &RustFunctionBody<'_>,
    names: &[String],
) -> bool {
    names
        .iter()
        .any(|name| function_calls_name(call_hits, function, name))
}

fn function_calls_name(
    call_hits: &[rust_source_scan::RustCallPath],
    function: &RustFunctionBody<'_>,
    name: &str,
) -> bool {
    call_hits.iter().any(|call| {
        call.line >= function.start_line
            && call.line < function.start_line + function.lines.len()
            && call_matches_name(call, name)
    })
}

fn direct_runtime_surface_marker(call: &rust_source_scan::RustCallPath) -> Option<String> {
    for marker in [
        "operator_for_runtime",
        "doctor_for_runtime",
        "doctor_for_runtime_with_args",
        "doctor_phase_and_next_for_runtime_with_args",
        "query_review_state",
        "query_workflow_execution_state",
        "query_workflow_routing_state",
        "query_workflow_routing_state_for_runtime",
        "phase_for_runtime",
        "handoff_for_runtime",
        "status_refresh",
    ] {
        if call_matches_name(call, marker) {
            return Some(marker.to_owned());
        }
    }
    call.receiver_runtime_path.as_ref()?;
    ["status", "review_gate", "finish_gate"]
        .into_iter()
        .find(|marker| call_matches_name(call, marker))
        .map(str::to_owned)
}

fn denied_helper_names(source: &str, denied_helper_calls: &[String]) -> Vec<String> {
    let mut names = denied_helper_calls
        .iter()
        .map(|call| call.trim_end_matches('(').to_owned())
        .chain(quarantine_prefixed_helper_names(source))
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    names
}

fn quarantine_prefixed_helper_names(source: &str) -> Vec<String> {
    let code = line_without_string_literals(source);
    identifier_tokens(&code)
        .filter(|identifier| identifier.starts_with("internal_only_"))
        .map(str::to_owned)
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

pub fn rust_function_bodies<'a>(rel: &str, source: &'a str) -> Vec<RustFunctionBody<'a>> {
    let lines = source.lines().collect::<Vec<_>>();
    rust_source_scan::function_spans(rel, source)
        .into_iter()
        .map(|span| RustFunctionBody {
            name: span.name,
            start_line: span.start_line,
            lines: lines[(span.start_line - 1)..span.end_line].to_vec(),
        })
        .collect()
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
            "run_public_featureforge_cli_failure_json(",
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

pub fn denied_helper_calls() -> Vec<String> {
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
            "internal_only_",
            "compatibility_",
            "workflow_pre",
            "flight_json(",
        ]),
        hidden_literal(&[
            "internal_only_",
            "compatibility_",
            "workflow_gate_review_json(",
        ]),
        hidden_literal(&[
            "internal_only_",
            "compatibility_",
            "workflow_gate_finish_json(",
        ]),
        hidden_literal(&["internal_only_workflow_", "pre", "flight_output("]),
        concat!("internal_only_workflow_", "gate_review_output(").to_owned(),
        concat!("internal_only_workflow_", "gate_finish_output(").to_owned(),
        concat!("concrete_public_", "command_args(").to_owned(),
        concat!("materialize_public_", "command_template(").to_owned(),
        concat!("fill_public_argv_", "template_value(").to_owned(),
    ]
}

fn denied_hidden_literals() -> Vec<String> {
    public_flow_hidden_command_or_flag_literals()
}

pub fn public_diagnostic_forbidden_patterns() -> Vec<String> {
    vec![
        hidden_literal(&["run gate", "-review"]),
        hidden_literal(&["run gate", "-finish"]),
        hidden_literal(&["retry gate", "-review"]),
        hidden_literal(&["retry gate", "-finish"]),
        "rebuild evidence".to_owned(),
        "rebuild its evidence".to_owned(),
        "rebuild the packet".to_owned(),
        "rebuild packet".to_owned(),
        "rebuild the execution evidence".to_owned(),
        "refresh execution evidence".to_owned(),
        "refresh evidence".to_owned(),
        "repair review state / reenter execution".to_owned(),
        "repair workflow routing".to_owned(),
        "repairing runtime routing".to_owned(),
        "record receipt".to_owned(),
        "record matching execution evidence".to_owned(),
        "record a dedicated-independent serial unit-review receipt".to_owned(),
        "repair unit-review receipt".to_owned(),
        "record the authoritative unit-review receipt".to_owned(),
        "repair the authoritative unit-review receipt".to_owned(),
        "restore authoritative unit-review receipt".to_owned(),
        "restore the authoritative unit-review receipt".to_owned(),
        "clear handoff_required".to_owned(),
        "clear the handoff_required".to_owned(),
        "review-dispatch recording".to_owned(),
        "review-dispatch lineage".to_owned(),
        "review dispatch lineage".to_owned(),
        "reviewable dispatch lineage".to_owned(),
        "final-review dispatch lineage".to_owned(),
        "dispatch lineage state".to_owned(),
        "supplied dispatch lineage".to_owned(),
        "this dispatch lineage".to_owned(),
        "current dispatch lineage".to_owned(),
        "recording more review-dispatch lineage".to_owned(),
        "recording review dispatch".to_owned(),
        "before recording review dispatch".to_owned(),
        "re-derive the workflow phase".to_owned(),
        "repair the current task-closure state".to_owned(),
        "restore authoritative event-log state".to_owned(),
        "restore or migrate authoritative event-log state".to_owned(),
    ]
}

pub fn public_diagnostic_hidden_command_token_patterns() -> Vec<String> {
    featureforge::execution::command_eligibility::hidden_command_or_flag_tokens()
        .iter()
        .map(|token| (*token).to_owned())
        .collect()
}

pub fn diagnostic_pattern_violations_for_source(
    rel: &str,
    source: &str,
    forbidden_patterns: &[String],
) -> Vec<String> {
    if rel.ends_with(".rs") {
        return diagnostic_pattern_violations_for_rust_source(rel, source, forbidden_patterns);
    }

    let mut violations = Vec::new();
    let source_lower = source.to_ascii_lowercase();

    for pattern in forbidden_patterns {
        let pattern_lower = pattern.to_ascii_lowercase();
        for (start, _) in source_lower.match_indices(&pattern_lower) {
            violations.push(format!(
                "{rel}:{} public diagnostics/docs must route through workflow operator, repair-review-state, close-current-task, or advance-late-stage instead of `{pattern}`",
                line_number_for_byte(source, start)
            ));
        }
    }
    violations
}

fn diagnostic_pattern_violations_for_rust_source(
    rel: &str,
    source: &str,
    forbidden_patterns: &[String],
) -> Vec<String> {
    let mut violations = HashSet::new();
    let literal_values = rust_source_scan::rust_production_string_literal_values(rel, source);

    for pattern in forbidden_patterns {
        let pattern_lower = pattern.to_ascii_lowercase();
        for value in &literal_values {
            if value.to_ascii_lowercase().contains(&pattern_lower) {
                violations.insert(format!(
                    "{rel} production string literal materializes hidden or stale public diagnostic wording `{pattern}`"
                ));
            }
        }
    }

    let mut violations = violations.into_iter().collect::<Vec<_>>();
    violations.sort();
    violations
}

fn rust_historical_comment_ranges(source: &str) -> Vec<Range<usize>> {
    let bytes = source.as_bytes();
    let mut ranges = Vec::new();
    let mut index = 0;

    while index < bytes.len() {
        if let Some(raw_string_end) = rust_raw_string_end(bytes, index) {
            index = raw_string_end;
            continue;
        }

        if bytes[index] == b'"' {
            index = rust_quoted_string_end(bytes, index);
            continue;
        }

        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'/') {
            let start = index;
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            if rust_comment_is_explicitly_historical(&source[start..index]) {
                ranges.push(start..index);
            }
            continue;
        }

        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'*') {
            let start = index;
            index = rust_block_comment_end(bytes, index);
            if rust_comment_is_explicitly_historical(&source[start..index]) {
                ranges.push(start..index);
            }
            continue;
        }

        index += 1;
    }

    ranges
}

fn rust_raw_string_end(bytes: &[u8], start: usize) -> Option<usize> {
    let raw_prefix_index = if bytes.get(start) == Some(&b'r') {
        start
    } else if matches!(bytes.get(start), Some(b'b' | b'c')) && bytes.get(start + 1) == Some(&b'r') {
        start + 1
    } else {
        return None;
    };

    let mut quote_index = raw_prefix_index + 1;
    while bytes.get(quote_index) == Some(&b'#') {
        quote_index += 1;
    }
    if bytes.get(quote_index) != Some(&b'"') {
        return None;
    }

    let hashes = quote_index - raw_prefix_index - 1;
    let mut index = quote_index + 1;
    while index < bytes.len() {
        if bytes[index] == b'"'
            && bytes
                .get(index + 1..index + 1 + hashes)
                .is_some_and(|suffix| suffix.iter().all(|byte| *byte == b'#'))
        {
            return Some(index + 1 + hashes);
        }
        index += 1;
    }

    Some(bytes.len())
}

fn rust_quoted_string_end(bytes: &[u8], start: usize) -> usize {
    let mut index = start + 1;
    let mut escaped = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if escaped {
            escaped = false;
        } else if byte == b'\\' {
            escaped = true;
        } else if byte == b'"' {
            return index + 1;
        }
        index += 1;
    }
    bytes.len()
}

fn rust_block_comment_end(bytes: &[u8], start: usize) -> usize {
    let mut depth = 1usize;
    let mut index = start + 2;
    while index + 1 < bytes.len() {
        if bytes[index] == b'/' && bytes[index + 1] == b'*' {
            depth += 1;
            index += 2;
        } else if bytes[index] == b'*' && bytes[index + 1] == b'/' {
            depth -= 1;
            index += 2;
            if depth == 0 {
                return index;
            }
        } else {
            index += 1;
        }
    }
    bytes.len()
}

fn rust_comment_is_explicitly_historical(comment: &str) -> bool {
    let trimmed = comment.trim_start();
    let body = if let Some(body) = trimmed.strip_prefix("//") {
        body
    } else if let Some(body) = trimmed.strip_prefix("/*") {
        body
    } else {
        return false;
    };
    let body = body.trim_start_matches(['/', '*', '!']).trim_start();
    body.to_ascii_lowercase().starts_with("historical")
}

fn range_is_inside_any(range: Range<usize>, ranges: &[Range<usize>]) -> bool {
    ranges
        .iter()
        .any(|allowed| range.start >= allowed.start && range.end <= allowed.end)
}

pub fn line_number_for_byte(source: &str, byte_index: usize) -> usize {
    source[..byte_index]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1
}

pub fn token_only_blocked_follow_up_violations(rel: &str, source: &str) -> Vec<String> {
    braced_blocks(source)
        .into_iter()
        .filter_map(|(block_start, block)| {
            if !top_level_field_value_starts_with(block, "required_follow_up", "Some")
                || !top_level_field_value_starts_with(block, "recommended_command", "None")
                || !top_level_field_value_starts_with(
                    block,
                    "recommended_public_command_argv",
                    "None",
                )
                || !top_level_field_value_starts_with(
                    block,
                    "recommended_public_command_template",
                    "None",
                )
                || !top_level_field_value_starts_with(block, "required_inputs", "Vec::new()")
                || top_level_field_value_starts_with(
                    block,
                    "rederive_via_workflow_operator",
                    "Some",
                )
            {
                return None;
            }
            let required_follow_up_offset = block_top_level_field_offset(block, "required_follow_up")
                .map(|offset| block_start + offset)
                .unwrap_or(block_start);
            Some(format!(
                "{rel}:{} emits required_follow_up without argv, template, inputs, requery, or diagnostic-only null follow-up",
                line_number_for_byte(source, required_follow_up_offset)
            ))
        })
        .collect()
}

fn braced_blocks(source: &str) -> Vec<(usize, &str)> {
    let mut blocks = Vec::new();
    let mut starts = Vec::new();
    for (index, ch) in source.char_indices() {
        match ch {
            '{' => starts.push(index),
            '}' => {
                let Some(start) = starts.pop() else {
                    continue;
                };
                blocks.push((start, &source[start..=index]));
            }
            _ => {}
        }
    }
    blocks
}

fn top_level_field_value_starts_with(block: &str, field: &str, expected_prefix: &str) -> bool {
    top_level_field_value(block, field).is_some_and(|value| value.starts_with(expected_prefix))
}

fn block_top_level_field_offset(block: &str, field: &str) -> Option<usize> {
    top_level_field_ranges(block)
        .into_iter()
        .find_map(|field_range| (field_range.name == field).then_some(field_range.name_start))
}

fn top_level_field_value<'a>(block: &'a str, field: &str) -> Option<&'a str> {
    top_level_field_ranges(block)
        .into_iter()
        .find_map(|field_range| {
            (field_range.name == field)
                .then(|| block[field_range.value_start..field_range.value_end].trim())
        })
}

#[derive(Debug)]
struct TopLevelField<'a> {
    name: &'a str,
    name_start: usize,
    value_start: usize,
    value_end: usize,
}

fn top_level_field_ranges(block: &str) -> Vec<TopLevelField<'_>> {
    if !block.starts_with('{') || !block.ends_with('}') {
        return Vec::new();
    }

    let mut fields = Vec::new();
    let mut index = 1;
    while index < block.len() - 1 {
        index = skip_field_gap(block, index);
        if index >= block.len() - 1 {
            break;
        }

        let name_start = index;
        let Some(name_end) = consume_identifier(block, name_start) else {
            index = next_char_index(block, index);
            continue;
        };

        let colon_index = skip_ascii_whitespace(block, name_end);
        if !block[colon_index..].starts_with(':') {
            index = next_char_index(block, name_end);
            continue;
        }

        let value_start = skip_ascii_whitespace(block, colon_index + 1);
        let value_end = top_level_field_value_end(block, value_start);
        fields.push(TopLevelField {
            name: &block[name_start..name_end],
            name_start,
            value_start,
            value_end,
        });
        index = value_end.saturating_add(1);
    }

    fields
}

fn skip_field_gap(block: &str, mut index: usize) -> usize {
    while index < block.len() {
        let next = skip_ascii_whitespace(block, index);
        if block[next..].starts_with(',') {
            index = next + 1;
            continue;
        }
        return next;
    }
    index
}

fn consume_identifier(block: &str, start: usize) -> Option<usize> {
    let mut chars = block[start..].char_indices();
    let (_, first) = chars.next()?;
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return None;
    }

    let mut end = start + first.len_utf8();
    for (offset, ch) in chars {
        if ch == '_' || ch.is_ascii_alphanumeric() {
            end = start + offset + ch.len_utf8();
        } else {
            break;
        }
    }
    Some(end)
}

fn skip_ascii_whitespace(block: &str, mut index: usize) -> usize {
    while let Some(ch) = block[index..].chars().next() {
        if !ch.is_ascii_whitespace() {
            break;
        }
        index += ch.len_utf8();
    }
    index
}

fn top_level_field_value_end(block: &str, value_start: usize) -> usize {
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut end = block.len() - 1;

    for (relative_offset, ch) in block[value_start..block.len() - 1].char_indices() {
        let index = value_start + relative_offset;
        match ch {
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            ',' if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 => {
                end = index;
                break;
            }
            _ => {}
        }
    }

    end
}

fn next_char_index(block: &str, index: usize) -> usize {
    block[index..]
        .chars()
        .next()
        .map_or(block.len(), |ch| index + ch.len_utf8())
}

pub fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

pub fn production_source_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_files_with_extensions(&repo_root().join("src"), &["rs"], &mut files);
    files.sort();
    files.dedup();
    files
}

pub fn rust_test_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rust_test_files(root, &mut files);
    files.sort();
    files
}

pub fn top_level_rust_test_files(root: &Path) -> Vec<PathBuf> {
    let mut files = fs::read_dir(root)
        .unwrap_or_else(|error| panic!("{} should be readable: {error}", root.display()))
        .map(|entry| {
            entry
                .expect("test directory entry should be readable")
                .path()
        })
        .filter(|path| {
            path.is_file()
                && path.extension().and_then(|extension| extension.to_str()) == Some("rs")
        })
        .collect::<Vec<_>>();
    files.sort();
    files
}

pub fn internal_compatibility_function_names(rel: &str, source: &str) -> Vec<String> {
    let syntax = rust_source_scan::parse_rust_source(rel, source);
    let mut names = Vec::new();
    collect_item_function_names_including_cfg_test(&syntax.items, &mut names);
    names.sort();
    names.dedup();
    let compatibility_prefix = hidden_literal(&["internal_only_", "compatibility_"]);
    let fs_prefix = hidden_literal(&["internal_only_", "fs"]);
    names
        .into_iter()
        .filter(|name| name.starts_with(&compatibility_prefix) || name.starts_with(&fs_prefix))
        .collect()
}

fn collect_item_function_names_including_cfg_test(items: &[syn::Item], names: &mut Vec<String>) {
    for item in items {
        match item {
            syn::Item::Fn(item_fn) => names.push(item_fn.sig.ident.to_string()),
            syn::Item::Mod(item_mod) => {
                if let Some((_, nested_items)) = &item_mod.content {
                    collect_item_function_names_including_cfg_test(nested_items, names);
                }
            }
            _ => {}
        }
    }
}

pub fn collect_files_with_extensions(dir: &Path, extensions: &[&str], files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap_or_else(|error| {
        panic!("{} should be readable: {error}", dir.display());
    }) {
        let entry = entry.expect("directory entry should be readable");
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().and_then(|name| name.to_str()) == Some("archive") {
                continue;
            }
            collect_files_with_extensions(&path, extensions, files);
        } else if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extensions.contains(&extension))
        {
            files.push(path);
        }
    }
}

pub fn production_command_authority_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    for root in ["src/execution", "src/workflow"] {
        collect_files_with_extensions(&repo_root().join(root), &["rs"], &mut files);
    }
    files.retain(|path| production_command_authority_scan_subject(&repo_relative(path)));
    files.sort();
    files.dedup();
    files
}

pub fn production_command_authority_scan_subject(rel: &str) -> bool {
    production_command_authority_scan_exemption_reason(rel).is_none()
}

fn production_command_authority_scan_exemption_reason(rel: &str) -> Option<&'static str> {
    if !(rel.starts_with("src/execution/") || rel.starts_with("src/workflow/")) {
        return Some("outside production route/status roots");
    }
    if rel.ends_with("_tests.rs") || rel.ends_with("unit_tests.rs") || rel.ends_with("/tests.rs") {
        return Some("test-only source module");
    }
    if matches!(
        rel,
        "src/execution/command_eligibility.rs"
            | "src/execution/command_eligibility/command_kind.rs"
            | "src/execution/command_eligibility/execution_target.rs"
            | "src/execution/public_command_types.rs"
    ) {
        return Some("public command parser/renderer owner");
    }
    None
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

pub fn repo_relative(path: &Path) -> String {
    path.strip_prefix(repo_root())
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}
