//! Pure register value <-> 16-bit word encoding.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Logical data type of a register point. Determines the number of 16-bit
/// words the point occupies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DataType {
    U16,
    I16,
    U32,
    I32,
    U64,
    I64,
    F16,
    F32,
    F64,
    /// String with a fixed byte length. Word count = ceil(byte_len / 2).
    String,
}

/// Byte/word ordering for multi-register values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Encoding {
    /// Big-endian bytes, big-endian words.
    BigEndian,
    /// Little-endian bytes, little-endian words.
    LittleEndian,
    /// Big-endian bytes, word-swapped (common "mid-little").
    BigEndianWordSwap,
    /// Little-endian bytes, word-swapped.
    LittleEndianWordSwap,
}

/// Runtime value of a register point.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Value {
    U16(u16),
    I16(i16),
    U32(u32),
    I32(i32),
    U64(u64),
    I64(i64),
    F16(f32),
    F32(f32),
    F64(f64),
    String(String),
    Bool(bool),
}

#[derive(Debug, Error)]
pub enum EncodingError {
    #[error("value type {value} does not match data type {data_type:?}")]
    TypeMismatch {
        value: &'static str,
        data_type: DataType,
    },
    #[error("string length {actual} exceeds fixed byte length {max}")]
    StringTooLong { actual: usize, max: usize },
    #[error("invalid UTF-8 in string value")]
    InvalidUtf8,
}

impl DataType {
    /// Number of 16-bit words occupied. For String, `byte_len` must be provided.
    #[must_use]
    pub fn word_count(self, byte_len: Option<usize>) -> usize {
        match self {
            Self::U16 | Self::I16 | Self::F16 => 1,
            Self::U32 | Self::I32 | Self::F32 => 2,
            Self::U64 | Self::I64 | Self::F64 => 4,
            Self::String => {
                let b = byte_len.unwrap_or(0);
                b.div_ceil(2)
            }
        }
    }
}

impl Value {
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::U16(_) => "U16",
            Self::I16(_) => "I16",
            Self::U32(_) => "U32",
            Self::I32(_) => "I32",
            Self::U64(_) => "U64",
            Self::I64(_) => "I64",
            Self::F16(_) => "F16",
            Self::F32(_) => "F32",
            Self::F64(_) => "F64",
            Self::String(_) => "String",
            Self::Bool(_) => "Bool",
        }
    }
}

/// Encode a [`Value`] into a sequence of 16-bit registers.
pub fn encode_value(
    value: &Value,
    data_type: DataType,
    encoding: Encoding,
    byte_len: Option<usize>,
) -> Result<Vec<u16>, EncodingError> {
    let bytes = value_to_bytes(value, data_type, byte_len)?;
    Ok(bytes_to_words(&bytes, encoding))
}

/// Decode a sequence of 16-bit registers into a [`Value`].
pub fn decode_value(
    words: &[u16],
    data_type: DataType,
    encoding: Encoding,
    byte_len: Option<usize>,
) -> Result<Value, EncodingError> {
    let bytes = words_to_bytes(words, encoding);
    bytes_to_value(&bytes, data_type, byte_len)
}

