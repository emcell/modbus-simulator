//! Turn raw `ModbusRequest` / `ModbusResponse` pairs into human-readable
//! summaries and decoded field lists, so the Traffic panel can show
//! something more useful than hex.

use modsim_core::encoding::{decode_value, Value};
use modsim_core::engine::{ModbusRequest, ModbusResponse};
use modsim_core::model::{DeviceType, RegisterId, RegisterKind, RegisterPoint};

#[derive(Debug, Clone)]
pub struct DecodedValue {
    pub register_name: String,
    pub address: u16,
    pub data_type: String,
    pub value: String,
}

/// Which register kind + `[start, start+quantity)` range does this request
/// operate on? Returns `None` for requests that don't target a register
/// table (there are none in our set, but future-proof).
fn request_range(req: &ModbusRequest) -> Option<(RegisterKind, u16, u16)> {
    match req {
        ModbusRequest::ReadCoils { address, quantity } => {
            Some((RegisterKind::Coil, *address, *quantity))
        }
        ModbusRequest::ReadDiscreteInputs { address, quantity } => {
            Some((RegisterKind::Discrete, *address, *quantity))
        }
        ModbusRequest::ReadHoldingRegisters { address, quantity } => {
            Some((RegisterKind::Holding, *address, *quantity))
        }
        ModbusRequest::ReadInputRegisters { address, quantity } => {
            Some((RegisterKind::Input, *address, *quantity))
        }
        ModbusRequest::WriteSingleCoil { address, .. } => Some((RegisterKind::Coil, *address, 1)),
        ModbusRequest::WriteSingleRegister { address, .. } => {
            Some((RegisterKind::Holding, *address, 1))
        }
        ModbusRequest::WriteMultipleCoils { address, values } => {
            Some((RegisterKind::Coil, *address, values.len() as u16))
        }
        ModbusRequest::WriteMultipleRegisters { address, values } => {
            Some((RegisterKind::Holding, *address, values.len() as u16))
        }
    }
}

/// The ids of device-type registers whose full word-range falls inside
/// the operation range. Used to mark per-register read/write activity.
pub fn affected_registers(req: &ModbusRequest, device_type: &DeviceType) -> Vec<RegisterId> {
    let Some((kind, start, quantity)) = request_range(req) else {
        return Vec::new();
    };
    let end = u32::from(start) + u32::from(quantity);
    device_type
        .registers
        .iter()
        .filter(|r| r.kind == kind)
        .filter(|r| {
            let rp_end = u32::from(r.address) + u32::from(r.word_count());
            u32::from(r.address) >= u32::from(start) && rp_end <= end
        })
        .map(|r| r.id)
        .collect()
}

/// A compact one-liner like `"Read Holding Registers start=0 len=2"`.
pub fn summarize_request(req: &ModbusRequest) -> String {
    match req {
        ModbusRequest::ReadCoils { address, quantity } => {
            format!("Read Coils start={address} len={quantity}")
        }
        ModbusRequest::ReadDiscreteInputs { address, quantity } => {
            format!("Read Discrete Inputs start={address} len={quantity}")
        }
        ModbusRequest::ReadHoldingRegisters { address, quantity } => {
            format!("Read Holding Registers start={address} len={quantity}")
        }
        ModbusRequest::ReadInputRegisters { address, quantity } => {
            format!("Read Input Registers start={address} len={quantity}")
        }
        ModbusRequest::WriteSingleCoil { address, value } => {
            format!("Write Single Coil addr={address} value={value}")
        }
        ModbusRequest::WriteSingleRegister { address, value } => {
            format!("Write Single Register addr={address} value={value} (0x{value:04X})")
        }
        ModbusRequest::WriteMultipleCoils { address, values } => {
            format!("Write Multiple Coils start={address} len={}", values.len())
        }
        ModbusRequest::WriteMultipleRegisters { address, values } => {
            format!(
                "Write Multiple Registers start={address} len={}",
                values.len()
            )
        }
    }
}

/// Summary for the response side (paired with the request so it knows
/// what operation the reply is for).
pub fn summarize_response(req: &ModbusRequest, resp: &ModbusResponse) -> String {
    match (req, resp) {
        (
            ModbusRequest::ReadCoils { .. } | ModbusRequest::ReadDiscreteInputs { .. },
            ModbusResponse::ReadCoils(bits) | ModbusResponse::ReadDiscreteInputs(bits),
        ) => format!("{} bits returned", bits.len()),
        (
            ModbusRequest::ReadHoldingRegisters { .. } | ModbusRequest::ReadInputRegisters { .. },
            ModbusResponse::ReadHoldingRegisters(words) | ModbusResponse::ReadInputRegisters(words),
        ) => format!("{} words returned", words.len()),
        (_, ModbusResponse::WriteSingleCoil { address, value }) => {
            format!("Ack Write Coil addr={address} value={value}")
        }
        (_, ModbusResponse::WriteSingleRegister { address, value }) => {
            format!("Ack Write Register addr={address} value={value}")
        }
        (_, ModbusResponse::WriteMultipleCoils { address, quantity }) => {
            format!("Ack Write Coils start={address} len={quantity}")
        }
        (_, ModbusResponse::WriteMultipleRegisters { address, quantity }) => {
            format!("Ack Write Registers start={address} len={quantity}")
        }
        _ => String::new(),
    }
}

