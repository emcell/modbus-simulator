//! Pure domain model for the Modbus simulator.
//!
//! Principles: read -> compute -> update. No I/O here.

pub mod behavior;
pub mod encoding;
pub mod engine;
pub mod model;

pub use behavior::{
    effective_behavior, DeviceBehavior, DeviceBehaviorOverrides, MissingBlockBehavior,
};
pub use encoding::{DataType, Encoding, Value};
pub use engine::{
    apply_state_update, process_request, EngineOutput, ModbusException, ModbusRequest,
    ModbusResponse, Outcome, StateUpdate, WordWrite,
};
pub use model::{
    Context, Device, DeviceId, DeviceType, DeviceTypeId, RegisterId, RegisterKind, RegisterPoint,
    RegisterValue, World,
};
