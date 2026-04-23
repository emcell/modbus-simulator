//! GraphQL schema.

use std::sync::Arc;

use async_graphql::{
    ComplexObject, Context, Enum, InputObject, Object, Result, Schema, SimpleObject, Subscription,
    ID,
};
use futures_util::Stream;
use modsim_core::behavior::{
    DeviceBehavior as CoreBehavior, DeviceBehaviorOverrides as CoreOverrides,
    MissingBlockBehavior as CoreMissing,
};
use modsim_core::encoding::{
    DataType as CoreDataType, Encoding as CoreEncoding, Value as CoreValue,
};
use modsim_core::model::{
    Context as CoreContext, ContextId, Device as CoreDevice, DeviceId,
    DeviceType as CoreDeviceType, DeviceTypeId, RegisterId, RegisterKind as CoreRegisterKind,
    RegisterPoint as CoreRegister, RtuTransport as CoreRtu, TcpTransport as CoreTcp,
};
use std::str::FromStr;
use tokio_stream::wrappers::BroadcastStream;

use crate::state::AppState;

pub type ApiSchema = Schema<Query, Mutation, SubscriptionRoot>;

pub fn build_schema(state: Arc<AppState>) -> ApiSchema {
    Schema::build(Query, Mutation, SubscriptionRoot)
        .data(state)
        .finish()
}

pub struct SubscriptionRoot;

#[derive(SimpleObject, Clone)]
pub struct DecodedField {
    pub register_name: String,
    pub address: i32,
    pub data_type: String,
    pub value: String,
}

#[derive(SimpleObject, Clone)]
pub struct TrafficEvent {
    pub direction: String,
    pub transport: String,
    pub slave_id: i32,
    pub function_code: i32,
    pub bytes_hex: String,
    pub timestamp_ms: String,
    pub summary: String,
    pub decoded: Vec<DecodedField>,
}

#[Subscription]
impl SubscriptionRoot {
    async fn traffic(&self, ctx: &Context<'_>) -> impl Stream<Item = TrafficEvent> {
        use futures_util::StreamExt;
        let state = ctx.data_unchecked::<Arc<AppState>>();
        let rx = state.events.subscribe();
        BroadcastStream::new(rx).filter_map(|ev| async move {
            match ev.ok()? {
                crate::state::WorldEvent::TrafficFrame(f) => Some(TrafficEvent {
                    direction: f.direction.to_string(),
                    transport: f.transport.to_string(),
                    slave_id: f.slave_id.into(),
                    function_code: f.function_code.into(),
                    bytes_hex: f.bytes_hex,
                    timestamp_ms: f.timestamp_ms.to_string(),
                    summary: f.summary,
                    decoded: f
                        .decoded
                        .into_iter()
                        .map(|d| DecodedField {
                            register_name: d.register_name,
                            address: d.address.into(),
                            data_type: d.data_type,
                            value: d.value,
                        })
                        .collect(),
                }),
                crate::state::WorldEvent::WorldChanged => None,
            }
        })
    }

    async fn world_changed(&self, ctx: &Context<'_>) -> impl Stream<Item = bool> {
        use futures_util::StreamExt;
        let state = ctx.data_unchecked::<Arc<AppState>>();
        let rx = state.events.subscribe();
        BroadcastStream::new(rx).filter_map(|ev| async move {
            match ev.ok()? {
                crate::state::WorldEvent::WorldChanged => Some(true),
                crate::state::WorldEvent::TrafficFrame(_) => None,
            }
        })
    }
}

// ---- enums -----------------------------------------------------------------

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum GqlDataType {
    U16,
    I16,
    U32,
    I32,
    U64,
    I64,
    F16,
    F32,
    F64,
    String,
}