/// Decode a read response against the device type's register definitions.
/// Only registers that fall fully within the requested range are reported.
pub fn decode_read(
    req: &ModbusRequest,
    resp: &ModbusResponse,
    device_type: &DeviceType,
) -> Vec<DecodedValue> {
    let (start, quantity, is_bit, kind) = match req {
        ModbusRequest::ReadCoils { address, quantity } => {
            (*address, *quantity, true, RegisterKind::Coil)
        }
        ModbusRequest::ReadDiscreteInputs { address, quantity } => {
            (*address, *quantity, true, RegisterKind::Discrete)
        }
        ModbusRequest::ReadHoldingRegisters { address, quantity } => {
            (*address, *quantity, false, RegisterKind::Holding)
        }
        ModbusRequest::ReadInputRegisters { address, quantity } => {
            (*address, *quantity, false, RegisterKind::Input)
        }
        _ => return Vec::new(),
    };

    let (bits, words) = match resp {
        ModbusResponse::ReadCoils(b) | ModbusResponse::ReadDiscreteInputs(b) => {
            (Some(b.as_slice()), None)
        }
        ModbusResponse::ReadHoldingRegisters(w) | ModbusResponse::ReadInputRegisters(w) => {
            (None, Some(w.as_slice()))
        }
        _ => return Vec::new(),
    };

    decode_registers_in_range(device_type, kind, start, quantity, is_bit, bits, words)
}

/// Decode the values carried in a write request. Useful for showing what
/// the master pushed into the simulator.
pub fn decode_write_request(req: &ModbusRequest, device_type: &DeviceType) -> Vec<DecodedValue> {
    match req {
        ModbusRequest::WriteSingleRegister { address, value } => decode_registers_in_range(
            device_type,
            RegisterKind::Holding,
            *address,
            1,
            false,
            None,
            Some(&[*value]),
        ),
        ModbusRequest::WriteMultipleRegisters { address, values } => decode_registers_in_range(
            device_type,
            RegisterKind::Holding,
            *address,
            values.len() as u16,
            false,
            None,
            Some(values.as_slice()),
        ),
        ModbusRequest::WriteSingleCoil { address, value } => decode_registers_in_range(
            device_type,
            RegisterKind::Coil,
            *address,
            1,
            true,
            Some(&[*value]),
            None,
        ),
        ModbusRequest::WriteMultipleCoils { address, values } => decode_registers_in_range(
            device_type,
            RegisterKind::Coil,
            *address,
            values.len() as u16,
            true,
            Some(values.as_slice()),
            None,
        ),
        _ => Vec::new(),
    }
}

fn decode_registers_in_range(
    device_type: &DeviceType,
    kind: RegisterKind,
    start: u16,
    quantity: u16,
    is_bit: bool,
    bits: Option<&[bool]>,
    words: Option<&[u16]>,
) -> Vec<DecodedValue> {
    let mut out = Vec::new();
    let end = u32::from(start) + u32::from(quantity);
    for rp in device_type.registers.iter().filter(|r| r.kind == kind) {
        let span = rp.word_count() as u32;
        let rp_end = u32::from(rp.address) + span;
        if u32::from(rp.address) < u32::from(start) || rp_end > end {
            continue; // does not fully fit in the requested range
        }
        let offset = (u32::from(rp.address) - u32::from(start)) as usize;
        if is_bit {
            let b = bits
                .and_then(|slice| slice.get(offset).copied())
                .unwrap_or(false);
            out.push(DecodedValue {
                register_name: rp.name.clone(),
                address: rp.address,
                data_type: if kind == RegisterKind::Coil {
                    "COIL".into()
                } else {
                    "DISCRETE".into()
                },
                value: b.to_string(),
            });
        } else {
            let slice_len = span as usize;
            let Some(words) = words else { continue };
            if offset + slice_len > words.len() {
                continue;
            }
            let slice = &words[offset..offset + slice_len];
            let v = decode_value(slice, rp.data_type, rp.encoding, rp.byte_length).ok();
            out.push(DecodedValue {
                register_name: rp.name.clone(),
                address: rp.address,
                data_type: data_type_name(rp),
                value: value_to_display(v.as_ref()),
            });
        }
    }
    out
}

fn data_type_name(rp: &RegisterPoint) -> String {
    format!("{:?}", rp.data_type)
        .to_uppercase()
        .replace("DATATYPE::", "")
}

fn value_to_display(v: Option<&Value>) -> String {
    match v {
        Some(Value::U16(n)) => format!("{n} (0x{n:04X})"),
        Some(Value::I16(n)) => n.to_string(),
        Some(Value::U32(n)) => format!("{n} (0x{n:08X})"),
        Some(Value::I32(n)) => n.to_string(),
        Some(Value::U64(n)) => format!("{n} (0x{n:016X})"),
        Some(Value::I64(n)) => n.to_string(),
        Some(Value::F16(n) | Value::F32(n)) => format!("{n}"),
        Some(Value::F64(n)) => format!("{n}"),
        Some(Value::String(s)) => format!("\"{s}\""),
        Some(Value::Bool(b)) => b.to_string(),
        None => "?".into(),
    }
}
