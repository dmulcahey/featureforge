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

    for template_path in template_paths {
        let skill_path = template_path.with_extension("");
        let current = fs::read_to_string(&skill_path)
            .unwrap_or_else(|error| panic!("{} should be readable: {error}", skill_path.display()));
        assert!(
            current.contains("<!-- AUTO-GENERATED from SKILL.md.tmpl"),
            "{} should be a generated skill doc",
            relative_display(root, &skill_path)
        );
        assert!(
            !current.contains("{{"),
            "{} should not contain unresolved template placeholders",
            relative_display(root, &skill_path)
        );
    }
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