impl From<GqlDataType> for CoreDataType {
    fn from(v: GqlDataType) -> Self {
        match v {
            GqlDataType::U16 => Self::U16,
            GqlDataType::I16 => Self::I16,
            GqlDataType::U32 => Self::U32,
            GqlDataType::I32 => Self::I32,
            GqlDataType::U64 => Self::U64,
            GqlDataType::I64 => Self::I64,
            GqlDataType::F16 => Self::F16,
            GqlDataType::F32 => Self::F32,
            GqlDataType::F64 => Self::F64,
            GqlDataType::String => Self::String,
        }
    }
}
impl From<CoreDataType> for GqlDataType {
    fn from(v: CoreDataType) -> Self {
        match v {
            CoreDataType::U16 => Self::U16,
            CoreDataType::I16 => Self::I16,
            CoreDataType::U32 => Self::U32,
            CoreDataType::I32 => Self::I32,
            CoreDataType::U64 => Self::U64,
            CoreDataType::I64 => Self::I64,
            CoreDataType::F16 => Self::F16,
            CoreDataType::F32 => Self::F32,
            CoreDataType::F64 => Self::F64,
            CoreDataType::String => Self::String,
        }
    }
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum GqlEncoding {
    BigEndian,
    LittleEndian,
    BigEndianWordSwap,
    LittleEndianWordSwap,
}

impl From<GqlEncoding> for CoreEncoding {
    fn from(v: GqlEncoding) -> Self {
        match v {
            GqlEncoding::BigEndian => Self::BigEndian,
            GqlEncoding::LittleEndian => Self::LittleEndian,
            GqlEncoding::BigEndianWordSwap => Self::BigEndianWordSwap,
            GqlEncoding::LittleEndianWordSwap => Self::LittleEndianWordSwap,
        }
    }
}
impl From<CoreEncoding> for GqlEncoding {
    fn from(v: CoreEncoding) -> Self {
        match v {
            CoreEncoding::BigEndian => Self::BigEndian,
            CoreEncoding::LittleEndian => Self::LittleEndian,
            CoreEncoding::BigEndianWordSwap => Self::BigEndianWordSwap,
            CoreEncoding::LittleEndianWordSwap => Self::LittleEndianWordSwap,
        }
    }
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum GqlRegisterKind {
    Holding,
    Input,
    Coil,
    Discrete,
}
impl From<GqlRegisterKind> for CoreRegisterKind {
    fn from(v: GqlRegisterKind) -> Self {
        match v {
            GqlRegisterKind::Holding => Self::Holding,
            GqlRegisterKind::Input => Self::Input,
            GqlRegisterKind::Coil => Self::Coil,
            GqlRegisterKind::Discrete => Self::Discrete,
        }
    }
}
impl From<CoreRegisterKind> for GqlRegisterKind {
    fn from(v: CoreRegisterKind) -> Self {
        match v {
            CoreRegisterKind::Holding => Self::Holding,
            CoreRegisterKind::Input => Self::Input,
            CoreRegisterKind::Coil => Self::Coil,
            CoreRegisterKind::Discrete => Self::Discrete,
        }
    }
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum GqlMissingBehavior {
    IllegalDataAddress,
    IllegalFunction,
    SlaveDeviceFailure,
    Timeout,
    ZeroFill,
}
impl From<GqlMissingBehavior> for CoreMissing {
    fn from(v: GqlMissingBehavior) -> Self {
        match v {
            GqlMissingBehavior::IllegalDataAddress => Self::IllegalDataAddress,
            GqlMissingBehavior::IllegalFunction => Self::IllegalFunction,
            GqlMissingBehavior::SlaveDeviceFailure => Self::SlaveDeviceFailure,
            GqlMissingBehavior::Timeout => Self::Timeout,
            GqlMissingBehavior::ZeroFill => Self::ZeroFill,
        }
    }
}
impl From<CoreMissing> for GqlMissingBehavior {
    fn from(v: CoreMissing) -> Self {
        match v {
            CoreMissing::IllegalDataAddress => Self::IllegalDataAddress,
            CoreMissing::IllegalFunction => Self::IllegalFunction,
            CoreMissing::SlaveDeviceFailure => Self::SlaveDeviceFailure,
            CoreMissing::Timeout => Self::Timeout,
            CoreMissing::ZeroFill => Self::ZeroFill,
        }
    }
}

// ---- output types ----------------------------------------------------------

#[derive(SimpleObject, Clone)]
pub struct Behavior {
    pub disabled_function_codes: Vec<i32>,
    pub max_registers_per_request: Option<i32>,
    pub missing_full_block: GqlMissingBehavior,
    pub missing_partial_block: GqlMissingBehavior,
    pub response_delay_ms: i32,
}

impl From<CoreBehavior> for Behavior {
    fn from(b: CoreBehavior) -> Self {
        Self {
            disabled_function_codes: b
                .disabled_function_codes
                .into_iter()
                .map(i32::from)
                .collect(),
            max_registers_per_request: b.max_registers_per_request.map(i32::from),
            missing_full_block: b.missing_full_block.into(),
            missing_partial_block: b.missing_partial_block.into(),
            response_delay_ms: b.response_delay_ms as i32,
        }
    }
}

#[derive(SimpleObject, Clone)]
pub struct ScalarValue {
    /// One of U16,I16,U32,I32,U64,I64,F16,F32,F64,STRING,BOOL
    pub data_type: String,
    /// Stringified numeric / string / boolean value.
    pub value: String,
}

fn value_to_gql(v: &CoreValue) -> ScalarValue {
    let (t, s) = match v {
        CoreValue::U16(n) => ("U16", n.to_string()),
        CoreValue::I16(n) => ("I16", n.to_string()),
        CoreValue::U32(n) => ("U32", n.to_string()),
        CoreValue::I32(n) => ("I32", n.to_string()),
        CoreValue::U64(n) => ("U64", n.to_string()),
        CoreValue::I64(n) => ("I64", n.to_string()),
        CoreValue::F16(n) => ("F16", n.to_string()),
        CoreValue::F32(n) => ("F32", n.to_string()),
        CoreValue::F64(n) => ("F64", n.to_string()),
        CoreValue::String(s) => ("STRING", s.clone()),
        CoreValue::Bool(b) => ("BOOL", b.to_string()),
    };
    ScalarValue {
        data_type: t.to_string(),
        value: s,
    }
}

fn parse_scalar(input: &ValueInput) -> Result<CoreValue> {
    let t = input.data_type.to_uppercase();
    let s = &input.value;
    Ok(match t.as_str() {
        "U16" => CoreValue::U16(s.parse()?),
        "I16" => CoreValue::I16(s.parse()?),
        "U32" => CoreValue::U32(s.parse()?),
        "I32" => CoreValue::I32(s.parse()?),
        "U64" => CoreValue::U64(s.parse()?),
        "I64" => CoreValue::I64(s.parse()?),
        "F16" | "F32" => CoreValue::F32(s.parse()?),
        "F64" => CoreValue::F64(s.parse()?),
        "STRING" => CoreValue::String(s.clone()),
        "BOOL" => CoreValue::Bool(s.parse()?),
        other => return Err(format!("unknown data_type {other}").into()),
    })
}

#[derive(SimpleObject, Clone)]
#[graphql(complex)]
pub struct RegisterPoint {
    pub id: ID,
    pub kind: GqlRegisterKind,
    pub address: i32,
    pub name: String,
    pub description: String,
    pub data_type: GqlDataType,
    pub encoding: GqlEncoding,
    pub byte_length: Option<i32>,
    pub default_value: ScalarValue,
}

#[ComplexObject]
impl RegisterPoint {
    async fn word_count(&self) -> i32 {
        // recompute from data type
        let core_dt: CoreDataType = self.data_type.into();
        let is_bit = matches!(self.kind, GqlRegisterKind::Coil | GqlRegisterKind::Discrete);
        if is_bit {
            1
        } else {
            core_dt.word_count(self.byte_length.map(|v| v as usize)) as i32
        }
    }
}

fn register_to_gql(r: &CoreRegister) -> RegisterPoint {
    RegisterPoint {
        id: ID(r.id.to_string()),
        kind: r.kind.into(),
        address: r.address.into(),
        name: r.name.clone(),
        description: r.description.clone(),
        data_type: r.data_type.into(),
        encoding: r.encoding.into(),
        byte_length: r.byte_length.map(|v| v as i32),
        default_value: value_to_gql(&r.default_value),
    }
}

#[derive(SimpleObject, Clone)]
pub struct DeviceType {
    pub id: ID,
    pub name: String,
    pub description: String,
    pub registers: Vec<RegisterPoint>,
    pub behavior: Behavior,
}

fn device_type_to_gql(dt: &CoreDeviceType) -> DeviceType {
    DeviceType {
        id: ID(dt.id.to_string()),
        name: dt.name.clone(),
        description: dt.description.clone(),
        registers: dt.registers.iter().map(register_to_gql).collect(),
        behavior: dt.behavior.clone().into(),
    }
}

#[derive(SimpleObject, Clone)]
pub struct RegisterValueEntry {
    pub register_id: ID,
    pub value: ScalarValue,
}

#[derive(SimpleObject, Clone)]
pub struct RegisterActivityEntry {
    pub register_id: ID,
    pub last_read_at_ms: Option<String>,
    pub last_write_at_ms: Option<String>,
}

#[derive(SimpleObject, Clone)]
pub struct Device {
    pub id: ID,
    pub name: String,
    pub slave_id: i32,
    pub device_type_id: ID,
    pub has_behavior_overrides: bool,
    pub register_values: Vec<RegisterValueEntry>,
    pub effective_behavior: Behavior,
    /// Epoch ms of the last read handled for this device, or null.
    pub last_read_at_ms: Option<String>,
    /// Epoch ms of the last write handled for this device, or null.
    pub last_write_at_ms: Option<String>,
    /// Per-register read/write timestamps (only registers that have seen
    /// at least one access appear here).
    pub register_activity: Vec<RegisterActivityEntry>,
}

fn device_to_gql(dev: &CoreDevice, dt: &CoreDeviceType, state: &AppState) -> Device {
    let eff =
        modsim_core::behavior::effective_behavior(&dt.behavior, dev.behavior_overrides.as_ref());
    let activity = state.device_activity(dev.id);
    let register_activity = state
        .register_activity_for_device(dev.id)
        .into_iter()
        .map(|(rid, a)| RegisterActivityEntry {
            register_id: ID(rid.to_string()),
            last_read_at_ms: a.last_read_at_ms.map(|n| n.to_string()),
            last_write_at_ms: a.last_write_at_ms.map(|n| n.to_string()),
        })
        .collect();
    Device {
        id: ID(dev.id.to_string()),
        name: dev.name.clone(),
        slave_id: dev.slave_id.into(),
        device_type_id: ID(dev.device_type_id.to_string()),
        has_behavior_overrides: dev.behavior_overrides.is_some(),
        register_values: dev
            .register_values
            .iter()
            .map(|(k, v)| RegisterValueEntry {
                register_id: ID(k.to_string()),
                value: value_to_gql(v),
            })
            .collect(),
        effective_behavior: eff.into(),
        last_read_at_ms: activity.last_read_at_ms.map(|n| n.to_string()),
        last_write_at_ms: activity.last_write_at_ms.map(|n| n.to_string()),
        register_activity,
    }
}

#[derive(SimpleObject, Clone)]
pub struct TcpTransport {
    pub enabled: bool,
    pub bind: String,
    pub port: i32,
}

#[derive(SimpleObject, Clone)]
pub struct RtuTransport {
    pub enabled: bool,
    pub device: String,
    pub baud_rate: i32,
    pub parity: String,
    pub data_bits: i32,
    pub stop_bits: i32,
    pub virtual_serial_id: Option<ID>,
}

#[derive(SimpleObject, Clone)]
pub struct SimContext {
    pub id: ID,
    pub name: String,
    pub active: bool,
    pub devices: Vec<Device>,
    pub tcp: TcpTransport,
    pub rtu: RtuTransport,
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum TransportState {
    Disabled,
    Running,
    Error,
}

#[derive(SimpleObject, Clone)]
pub struct TransportStatusView {
    pub state: TransportState,
    pub description: String,
    pub error: Option<String>,
}

#[derive(SimpleObject, Clone)]
pub struct TransportStatus {
    pub tcp: TransportStatusView,
    pub rtu: TransportStatusView,
}

fn state_to_view(s: &crate::supervisor::TransportState) -> TransportStatusView {
    match s {
        crate::supervisor::TransportState::Disabled => TransportStatusView {
            state: TransportState::Disabled,
            description: String::new(),
            error: None,
        },
        crate::supervisor::TransportState::Running { description } => TransportStatusView {
            state: TransportState::Running,
            description: description.clone(),
            error: None,
        },
        crate::supervisor::TransportState::Error {
            description,
            message,
        } => TransportStatusView {
            state: TransportState::Error,
            description: description.clone(),
            error: Some(message.clone()),
        },
    }
}

#[derive(SimpleObject, Clone)]
pub struct VirtualSerial {
    pub id: ID,
    pub slave_path: String,
    pub symlink_path: Option<String>,
    pub in_use: bool,
}

// ---- input objects ---------------------------------------------------------

#[derive(InputObject)]
pub struct ValueInput {
    pub data_type: String,
    pub value: String,
}

#[derive(InputObject)]
pub struct BehaviorInput {
    pub disabled_function_codes: Vec<i32>,
    pub max_registers_per_request: Option<i32>,
    pub missing_full_block: GqlMissingBehavior,
    pub missing_partial_block: GqlMissingBehavior,
    pub response_delay_ms: i32,
}

impl From<BehaviorInput> for CoreBehavior {
    fn from(b: BehaviorInput) -> Self {
        Self {
            disabled_function_codes: b
                .disabled_function_codes
                .into_iter()
                .map(|v| v as u8)
                .collect(),
            max_registers_per_request: b.max_registers_per_request.map(|v| v as u16),
            missing_full_block: b.missing_full_block.into(),
            missing_partial_block: b.missing_partial_block.into(),
            response_delay_ms: b.response_delay_ms.max(0) as u32,
        }
    }
}

#[derive(InputObject)]
pub struct RegisterInput {
    pub id: Option<ID>,
    pub kind: GqlRegisterKind,
    pub address: i32,
    pub name: String,
    pub description: Option<String>,
    pub data_type: GqlDataType,
    pub encoding: GqlEncoding,
    pub byte_length: Option<i32>,
    pub default_value: ValueInput,
}

#[derive(InputObject)]
pub struct CreateDeviceTypeInput {
    pub name: String,
    pub description: Option<String>,
}

#[derive(InputObject)]
pub struct CreateDeviceInput {
    pub name: String,
    pub slave_id: i32,
    pub device_type_id: ID,
}

#[derive(InputObject)]
pub struct BehaviorOverridesInput {
    pub response_delay_ms: Option<i32>,
    pub max_registers_per_request: Option<i32>,
    pub clear_max_registers: Option<bool>,
    pub disabled_function_codes: Option<Vec<i32>>,
    pub missing_full_block: Option<GqlMissingBehavior>,
    pub missing_partial_block: Option<GqlMissingBehavior>,
}

impl From<BehaviorOverridesInput> for CoreOverrides {
    fn from(b: BehaviorOverridesInput) -> Self {
        let max = if b.clear_max_registers.unwrap_or(false) {
            Some(None)
        } else {
            b.max_registers_per_request.map(|v| Some(v as u16))
        };
        Self {
            disabled_function_codes: b
                .disabled_function_codes
                .map(|v| v.into_iter().map(|n| n as u8).collect()),
            max_registers_per_request: max,
            missing_full_block: b.missing_full_block.map(Into::into),
            missing_partial_block: b.missing_partial_block.map(Into::into),
            response_delay_ms: b.response_delay_ms.map(|v| v.max(0) as u32),
        }
    }
}

// ---- Query -----------------------------------------------------------------

pub struct Query;

#[Object]
impl Query {
    async fn device_types(&self, ctx: &Context<'_>) -> Vec<DeviceType> {
        let state = ctx.data_unchecked::<Arc<AppState>>();
        state
            .world
            .read()
            .device_types
            .iter()
            .map(device_type_to_gql)
            .collect()
    }