fn value_to_bytes(
    value: &Value,
    data_type: DataType,
    byte_len: Option<usize>,
) -> Result<Vec<u8>, EncodingError> {
    fn mismatch(value: &Value, data_type: DataType) -> EncodingError {
        EncodingError::TypeMismatch {
            value: value.type_name(),
            data_type,
        }
    }
    match (value, data_type) {
        (Value::U16(v), DataType::U16) => Ok(v.to_be_bytes().to_vec()),
        (Value::I16(v), DataType::I16) => Ok(v.to_be_bytes().to_vec()),
        (Value::U32(v), DataType::U32) => Ok(v.to_be_bytes().to_vec()),
        (Value::I32(v), DataType::I32) => Ok(v.to_be_bytes().to_vec()),
        (Value::U64(v), DataType::U64) => Ok(v.to_be_bytes().to_vec()),
        (Value::I64(v), DataType::I64) => Ok(v.to_be_bytes().to_vec()),
        (Value::F16(v), DataType::F16) => Ok(half::f16::from_f32(*v).to_be_bytes().to_vec()),
        (Value::F32(v), DataType::F32) => Ok(v.to_be_bytes().to_vec()),
        (Value::F64(v), DataType::F64) => Ok(v.to_be_bytes().to_vec()),
        (Value::String(s), DataType::String) => {
            let max = byte_len.unwrap_or(0);
            let bytes = s.as_bytes();
            if bytes.len() > max {
                return Err(EncodingError::StringTooLong {
                    actual: bytes.len(),
                    max,
                });
            }
            let mut out = vec![0u8; max.div_ceil(2) * 2];
            out[..bytes.len()].copy_from_slice(bytes);
            Ok(out)
        }
        // Accept U16(0|1) as bool-like too? Keep strict for now.
        (v, dt) => Err(mismatch(v, dt)),
    }
}

fn bytes_to_value(
    bytes: &[u8],
    data_type: DataType,
    byte_len: Option<usize>,
) -> Result<Value, EncodingError> {
    match data_type {
        DataType::U16 => Ok(Value::U16(u16::from_be_bytes([bytes[0], bytes[1]]))),
        DataType::I16 => Ok(Value::I16(i16::from_be_bytes([bytes[0], bytes[1]]))),
        DataType::U32 => {
            let arr: [u8; 4] = bytes[..4].try_into().unwrap_or([0; 4]);
            Ok(Value::U32(u32::from_be_bytes(arr)))
        }
        DataType::I32 => {
            let arr: [u8; 4] = bytes[..4].try_into().unwrap_or([0; 4]);
            Ok(Value::I32(i32::from_be_bytes(arr)))
        }
        DataType::U64 => {
            let arr: [u8; 8] = bytes[..8].try_into().unwrap_or([0; 8]);
            Ok(Value::U64(u64::from_be_bytes(arr)))
        }
        DataType::I64 => {
            let arr: [u8; 8] = bytes[..8].try_into().unwrap_or([0; 8]);
            Ok(Value::I64(i64::from_be_bytes(arr)))
        }
        DataType::F16 => {
            let h = half::f16::from_be_bytes([bytes[0], bytes[1]]);
            Ok(Value::F16(h.to_f32()))
        }
        DataType::F32 => {
            let arr: [u8; 4] = bytes[..4].try_into().unwrap_or([0; 4]);
            Ok(Value::F32(f32::from_be_bytes(arr)))
        }
        DataType::F64 => {
            let arr: [u8; 8] = bytes[..8].try_into().unwrap_or([0; 8]);
            Ok(Value::F64(f64::from_be_bytes(arr)))
        }
        DataType::String => {
            let max = byte_len.unwrap_or(bytes.len());
            let slice = &bytes[..max.min(bytes.len())];
            let end = slice.iter().position(|b| *b == 0).unwrap_or(slice.len());
            std::str::from_utf8(&slice[..end])
                .map(|s| Value::String(s.to_string()))
                .map_err(|_| EncodingError::InvalidUtf8)
        }
    }
}

fn bytes_to_words(bytes: &[u8], encoding: Encoding) -> Vec<u16> {
    // Input bytes are in canonical big-endian form (from to_be_bytes).
    // Apply requested byte + word order.
    let mut out = Vec::with_capacity(bytes.len() / 2);
    let pairs: Vec<(u8, u8)> = bytes.chunks_exact(2).map(|c| (c[0], c[1])).collect();
    let reordered: Vec<u16> = match encoding {
        Encoding::BigEndian => pairs
            .iter()
            .map(|(h, l)| u16::from_be_bytes([*h, *l]))
            .collect(),
        Encoding::LittleEndian => pairs
            .iter()
            .map(|(h, l)| u16::from_be_bytes([*l, *h]))
            .collect(),
        Encoding::BigEndianWordSwap => {
            let mut v: Vec<u16> = pairs
                .iter()
                .map(|(h, l)| u16::from_be_bytes([*h, *l]))
                .collect();
            v.reverse();
            v
        }
        Encoding::LittleEndianWordSwap => {
            let mut v: Vec<u16> = pairs
                .iter()
                .map(|(h, l)| u16::from_be_bytes([*l, *h]))
                .collect();
            v.reverse();
            v
        }
    };
    out.extend(reordered);
    out
}

