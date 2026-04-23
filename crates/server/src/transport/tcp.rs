//! Modbus TCP server.
//!
//! Implements the MBAP header manually so the engine retains full control
//! over "faulty" behavior (timeouts, exceptions, etc).

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use modsim_core::engine::{apply_state_update, process_request, Outcome};
use modsim_core::{effective_behavior, Device, DeviceType};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::sleep;
use tracing::{debug, info, warn};

use crate::state::{AppState, DecodedValue, TrafficFrame, WorldEvent};
use crate::transport::pdu::{build_exception, build_response, parse_request};
use crate::transport::semantics;
use modsim_core::engine::{ModbusRequest, ModbusResponse};

pub async fn run(state: Arc<AppState>, bind: String, port: u16) -> Result<()> {
    let listener = bind_listener(&bind, port).await?;
    serve_listener(state, listener).await
}

/// Bind a TCP listener without starting the accept loop. Used by the
/// transport supervisor so synchronous bind errors (port in use, etc.)
/// can be surfaced to the UI before we commit to spawning the serve
/// task.
pub async fn bind_listener(bind: &str, port: u16) -> Result<TcpListener> {
    let addr = format!("{bind}:{port}");
    Ok(TcpListener::bind(&addr).await?)
}

pub async fn serve_listener(state: Arc<AppState>, listener: TcpListener) -> Result<()> {
    info!("modbus TCP listening on {}", listener.local_addr()?);
    loop {
        let (sock, peer) = listener.accept().await?;
        let st = state.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_client(st, sock).await {
                debug!("modbus TCP client {peer} disconnected: {e}");
            }
        });
    }
}

async fn handle_client(state: Arc<AppState>, mut sock: TcpStream) -> Result<()> {
    loop {
        let mut header = [0u8; 7];
        sock.read_exact(&mut header).await?;
        let transaction_id = u16::from_be_bytes([header[0], header[1]]);
        let protocol_id = u16::from_be_bytes([header[2], header[3]]);
        let length = u16::from_be_bytes([header[4], header[5]]);
        let unit_id = header[6];
        if protocol_id != 0 || !(2..=260).contains(&length) {
            warn!("invalid MBAP header");
            return Ok(());
        }
        let pdu_len = (length - 1) as usize;
        let mut pdu = vec![0u8; pdu_len];
        sock.read_exact(&mut pdu).await?;

        let out = dispatch_full(&state, "tcp", unit_id, &pdu).await;
        let reply = match out {
            DispatchOut::Silence => continue,
            DispatchOut::Reply(bytes) => bytes,
        };

        let len = (reply.len() + 1) as u16;
        let mut frame = Vec::with_capacity(7 + reply.len());
        frame.extend_from_slice(&transaction_id.to_be_bytes());
        frame.extend_from_slice(&0u16.to_be_bytes());
        frame.extend_from_slice(&len.to_be_bytes());
        frame.push(unit_id);
        frame.extend_from_slice(&reply);
        sock.write_all(&frame).await?;
    }
}

pub(crate) enum DispatchOut {
    Silence,
    Reply(Vec<u8>),
}

/// Parse the PDU, drive the engine, emit in+out traffic events with
/// semantic annotations, track per-device activity. Shared between TCP and
/// RTU so both transports produce identical traffic metadata.
pub(crate) async fn dispatch_full(
    state: &Arc<AppState>,
    transport: &'static str,
    slave_id: u8,
    pdu: &[u8],
) -> DispatchOut {
    let fc = pdu.first().copied().unwrap_or(0);
    let Some((device, device_type)) = state.resolve_slave(slave_id) else {
        return DispatchOut::Silence;
    };
    let req = match parse_request(pdu) {
        Ok(r) => r,
        Err(_) => {
            let reply = build_exception(fc, modsim_core::ModbusException::IllegalFunction);
            emit_in(state, transport, slave_id, fc, pdu, None, &device_type);
            emit_out_exception(state, transport, slave_id, &reply);
            return DispatchOut::Reply(reply);
        }
    };

    // Emit the "in" event with semantic summary.
    emit_in(
        state,
        transport,
        slave_id,
        fc,
        pdu,
        Some(&req),
        &device_type,
    );

    let behavior = effective_behavior(&device_type.behavior, device.behavior_overrides.as_ref());
    let out = process_request(&req, &device, &device_type, &behavior);

    if behavior.response_delay_ms > 0 {
        sleep(Duration::from_millis(u64::from(behavior.response_delay_ms))).await;
    }

    if !out.state_update.writes.is_empty() {
        apply_writes(state, &device, &device_type, &out.state_update);
    }

    // Activity tracking: remember when the simulator last handled a read
    // or a write for this device AND each individual register in range.
    let now = now_ms();
    let touched = semantics::affected_registers(&req, &device_type);
    if is_write_request(&req) {
        state.mark_device_write(device.id, now);
        for rid in &touched {
            state.mark_register_write(device.id, *rid, now);
        }
    } else {
        state.mark_device_read(device.id, now);
        for rid in &touched {
            state.mark_register_read(device.id, *rid, now);
        }
    }

    match out.outcome {
        Outcome::Response(r) => {
            let reply = build_response(&r);
            emit_out_response(state, transport, slave_id, &reply, &req, &r, &device_type);
            DispatchOut::Reply(reply)
        }
        Outcome::Exception(ex) => {
            let reply = build_exception(fc, ex);
            emit_out_exception(state, transport, slave_id, &reply);
            DispatchOut::Reply(reply)
        }
        Outcome::Silence => DispatchOut::Silence,
    }
}