    async fn device_type(&self, ctx: &Context<'_>, id: ID) -> Result<Option<DeviceType>> {
        let state = ctx.data_unchecked::<Arc<AppState>>();
        let tid = DeviceTypeId::from_str(&id.0)?;
        Ok(state.world.read().device_type(tid).map(device_type_to_gql))
    }

    async fn devices(&self, ctx: &Context<'_>) -> Vec<Device> {
        let state = ctx.data_unchecked::<Arc<AppState>>();
        let w = state.world.read();
        let Some(c) = w.active_context() else {
            return Vec::new();
        };
        c.devices
            .iter()
            .filter_map(|d| {
                w.device_type(d.device_type_id)
                    .map(|dt| device_to_gql(d, dt, state))
            })
            .collect()
    }

    async fn contexts(&self, ctx: &Context<'_>) -> Vec<SimContext> {
        let state = ctx.data_unchecked::<Arc<AppState>>();
        let w = state.world.read();
        let active = w.active_context_id;
        w.contexts
            .iter()
            .map(|c| context_to_gql(c, active == Some(c.id), &w.device_types, state))
            .collect()
    }

    async fn active_context(&self, ctx: &Context<'_>) -> Option<SimContext> {
        let state = ctx.data_unchecked::<Arc<AppState>>();
        let w = state.world.read();
        let c = w.active_context()?;
        Some(context_to_gql(c, true, &w.device_types, state))
    }

