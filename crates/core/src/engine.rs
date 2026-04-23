//! Pure request -> response engine.
//!
//! Given a modbus request, the device instance state and the effective
//! behavior, produce one of: a response, an exception, or silence (timeout).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::behavior::{DeviceBehavior, MissingBlockBehavior};
use crate::encoding::{decode_value, encode_value, Value};
use crate::model::{Device, DeviceType, RegisterKind, RegisterPoint};

/// Decoded modbus request (transport-agnostic).
#[derive(Debug, Clone, PartialEq)]
pub enum ModbusRequest {
    ReadCoils { address: u16, quantity: u16 },
    ReadDiscreteInputs { address: u16, quantity: u16 },
    ReadHoldingRegisters { address: u16, quantity: u16 },
    ReadInputRegisters { address: u16, quantity: u16 },
    WriteSingleCoil { address: u16, value: bool },
    WriteSingleRegister { address: u16, value: u16 },
    WriteMultipleCoils { address: u16, values: Vec<bool> },
    WriteMultipleRegisters { address: u16, values: Vec<u16> },
}

impl ModbusRequest {
    #[must_use]
    pub fn function_code(&self) -> u8 {
        match self {
            Self::ReadCoils { .. } => 1,
            Self::ReadDiscreteInputs { .. } => 2,
            Self::ReadHoldingRegisters { .. } => 3,
            Self::ReadInputRegisters { .. } => 4,
            Self::WriteSingleCoil { .. } => 5,
            Self::WriteSingleRegister { .. } => 6,
            Self::WriteMultipleCoils { .. } => 15,
            Self::WriteMultipleRegisters { .. } => 16,
        }
    }

    fn quantity(&self) -> u16 {
        match self {
            Self::ReadCoils { quantity, .. }
            | Self::ReadDiscreteInputs { quantity, .. }
            | Self::ReadHoldingRegisters { quantity, .. }
            | Self::ReadInputRegisters { quantity, .. } => *quantity,
            Self::WriteSingleCoil { .. } | Self::WriteSingleRegister { .. } => 1,
            Self::WriteMultipleCoils { values, .. } => values.len() as u16,
            Self::WriteMultipleRegisters { values, .. } => values.len() as u16,
        }
    }

    fn target_kind(&self) -> Option<RegisterKind> {
        match self {
            Self::ReadCoils { .. }
            | Self::WriteSingleCoil { .. }
            | Self::WriteMultipleCoils { .. } => Some(RegisterKind::Coil),
            Self::ReadDiscreteInputs { .. } => Some(RegisterKind::Discrete),
            Self::ReadHoldingRegisters { .. }
            | Self::WriteSingleRegister { .. }
            | Self::WriteMultipleRegisters { .. } => Some(RegisterKind::Holding),
            Self::ReadInputRegisters { .. } => Some(RegisterKind::Input),
        }
    }
}

/// Modbus exception codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum ModbusException {
    IllegalFunction = 0x01,
    IllegalDataAddress = 0x02,
    IllegalDataValue = 0x03,
    SlaveDeviceFailure = 0x04,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ModbusResponse {
    ReadCoils(Vec<bool>),
    ReadDiscreteInputs(Vec<bool>),
    ReadHoldingRegisters(Vec<u16>),
    ReadInputRegisters(Vec<u16>),
    WriteSingleCoil { address: u16, value: bool },
    WriteSingleRegister { address: u16, value: u16 },
    WriteMultipleCoils { address: u16, quantity: u16 },
    WriteMultipleRegisters { address: u16, quantity: u16 },
}

/// Output of [`process_request`]. `None` = silence (timeout).
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Response(ModbusResponse),
    Exception(ModbusException),
    Silence,
}

