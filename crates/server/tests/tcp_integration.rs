//! End-to-end Modbus TCP integration test using a raw client against the
//! server's transport layer.

use std::collections::BTreeMap;
use std::sync::Arc;

use modsim_core::behavior::{DeviceBehavior, MissingBlockBehavior};
use modsim_core::encoding::{DataType, Encoding, Value};
use modsim_core::model::{
    Context, Device, DeviceId, DeviceType, DeviceTypeId, RegisterId, RegisterKind, RegisterPoint,
    World,
};
use modsim_server::persistence::{AppSettings, Store};
use modsim_server::state::AppState;
use modsim_server::transport::tcp;
use tempfile_dir::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

mod tempfile_dir {
    use std::path::PathBuf;
    pub struct TempDir(pub PathBuf);
    impl TempDir {
        pub fn new() -> Self {
            let p = std::env::temp_dir().join(format!("modsim-test-{}", uuid_v4_hex()));
            std::fs::create_dir_all(&p).unwrap();
            Self(p)
        }
        pub fn path(&self) -> &std::path::Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn uuid_v4_hex() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        format!(
            "{}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
            std::process::id()
        )
    }
}

fn seed_world() -> World {
    let r1 = RegisterId(uuid::Uuid::new_v4());
    let r2 = RegisterId(uuid::Uuid::new_v4());
    let dt_id = DeviceTypeId(uuid::Uuid::new_v4());
    let dt = DeviceType {
        id: dt_id,
        name: "TestDev".into(),
        description: String::new(),
        registers: vec![
            RegisterPoint {
                id: r1,
                kind: RegisterKind::Holding,
                address: 10,
                name: "a".into(),
                description: String::new(),
                data_type: DataType::U16,
                encoding: Encoding::BigEndian,
                byte_length: None,
                default_value: Value::U16(0x1234),
            },
            RegisterPoint {
                id: r2,
                kind: RegisterKind::Holding,
                address: 11,
                name: "b".into(),
                description: String::new(),
                data_type: DataType::U16,
                encoding: Encoding::BigEndian,
                byte_length: None,
                default_value: Value::U16(0x5678),
            },
        ],
        behavior: DeviceBehavior {
            missing_partial_block: MissingBlockBehavior::ZeroFill,
            ..Default::default()
        },
    };
    let device = Device {
        id: DeviceId(uuid::Uuid::new_v4()),
        name: "d1".into(),
        slave_id: 1,
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

async fn start_server() -> u16 {
    let tmp = TempDir::new();
    let store = Store::with_root(tmp.path().to_path_buf()).unwrap();
    // intentionally leak the tempdir for the lifetime of the test binary
    std::mem::forget(tmp);
    let state = AppState::new(seed_world(), AppSettings::default(), store);
    // Bind to ephemeral port
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let st = Arc::clone(&state);
    tokio::spawn(async move {
        let _ = tcp::run(st, "127.0.0.1".into(), port).await;
    });
    // give the server a moment
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    port
}

async fn send_request(port: u16, unit_id: u8, pdu: &[u8]) -> Vec<u8> {
    let mut sock = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let tid: u16 = 1;
    let len = (pdu.len() + 1) as u16;
    let mut frame = Vec::new();
    frame.extend_from_slice(&tid.to_be_bytes());
    frame.extend_from_slice(&0u16.to_be_bytes());
    frame.extend_from_slice(&len.to_be_bytes());
    frame.push(unit_id);
    frame.extend_from_slice(pdu);
    sock.write_all(&frame).await.unwrap();

    let mut header = [0u8; 7];
    sock.read_exact(&mut header).await.unwrap();
    let resp_len = u16::from_be_bytes([header[4], header[5]]) as usize - 1;
    let mut pdu_out = vec![0u8; resp_len];
    sock.read_exact(&mut pdu_out).await.unwrap();
    pdu_out
}

#[tokio::test]
async fn tcp_read_holding_registers() {
    let port = start_server().await;
    // read holding registers, address 10, quantity 2
    let pdu = vec![0x03, 0x00, 0x0A, 0x00, 0x02];
    let resp = send_request(port, 1, &pdu).await;
    assert_eq!(resp, vec![0x03, 0x04, 0x12, 0x34, 0x56, 0x78]);
}

#[tokio::test]
async fn tcp_missing_partial_block_zero_fills() {
    let port = start_server().await;
    // request overlapping range (10..14), only 10,11 exist. Behavior=ZeroFill for partial.
    let pdu = vec![0x03, 0x00, 0x0A, 0x00, 0x04];
    let resp = send_request(port, 1, &pdu).await;
    assert_eq!(
        resp,
        vec![0x03, 0x08, 0x12, 0x34, 0x56, 0x78, 0x00, 0x00, 0x00, 0x00]
    );
}

#[tokio::test]
async fn tcp_missing_full_block_returns_exception() {
    let port = start_server().await;
    // full miss at address 500
    let pdu = vec![0x03, 0x01, 0xF4, 0x00, 0x02];
    let resp = send_request(port, 1, &pdu).await;
    assert_eq!(resp, vec![0x83, 0x02]);
}

#[tokio::test]
async fn tcp_write_single_register_roundtrip() {
    let port = start_server().await;
    // write 0xCAFE to address 10
    let write_pdu = vec![0x06, 0x00, 0x0A, 0xCA, 0xFE];
    let resp = send_request(port, 1, &write_pdu).await;
    assert_eq!(resp, vec![0x06, 0x00, 0x0A, 0xCA, 0xFE]);
    // read it back
    let read_pdu = vec![0x03, 0x00, 0x0A, 0x00, 0x01];
    let resp = send_request(port, 1, &read_pdu).await;
    assert_eq!(resp, vec![0x03, 0x02, 0xCA, 0xFE]);
}