    async fn transport_status(&self, ctx: &Context<'_>) -> TransportStatus {
        let state = ctx.data_unchecked::<Arc<AppState>>();
        let snap = state.supervisor.snapshot();
        TransportStatus {
            tcp: state_to_view(&snap.tcp),
            rtu: state_to_view(&snap.rtu),
        }
    }

    async fn virtual_serials(&self, ctx: &Context<'_>) -> Vec<VirtualSerial> {
        let state = ctx.data_unchecked::<Arc<AppState>>();
        state
            .ptys
            .list()
            .into_iter()
            .map(|v| VirtualSerial {
                id: ID(v.id),
                slave_path: v.slave_path.display().to_string(),
                symlink_path: v.symlink_path.map(|p| p.display().to_string()),
                in_use: v.in_use,
            })
            .collect()
    }
}

fn context_to_gql(
    c: &CoreContext,
    active: bool,
    types: &[CoreDeviceType],
    state: &AppState,
) -> SimContext {
    let devices = c
        .devices
        .iter()
        .filter_map(|d| {
            types
                .iter()
                .find(|t| t.id == d.device_type_id)
                .map(|dt| device_to_gql(d, dt, state))
        })
        .collect();
    SimContext {
        id: ID(c.id.to_string()),
        name: c.name.clone(),
        active,
        devices,
        tcp: TcpTransport {
            enabled: c.transport.tcp.enabled,
            bind: c.transport.tcp.bind.clone(),
            port: c.transport.tcp.port.into(),
        },
        rtu: RtuTransport {
            enabled: c.transport.rtu.enabled,
            device: c.transport.rtu.device.clone(),
            baud_rate: c.transport.rtu.baud_rate as i32,
            parity: c.transport.rtu.parity.clone(),
            data_bits: c.transport.rtu.data_bits.into(),
            stop_bits: c.transport.rtu.stop_bits.into(),
            virtual_serial_id: c.transport.rtu.virtual_serial_id.clone().map(ID),
        },
    }
}

// ---- Mutation --------------------------------------------------------------

pub struct Mutation;

#[Object]
impl Mutation {
    // Device type CRUD
    async fn create_device_type(
        &self,
        ctx: &Context<'_>,
        input: CreateDeviceTypeInput,
    ) -> Result<DeviceType> {
        let state = ctx.data_unchecked::<Arc<AppState>>();
        let id = DeviceTypeId::new();
        let dt = CoreDeviceType {
            id,
            name: input.name,
            description: input.description.unwrap_or_default(),
            registers: Vec::new(),
            behavior: CoreBehavior::default(),
        };
        state.world.write().device_types.push(dt.clone());
        state.save_device_type(id)?;
        state.notify(crate::state::WorldEvent::WorldChanged);
        Ok(device_type_to_gql(&dt))
    }

