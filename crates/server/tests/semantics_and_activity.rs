//! End-to-end check that the Traffic subscription carries semantic
//! summaries + decoded values and that per-device activity timestamps
//! get updated.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use modsim_core::behavior::DeviceBehavior;
use modsim_core::encoding::{DataType, Encoding, Value};
use modsim_core::model::{
    Context, Device, DeviceId, DeviceType, DeviceTypeId, RegisterId, RegisterKind, RegisterPoint,
    World,
};
use modsim_server::graphql::build_schema;
use modsim_server::http::router;
use modsim_server::persistence::{AppSettings, Store};
use modsim_server::state::AppState;
use modsim_server::transport::tcp as modbus_tcp;
use serde_json::{json, Value as JsonValue};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::Message;

fn tmp_root() -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!(
        "modsim-sem-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}

struct Seed {
    world: World,
    dev_id: DeviceId,
    reg_u32: RegisterId,
    reg_str: RegisterId,
}

fn seed_world() -> Seed {
    let reg_u32 = RegisterId(uuid::Uuid::new_v4());
    let reg_str = RegisterId(uuid::Uuid::new_v4());
    let dt_id = DeviceTypeId(uuid::Uuid::new_v4());
    let dt = DeviceType {
        id: dt_id,
        name: "T".into(),
        description: String::new(),
        registers: vec![
            RegisterPoint {
                id: reg_u32,
                kind: RegisterKind::Holding,
                address: 0,
                name: "power".into(),
                description: String::new(),
                data_type: DataType::U32,
                encoding: Encoding::BigEndian,
                byte_length: None,
                default_value: Value::U32(42000),
            },
            RegisterPoint {
                id: reg_str,
                kind: RegisterKind::Holding,
                address: 10,
                name: "label".into(),
                description: String::new(),
                data_type: DataType::String,
                encoding: Encoding::BigEndian,
                byte_length: Some(4),
                default_value: Value::String("AB".into()),
            },
        ],
        behavior: DeviceBehavior::default(),
    };
    let dev_id = DeviceId(uuid::Uuid::new_v4());
    let device = Device {
        id: dev_id,
        name: "d".into(),
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
    Seed {
        world: World {
            device_types: vec![dt],
            contexts: vec![ctx],
            active_context_id: Some(ctx_id),
        },
        dev_id,
        reg_u32,
        reg_str,
    }
}

struct Fixture {
    http_port: u16,
    modbus_port: u16,
    state: Arc<AppState>,
    dev_id: DeviceId,
    reg_u32: RegisterId,
    reg_str: RegisterId,
}

async fn start() -> Fixture {
    let store = Store::with_root(tmp_root()).unwrap();
    let seed = seed_world();
    let state = AppState::new(seed.world, AppSettings::default(), store);

    let http_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let http_port = http_listener.local_addr().unwrap().port();
    let schema = build_schema(state.clone());
    let app = router(state.clone(), schema);
    tokio::spawn(async move {
        axum::serve(http_listener, app).await.ok();
    });

    let modbus_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let modbus_port = modbus_listener.local_addr().unwrap().port();
    drop(modbus_listener);
    let st = state.clone();
    tokio::spawn(async move {
        let _ = modbus_tcp::run(st, "127.0.0.1".into(), modbus_port).await;
    });

    tokio::time::sleep(Duration::from_millis(200)).await;
    Fixture {
        http_port,
        modbus_port,
        state,
        dev_id: seed.dev_id,
        reg_u32: seed.reg_u32,
        reg_str: seed.reg_str,
    }
}

async fn send_modbus_read(port: u16, address: u16, quantity: u16) {
    let mut sock = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let mut pdu = vec![0x03u8];
    pdu.extend_from_slice(&address.to_be_bytes());
    pdu.extend_from_slice(&quantity.to_be_bytes());
    let tid: u16 = 1;
    let len = (pdu.len() + 1) as u16;
    let mut frame = Vec::new();
    frame.extend_from_slice(&tid.to_be_bytes());
    frame.extend_from_slice(&0u16.to_be_bytes());
    frame.extend_from_slice(&len.to_be_bytes());
    frame.push(1);
    frame.extend_from_slice(&pdu);
    sock.write_all(&frame).await.unwrap();
    let mut header = [0u8; 7];
    sock.read_exact(&mut header).await.unwrap();
    let pdu_len = u16::from_be_bytes([header[4], header[5]]) as usize - 1;
    let mut out = vec![0u8; pdu_len];
    sock.read_exact(&mut out).await.unwrap();
}

async fn send_modbus_write_multiple(port: u16, address: u16, values: &[u16]) {
    let mut sock = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let mut pdu = vec![0x10u8];
    pdu.extend_from_slice(&address.to_be_bytes());
    pdu.extend_from_slice(&(values.len() as u16).to_be_bytes());
    pdu.push((values.len() * 2) as u8);
    for v in values {
        pdu.extend_from_slice(&v.to_be_bytes());
    }
    let tid: u16 = 2;
    let len = (pdu.len() + 1) as u16;
    let mut frame = Vec::new();
    frame.extend_from_slice(&tid.to_be_bytes());
    frame.extend_from_slice(&0u16.to_be_bytes());
    frame.extend_from_slice(&len.to_be_bytes());
    frame.push(1);
    frame.extend_from_slice(&pdu);
    sock.write_all(&frame).await.unwrap();
    let mut header = [0u8; 7];
    sock.read_exact(&mut header).await.unwrap();
    let pdu_len = u16::from_be_bytes([header[4], header[5]]) as usize - 1;
    let mut out = vec![0u8; pdu_len];
    sock.read_exact(&mut out).await.unwrap();
}

async fn send_modbus_write_single(port: u16, address: u16, value: u16) {
    let mut sock = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let mut pdu = vec![0x06u8];
    pdu.extend_from_slice(&address.to_be_bytes());
    pdu.extend_from_slice(&value.to_be_bytes());
    let tid: u16 = 2;
    let len = (pdu.len() + 1) as u16;
    let mut frame = Vec::new();
    frame.extend_from_slice(&tid.to_be_bytes());
    frame.extend_from_slice(&0u16.to_be_bytes());
    frame.extend_from_slice(&len.to_be_bytes());
    frame.push(1);
    frame.extend_from_slice(&pdu);
    sock.write_all(&frame).await.unwrap();
    let mut header = [0u8; 7];
    sock.read_exact(&mut header).await.unwrap();
    let pdu_len = u16::from_be_bytes([header[4], header[5]]) as usize - 1;
    let mut out = vec![0u8; pdu_len];
    sock.read_exact(&mut out).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn traffic_subscription_includes_summary_and_decoded_values() {
    let Fixture {
        http_port: http,
        modbus_port: modbus,
        ..
    } = start().await;

    let url = format!("ws://127.0.0.1:{http}/graphql/ws");
    let mut req = url.into_client_request().unwrap();
    req.headers_mut().insert(
        "sec-websocket-protocol",
        "graphql-transport-ws".parse().unwrap(),
    );
    let (mut ws, _) = tokio_tungstenite::connect_async(req).await.unwrap();

    ws.send(Message::Text(
        json!({"type":"connection_init"}).to_string().into(),
    ))
    .await
    .unwrap();
    // drain ack
    let _ = timeout(Duration::from_secs(2), ws.next()).await;
    ws.send(Message::Text(
        json!({
            "id":"1",
            "type":"subscribe",
            "payload":{"query":"subscription { traffic { direction summary decoded { registerName dataType value } } }"}
        }).to_string().into(),
    )).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Trigger: read 2 words at address 0 → covers the `power` U32 register.
    send_modbus_read(modbus, 0, 2).await;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    let mut in_summary = None;
    let mut out_summary = None;
    let mut decoded_value = None;
    while in_summary.is_none() || out_summary.is_none() || decoded_value.is_none() {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        let Ok(Some(Ok(m))) = timeout(remaining, ws.next()).await else {
            break;
        };
        let Message::Text(t) = m else { continue };
        let v: JsonValue = match serde_json::from_str(&t) {
            Ok(v) => v,
            _ => continue,
        };
        if v["type"] != "next" {
            continue;
        }
        let tf = &v["payload"]["data"]["traffic"];
        let dir = tf["direction"].as_str().unwrap_or("");
        let sum = tf["summary"].as_str().unwrap_or("").to_string();
        if dir == "in" {
            in_summary = Some(sum.clone());
        }
        if dir == "out" {
            out_summary = Some(sum);
            if let Some(decoded) = tf["decoded"].as_array() {
                for d in decoded {
                    if d["registerName"].as_str() == Some("power") {
                        decoded_value = Some((
                            d["dataType"].as_str().unwrap_or("").to_string(),
                            d["value"].as_str().unwrap_or("").to_string(),
                        ));
                    }
                }
            }
        }
    }

    assert_eq!(
        in_summary.as_deref(),
        Some("Read Holding Registers start=0 len=2"),
        "request summary mismatch"
    );
    assert_eq!(
        out_summary.as_deref(),
        Some("2 words returned"),
        "response summary mismatch"
    );
    let (dt, val) = decoded_value.expect("expected 'power' register in decoded list");
    assert_eq!(dt, "U32");
    assert!(val.contains("42000"), "decoded value: {val}");
}

#[tokio::test(flavor = "multi_thread")]
async fn device_activity_timestamps_track_reads_and_writes() {
    let Fixture {
        http_port: http,
        modbus_port: modbus,
        state,
        dev_id,
        ..
    } = start().await;

    // Initially no activity.
    assert!(state.device_activity(dev_id).last_read_at_ms.is_none());
    assert!(state.device_activity(dev_id).last_write_at_ms.is_none());

    send_modbus_read(modbus, 0, 2).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let after_read = state.device_activity(dev_id);
    assert!(
        after_read.last_read_at_ms.is_some(),
        "read should update last_read_at_ms"
    );
    assert!(
        after_read.last_write_at_ms.is_none(),
        "read must not touch last_write_at_ms"
    );

    send_modbus_write_single(modbus, 0, 42).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let after_write = state.device_activity(dev_id);
    assert!(
        after_write.last_write_at_ms.is_some(),
        "write should update last_write_at_ms"
    );
    assert_eq!(
        after_write.last_read_at_ms, after_read.last_read_at_ms,
        "write must not overwrite last_read_at_ms"
    );

    // GraphQL should surface the same values as strings.
    let client = reqwest::Client::new();
    let r = client
        .post(format!("http://127.0.0.1:{http}/graphql"))
        .header("content-type", "application/json")
        .body(r#"{"query":"{ devices { lastReadAtMs lastWriteAtMs } }"}"#)
        .send()
        .await
        .unwrap();
    let body: JsonValue = r.json().await.unwrap();
    let dev = &body["data"]["devices"][0];
    assert!(dev["lastReadAtMs"].is_string());
    assert!(dev["lastWriteAtMs"].is_string());
}

#[tokio::test(flavor = "multi_thread")]
async fn register_level_activity_tracks_per_register_reads_and_writes() {
    let Fixture {
        http_port: http,
        modbus_port: modbus,
        state,
        dev_id,
        reg_u32,
        reg_str,
    } = start().await;

    // Read only the U32 register at address 0 (spans 2 words).
    send_modbus_read(modbus, 0, 2).await;
    tokio::time::sleep(Duration::from_millis(80)).await;

    let map: std::collections::HashMap<_, _> = state
        .register_activity_for_device(dev_id)
        .into_iter()
        .collect();
    let u32_act = map.get(&reg_u32).copied().unwrap_or_default();
    assert!(
        u32_act.last_read_at_ms.is_some(),
        "U32 register should be marked read"
    );
    assert!(
        u32_act.last_write_at_ms.is_none(),
        "U32 register write stays untouched by a read"
    );
    assert!(
        !map.contains_key(&reg_str),
        "string register at address 10 is outside range and must not be touched: {map:?}"
    );

    // Write two words via FC16 to fully cover the U32 register.
    send_modbus_write_multiple(modbus, 0, &[0xAABB, 0xCCDD]).await;
    tokio::time::sleep(Duration::from_millis(80)).await;
    let map: std::collections::HashMap<_, _> = state
        .register_activity_for_device(dev_id)
        .into_iter()
        .collect();
    let u32_after = map.get(&reg_u32).copied().unwrap_or_default();
    assert!(u32_after.last_write_at_ms.is_some());
    assert_eq!(
        u32_after.last_read_at_ms, u32_act.last_read_at_ms,
        "write must not touch last_read_at_ms"
    );

    // GraphQL surfaces the map too.
    let client = reqwest::Client::new();
    let r = client
        .post(format!("http://127.0.0.1:{http}/graphql"))
        .header("content-type", "application/json")
        .body(
            r#"{"query":"{ devices { registerActivity { registerId lastReadAtMs lastWriteAtMs } } }"}"#,
        )
        .send()
        .await
        .unwrap();
    let body: JsonValue = r.json().await.unwrap();
    let entries = body["data"]["devices"][0]["registerActivity"]
        .as_array()
        .unwrap();
    let u32_entry = entries
        .iter()
        .find(|e| e["registerId"].as_str() == Some(&reg_u32.to_string()))
        .expect("U32 register activity missing in GraphQL payload");
    assert!(u32_entry["lastReadAtMs"].is_string());
    assert!(u32_entry["lastWriteAtMs"].is_string());
    // The string register never fired → should not appear.
    assert!(
        !entries
            .iter()
            .any(|e| e["registerId"].as_str() == Some(&reg_str.to_string())),
        "string register should not appear in registerActivity"
    );
}
