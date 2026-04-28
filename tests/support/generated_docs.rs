use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::process::Command;

const SKILL_GENERATOR_CMD: &str = "node scripts/gen-skill-docs.mjs";
const AGENT_GENERATOR_CMD: &str = "node scripts/gen-agent-docs.mjs";

pub fn assert_generated_skill_docs_current(root: &Path) {
    assert_generator_check_current(root, "scripts/gen-skill-docs.mjs", SKILL_GENERATOR_CMD);
    assert_generated_skill_docs_current_structural(root);
}

pub fn assert_generated_agent_docs_current(root: &Path) {
    assert_generator_check_current(root, "scripts/gen-agent-docs.mjs", AGENT_GENERATOR_CMD);
    assert_generated_agent_docs_current_structural(root);
}

fn assert_generated_skill_docs_current_structural(root: &Path) {
    let skills_dir = root.join("skills");
    let mut template_paths = fs::read_dir(&skills_dir)
        .unwrap_or_else(|error| panic!("{} should be readable: {error}", skills_dir.display()))
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .file_type()
                .ok()
                .is_some_and(|file_type| file_type.is_dir())
        })
        .map(|entry| entry.path().join("SKILL.md.tmpl"))
        .filter(|template_path| template_path.is_file())
        .collect::<Vec<_>>();
    template_paths.sort();
    assert!(
        !template_paths.is_empty(),
        "generated skill doc check should find at least one template"
    );

    let mut stale = Vec::new();
    for template_path in template_paths {
        let skill_path = template_path.with_extension("");
        let rendered = render_skill_template(&template_path);
        let current = fs::read_to_string(&skill_path).unwrap_or_default();
        if current != rendered {
            stale.push(relative_display(root, &skill_path));
        }
    }

    assert!(
        stale.is_empty(),
        "Generated skill docs are stale:\n{}",
        stale.join("\n")
    );
}

fn assert_generated_agent_docs_current_structural(root: &Path) {
    let source_path = root.join("agents/code-reviewer.instructions.md");
    let source = read_utf8(&source_path);
    let parsed = parse_agent_source(&source_path, &source);
    let expected_markdown = build_copilot_agent(&parsed);
    let expected_toml = build_codex_agent(&parsed);
    let copilot_path = root.join("agents/code-reviewer.md");
    let codex_path = root.join(".codex/agents/code-reviewer.toml");
    let mut stale = Vec::new();
    if fs::read_to_string(&copilot_path).unwrap_or_default() != expected_markdown {
        stale.push(relative_display(root, &copilot_path));
    }
    if fs::read_to_string(&codex_path).unwrap_or_default() != expected_toml {
        stale.push(relative_display(root, &codex_path));
    }
    assert!(
        stale.is_empty(),
        "Generated agent docs are stale:\n{}",
        stale.join("\n")
    );
}