    async fn export_device_type(&self, ctx: &Context<'_>, id: ID) -> Result<String> {
        let state = ctx.data_unchecked::<Arc<AppState>>();
        let tid = DeviceTypeId::from_str(&id.0)?;
        let w = state.world.read();
        let dt = w.device_type(tid).ok_or("device type not found")?;
        Ok(serde_json::to_string_pretty(dt)?)
    }

    async fn import_device_type(&self, ctx: &Context<'_>, data: String) -> Result<DeviceType> {
        let state = ctx.data_unchecked::<Arc<AppState>>();
        let mut dt: CoreDeviceType = serde_json::from_str(&data)
            .map_err(|e| async_graphql::Error::new(format!("invalid device type JSON: {e}")))?;
        // Fresh ids so a re-import onto the same machine doesn't collide
        // with the source type (or with previous imports).
        dt.id = DeviceTypeId::new();
        for r in &mut dt.registers {
            r.id = RegisterId::new();
        }
        let new_id = dt.id;
        state.world.write().device_types.push(dt.clone());
        state.save_device_type(new_id)?;
        state.notify(crate::state::WorldEvent::WorldChanged);
        Ok(device_type_to_gql(&dt))
    }

    async fn delete_device_type(&self, ctx: &Context<'_>, id: ID) -> Result<bool> {
        let state = ctx.data_unchecked::<Arc<AppState>>();
        let tid = DeviceTypeId::from_str(&id.0)?;
        // reject if any device references it
        {
            let w = state.world.read();
            let in_use = w
                .contexts
                .iter()
                .any(|c| c.devices.iter().any(|d| d.device_type_id == tid));
            if in_use {
                return Err("device type in use by device instances".into());
            }
        }
        let removed = {
            let mut w = state.world.write();
            let before = w.device_types.len();
            w.device_types.retain(|t| t.id != tid);
            w.device_types.len() != before
        };
        if removed {
            state.store.delete_device_type(tid)?;
            state.notify(crate::state::WorldEvent::WorldChanged);
        }
        Ok(removed)
    }

