//! Modbus RTU transport.
//!
//! Frame: [slave_id][pdu...][crc_lo][crc_hi]
//!
//! Reuses the pure engine — one serial port can host multiple simulated
//! slaves differentiated by slave_id.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use modsim_core::engine::{apply_state_update, process_request, Outcome};
use modsim_core::{effective_behavior, Device, DeviceType};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::time::{sleep, timeout};
use tokio_serial::{DataBits, Parity, SerialPortBuilderExt, SerialStream, StopBits};
use tracing::{debug, info, warn};

use crate::state::{AppState, TrafficFrame, WorldEvent};
use crate::transport::pdu::{build_exception, build_response, parse_request};

/// Compute Modbus CRC16 (poly 0xA001, init 0xFFFF, reflected).
#[must_use]
pub fn crc16(bytes: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for b in bytes {
        crc ^= u16::from(*b);
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xA001;
            } else {
                crc >>= 1;
            }
        }
    }
    crc
}

#[derive(Debug, Clone, Copy)]
pub struct SerialConfig<'a> {
    pub device: &'a str,
    pub baud_rate: u32,
    pub data_bits: u8,
    pub stop_bits: u8,
    /// Case-insensitive: N(one) / E(ven) / O(dd).
    pub parity: &'a str,
}

pub async fn run(state: Arc<AppState>, cfg: SerialConfig<'_>) -> Result<()> {
    let port = open_port(cfg)?;
    info!("modbus RTU listening on {}", cfg.device);
    serve(state, port).await
}

/// Run the RTU serve loop on an arbitrary async stream. Useful for tests
/// that wire the simulator to a PTY without going through the serial
/// termios setup (which some virtual-TTY devices don't fully support).
pub async fn run_on_stream<S>(state: Arc<AppState>, stream: S) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    serve(state, stream).await
}

fn open_port(cfg: SerialConfig<'_>) -> Result<SerialStream> {
    let data_bits = match cfg.data_bits {
        5 => DataBits::Five,
        6 => DataBits::Six,
        7 => DataBits::Seven,
        _ => DataBits::Eight,
    };
    let stop_bits = match cfg.stop_bits {
        2 => StopBits::Two,
        _ => StopBits::One,
    };
    let parity = match cfg.parity.chars().next().map(|c| c.to_ascii_uppercase()) {
        Some('E') => Parity::Even,
        Some('O') => Parity::Odd,
        _ => Parity::None,
    };
    let port = tokio_serial::new(cfg.device, cfg.baud_rate)
        .data_bits(data_bits)
        .stop_bits(stop_bits)
        .parity(parity)
        .timeout(Duration::from_millis(50))
        .open_native_async()?;
    Ok(port)
}

async fn serve<S>(state: Arc<AppState>, mut port: S) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut buf = Vec::with_capacity(512);
    let mut scratch = [0u8; 256];
    loop {
        // Inter-frame gap detection: wait for bytes, then read in bursts
        // until quiet for > ~3.5 character times. We approximate with a fixed
        // small timeout since bauds vary.
        let n = match port.read(&mut scratch).await {
            Ok(0) => {
                sleep(Duration::from_millis(10)).await;
                continue;
            }
            Ok(n) => n,
            Err(e) => {
                debug!("serial read: {e}");
                return Ok(());
            }
        };
        buf.extend_from_slice(&scratch[..n]);

        // Drain any immediately-available follow-up bytes.
        loop {
            match timeout(Duration::from_millis(5), port.read(&mut scratch)).await {
                Ok(Ok(0)) => break,
                Ok(Ok(n)) => buf.extend_from_slice(&scratch[..n]),
                _ => break,
            }
        }

        if buf.len() < 4 {
            warn!("rtu: short frame ({} bytes)", buf.len());
            buf.clear();
            continue;
        }
        let crc_received = u16::from_le_bytes([buf[buf.len() - 2], buf[buf.len() - 1]]);
        let crc_calc = crc16(&buf[..buf.len() - 2]);
        if crc_received != crc_calc {
            warn!("rtu: crc mismatch");
            buf.clear();
            continue;
        }
        let slave_id = buf[0];
        let pdu = buf[1..buf.len() - 2].to_vec();
        buf.clear();

        emit_traffic(
            &state,
            "in",
            slave_id,
            pdu.first().copied().unwrap_or(0),
            &pdu,
        );

        let Some(reply_pdu) = dispatch(&state, slave_id, &pdu).await else {
            continue; // silence
        };

        emit_traffic(
            &state,
            "out",
            slave_id,
            reply_pdu.first().copied().unwrap_or(0),
            &reply_pdu,
        );

        // Broadcast address 0: no response.
        if slave_id == 0 {
            continue;
        }
        let mut frame = Vec::with_capacity(1 + reply_pdu.len() + 2);
        frame.push(slave_id);
        frame.extend_from_slice(&reply_pdu);
        let crc = crc16(&frame);
        frame.extend_from_slice(&crc.to_le_bytes());
        if let Err(e) = port.write_all(&frame).await {
            debug!("serial write: {e}");
            return Ok(());
        }
    }
}

