use crate::diagnostics::{DiagnosticError, FailureClass};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RepoPath(String);

impl RepoPath {
    pub fn parse(input: &str) -> Result<Self, DiagnosticError> {
        normalize_repo_relative_path(input).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub fn normalize_repo_relative_path(input: &str) -> Result<String, DiagnosticError> {
    if input.is_empty() || input.starts_with('/') {
        return Err(invalid_repo_path(input));
    }
    if looks_like_windows_absolute(input) {
        return Err(invalid_repo_path(input));
    }

    let normalized_input = input.replace('\\', "/");
    if normalized_input.is_empty()
        || normalized_input.starts_with('/')
        || normalized_input.starts_with("//")
        || looks_like_windows_absolute(&normalized_input)
    {
        return Err(invalid_repo_path(input));
    }

    let mut parts = Vec::new();
    for part in normalized_input.split('/') {
        match part {
            "" | "." => {}
            ".." => return Err(invalid_repo_path(input)),
            value => parts.push(value),
        }
    }

    if parts.is_empty() {
        return Err(invalid_repo_path(input));
    }

    Ok(parts.join("/"))
}

pub fn normalize_whitespace(value: &str) -> String {
    let mut normalized = String::new();
    for token in value.split_whitespace() {
        if !normalized.is_empty() {
            normalized.push(' ');
        }
        normalized.push_str(token);
    }
    normalized
}

pub fn normalize_identifier_token(value: &str) -> String {
    let normalized = normalize_whitespace(value);
    if normalized.is_empty() {
        return String::new();
    }

    let mut output = String::with_capacity(normalized.len());
    for ch in normalized.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
            output.push(ch);
        } else {
            output.push('-');
        }
    }

    if output.chars().all(|ch| ch == '-') {
        String::new()
    } else {
        output
    }
}

fn invalid_repo_path(input: &str) -> DiagnosticError {
    DiagnosticError::new(
        FailureClass::InvalidRepoPath,
        format!("Paths must stay repo-relative and non-traversing: {input}"),
    )
}

fn looks_like_windows_absolute(input: &str) -> bool {
    let bytes = input.as_bytes();
    matches!(bytes, [drive, b':', b'/' | b'\\', ..] if drive.is_ascii_alphabetic())
        || input.starts_with("\\\\")
}
