//! Modbus PDU parsing and serialization. Transport-agnostic.

use modsim_core::engine::{ModbusException, ModbusRequest, ModbusResponse};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PduError {
    #[error("short pdu")]
    Short,
    #[error("unsupported function code {0}")]
    UnsupportedFunction(u8),
    #[error("invalid quantity")]
    InvalidQuantity,
    #[error("invalid byte count")]
    InvalidByteCount,
}

/// Parse a PDU (starting with the function code byte).
pub fn parse_request(pdu: &[u8]) -> Result<ModbusRequest, PduError> {
    if pdu.is_empty() {
        return Err(PduError::Short);
    }
    let fc = pdu[0];
    let data = &pdu[1..];
    match fc {
        1..=4 => {
            if data.len() < 4 {
                return Err(PduError::Short);
            }
            let address = u16::from_be_bytes([data[0], data[1]]);
            let quantity = u16::from_be_bytes([data[2], data[3]]);
            Ok(match fc {
                1 => ModbusRequest::ReadCoils { address, quantity },
                2 => ModbusRequest::ReadDiscreteInputs { address, quantity },
                3 => ModbusRequest::ReadHoldingRegisters { address, quantity },
                _ => ModbusRequest::ReadInputRegisters { address, quantity },
            })
        }
        5 => {
            if data.len() < 4 {
                return Err(PduError::Short);
            }
            let address = u16::from_be_bytes([data[0], data[1]]);
            let val = u16::from_be_bytes([data[2], data[3]]);
            let value = match val {
                0x0000 => false,
                0xFF00 => true,
                _ => return Err(PduError::InvalidQuantity),
            };
            Ok(ModbusRequest::WriteSingleCoil { address, value })
        }
        6 => {
            if data.len() < 4 {
                return Err(PduError::Short);
            }
            let address = u16::from_be_bytes([data[0], data[1]]);
            let value = u16::from_be_bytes([data[2], data[3]]);
            Ok(ModbusRequest::WriteSingleRegister { address, value })
        }
        15 => {
            if data.len() < 5 {
                return Err(PduError::Short);
            }
            let address = u16::from_be_bytes([data[0], data[1]]);
            let quantity = u16::from_be_bytes([data[2], data[3]]) as usize;
            let byte_count = data[4] as usize;
            if data.len() < 5 + byte_count {
                return Err(PduError::Short);
            }
            if byte_count != quantity.div_ceil(8) {
                return Err(PduError::InvalidByteCount);
            }
            let bits_bytes = &data[5..5 + byte_count];
            let mut values = Vec::with_capacity(quantity);
            for i in 0..quantity {
                let b = bits_bytes[i / 8];
                values.push(((b >> (i % 8)) & 1) != 0);
            }
            Ok(ModbusRequest::WriteMultipleCoils { address, values })
        }
        16 => {
            if data.len() < 5 {
                return Err(PduError::Short);
            }
            let address = u16::from_be_bytes([data[0], data[1]]);
            let quantity = u16::from_be_bytes([data[2], data[3]]) as usize;
            let byte_count = data[4] as usize;
            if byte_count != quantity * 2 || data.len() < 5 + byte_count {
                return Err(PduError::InvalidByteCount);
            }
            let regs: Vec<u16> = data[5..5 + byte_count]
                .chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect();
            Ok(ModbusRequest::WriteMultipleRegisters {
                address,
                values: regs,
            })
        }
        other => Err(PduError::UnsupportedFunction(other)),
    }
}

/// Build a response PDU from an engine response. Function code is derived
/// from the response variant.
pub fn build_response(resp: &ModbusResponse) -> Vec<u8> {
    match resp {
        ModbusResponse::ReadCoils(bits) => build_bits(1, bits),
        ModbusResponse::ReadDiscreteInputs(bits) => build_bits(2, bits),
        ModbusResponse::ReadHoldingRegisters(words) => build_words(3, words),
        ModbusResponse::ReadInputRegisters(words) => build_words(4, words),
        ModbusResponse::WriteSingleCoil { address, value } => {
            let v = if *value { 0xFF00 } else { 0x0000 };
            let mut out = vec![5];
            out.extend_from_slice(&address.to_be_bytes());
            out.extend_from_slice(&u16::to_be_bytes(v));
            out
        }
        ModbusResponse::WriteSingleRegister { address, value } => {
            let mut out = vec![6];
            out.extend_from_slice(&address.to_be_bytes());
            out.extend_from_slice(&value.to_be_bytes());
            out
        }
        ModbusResponse::WriteMultipleCoils { address, quantity } => {
            let mut out = vec![15];
            out.extend_from_slice(&address.to_be_bytes());
            out.extend_from_slice(&quantity.to_be_bytes());
            out
        }
        ModbusResponse::WriteMultipleRegisters { address, quantity } => {
            let mut out = vec![16];
            out.extend_from_slice(&address.to_be_bytes());
            out.extend_from_slice(&quantity.to_be_bytes());
            out
        }
    }
}

fn build_bits(fc: u8, bits: &[bool]) -> Vec<u8> {
    let byte_count = bits.len().div_ceil(8);
    let mut out = vec![fc, byte_count as u8];
    let mut cur = vec![0u8; byte_count];
    for (i, b) in bits.iter().enumerate() {
        if *b {
            cur[i / 8] |= 1 << (i % 8);
        }
    }
    out.extend_from_slice(&cur);
    out
}

fn build_words(fc: u8, words: &[u16]) -> Vec<u8> {
    let byte_count = words.len() * 2;
    let mut out = vec![fc, byte_count as u8];
    for w in words {
        out.extend_from_slice(&w.to_be_bytes());
    }
    out
}

pub fn build_exception(function_code: u8, ex: ModbusException) -> Vec<u8> {
    vec![function_code | 0x80, ex as u8]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_read_holding() {
        let pdu = [0x03, 0x00, 0x10, 0x00, 0x02];
        let req = parse_request(&pdu).unwrap();
        assert_eq!(
            req,
            ModbusRequest::ReadHoldingRegisters {
                address: 0x10,
                quantity: 2
            }
        );
    }

    #[test]
    fn build_read_holding_response() {
        let bytes = build_response(&ModbusResponse::ReadHoldingRegisters(vec![0x1234, 0x5678]));
        assert_eq!(bytes, vec![0x03, 0x04, 0x12, 0x34, 0x56, 0x78]);
    }

    #[test]
    fn exception_pdu() {
        let bytes = build_exception(3, ModbusException::IllegalDataAddress);
        assert_eq!(bytes, vec![0x83, 0x02]);
    }

    #[test]
    fn parse_write_single_coil() {
        let pdu = [0x05, 0x00, 0x01, 0xFF, 0x00];
        assert_eq!(
            parse_request(&pdu).unwrap(),
            ModbusRequest::WriteSingleCoil {
                address: 1,
                value: true
            }
        );
    }

    #[test]
    fn parse_write_multiple_registers() {
        let pdu = [0x10, 0x00, 0x05, 0x00, 0x02, 0x04, 0x00, 0x0A, 0x00, 0x0B];
        assert_eq!(
            parse_request(&pdu).unwrap(),
            ModbusRequest::WriteMultipleRegisters {
                address: 5,
                values: vec![10, 11]
            }
        );
    }
}
