//! Domain entities: device types, device instances, contexts.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::behavior::{DeviceBehavior, DeviceBehaviorOverrides};
use crate::encoding::{DataType, Encoding, Value};

macro_rules! id_type {
    ($name:ident) => {
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub Uuid);

        impl $name {
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }

        impl std::str::FromStr for $name {
            type Err = uuid::Error;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Ok(Self(Uuid::parse_str(s)?))
            }
        }
    };
}

id_type!(DeviceTypeId);
id_type!(DeviceId);
id_type!(RegisterId);
id_type!(ContextId);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RegisterKind {
    Holding,
    Input,
    Coil,
    Discrete,
}

impl RegisterKind {
    #[must_use]
    pub fn is_bit(self) -> bool {
        matches!(self, Self::Coil | Self::Discrete)
    }

    #[must_use]
    pub fn is_writable(self) -> bool {
        matches!(self, Self::Holding | Self::Coil)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegisterPoint {
    pub id: RegisterId,
    pub kind: RegisterKind,
    pub address: u16,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub data_type: DataType,
    pub encoding: Encoding,
    /// Only meaningful for `DataType::String`.
    #[serde(default)]
    pub byte_length: Option<usize>,
    pub default_value: Value,
}

impl RegisterPoint {
    /// How many 16-bit registers (or bit slots) this point occupies.
    #[must_use]
    pub fn word_count(&self) -> u16 {
        if self.kind.is_bit() {
            1
        } else {
            self.data_type.word_count(self.byte_length) as u16
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceType {
    pub id: DeviceTypeId,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub registers: Vec<RegisterPoint>,
    #[serde(default)]
    pub behavior: DeviceBehavior,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegisterValue {
    pub register_id: RegisterId,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Device {
    pub id: DeviceId,
    pub name: String,
    pub slave_id: u8,
    pub device_type_id: DeviceTypeId,
    #[serde(default)]
    pub behavior_overrides: Option<DeviceBehaviorOverrides>,
    /// Per-instance runtime values. Missing entries fall back to the type default.
    #[serde(default)]
    pub register_values: BTreeMap<RegisterId, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TcpTransport {
    pub enabled: bool,
    pub bind: String,
    pub port: u16,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RtuTransport {
    pub enabled: bool,
    pub device: String,
    pub baud_rate: u32,
    pub parity: String,
    pub data_bits: u8,
    pub stop_bits: u8,
    /// When set, the RTU loop takes ownership of the corresponding virtual
    /// serial's master fd instead of opening `device` via the serial-port
    /// driver. Bypasses termios-ioctl limitations on PTYs.
    #[serde(default)]
    pub virtual_serial_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TransportConfig {
    #[serde(default)]
    pub tcp: TcpTransport,
    #[serde(default)]
    pub rtu: RtuTransport,
}

/// A context bundles devices + transport settings. Device types are shared
/// and referenced by id (persisted separately so they stay reusable across
/// contexts).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Context {
    pub id: ContextId,
    pub name: String,
    #[serde(default)]
    pub devices: Vec<Device>,
    #[serde(default)]
    pub transport: TransportConfig,
}

/// The full in-memory world: device types, the currently loaded context, and
/// the list of known contexts. Pure data.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct World {
    pub device_types: Vec<DeviceType>,
    pub contexts: Vec<Context>,
    pub active_context_id: Option<ContextId>,
}

impl World {
    #[must_use]
    pub fn device_type(&self, id: DeviceTypeId) -> Option<&DeviceType> {
        self.device_types.iter().find(|t| t.id == id)
    }

    #[must_use]
    pub fn active_context(&self) -> Option<&Context> {
        let id = self.active_context_id?;
        self.contexts.iter().find(|c| c.id == id)
    }

    pub fn active_context_mut(&mut self) -> Option<&mut Context> {
        let id = self.active_context_id?;
        self.contexts.iter_mut().find(|c| c.id == id)
    }
}
