use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub type RecommendedPublicCommandArgv = Option<Vec<String>>;
pub type RecommendedPublicCommandTemplate = Option<PublicCommandTemplate>;
pub type PublicCommandInputValues = BTreeMap<String, String>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PublicCommandTemplate {
    pub command_kind: String,
    pub base_argv: Vec<String>,
    pub required_input_names: Vec<String>,
    pub input_bindings: Vec<PublicCommandTemplateInput>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PublicCommandInputKind {
    Text,
    Enum,
    Path,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PublicCommandInputRequirement {
    pub name: String,
    pub kind: PublicCommandInputKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub must_exist: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_when: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PublicCommandTemplateInput {
    pub name: String,
    pub kind: PublicCommandInputKind,
    pub binding: PublicCommandInputBinding,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub must_exist: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_when: Option<String>,
    pub shell_escape_by_caller: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PublicCommandInputBinding {
    pub kind: PublicCommandInputBindingKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flag: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PublicCommandInputBindingKind {
    Flag,
    Virtual,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublicCommandTemplateBindingError {
    MissingInput {
        name: String,
    },
    UnknownInput {
        name: String,
    },
    EmptyInput {
        name: String,
    },
    InvalidEnumValue {
        name: String,
        value: String,
        allowed: Vec<String>,
    },
    MissingInputMetadata {
        name: String,
    },
    MissingFlagBinding {
        name: String,
    },
    InvalidRequiredWhen {
        name: String,
        required_when: String,
    },
}

impl fmt::Display for PublicCommandTemplateBindingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingInput { name } => write!(formatter, "missing required input `{name}`"),
            Self::UnknownInput { name } => write!(formatter, "unknown input `{name}`"),
            Self::EmptyInput { name } => write!(formatter, "input `{name}` must not be empty"),
            Self::InvalidEnumValue {
                name,
                value,
                allowed,
            } => write!(
                formatter,
                "input `{name}` value `{value}` is not one of: {}",
                allowed.join(", ")
            ),
            Self::MissingInputMetadata { name } => {
                write!(formatter, "template is missing metadata for input `{name}`")
            }
            Self::MissingFlagBinding { name } => {
                write!(
                    formatter,
                    "template input `{name}` is missing its CLI flag binding"
                )
            }
            Self::InvalidRequiredWhen {
                name,
                required_when,
            } => write!(
                formatter,
                "template input `{name}` has unsupported required_when expression `{required_when}`"
            ),
        }
    }
}

impl std::error::Error for PublicCommandTemplateBindingError {}

pub fn materialize_public_command_argv(
    template: &PublicCommandTemplate,
    inputs: &PublicCommandInputValues,
) -> Result<Vec<String>, PublicCommandTemplateBindingError> {
    validate_template_metadata(template, inputs)?;
    let mut argv = template.base_argv.clone();
    for input in &template.input_bindings {
        if !input_requirement_is_active(input, inputs)? {
            continue;
        }
        let value = required_bound_input_value(input, inputs)?;
        validate_bound_input_value(input, value)?;
        match input.binding.kind {
            PublicCommandInputBindingKind::Flag => {
                let flag = input.binding.flag.as_deref().ok_or_else(|| {
                    PublicCommandTemplateBindingError::MissingFlagBinding {
                        name: input.name.clone(),
                    }
                })?;
                argv.push(flag.to_owned());
                argv.push(value.trim().to_owned());
            }
            PublicCommandInputBindingKind::Virtual => {}
        }
    }
    Ok(argv)
}

fn validate_template_metadata(
    template: &PublicCommandTemplate,
    inputs: &PublicCommandInputValues,
) -> Result<(), PublicCommandTemplateBindingError> {
    let metadata_names = template
        .input_bindings
        .iter()
        .map(|input| input.name.as_str())
        .collect::<BTreeSet<_>>();
    for required_name in &template.required_input_names {
        if !metadata_names.contains(required_name.as_str()) {
            return Err(PublicCommandTemplateBindingError::MissingInputMetadata {
                name: required_name.clone(),
            });
        }
    }
    for input_name in inputs.keys() {
        if !metadata_names.contains(input_name.as_str()) {
            return Err(PublicCommandTemplateBindingError::UnknownInput {
                name: input_name.clone(),
            });
        }
    }
    Ok(())
}

fn input_requirement_is_active(
    input: &PublicCommandTemplateInput,
    inputs: &PublicCommandInputValues,
) -> Result<bool, PublicCommandTemplateBindingError> {
    let Some(required_when) = input.required_when.as_deref() else {
        return Ok(true);
    };
    if let Some((controller, expected)) = required_when.split_once("!=") {
        let actual = required_bound_controller_value(inputs, controller)?;
        return Ok(actual.trim() != expected.trim());
    }
    if let Some((controller, expected)) = required_when.split_once('=') {
        let actual = required_bound_controller_value(inputs, controller)?;
        return Ok(actual.trim() == expected.trim());
    }
    Err(PublicCommandTemplateBindingError::InvalidRequiredWhen {
        name: input.name.clone(),
        required_when: required_when.to_owned(),
    })
}

fn required_bound_controller_value<'a>(
    inputs: &'a PublicCommandInputValues,
    controller: &str,
) -> Result<&'a str, PublicCommandTemplateBindingError> {
    let controller = controller.trim();
    inputs.get(controller).map(String::as_str).ok_or_else(|| {
        PublicCommandTemplateBindingError::MissingInput {
            name: controller.to_owned(),
        }
    })
}

fn required_bound_input_value<'a>(
    input: &PublicCommandTemplateInput,
    inputs: &'a PublicCommandInputValues,
) -> Result<&'a str, PublicCommandTemplateBindingError> {
    inputs
        .get(&input.name)
        .map(String::as_str)
        .ok_or_else(|| PublicCommandTemplateBindingError::MissingInput {
            name: input.name.clone(),
        })
        .and_then(|value| {
            if value.trim().is_empty() {
                Err(PublicCommandTemplateBindingError::EmptyInput {
                    name: input.name.clone(),
                })
            } else {
                Ok(value)
            }
        })
}

fn validate_bound_input_value(
    input: &PublicCommandTemplateInput,
    value: &str,
) -> Result<(), PublicCommandTemplateBindingError> {
    if input.kind == PublicCommandInputKind::Enum
        && !input.values.iter().any(|allowed| allowed == value.trim())
    {
        return Err(PublicCommandTemplateBindingError::InvalidEnumValue {
            name: input.name.clone(),
            value: value.trim().to_owned(),
            allowed: input.values.clone(),
        });
    }
    Ok(())
}

fn is_false(value: &bool) -> bool {
    !*value
}
