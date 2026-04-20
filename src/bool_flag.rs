use std::ops::Not;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
/// Runtime struct.
pub struct BoolFlag(bool);

impl BoolFlag {
    #[must_use]
    /// Runtime constant.
    pub const fn is_true(self) -> bool {
        self.0
    }
}

impl From<bool> for BoolFlag {
    fn from(value: bool) -> Self {
        Self(value)
    }
}

impl From<BoolFlag> for bool {
    fn from(value: BoolFlag) -> Self {
        value.0
    }
}

impl PartialEq<bool> for BoolFlag {
    fn eq(&self, other: &bool) -> bool {
        self.0 == *other
    }
}

impl PartialEq<BoolFlag> for bool {
    fn eq(&self, other: &BoolFlag) -> bool {
        *self == other.0
    }
}

impl Not for BoolFlag {
    type Output = bool;

    fn not(self) -> Self::Output {
        !self.0
    }
}