    async fn rename_device_type(
        &self,
        ctx: &Context<'_>,
        id: ID,
        name: String,
        description: Option<String>,
    ) -> Result<DeviceType> {
        let state = ctx.data_unchecked::<Arc<AppState>>();
        let tid = DeviceTypeId::from_str(&id.0)?;
        let updated = {
            let mut w = state.world.write();
            let dt = w
                .device_types
                .iter_mut()
                .find(|t| t.id == tid)
                .ok_or("not found")?;
            dt.name = name;
            if let Some(d) = description {
                dt.description = d;
            }
            dt.clone()
        };
        state.save_device_type(tid)?;
        state.notify(crate::state::WorldEvent::WorldChanged);
        Ok(device_type_to_gql(&updated))
    }

    async fn update_behavior(
        &self,
        ctx: &Context<'_>,
        device_type_id: ID,
        input: BehaviorInput,
    ) -> Result<Behavior> {
        let state = ctx.data_unchecked::<Arc<AppState>>();
        let tid = DeviceTypeId::from_str(&device_type_id.0)?;
        let new: CoreBehavior = input.into();
        {
            let mut w = state.world.write();
            let dt = w
                .device_types
                .iter_mut()
                .find(|t| t.id == tid)
                .ok_or("not found")?;
            dt.behavior = new.clone();
        }
        state.save_device_type(tid)?;
        state.notify(crate::state::WorldEvent::WorldChanged);
        Ok(new.into())
    }

    async fn upsert_register(
        &self,
        ctx: &Context<'_>,
        device_type_id: ID,
        input: RegisterInput,
    ) -> Result<RegisterPoint> {
        let state = ctx.data_unchecked::<Arc<AppState>>();
        let tid = DeviceTypeId::from_str(&device_type_id.0)?;
        let id = if let Some(rid) = &input.id {
            RegisterId::from_str(&rid.0)?
        } else {
            RegisterId::new()
        };
        let reg = CoreRegister {
            id,
            kind: input.kind.into(),
            address: input.address as u16,
            name: input.name,
            description: input.description.unwrap_or_default(),
            data_type: input.data_type.into(),
            encoding: input.encoding.into(),
            byte_length: input.byte_length.map(|v| v as usize),
            default_value: parse_scalar(&input.default_value)?,
        };
        {
            let mut w = state.world.write();
            let dt = w
                .device_types
                .iter_mut()
                .find(|t| t.id == tid)
                .ok_or("not found")?;
            if let Some(existing) = dt.registers.iter_mut().find(|r| r.id == id) {
                *existing = reg.clone();
            } else {
                dt.registers.push(reg.clone());
            }
        }
        state.save_device_type(tid)?;
        state.notify(crate::state::WorldEvent::WorldChanged);
        Ok(register_to_gql(&reg))
    }

    async fn delete_register(&self, ctx: &Context<'_>, id: ID) -> Result<bool> {
        let state = ctx.data_unchecked::<Arc<AppState>>();
        let rid = RegisterId::from_str(&id.0)?;
        let mut changed_type: Option<DeviceTypeId> = None;
        {
            let mut w = state.world.write();
            for dt in &mut w.device_types {
                let before = dt.registers.len();
                dt.registers.retain(|r| r.id != rid);
                if dt.registers.len() != before {
                    changed_type = Some(dt.id);
                    break;
                }
            }
        }
        if let Some(tid) = changed_type {
            state.save_device_type(tid)?;
            state.notify(crate::state::WorldEvent::WorldChanged);
            return Ok(true);
        }
        Ok(false)
    }