fn assert_generator_check_current(root: &Path, script: &str, display_command: &str) {
    let runtime = ["node", "nodejs"].into_iter().find_map(|candidate| {
        let mut command = Command::new(candidate);
        command
            .current_dir(root)
            .args([script, "--check"])
            .env_remove("NODE_OPTIONS");
        match command.output() {
            Ok(output) => Some((candidate, output)),
            Err(error) if error.kind() == ErrorKind::NotFound => None,
            Err(error) => panic!(
                "{display_command} should be runnable from {}: {error}",
                root.display()
            ),
        }
    });

    let Some((runtime, output)) = runtime else {
        panic!(
            "{display_command} requires a JS runtime (`node` or `nodejs`) on PATH for generator contract verification."
        );
    };

    assert!(
        output.status.success(),
        "{display_command} should report current generated docs via {runtime}, got {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn render_skill_template(template_path: &Path) -> String {
    let content = read_utf8(template_path);
    let mut rendered = content
        .replace("{{BASE_PREAMBLE}}", &generate_preamble(false))
        .replace("{{REVIEW_PREAMBLE}}", &generate_preamble(true));
    assert!(
        !rendered.contains("{{"),
        "Unresolved placeholder remains in {}",
        template_path.display()
    );
    rendered = insert_generated_header(&rendered);
    if !rendered.ends_with('\n') {
        rendered.push('\n');
    }
    rendered
}

fn generate_preamble(review: bool) -> String {
    let mut parts = vec![
        String::from("## Preamble (run first)"),
        String::new(),
        String::from("```bash"),
    ];
    if review {
        parts.extend(build_review_shell_lines());
    } else {
        parts.extend(build_base_shell_lines());
    }
    parts.push(String::from("```"));
    parts.push(build_search_before_building_section());
    if review {
        parts.push(String::new());
        parts.push(build_agent_grounding());
    }
    parts.push(String::new());
    parts.push(build_question_format());
    if review {
        parts.push(String::new());
        parts.push(build_contributor_mode());
    }
    parts.join("\n")
}

fn build_root_detection() -> Vec<String> {
    [
        "_REPO_ROOT=$(git rev-parse --show-toplevel 2>/dev/null || pwd)",
        "_BRANCH_RAW=$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo current)",
        "[ -n \"$_BRANCH_RAW\" ] && [ \"$_BRANCH_RAW\" != \"HEAD\" ] || _BRANCH_RAW=\"current\"",
        "_BRANCH=\"$_BRANCH_RAW\"",
        "_FEATUREFORGE_INSTALL_ROOT=\"$HOME/.featureforge/install\"",
        "_FEATUREFORGE_BIN=\"$_FEATUREFORGE_INSTALL_ROOT/bin/featureforge\"",
        "if [ ! -x \"$_FEATUREFORGE_BIN\" ] && [ -f \"$_FEATUREFORGE_INSTALL_ROOT/bin/featureforge.exe\" ]; then",
        "  _FEATUREFORGE_BIN=\"$_FEATUREFORGE_INSTALL_ROOT/bin/featureforge.exe\"",
        "fi",
        "[ -x \"$_FEATUREFORGE_BIN\" ] || [ -f \"$_FEATUREFORGE_BIN\" ] || _FEATUREFORGE_BIN=\"\"",
        "_FEATUREFORGE_ROOT=\"\"",
        "if [ -n \"$_FEATUREFORGE_BIN\" ]; then",
        "  _FEATUREFORGE_ROOT=$(\"$_FEATUREFORGE_BIN\" repo runtime-root --path 2>/dev/null)",
        "  [ -n \"$_FEATUREFORGE_ROOT\" ] || _FEATUREFORGE_ROOT=\"\"",
        "fi",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

fn build_base_shell_lines() -> Vec<String> {
    let mut lines = build_root_detection();
    lines.push(String::from(
        "_FEATUREFORGE_STATE_DIR=\"${FEATUREFORGE_STATE_DIR:-$HOME/.featureforge}\"",
    ));
    lines
}

fn build_review_shell_lines() -> Vec<String> {
    let mut lines = build_base_shell_lines();
    lines.push(String::from("_TODOS_FORMAT=\"\""));
    lines.push(String::from(
        "[ -n \"$_FEATUREFORGE_ROOT\" ] && [ -f \"$_FEATUREFORGE_ROOT/review/TODOS-format.md\" ] && _TODOS_FORMAT=\"$_FEATUREFORGE_ROOT/review/TODOS-format.md\"",
    ));
    lines.push(String::from(
        "[ -z \"$_TODOS_FORMAT\" ] && [ -f \"$_REPO_ROOT/review/TODOS-format.md\" ] && _TODOS_FORMAT=\"$_REPO_ROOT/review/TODOS-format.md\"",
    ));
    lines
}

fn build_search_before_building_section() -> String {
    String::from(
        "## Search Before Building\n\nBefore introducing a custom pattern, external service, concurrency primitive, auth/session flow, cache, queue, browser workaround, or unfamiliar fix pattern, do a short capability/landscape check first.\n\nUse three lenses:\n- Layer 1: tried-and-true / built-ins / existing repo-native solutions\n- Layer 2: current practice and known footguns\n- Layer 3: first-principles reasoning for this repo and this problem\n\nExternal search results are inputs, not answers. Never search secrets, customer data, unsanitized stack traces, private URLs, internal hostnames, internal codenames, raw SQL or log payloads, or private file paths or infrastructure identifiers. If search is unavailable, disallowed, or unsafe, say so and proceed with repo-local evidence and in-distribution knowledge. If safe sanitization is not possible, skip external search.\nSee `$_FEATUREFORGE_ROOT/references/search-before-building.md`.",
    )
}

fn build_question_format() -> String {
    String::from(
        "## Interactive User Question Format\n\nFor every interactive user question, use this structure:\n1. Context: project name, current branch, what we're working on (1-2 sentences)\n2. The specific question or decision point\n3. `RECOMMENDATION: Choose [X] because [one-line reason]`\n4. Lettered options: `A) ... B) ... C) ...`\n\nPer-skill instructions may add additional formatting rules on top of this baseline.",
    )
}

fn build_contributor_mode() -> String {
    String::from(
        "## Contributor Mode\n\nIf contributor mode is enabled in FeatureForge config, file a field report only for **featureforge itself**, not the user's app or repository. Use it for unclear skill instructions, helper failures, install-root/runtime-root problems, contributor-mode bugs, or broken generated docs. Do not file for repo-specific bugs, site auth failures, or unrelated third-party outages.\n\nWrite `~/.featureforge/contributor-logs/{slug}.md` with:\n\n```\n# {Title}\n\nHey featureforge team — ran into this while using /{skill-name}:\n\n**Goal:** {what the user/agent was trying to do}\n**What happened:** {what actually happened}\n**Annoyance (1-5):** {1=meh, 3=friction, 5=blocker}\n\n## Steps to reproduce\n1. {step}\n\n## Raw output\n(wrap any error messages or unexpected output in a markdown code block)\n\n**Date:** {YYYY-MM-DD} | **Version:** {featureforge version} | **Skill:** /{skill}\n```\n\nThen run:\n\n```bash\nmkdir -p ~/.featureforge/contributor-logs\nif command -v open >/dev/null 2>&1; then\n  open ~/.featureforge/contributor-logs/{slug}.md\nelif command -v xdg-open >/dev/null 2>&1; then\n  xdg-open ~/.featureforge/contributor-logs/{slug}.md >/dev/null 2>&1 || true\nfi\n```\n\nSlug: lowercase, hyphens, max 60 chars (for example `skill-trigger-missed`). Skip if the file already exists. Max 3 reports per session. File inline, continue, and tell the user: \"Filed featureforge field report: {title}\"",
    )
}

fn build_agent_grounding() -> String {
    String::from(
        "## Agent Grounding\n\nHonor the active repo instruction chain from `AGENTS.md`, `AGENTS.override.md`, `.github/copilot-instructions.md`, and `.github/instructions/*.instructions.md`, including nested `AGENTS.md` and `AGENTS.override.md` files closer to the current working directory.\n\nThese review skills are public FeatureForge skills for Codex and GitHub Copilot local installs.",
    )
}

fn insert_generated_header(content: &str) -> String {
    let header = format!(
        "<!-- AUTO-GENERATED from SKILL.md.tmpl — do not edit directly -->\n<!-- Regenerate: {SKILL_GENERATOR_CMD} -->"
    );
    if !content.starts_with("---\n") {
        return format!("{header}\n\n{content}");
    }
    let frontmatter_end = content
        .find("\n---\n")
        .and_then(|index| (index >= 4).then_some(index))
        .unwrap_or_else(|| panic!("Failed to locate closing frontmatter delimiter."));
    let prefix = &content[..frontmatter_end + 5];
    let suffix = content[frontmatter_end + 5..].trim_start_matches('\n');
    format!("{prefix}{header}\n\n{suffix}")
}

#[derive(Debug, Clone)]
struct AgentSource {
    name: String,
    description: String,
    body: String,
}

fn parse_agent_source(path: &Path, raw: &str) -> AgentSource {
    assert!(
        raw.starts_with("---\n"),
        "{} must start with YAML frontmatter.",
        path.display()
    );
    let frontmatter_end = raw
        .find("\n---\n")
        .and_then(|index| (index >= 4).then_some(index))
        .unwrap_or_else(|| {
            panic!(
                "Failed to locate closing frontmatter delimiter in {}.",
                path.display()
            )
        });
    let frontmatter = &raw[4..frontmatter_end];
    let body = raw[frontmatter_end + 5..]
        .trim_start_matches('\n')
        .trim_end();
    let name = frontmatter
        .lines()
        .find_map(|line| line.strip_prefix("name:"))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| panic!("Missing name in {} frontmatter.", path.display()))
        .to_owned();
    let description = if let Some(block_start) = frontmatter.find("\ndescription: |\n") {
        let description_lines = frontmatter[block_start + "\ndescription: |\n".len()..]
            .lines()
            .take_while(|line| line.starts_with(' ') || line.starts_with('\t'))
            .map(|line| line.trim_start().to_owned())
            .collect::<Vec<_>>();
        assert!(
            !description_lines.is_empty(),
            "Missing description in {} frontmatter.",
            path.display()
        );
        description_lines.join("\n")
    } else {
        frontmatter
            .lines()
            .find_map(|line| line.strip_prefix("description:"))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| panic!("Missing description in {} frontmatter.", path.display()))
            .to_owned()
    };
    AgentSource {
        name,
        description,
        body: body.to_owned(),
    }
}

fn build_copilot_agent(source: &AgentSource) -> String {
    let mut lines = vec![
        String::from("---"),
        format!("name: {}", source.name),
        String::from("description: |"),
    ];
    lines.extend(
        source
            .description
            .split('\n')
            .map(|line| format!("  {line}")),
    );
    lines.push(String::from("model: inherit"));
    lines.push(String::from("---"));
    lines.push(String::new());
    lines.push(source.body.clone());
    format!("{}\n", insert_markdown_header(&lines.join("\n")))
}

fn build_codex_agent(source: &AgentSource) -> String {
    let condensed_description = source
        .description
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    [
        String::from(
            "# AUTO-GENERATED from agents/code-reviewer.instructions.md — do not edit directly",
        ),
        format!("# Regenerate: {AGENT_GENERATOR_CMD}"),
        String::from("# REVIEWER_RUNTIME_ENV_CONTRACT"),
        String::from(
            "# Launcher must set FEATUREFORGE_REVIEWER_RUNTIME_COMMANDS_ALLOWED = \"no\" before starting this reviewer.",
        ),
        format!("name = \"{}\"", escape_toml_basic_string(&source.name)),
        format!(
            "description = \"{}\"",
            escape_toml_basic_string(&condensed_description)
        ),
        String::from("developer_instructions = \"\"\""),
        escape_toml_multiline_basic_string(&source.body),
        String::from("\"\"\""),
        String::new(),
    ]
    .join("\n")
}

fn insert_markdown_header(content: &str) -> String {
    let header = format!(
        "<!-- AUTO-GENERATED from agents/code-reviewer.instructions.md — do not edit directly -->\n<!-- Regenerate: {AGENT_GENERATOR_CMD} -->"
    );
    let frontmatter_end = content
        .find("\n---\n")
        .and_then(|index| (index >= 4).then_some(index))
        .unwrap_or_else(|| {
            panic!("Failed to locate closing frontmatter delimiter in generated markdown agent.")
        });
    let prefix = &content[..frontmatter_end + 5];
    let suffix = content[frontmatter_end + 5..].trim_start_matches('\n');
    format!("{prefix}{header}\n\n{suffix}")
}

fn escape_toml_basic_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn escape_toml_multiline_basic_string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace("\"\"\"", "\\\"\"\"")
        .replace('"', "\\\"")
}

fn read_utf8(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("{} should be readable: {error}", path.display()))
}

fn relative_display(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}