/// Mutations the engine wants to apply to device state. The caller is
/// responsible for writing them back — keeping the engine itself pure.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StateUpdate {
    /// Updated register values, keyed by register point id as string (since
    /// we don't have the id here — actually we do). For simplicity we return
    /// the full new raw word map after the write.
    pub writes: Vec<WordWrite>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WordWrite {
    pub kind: RegisterKind,
    pub address: u16,
    pub word: u16,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EngineOutput {
    pub outcome: Outcome,
    pub state_update: StateUpdate,
}

/// Process a single modbus request.
pub fn process_request(
    request: &ModbusRequest,
    device: &Device,
    device_type: &DeviceType,
    effective: &DeviceBehavior,
) -> EngineOutput {
    let fc = request.function_code();
    if effective.disabled_function_codes.contains(&fc) {
        return EngineOutput {
            outcome: Outcome::Exception(ModbusException::IllegalFunction),
            state_update: StateUpdate::default(),
        };
    }

    if let Some(max) = effective.max_registers_per_request {
        if request.quantity() > max {
            return EngineOutput {
                outcome: Outcome::Exception(ModbusException::IllegalDataValue),
                state_update: StateUpdate::default(),
            };
        }
    }

    let Some(kind) = request.target_kind() else {
        return EngineOutput {
            outcome: Outcome::Exception(ModbusException::IllegalFunction),
            state_update: StateUpdate::default(),
        };
    };

    // Materialize the word map for the target kind.
    let map = materialize_words(device, device_type, kind);

    match request {
        ModbusRequest::ReadCoils { address, quantity }
        | ModbusRequest::ReadDiscreteInputs { address, quantity } => read_bits(
            &map,
            *address,
            *quantity,
            effective,
            kind == RegisterKind::Coil,
        ),
        ModbusRequest::ReadHoldingRegisters { address, quantity } => {
            read_words(&map, *address, *quantity, effective, true)
        }
        ModbusRequest::ReadInputRegisters { address, quantity } => {
            read_words(&map, *address, *quantity, effective, false)
        }
        ModbusRequest::WriteSingleCoil { address, value } => {
            write_single_bit(&map, *address, *value, effective)
        }
        ModbusRequest::WriteSingleRegister { address, value } => {
            write_single_word(&map, *address, *value, effective)
        }
        ModbusRequest::WriteMultipleCoils { address, values } => {
            write_multiple_bits(&map, *address, values, effective)
        }
        ModbusRequest::WriteMultipleRegisters { address, values } => {
            write_multiple_words(&map, *address, values, effective)
        }
    }
}

/// For a given register kind, compute (address -> word) including coverage
/// information. For coils/discretes a "word" holds 0 or 1 in the low bit.
fn materialize_words(
    device: &Device,
    device_type: &DeviceType,
    kind: RegisterKind,
) -> BTreeMap<u16, u16> {
    let mut out = BTreeMap::new();
    for rp in device_type.registers.iter().filter(|r| r.kind == kind) {
        let value = device
            .register_values
            .get(&rp.id)
            .cloned()
            .unwrap_or_else(|| rp.default_value.clone());
        let words = encode_point(rp, &value);
        for (i, w) in words.into_iter().enumerate() {
            out.insert(rp.address.saturating_add(i as u16), w);
        }
    }
    out
}

fn encode_point(rp: &RegisterPoint, value: &Value) -> Vec<u16> {
    if rp.kind.is_bit() {
        let b = match value {
            Value::Bool(v) => *v,
            Value::U16(n) => *n != 0,
            _ => false,
        };
        return vec![u16::from(b)];
    }
    encode_value(value, rp.data_type, rp.encoding, rp.byte_length)
        .unwrap_or_else(|_| vec![0u16; rp.data_type.word_count(rp.byte_length)])
}

fn range(address: u16, quantity: u16) -> std::ops::Range<u32> {
    let start = u32::from(address);
    let end = start + u32::from(quantity);
    start..end
}

enum Coverage {
    Full,
    None,
    Partial,
}

fn coverage(map: &BTreeMap<u16, u16>, address: u16, quantity: u16) -> Coverage {
    let r = range(address, quantity);
    let mut hits = 0u32;
    for a in r.clone() {
        if a <= u32::from(u16::MAX) && map.contains_key(&(a as u16)) {
            hits += 1;
        }
    }
    let total = r.end - r.start;
    if hits == 0 {
        Coverage::None
    } else if hits == total {
        Coverage::Full
    } else {
        Coverage::Partial
    }
}

fn missing_outcome(behavior: MissingBlockBehavior) -> Outcome {
    match behavior {
        MissingBlockBehavior::IllegalDataAddress => {
            Outcome::Exception(ModbusException::IllegalDataAddress)
        }
        MissingBlockBehavior::IllegalFunction => {
            Outcome::Exception(ModbusException::IllegalFunction)
        }
        MissingBlockBehavior::SlaveDeviceFailure => {
            Outcome::Exception(ModbusException::SlaveDeviceFailure)
        }
        MissingBlockBehavior::Timeout => Outcome::Silence,
        // ZeroFill is caller-dependent; signaled separately.
        MissingBlockBehavior::ZeroFill => Outcome::Exception(ModbusException::SlaveDeviceFailure),
    }
}

fn handle_missing(
    map: &BTreeMap<u16, u16>,
    address: u16,
    quantity: u16,
    effective: &DeviceBehavior,
) -> Option<Outcome> {
    match coverage(map, address, quantity) {
        Coverage::Full => None,
        Coverage::None => {
            if effective.missing_full_block == MissingBlockBehavior::ZeroFill {
                None // caller zero-fills
            } else {
                Some(missing_outcome(effective.missing_full_block))
            }
        }
        Coverage::Partial => {
            if effective.missing_partial_block == MissingBlockBehavior::ZeroFill {
                None
            } else {
                Some(missing_outcome(effective.missing_partial_block))
            }
        }
    }
}

fn read_words(
    map: &BTreeMap<u16, u16>,
    address: u16,
    quantity: u16,
    effective: &DeviceBehavior,
    is_holding: bool,
) -> EngineOutput {
    if let Some(o) = handle_missing(map, address, quantity, effective) {
        return EngineOutput {
            outcome: o,
            state_update: StateUpdate::default(),
        };
    }
    let mut out = Vec::with_capacity(quantity as usize);
    for i in 0..quantity {
        let a = address.wrapping_add(i);
        out.push(map.get(&a).copied().unwrap_or(0));
    }
    let response = if is_holding {
        ModbusResponse::ReadHoldingRegisters(out)
    } else {
        ModbusResponse::ReadInputRegisters(out)
    };
    EngineOutput {
        outcome: Outcome::Response(response),
        state_update: StateUpdate::default(),
    }
}

fn read_bits(
    map: &BTreeMap<u16, u16>,
    address: u16,
    quantity: u16,
    effective: &DeviceBehavior,
    is_coil: bool,
) -> EngineOutput {
    if let Some(o) = handle_missing(map, address, quantity, effective) {
        return EngineOutput {
            outcome: o,
            state_update: StateUpdate::default(),
        };
    }
    let mut out = Vec::with_capacity(quantity as usize);
    for i in 0..quantity {
        let a = address.wrapping_add(i);
        out.push(map.get(&a).copied().unwrap_or(0) != 0);
    }
    let response = if is_coil {
        ModbusResponse::ReadCoils(out)
    } else {
        ModbusResponse::ReadDiscreteInputs(out)
    };
    EngineOutput {
        outcome: Outcome::Response(response),
        state_update: StateUpdate::default(),
    }
}

fn write_single_word(
    map: &BTreeMap<u16, u16>,
    address: u16,
    value: u16,
    effective: &DeviceBehavior,
) -> EngineOutput {
    if !map.contains_key(&address) {
        return EngineOutput {
            outcome: missing_outcome(effective.missing_full_block),
            state_update: StateUpdate::default(),
        };
    }
    EngineOutput {
        outcome: Outcome::Response(ModbusResponse::WriteSingleRegister { address, value }),
        state_update: StateUpdate {
            writes: vec![WordWrite {
                kind: RegisterKind::Holding,
                address,
                word: value,
            }],
        },
    }
}

fn write_single_bit(
    map: &BTreeMap<u16, u16>,
    address: u16,
    value: bool,
    effective: &DeviceBehavior,
) -> EngineOutput {
    if !map.contains_key(&address) {
        return EngineOutput {
            outcome: missing_outcome(effective.missing_full_block),
            state_update: StateUpdate::default(),
        };
    }
    EngineOutput {
        outcome: Outcome::Response(ModbusResponse::WriteSingleCoil { address, value }),
        state_update: StateUpdate {
            writes: vec![WordWrite {
                kind: RegisterKind::Coil,
                address,
                word: u16::from(value),
            }],
        },
    }
}

fn write_multiple_words(
    map: &BTreeMap<u16, u16>,
    address: u16,
    values: &[u16],
    effective: &DeviceBehavior,
) -> EngineOutput {
    let quantity = values.len() as u16;
    if let Some(o) = handle_missing(map, address, quantity, effective) {
        return EngineOutput {
            outcome: o,
            state_update: StateUpdate::default(),
        };
    }
    let writes = values
        .iter()
        .enumerate()
        .filter_map(|(i, v)| {
            let a = address.wrapping_add(i as u16);
            if map.contains_key(&a) {
                Some(WordWrite {
                    kind: RegisterKind::Holding,
                    address: a,
                    word: *v,
                })
            } else {
                None
            }
        })
        .collect();
    EngineOutput {
        outcome: Outcome::Response(ModbusResponse::WriteMultipleRegisters { address, quantity }),
        state_update: StateUpdate { writes },
    }
}

fn write_multiple_bits(
    map: &BTreeMap<u16, u16>,
    address: u16,
    values: &[bool],
    effective: &DeviceBehavior,
) -> EngineOutput {
    let quantity = values.len() as u16;
    if let Some(o) = handle_missing(map, address, quantity, effective) {
        return EngineOutput {
            outcome: o,
            state_update: StateUpdate::default(),
        };
    }
    let writes = values
        .iter()
        .enumerate()
        .filter_map(|(i, v)| {
            let a = address.wrapping_add(i as u16);
            if map.contains_key(&a) {
                Some(WordWrite {
                    kind: RegisterKind::Coil,
                    address: a,
                    word: u16::from(*v),
                })
            } else {
                None
            }
        })
        .collect();
    EngineOutput {
        outcome: Outcome::Response(ModbusResponse::WriteMultipleCoils { address, quantity }),
        state_update: StateUpdate { writes },
    }
}

/// Apply [`StateUpdate`] writes back to the device's per-register values by
/// decoding the affected register points from the updated word map.
pub fn apply_state_update(device: &mut Device, device_type: &DeviceType, update: &StateUpdate) {
    if update.writes.is_empty() {
        return;
    }
    // Group writes by kind for targeted rebuild.
    use std::collections::BTreeSet;
    let mut kinds: BTreeSet<RegisterKind> = BTreeSet::new();
    let mut pending: BTreeMap<(RegisterKind, u16), u16> = BTreeMap::new();
    for w in &update.writes {
        kinds.insert(w.kind);
        pending.insert((w.kind, w.address), w.word);
    }
    for kind in kinds {
        // Build current map, overlay writes.
        let mut map = materialize_words(device, device_type, kind);
        for ((k, a), w) in &pending {
            if *k == kind {
                map.insert(*a, *w);
            }
        }
        // Decode each affected point and stash updated value.
        for rp in device_type.registers.iter().filter(|r| r.kind == kind) {
            let span = rp.word_count();
            let mut words = Vec::with_capacity(span as usize);
            for i in 0..span {
                words.push(map.get(&rp.address.wrapping_add(i)).copied().unwrap_or(0));
            }
            if rp.kind.is_bit() {
                let v = Value::Bool(words.first().copied().unwrap_or(0) != 0);
                device.register_values.insert(rp.id, v);
            } else if let Ok(v) = decode_value(&words, rp.data_type, rp.encoding, rp.byte_length) {
                device.register_values.insert(rp.id, v);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::behavior::DeviceBehavior;
    use crate::encoding::{DataType, Encoding};
    use crate::model::{DeviceId, DeviceTypeId, RegisterId};

    fn make(behavior: DeviceBehavior) -> (Device, DeviceType) {
        let r_id = RegisterId::new();
        let dt = DeviceType {
            id: DeviceTypeId::new(),
            name: "t".into(),
            description: String::new(),
            registers: vec![RegisterPoint {
                id: r_id,
                kind: RegisterKind::Holding,
                address: 100,
                name: "r1".into(),
                description: String::new(),
                data_type: DataType::U16,
                encoding: Encoding::BigEndian,
                byte_length: None,
                default_value: Value::U16(0xAAAA),
            }],
            behavior,
        };
        let dev = Device {
            id: DeviceId::new(),
            name: "d1".into(),
            slave_id: 1,
            device_type_id: dt.id,
            behavior_overrides: None,
            register_values: BTreeMap::new(),
        };
        (dev, dt)
    }

    #[test]
    fn read_existing_register_returns_default() {
        let (dev, dt) = make(DeviceBehavior::default());
        let req = ModbusRequest::ReadHoldingRegisters {
            address: 100,
            quantity: 1,
        };
        let out = process_request(&req, &dev, &dt, &dt.behavior);
        assert_eq!(
            out.outcome,
            Outcome::Response(ModbusResponse::ReadHoldingRegisters(vec![0xAAAA]))
        );
    }

    #[test]
    fn missing_full_block_returns_exception() {
        let (dev, dt) = make(DeviceBehavior::default());
        let req = ModbusRequest::ReadHoldingRegisters {
            address: 0,
            quantity: 5,
        };
        let out = process_request(&req, &dev, &dt, &dt.behavior);
        assert_eq!(
            out.outcome,
            Outcome::Exception(ModbusException::IllegalDataAddress)
        );
    }

    #[test]
    fn missing_partial_block_returns_exception() {
        let (dev, dt) = make(DeviceBehavior::default());
        let req = ModbusRequest::ReadHoldingRegisters {
            address: 100,
            quantity: 5,
        };
        let out = process_request(&req, &dev, &dt, &dt.behavior);
        assert_eq!(
            out.outcome,
            Outcome::Exception(ModbusException::IllegalDataAddress)
        );
    }

    #[test]
    fn zero_fill_on_partial_block() {
        let b = DeviceBehavior {
            missing_partial_block: MissingBlockBehavior::ZeroFill,
            ..Default::default()
        };
        let (dev, dt) = make(b.clone());
        let req = ModbusRequest::ReadHoldingRegisters {
            address: 100,
            quantity: 3,
        };
        let out = process_request(&req, &dev, &dt, &b);
        assert_eq!(
            out.outcome,
            Outcome::Response(ModbusResponse::ReadHoldingRegisters(vec![0xAAAA, 0, 0]))
        );
    }

    #[test]
    fn disabled_function_returns_illegal_function() {
        let b = DeviceBehavior {
            disabled_function_codes: vec![3],
            ..Default::default()
        };
        let (dev, dt) = make(b.clone());
        let req = ModbusRequest::ReadHoldingRegisters {
            address: 100,
            quantity: 1,
        };
        let out = process_request(&req, &dev, &dt, &b);
        assert_eq!(
            out.outcome,
            Outcome::Exception(ModbusException::IllegalFunction)
        );
    }

    #[test]
    fn max_registers_enforced() {
        let b = DeviceBehavior {
            max_registers_per_request: Some(1),
            missing_full_block: MissingBlockBehavior::ZeroFill,
            ..Default::default()
        };
        let (dev, dt) = make(b.clone());
        let req = ModbusRequest::ReadHoldingRegisters {
            address: 0,
            quantity: 2,
        };
        let out = process_request(&req, &dev, &dt, &b);
        assert_eq!(
            out.outcome,
            Outcome::Exception(ModbusException::IllegalDataValue)
        );
    }

    #[test]
    fn timeout_behavior_returns_silence() {
        let b = DeviceBehavior {
            missing_full_block: MissingBlockBehavior::Timeout,
            ..Default::default()
        };
        let (dev, dt) = make(b.clone());
        let req = ModbusRequest::ReadHoldingRegisters {
            address: 0,
            quantity: 1,
        };
        let out = process_request(&req, &dev, &dt, &b);
        assert_eq!(out.outcome, Outcome::Silence);
    }

    #[test]
    fn write_single_register_updates_state() {
        let (mut dev, dt) = make(DeviceBehavior::default());
        let req = ModbusRequest::WriteSingleRegister {
            address: 100,
            value: 0x1234,
        };
        let out = process_request(&req, &dev, &dt, &dt.behavior);
        apply_state_update(&mut dev, &dt, &out.state_update);
        let (_, r_id) = (dt.registers[0].address, dt.registers[0].id);
        assert_eq!(dev.register_values.get(&r_id), Some(&Value::U16(0x1234)));
    }
}