    // Device instance CRUD
    async fn create_device(&self, ctx: &Context<'_>, input: CreateDeviceInput) -> Result<Device> {
        let state = ctx.data_unchecked::<Arc<AppState>>();
        let tid = DeviceTypeId::from_str(&input.device_type_id.0)?;
        let ctx_id = {
            let w = state.world.read();
            w.device_type(tid).ok_or("unknown device type")?;
            w.active_context_id.ok_or("no active context")?
        };
        let dev = CoreDevice {
            id: DeviceId::new(),
            name: input.name,
            slave_id: input.slave_id as u8,
            device_type_id: tid,
            behavior_overrides: None,
            register_values: Default::default(),
        };
        let dt = state
            .world
            .read()
            .device_type(tid)
            .cloned()
            .ok_or("unknown device type")?;
        {
            let mut w = state.world.write();
            if let Some(c) = w.active_context_mut() {
                c.devices.push(dev.clone());
            }
        }
        state.save_context(ctx_id)?;
        state.notify(crate::state::WorldEvent::WorldChanged);
        Ok(device_to_gql(&dev, &dt, state))
    }

    async fn delete_device(&self, ctx: &Context<'_>, id: ID) -> Result<bool> {
        let state = ctx.data_unchecked::<Arc<AppState>>();
        let did = DeviceId::from_str(&id.0)?;
        let ctx_id;
        let removed;
        {
            let mut w = state.world.write();
            ctx_id = w.active_context_id;
            removed = if let Some(c) = w.active_context_mut() {
                let before = c.devices.len();
                c.devices.retain(|d| d.id != did);
                c.devices.len() != before
            } else {
                false
            };
        }
        if removed {
            if let Some(cid) = ctx_id {
                state.save_context(cid)?;
            }
            state.notify(crate::state::WorldEvent::WorldChanged);
        }
        Ok(removed)
    }

    async fn set_register_value(
        &self,
        ctx: &Context<'_>,
        device_id: ID,
        register_id: ID,
        value: ValueInput,
    ) -> Result<RegisterValueEntry> {
        let state = ctx.data_unchecked::<Arc<AppState>>();
        let did = DeviceId::from_str(&device_id.0)?;
        let rid = RegisterId::from_str(&register_id.0)?;
        let parsed = parse_scalar(&value)?;
        let ctx_id;
        {
            let mut w = state.world.write();
            ctx_id = w.active_context_id;
            let Some(c) = w.active_context_mut() else {
                return Err("no active context".into());
            };
            let d = c
                .devices
                .iter_mut()
                .find(|d| d.id == did)
                .ok_or("device not found")?;
            d.register_values.insert(rid, parsed.clone());
        }
        if let Some(cid) = ctx_id {
            state.save_context(cid)?;
        }
        state.notify(crate::state::WorldEvent::WorldChanged);
        Ok(RegisterValueEntry {
            register_id,
            value: value_to_gql(&parsed),
        })
    }

    async fn set_behavior_overrides(
        &self,
        ctx: &Context<'_>,
        device_id: ID,
        input: Option<BehaviorOverridesInput>,
    ) -> Result<bool> {
        let state = ctx.data_unchecked::<Arc<AppState>>();
        let did = DeviceId::from_str(&device_id.0)?;
        let ov = input.map(Into::into);
        let ctx_id;
        {
            let mut w = state.world.write();
            ctx_id = w.active_context_id;
            let Some(c) = w.active_context_mut() else {
                return Err("no active context".into());
            };
            let d = c
                .devices
                .iter_mut()
                .find(|d| d.id == did)
                .ok_or("device not found")?;
            d.behavior_overrides = ov;
        }
        if let Some(cid) = ctx_id {
            state.save_context(cid)?;
        }
        state.notify(crate::state::WorldEvent::WorldChanged);
        Ok(true)
    }

    // Contexts
    async fn create_context(&self, ctx: &Context<'_>, name: String) -> Result<SimContext> {
        let state = ctx.data_unchecked::<Arc<AppState>>();
        let c = CoreContext {
            id: ContextId::new(),
            name,
            devices: Vec::new(),
            transport: Default::default(),
        };
        {
            let mut w = state.world.write();
            w.contexts.push(c.clone());
            if w.active_context_id.is_none() {
                w.active_context_id = Some(c.id);
            }
        }
        state.save_context(c.id)?;
        state.save_active()?;
        state.notify(crate::state::WorldEvent::WorldChanged);
        let active = state.world.read().active_context_id == Some(c.id);
        let types = state.world.read().device_types.clone();
        Ok(context_to_gql(&c, active, &types, state))
    }

    async fn switch_context(&self, ctx: &Context<'_>, id: ID) -> Result<bool> {
        let state = ctx.data_unchecked::<Arc<AppState>>();
        let cid = ContextId::from_str(&id.0)?;
        {
            let mut w = state.world.write();
            if !w.contexts.iter().any(|c| c.id == cid) {
                return Err("context not found".into());
            }
            w.active_context_id = Some(cid);
        }
        state.save_active()?;
        state.reconfigure_transports().await;
        state.notify(crate::state::WorldEvent::WorldChanged);
        Ok(true)
    }

