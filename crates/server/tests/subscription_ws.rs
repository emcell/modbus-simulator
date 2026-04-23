//! End-to-end test for GraphQL subscriptions over WebSocket.
//!
//! Connects with the `graphql-transport-ws` subprotocol (same as the
//! frontend's `subscriptions.ts`), subscribes to `traffic`, then pushes a
//! Modbus TCP request through the simulator and asserts that the frame
//! appears on the subscription.

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
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::Message;

fn tmp_root() -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!(
        "modsim-subws-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn seed_world() -> World {
    let r = RegisterId(uuid::Uuid::new_v4());
    let dt_id = DeviceTypeId(uuid::Uuid::new_v4());
    let dt = DeviceType {
        id: dt_id,
        name: "S".into(),
        description: String::new(),
        registers: vec![RegisterPoint {
            id: r,
            kind: RegisterKind::Holding,
            address: 0,
            name: "h".into(),
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

async fn start_full_server() -> (u16, u16, Arc<AppState>) {
    let store = Store::with_root(tmp_root()).unwrap();
    let state = AppState::new(seed_world(), AppSettings::default(), store);

    // HTTP listener (for /graphql/ws)
    let http_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let http_port = http_listener.local_addr().unwrap().port();
    let schema = build_schema(state.clone());
    let app = router(state.clone(), schema);
    tokio::spawn(async move {
        axum::serve(http_listener, app).await.ok();
    });

    // Modbus TCP listener
    let modbus_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let modbus_port = modbus_listener.local_addr().unwrap().port();
    drop(modbus_listener);
    let st = state.clone();
    tokio::spawn(async move {
        let _ = modbus_tcp::run(st, "127.0.0.1".into(), modbus_port).await;
    });

    tokio::time::sleep(Duration::from_millis(200)).await;
    (http_port, modbus_port, state)
}

async fn send_modbus_read(port: u16) {
    let mut sock = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let pdu = [0x03u8, 0x00, 0x00, 0x00, 0x01];
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
    let mut pdu_out = vec![0u8; pdu_len];
    sock.read_exact(&mut pdu_out).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn ws_subscription_receives_traffic_frames() {
    let (http_port, modbus_port, _state) = start_full_server().await;

    // Connect with graphql-transport-ws subprotocol (same as the frontend).
    let url = format!("ws://127.0.0.1:{http_port}/graphql/ws");
    let mut req = url.into_client_request().unwrap();
    req.headers_mut().insert(
        "sec-websocket-protocol",
        "graphql-transport-ws".parse().unwrap(),
    );
    let (mut ws, _resp) = tokio_tungstenite::connect_async(req)
        .await
        .expect("ws connect");

    // Handshake
    ws.send(Message::Text(
        json!({"type": "connection_init"}).to_string().into(),
    ))
    .await
    .unwrap();

    // Wait for connection_ack
    let ack = timeout(Duration::from_secs(3), ws.next())
        .await
        .expect("ack timeout")
        .expect("stream end")
        .expect("ws error");
    let text = match ack {
        Message::Text(t) => t,
        other => panic!("unexpected {other:?}"),
    };
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["type"], "connection_ack", "got {text}");

    // Subscribe to traffic
    ws.send(Message::Text(
        json!({
            "id": "1",
            "type": "subscribe",
            "payload": {
                "query": "subscription { traffic { direction transport slaveId functionCode bytesHex timestampMs } }",
                "variables": {}
            }
        })
        .to_string()
        .into(),
    ))
    .await
    .unwrap();

    // Give the subscription a moment to attach to the broadcast channel.
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Trigger a Modbus request which should emit two traffic events
    // (direction=in + direction=out).
    send_modbus_read(modbus_port).await;

    // Collect events from the subscription.
    let mut directions = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while directions.len() < 2 {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        let msg = match timeout(remaining, ws.next()).await {
            Ok(Some(Ok(m))) => m,
            _ => break,
        };
        let text = match msg {
            Message::Text(t) => t,
            Message::Ping(p) => {
                ws.send(Message::Pong(p)).await.ok();
                continue;
            }
            _ => continue,
        };
        let v: serde_json::Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if v["type"] == "next" {
            let dir = v["payload"]["data"]["traffic"]["direction"]
                .as_str()
                .unwrap_or("")
                .to_string();
            let fc = v["payload"]["data"]["traffic"]["functionCode"]
                .as_i64()
                .unwrap_or(0);
            directions.push((dir, fc));
        }
    }

    assert!(
        directions.iter().any(|(d, fc)| d == "in" && *fc == 3),
        "expected inbound FC03 frame, got {directions:?}"
    );
    assert!(
        directions.iter().any(|(d, fc)| d == "out" && *fc == 3),
        "expected outbound FC03 frame, got {directions:?}"
    );
}
