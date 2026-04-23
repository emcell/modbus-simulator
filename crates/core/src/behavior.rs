//! Device behavior configuration and merging logic.

use serde::{Deserialize, Serialize};

/// How the simulator responds when a request touches non-existent registers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MissingBlockBehavior {
    #[default]
    IllegalDataAddress,
    IllegalFunction,
    SlaveDeviceFailure,
    /// Do not respond at all.
    Timeout,
    /// Return zeros for the missing parts (block succeeds).
    ZeroFill,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceBehavior {
    /// Modbus function codes (e.g. 3, 4, 6, 16) that this device does NOT support.
    #[serde(default)]
    pub disabled_function_codes: Vec<u8>,
    /// Maximum number of registers/coils per request. `None` = unlimited.
    #[serde(default)]
    pub max_registers_per_request: Option<u16>,
    #[serde(default)]
    pub missing_full_block: MissingBlockBehavior,
    /// Behavior when the block partially overlaps existing registers.
    #[serde(default)]
    pub missing_partial_block: MissingBlockBehavior,
    /// Artificial response delay in milliseconds.
    #[serde(default)]
    pub response_delay_ms: u32,
}

impl Default for DeviceBehavior {
    fn default() -> Self {
        Self {
            disabled_function_codes: Vec::new(),
            max_registers_per_request: None,
            missing_full_block: MissingBlockBehavior::IllegalDataAddress,
            missing_partial_block: MissingBlockBehavior::IllegalDataAddress,
            response_delay_ms: 0,
        }
    }
}

/// Per-instance overrides. `None` fields inherit from the device type.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceBehaviorOverrides {
    #[serde(default)]
    pub disabled_function_codes: Option<Vec<u8>>,
    #[serde(default)]
    pub max_registers_per_request: Option<Option<u16>>,
    #[serde(default)]
    pub missing_full_block: Option<MissingBlockBehavior>,
    #[serde(default)]
    pub missing_partial_block: Option<MissingBlockBehavior>,
    #[serde(default)]
    pub response_delay_ms: Option<u32>,
}

/// Merge overrides on top of a base behavior. Pure function.
#[must_use]
pub fn effective_behavior(
    base: &DeviceBehavior,
    overrides: Option<&DeviceBehaviorOverrides>,
) -> DeviceBehavior {
    let mut out = base.clone();
    let Some(o) = overrides else {
        return out;
    };
    if let Some(v) = &o.disabled_function_codes {
        out.disabled_function_codes = v.clone();
    }
    if let Some(v) = o.max_registers_per_request {
        out.max_registers_per_request = v;
    }
    if let Some(v) = o.missing_full_block {
        out.missing_full_block = v;
    }
    if let Some(v) = o.missing_partial_block {
        out.missing_partial_block = v;
    }
    if let Some(v) = o.response_delay_ms {
        out.response_delay_ms = v;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overrides_none_returns_base() {
        let base = DeviceBehavior {
            response_delay_ms: 100,
            ..Default::default()
        };
        let eff = effective_behavior(&base, None);
        assert_eq!(eff, base);
    }

    #[test]
    fn overrides_replace_specific_fields() {
        let base = DeviceBehavior {
            response_delay_ms: 100,
            ..Default::default()
        };
        let ov = DeviceBehaviorOverrides {
            response_delay_ms: Some(500),
            ..Default::default()
        };
        let eff = effective_behavior(&base, Some(&ov));
        assert_eq!(eff.response_delay_ms, 500);
    }

    #[test]
    fn overrides_can_clear_max_registers() {
        let base = DeviceBehavior {
            max_registers_per_request: Some(8),
            ..Default::default()
        };
        let ov = DeviceBehaviorOverrides {
            max_registers_per_request: Some(None),
            ..Default::default()
        };
        let eff = effective_behavior(&base, Some(&ov));
        assert_eq!(eff.max_registers_per_request, None);
    }
}