    async fn delete_context(&self, ctx: &Context<'_>, id: ID) -> Result<bool> {
        let state = ctx.data_unchecked::<Arc<AppState>>();
        let cid = ContextId::from_str(&id.0)?;
        let removed = {
            let mut w = state.world.write();
            let before = w.contexts.len();
            w.contexts.retain(|c| c.id != cid);
            if w.active_context_id == Some(cid) {
                w.active_context_id = w.contexts.first().map(|c| c.id);
            }
            w.contexts.len() != before
        };
        if removed {
            state.store.delete_context(cid)?;
            state.save_active()?;
            state.reconfigure_transports().await;
            state.notify(crate::state::WorldEvent::WorldChanged);
        }
        Ok(removed)
    }

    async fn configure_tcp(
        &self,
        ctx: &Context<'_>,
        enabled: bool,
        bind: String,
        port: i32,
    ) -> Result<bool> {
        let state = ctx.data_unchecked::<Arc<AppState>>();
        let ctx_id;
        {
            let mut w = state.world.write();
            ctx_id = w.active_context_id;
            let Some(c) = w.active_context_mut() else {
                return Err("no active context".into());
            };
            c.transport.tcp = CoreTcp {
                enabled,
                bind,
                port: port as u16,
            };
        }
        if let Some(cid) = ctx_id {
            state.save_context(cid)?;
        }
        state.reconfigure_transports().await;
        state.notify(crate::state::WorldEvent::WorldChanged);
        Ok(true)
    }

    async fn configure_rtu(
        &self,
        ctx: &Context<'_>,
        enabled: bool,
        device: String,
        baud_rate: i32,
        parity: String,
        data_bits: i32,
        stop_bits: i32,
        virtual_serial_id: Option<ID>,
    ) -> Result<bool> {
        let state = ctx.data_unchecked::<Arc<AppState>>();
        let ctx_id;
        {
            let mut w = state.world.write();
            ctx_id = w.active_context_id;
            let Some(c) = w.active_context_mut() else {
                return Err("no active context".into());
            };
            c.transport.rtu = CoreRtu {
                enabled,
                device,
                baud_rate: baud_rate.max(0) as u32,
                parity,
                data_bits: data_bits.clamp(5, 8) as u8,
                stop_bits: stop_bits.clamp(1, 2) as u8,
                virtual_serial_id: virtual_serial_id.map(|id| id.0),
            };
        }
        if let Some(cid) = ctx_id {
            state.save_context(cid)?;
        }
        state.reconfigure_transports().await;
        state.notify(crate::state::WorldEvent::WorldChanged);
        Ok(true)
    }

    async fn create_virtual_serial(
        &self,
        ctx: &Context<'_>,
        symlink_path: Option<String>,
    ) -> Result<VirtualSerial> {
        let state = ctx.data_unchecked::<Arc<AppState>>();
        let info = state
            .ptys
            .create(symlink_path.map(std::path::PathBuf::from))
            .map_err(|e| async_graphql::Error::new(e.to_string()))?;
        state.save_virtual_serials()?;
        Ok(VirtualSerial {
            id: ID(info.id),
            slave_path: info.slave_path.display().to_string(),
            symlink_path: info.symlink_path.map(|p| p.display().to_string()),
            in_use: info.in_use,
        })
    }

    async fn remove_virtual_serial(&self, ctx: &Context<'_>, id: ID) -> Result<bool> {
        let state = ctx.data_unchecked::<Arc<AppState>>();
        let removed = state.ptys.remove(&id.0);
        if removed {
            state.save_virtual_serials()?;
        }
        Ok(removed)
    }

    async fn export_context(&self, ctx: &Context<'_>, id: ID) -> Result<String> {
        let state = ctx.data_unchecked::<Arc<AppState>>();
        let cid = ContextId::from_str(&id.0)?;
        let w = state.world.read();
        let c = w.contexts.iter().find(|c| c.id == cid).ok_or("not found")?;
        // embed referenced device types
        let mut types = Vec::new();
        for d in &c.devices {
            if let Some(t) = w.device_type(d.device_type_id) {
                if !types.iter().any(|x: &CoreDeviceType| x.id == t.id) {
                    types.push(t.clone());
                }
            }
        }
        let bundle = serde_json::json!({ "context": c, "device_types": types });
        Ok(serde_json::to_string_pretty(&bundle)?)
    }

    async fn import_context(&self, ctx: &Context<'_>, data: String) -> Result<SimContext> {
        let state = ctx.data_unchecked::<Arc<AppState>>();
        let v: serde_json::Value = serde_json::from_str(&data)?;
        let types: Vec<CoreDeviceType> =
            serde_json::from_value(v.get("device_types").cloned().unwrap_or_default())?;
        let mut c: CoreContext =
            serde_json::from_value(v.get("context").cloned().unwrap_or_default())?;
        c.id = ContextId::new();
        {
            let mut w = state.world.write();
            for t in types {
                if !w.device_types.iter().any(|x| x.id == t.id) {
                    w.device_types.push(t);
                }
            }
            w.contexts.push(c.clone());
        }
        state.save_world_full()?;
        state.notify(crate::state::WorldEvent::WorldChanged);
        let types_snapshot = state.world.read().device_types.clone();
        Ok(context_to_gql(&c, false, &types_snapshot, state))
    }
}
