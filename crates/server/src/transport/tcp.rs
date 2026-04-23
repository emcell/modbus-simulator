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

use crate::state::{AppState, TrafficFrame, WorldEvent};
use crate::transport::pdu::{build_exception, build_response, parse_request};

pub async fn run(state: Arc<AppState>, bind: String, port: u16) -> Result<()> {
    let addr = format!("{bind}:{port}");
    let listener = TcpListener::bind(&addr).await?;
    info!("modbus TCP listening on {addr}");
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

        emit_traffic(
            &state,
            "in",
            unit_id,
            pdu.first().copied().unwrap_or(0),
            &pdu,
        );

        let reply = match dispatch(&state, unit_id, &pdu).await {
            Dispatched::Silence => continue,
            Dispatched::Reply(bytes) => bytes,
        };

        emit_traffic(
            &state,
            "out",
            unit_id,
            reply.first().copied().unwrap_or(0),
            &reply,
        );

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

enum Dispatched {
    Silence,
    Reply(Vec<u8>),
}

async fn dispatch(state: &Arc<AppState>, slave_id: u8, pdu: &[u8]) -> Dispatched {
    let fc = pdu.first().copied().unwrap_or(0);
    let Some((device, device_type)) = state.resolve_slave(slave_id) else {
        // Unknown slave id: silent (matches common RTU-bus-like behavior).
        return Dispatched::Silence;
    };
    let req = match parse_request(pdu) {
        Ok(r) => r,
        Err(_) => {
            return Dispatched::Reply(build_exception(
                fc,
                modsim_core::ModbusException::IllegalFunction,
            ));
        }
    };
    let behavior = effective_behavior(&device_type.behavior, device.behavior_overrides.as_ref());
    let out = process_request(&req, &device, &device_type, &behavior);

    if behavior.response_delay_ms > 0 {
        sleep(Duration::from_millis(u64::from(behavior.response_delay_ms))).await;
    }

    // Apply any writes back to device state.
    if !out.state_update.writes.is_empty() {
        apply_writes(state, &device, &device_type, &out.state_update);
    }

    match out.outcome {
        Outcome::Response(r) => Dispatched::Reply(build_response(&r)),
        Outcome::Exception(ex) => Dispatched::Reply(build_exception(fc, ex)),
        Outcome::Silence => Dispatched::Silence,
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

fn emit_traffic(
    state: &Arc<AppState>,
    direction: &'static str,
    slave_id: u8,
    fc: u8,
    bytes: &[u8],
) {
    let hex = bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ");
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    state.notify(WorldEvent::TrafficFrame(TrafficFrame {
        direction,
        transport: "tcp",
        slave_id,
        function_code: fc,
        bytes_hex: hex,
        timestamp_ms: ts,
    }));
}