fn apply_writes(
    state: &Arc<AppState>,
    device: &Device,
    device_type: &DeviceType,
    update: &modsim_core::StateUpdate,
) {
    let mut snapshot = device.clone();
    apply_state_update(&mut snapshot, device_type, update);
    let dev_id = device.id;
    state.apply_device_update(|ctx| {
        if let Some(d) = ctx.devices.iter_mut().find(|d| d.id == dev_id) {
            d.register_values = snapshot.register_values.clone();
        }
    });
    if let Some(cid) = state.world.read().active_context_id {
        let _ = state.save_context(cid);
    }
    state.notify(WorldEvent::WorldChanged);
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default()
}

fn is_write_request(req: &ModbusRequest) -> bool {
    matches!(
        req,
        ModbusRequest::WriteSingleCoil { .. }
            | ModbusRequest::WriteSingleRegister { .. }
            | ModbusRequest::WriteMultipleCoils { .. }
            | ModbusRequest::WriteMultipleRegisters { .. }
    )
}

fn hex_of(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn decoded_to_state(ds: Vec<semantics::DecodedValue>) -> Vec<DecodedValue> {
    ds.into_iter()
        .map(|d| DecodedValue {
            register_name: d.register_name,
            address: d.address,
            data_type: d.data_type,
            value: d.value,
        })
        .collect()
}

fn emit_in(
    state: &Arc<AppState>,
    transport: &'static str,
    slave_id: u8,
    fc: u8,
    bytes: &[u8],
    req: Option<&ModbusRequest>,
    device_type: &modsim_core::model::DeviceType,
) {
    let (summary, decoded) = match req {
        Some(r) => {
            let s = semantics::summarize_request(r);
            // For writes we can decode the values being pushed.
            let d = if is_write_request(r) {
                decoded_to_state(semantics::decode_write_request(r, device_type))
            } else {
                Vec::new()
            };
            (s, d)
        }
        None => ("(unparseable request)".into(), Vec::new()),
    };
    state.notify(WorldEvent::TrafficFrame(TrafficFrame {
        direction: "in",
        transport,
        slave_id,
        function_code: fc,
        bytes_hex: hex_of(bytes),
        timestamp_ms: now_ms(),
        summary,
        decoded,
    }));
}

fn emit_out_response(
    state: &Arc<AppState>,
    transport: &'static str,
    slave_id: u8,
    reply_bytes: &[u8],
    req: &ModbusRequest,
    resp: &ModbusResponse,
    device_type: &modsim_core::model::DeviceType,
) {
    let summary = semantics::summarize_response(req, resp);
    let decoded = decoded_to_state(semantics::decode_read(req, resp, device_type));
    let fc = reply_bytes.first().copied().unwrap_or(0);
    state.notify(WorldEvent::TrafficFrame(TrafficFrame {
        direction: "out",
        transport,
        slave_id,
        function_code: fc,
        bytes_hex: hex_of(reply_bytes),
        timestamp_ms: now_ms(),
        summary,
        decoded,
    }));
}

fn emit_out_exception(
    state: &Arc<AppState>,
    transport: &'static str,
    slave_id: u8,
    reply_bytes: &[u8],
) {
    let fc = reply_bytes.first().copied().unwrap_or(0);
    let ex_code = reply_bytes.get(1).copied().unwrap_or(0);
    state.notify(WorldEvent::TrafficFrame(TrafficFrame {
        direction: "out",
        transport,
        slave_id,
        function_code: fc,
        bytes_hex: hex_of(reply_bytes),
        timestamp_ms: now_ms(),
        summary: format!("Exception 0x{ex_code:02X}"),
        decoded: Vec::new(),
    }));
}