fn words_to_bytes(words: &[u16], encoding: Encoding) -> Vec<u8> {
    let ordered: Vec<u16> = match encoding {
        Encoding::BigEndian | Encoding::LittleEndian => words.to_vec(),
        Encoding::BigEndianWordSwap | Encoding::LittleEndianWordSwap => {
            let mut v = words.to_vec();
            v.reverse();
            v
        }
    };
    let mut out = Vec::with_capacity(ordered.len() * 2);
    for w in ordered {
        match encoding {
            Encoding::BigEndian | Encoding::BigEndianWordSwap => {
                out.extend_from_slice(&w.to_be_bytes());
            }
            Encoding::LittleEndian | Encoding::LittleEndianWordSwap => {
                let be = w.to_be_bytes();
                out.push(be[1]);
                out.push(be[0]);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u16_roundtrip() {
        let v = Value::U16(0xABCD);
        let w = encode_value(&v, DataType::U16, Encoding::BigEndian, None).unwrap();
        assert_eq!(w, vec![0xABCD]);
        let back = decode_value(&w, DataType::U16, Encoding::BigEndian, None).unwrap();
        assert_eq!(back, v);
    }

    #[test]
    fn u32_be_word_order() {
        let v = Value::U32(0x1234_5678);
        let w = encode_value(&v, DataType::U32, Encoding::BigEndian, None).unwrap();
        assert_eq!(w, vec![0x1234, 0x5678]);
    }

    #[test]
    fn u32_word_swap() {
        let v = Value::U32(0x1234_5678);
        let w = encode_value(&v, DataType::U32, Encoding::BigEndianWordSwap, None).unwrap();
        assert_eq!(w, vec![0x5678, 0x1234]);
        let back = decode_value(&w, DataType::U32, Encoding::BigEndianWordSwap, None).unwrap();
        assert_eq!(back, v);
    }

    #[test]
    fn u32_little_endian() {
        let v = Value::U32(0x1234_5678);
        let w = encode_value(&v, DataType::U32, Encoding::LittleEndian, None).unwrap();
        // BE bytes: 12 34 56 78 -> LE bytes swap within each word: 34 12 , 78 56
        // as u16 BE from those: 0x3412, 0x7856
        assert_eq!(w, vec![0x3412, 0x7856]);
        let back = decode_value(&w, DataType::U32, Encoding::LittleEndian, None).unwrap();
        assert_eq!(back, v);
    }

    #[test]
    fn f32_roundtrip() {
        let v = Value::F32(3.14);
        let w = encode_value(&v, DataType::F32, Encoding::BigEndian, None).unwrap();
        let back = decode_value(&w, DataType::F32, Encoding::BigEndian, None).unwrap();
        assert_eq!(back, v);
    }

    #[test]
    fn string_fixed_length() {
        let v = Value::String("Hi".to_string());
        let w = encode_value(&v, DataType::String, Encoding::BigEndian, Some(6)).unwrap();
        assert_eq!(w.len(), 3);
        let back = decode_value(&w, DataType::String, Encoding::BigEndian, Some(6)).unwrap();
        assert_eq!(back, Value::String("Hi".to_string()));
    }

    #[test]
    fn type_mismatch_errors() {
        let v = Value::U16(1);
        assert!(encode_value(&v, DataType::U32, Encoding::BigEndian, None).is_err());
    }

    #[test]
    fn word_count() {
        assert_eq!(DataType::U16.word_count(None), 1);
        assert_eq!(DataType::U32.word_count(None), 2);
        assert_eq!(DataType::U64.word_count(None), 4);
        assert_eq!(DataType::String.word_count(Some(7)), 4);
        assert_eq!(DataType::String.word_count(Some(8)), 4);
    }
}