async fn dispatch(state: &Arc<AppState>, slave_id: u8, pdu: &[u8]) -> Option<Vec<u8>> {
    let fc = pdu.first().copied().unwrap_or(0);
    let (device, device_type) = state.resolve_slave(slave_id)?;
    let req = match parse_request(pdu) {
        Ok(r) => r,
        Err(_) => {
            return Some(build_exception(
                fc,
                modsim_core::ModbusException::IllegalFunction,
            ))
        }
    };
    let behavior = effective_behavior(&device_type.behavior, device.behavior_overrides.as_ref());
    let out = process_request(&req, &device, &device_type, &behavior);
    if behavior.response_delay_ms > 0 {
        sleep(Duration::from_millis(u64::from(behavior.response_delay_ms))).await;
    }
    if !out.state_update.writes.is_empty() {
        apply_writes(state, &device, &device_type, &out.state_update);
    }
    match out.outcome {
        Outcome::Response(r) => Some(build_response(&r)),
        Outcome::Exception(ex) => Some(build_exception(fc, ex)),
        Outcome::Silence => None,
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
        transport: "rtu",
        slave_id,
        function_code: fc,
        bytes_hex: hex,
        timestamp_ms: ts,
    }));
}

/// Build a full RTU request frame [slave_id | pdu | crc_lo crc_hi].
#[must_use]
pub fn build_frame(slave_id: u8, pdu: &[u8]) -> Vec<u8> {
    let mut f = Vec::with_capacity(1 + pdu.len() + 2);
    f.push(slave_id);
    f.extend_from_slice(pdu);
    let crc = crc16(&f);
    f.extend_from_slice(&crc.to_le_bytes());
    f
}

/// Process a single RTU frame in isolation — returns the reply frame, or
/// `None` for silence. Exposed for tests.
pub async fn process_frame(state: &Arc<AppState>, frame: &[u8]) -> Option<Vec<u8>> {
    if frame.len() < 4 {
        return None;
    }
    let crc_received = u16::from_le_bytes([frame[frame.len() - 2], frame[frame.len() - 1]]);
    let crc_calc = crc16(&frame[..frame.len() - 2]);
    if crc_received != crc_calc {
        return None;
    }
    let slave_id = frame[0];
    let pdu = &frame[1..frame.len() - 2];
    emit_traffic(
        state,
        "in",
        slave_id,
        pdu.first().copied().unwrap_or(0),
        pdu,
    );
    let reply_pdu = dispatch(state, slave_id, pdu).await?;
    emit_traffic(
        state,
        "out",
        slave_id,
        reply_pdu.first().copied().unwrap_or(0),
        &reply_pdu,
    );
    if slave_id == 0 {
        return None;
    }
    Some(build_frame(slave_id, &reply_pdu))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_known_vector() {
        // classic test: 01 03 00 00 00 0A -> C5 CD (LE)
        let bytes = [0x01, 0x03, 0x00, 0x00, 0x00, 0x0A];
        let c = crc16(&bytes);
        assert_eq!(c.to_le_bytes(), [0xC5, 0xCD]);
    }

    #[test]
    fn roundtrip_frame_wrap() {
        let pdu = [0x03, 0x00, 0x0A, 0x00, 0x01];
        let f = build_frame(7, &pdu);
        assert_eq!(f[0], 7);
        assert_eq!(&f[1..6], &pdu);
        let crc = u16::from_le_bytes([f[6], f[7]]);
        assert_eq!(crc, crc16(&f[..6]));
    }
}
