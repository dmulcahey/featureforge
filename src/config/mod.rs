use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::cli::config::{ConfigGetArgs, ConfigSetArgs};
use crate::diagnostics::{DiagnosticError, FailureClass};

const LEGACY_CONFIG_FILE: &str = "config.yaml";
const CANONICAL_CONFIG_FILE: &str = "config/config.yaml";
const CONFIG_BACKUP_FILE: &str = "config.yaml.bak";

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct ConfigValues {
    update_check: Option<bool>,
    superpowers_contributor: Option<bool>,
}

pub fn get(args: &ConfigGetArgs) -> Result<String, DiagnosticError> {
    let state_dir = state_dir();
    let config = load_config(&state_dir)?;
    let value = match normalize_key(&args.key)?.as_str() {
        "update_check" => config.update_check.map(render_bool),
        "superpowers_contributor" => config.superpowers_contributor.map(render_bool),
        _ => None,
    };
    Ok(value.unwrap_or_default())
}

pub fn set(args: &ConfigSetArgs) -> Result<String, DiagnosticError> {
    let state_dir = state_dir();
    let mut config = load_config(&state_dir)?;
    let key = normalize_key(&args.key)?;
    let value = parse_bool(&args.value)?;

    match key.as_str() {
        "update_check" => config.update_check = Some(value),
        "superpowers_contributor" => config.superpowers_contributor = Some(value),
        _ => {}
    }

    write_config(&state_dir.join(CANONICAL_CONFIG_FILE), &config)?;
    Ok(String::new())
}

pub fn list() -> Result<String, DiagnosticError> {
    let state_dir = state_dir();
    let config = load_config(&state_dir)?;
    Ok(render_config(&config))
}

fn state_dir() -> PathBuf {
    env::var_os("SUPERPOWERS_STATE_DIR")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".superpowers")))
        .unwrap_or_else(|| PathBuf::from(".superpowers"))
}

fn load_config(state_dir: &Path) -> Result<ConfigValues, DiagnosticError> {
    let canonical_path = state_dir.join(CANONICAL_CONFIG_FILE);
    if canonical_path.is_file() {
        return parse_config_file(&canonical_path);
    }

    let legacy_path = state_dir.join(LEGACY_CONFIG_FILE);
    if !legacy_path.is_file() {
        return Ok(ConfigValues::default());
    }

    let contents = fs::read_to_string(&legacy_path).map_err(|error| {
        DiagnosticError::new(
            FailureClass::InvalidConfigFormat,
            format!(
                "Could not read the legacy config file {}: {error}",
                legacy_path.display()
            ),
        )
    })?;
    let parsed = parse_config_source(&contents)?;

    let backup_path = state_dir.join(CONFIG_BACKUP_FILE);
    if !backup_path.exists() {
        write_atomic(&backup_path, &contents)?;
    }
    write_config(&canonical_path, &parsed)?;

    Ok(parsed)
}

fn parse_config_file(path: &Path) -> Result<ConfigValues, DiagnosticError> {
    let contents = fs::read_to_string(path).map_err(|error| {
        DiagnosticError::new(
            FailureClass::InvalidConfigFormat,
            format!("Could not read config file {}: {error}", path.display()),
        )
    })?;
    parse_config_source(&contents)
}

fn parse_config_source(source: &str) -> Result<ConfigValues, DiagnosticError> {
    let mut config = ConfigValues::default();

    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if line.starts_with(' ') || line.starts_with('\t') {
            return Err(invalid_config(
                "Nested or indented YAML entries are not supported.",
            ));
        }
        let (raw_key, raw_value) = trimmed
            .split_once(':')
            .ok_or_else(|| invalid_config("Config entries must use a single 'key: value' form."))?;
        let key = normalize_key(raw_key)?;
        let value = parse_bool(raw_value.trim())?;

        match key.as_str() {
            "update_check" => config.update_check = Some(value),
            "superpowers_contributor" => config.superpowers_contributor = Some(value),
            _ => return Err(invalid_config("Unsupported config key.")),
        }
    }

    Ok(config)
}

fn normalize_key(key: &str) -> Result<String, DiagnosticError> {
    let trimmed = key.trim();
    match trimmed {
        "update_check" | "superpowers_contributor" => Ok(trimmed.to_owned()),
        _ => Err(invalid_config("Unsupported config key.")),
    }
}

fn parse_bool(value: &str) -> Result<bool, DiagnosticError> {
    match value.trim() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(invalid_config(
            "Config values must be plain true or false scalars.",
        )),
    }
}

fn render_config(config: &ConfigValues) -> String {
    let mut lines = Vec::new();
    if let Some(value) = config.update_check {
        lines.push(format!("update_check: {}", render_bool(value)));
    }
    if let Some(value) = config.superpowers_contributor {
        lines.push(format!("superpowers_contributor: {}", render_bool(value)));
    }
    if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    }
}

fn render_bool(value: bool) -> String {
    if value {
        String::from("true")
    } else {
        String::from("false")
    }
}

fn write_config(path: &Path, config: &ConfigValues) -> Result<(), DiagnosticError> {
    write_atomic(path, &render_config(config))
}

fn write_atomic(path: &Path, contents: &str) -> Result<(), DiagnosticError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            DiagnosticError::new(
                FailureClass::InvalidConfigFormat,
                format!(
                    "Could not create config directory {}: {error}",
                    parent.display()
                ),
            )
        })?;
    }
    let tmp_path = path.with_extension(format!(
        "{}.tmp",
        path.extension()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or("config")
    ));
    fs::write(&tmp_path, contents).map_err(|error| {
        DiagnosticError::new(
            FailureClass::InvalidConfigFormat,
            format!(
                "Could not write config temp file {}: {error}",
                tmp_path.display()
            ),
        )
    })?;
    fs::rename(&tmp_path, path).map_err(|error| {
        DiagnosticError::new(
            FailureClass::InvalidConfigFormat,
            format!(
                "Could not move config temp file into place {}: {error}",
                path.display()
            ),
        )
    })?;
    Ok(())
}

fn invalid_config(message: &str) -> DiagnosticError {
    DiagnosticError::new(FailureClass::InvalidConfigFormat, message)
}
