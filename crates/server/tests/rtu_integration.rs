//! RTU frame-level integration test (no actual serial device).
//!
//! Exercises the dispatch path, CRC validation, and silence-on-unknown-slave
//! behavior without needing a PTY or physical port.

use std::collections::BTreeMap;

use modsim_core::behavior::DeviceBehavior;
use modsim_core::encoding::{DataType, Encoding, Value};
use modsim_core::model::{
    Context, Device, DeviceId, DeviceType, DeviceTypeId, RegisterId, RegisterKind, RegisterPoint,
    World,
};
use modsim_server::persistence::{AppSettings, Store};
use modsim_server::state::AppState;
use modsim_server::transport::rtu::{build_frame, crc16, process_frame};

fn seed_world() -> World {
    let r1 = RegisterId(uuid::Uuid::new_v4());
    let dt_id = DeviceTypeId(uuid::Uuid::new_v4());
    let dt = DeviceType {
        id: dt_id,
        name: "T".into(),
        description: String::new(),
        registers: vec![RegisterPoint {
            id: r1,
            kind: RegisterKind::Holding,
            address: 10,
            name: "v".into(),
            description: String::new(),
            data_type: DataType::U16,
            encoding: Encoding::BigEndian,
            byte_length: None,
            default_value: Value::U16(0xBEEF),
        }],
        behavior: DeviceBehavior::default(),
    };
    let device = Device {
        id: DeviceId(uuid::Uuid::new_v4()),
        name: "d".into(),
        slave_id: 7,
        device_type_id: dt_id,
        behavior_overrides: None,
        register_values: BTreeMap::new(),
    };
    let ctx = Context {
        id: modsim_core::model::ContextId(uuid::Uuid::new_v4()),
        name: "c".into(),
        devices: vec![device],
        transport: Default::default(),
    };
    let ctx_id = ctx.id;
    World {
        device_types: vec![dt],
        contexts: vec![ctx],
        active_context_id: Some(ctx_id),
    }
}

fn tmp_root() -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!(
        "modsim-rtu-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}

#[tokio::test]
async fn rtu_read_holding_matches_slave_id() {
    let store = Store::with_root(tmp_root()).unwrap();
    let state = AppState::new(seed_world(), AppSettings::default(), store);
    let pdu = [0x03, 0x00, 0x0A, 0x00, 0x01];
    let frame = build_frame(7, &pdu);
    let resp = process_frame(&state, &frame).await.expect("reply");
    // expected PDU: [03, 02, BE, EF]; full frame with slave id + crc
    let expected_pdu = [0x03u8, 0x02, 0xBE, 0xEF];
    let mut expected = vec![7];
    expected.extend_from_slice(&expected_pdu);
    let crc = crc16(&expected);
    expected.extend_from_slice(&crc.to_le_bytes());
    assert_eq!(resp, expected);
}

#[tokio::test]
async fn rtu_unknown_slave_is_silent() {
    let store = Store::with_root(tmp_root()).unwrap();
    let state = AppState::new(seed_world(), AppSettings::default(), store);
    let pdu = [0x03, 0x00, 0x0A, 0x00, 0x01];
    let frame = build_frame(99, &pdu);
    assert!(process_frame(&state, &frame).await.is_none());
}

#[tokio::test]
async fn rtu_bad_crc_is_silent() {
    let store = Store::with_root(tmp_root()).unwrap();
    let state = AppState::new(seed_world(), AppSettings::default(), store);
    let pdu = [0x03, 0x00, 0x0A, 0x00, 0x01];
    let mut frame = build_frame(7, &pdu);
    let last = frame.len() - 1;
    frame[last] ^= 0xFF;
    assert!(process_frame(&state, &frame).await.is_none());
}

#[tokio::test]
async fn rtu_broadcast_does_not_reply() {
    let store = Store::with_root(tmp_root()).unwrap();
    let state = AppState::new(seed_world(), AppSettings::default(), store);
    // slave_id 0 = broadcast. Even if we somehow resolved it, RTU spec
    // forbids replies. Our resolve_slave won't find it so this is silent
    // anyway, which is also correct.
    let pdu = [0x06, 0x00, 0x0A, 0x12, 0x34];
    let frame = build_frame(0, &pdu);
    assert!(process_frame(&state, &frame).await.is_none());
}
