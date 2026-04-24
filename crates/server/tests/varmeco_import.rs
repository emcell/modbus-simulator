//! End-to-end check that the `importVarmecoCsv` GraphQL mutation reads a
//! Varmeco fixture file and produces a device type with the expected
//! register set.

use std::path::PathBuf;

use modsim_core::model::World;
use modsim_server::graphql::build_schema;
use modsim_server::http::router;
use modsim_server::persistence::{AppSettings, Store};
use modsim_server::state::AppState;
use serde_json::Value as JsonValue;

fn tmp_root() -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "modsim-varimp-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("varmeco_csv_format")
        .join(name)
}

async fn start() -> u16 {
    let store = Store::with_root(tmp_root()).unwrap();
    let state = AppState::new(World::default(), AppSettings::default(), store);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let schema = build_schema(state.clone());
    let app = router(state, schema);
    tokio::spawn(async move {
        axum::serve(listener, app).await.ok();
    });
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    port
}

#[tokio::test(flavor = "multi_thread")]
async fn importing_vfnova_csv_creates_device_type_with_registers() {
    let port = start().await;
    let csv = std::fs::read_to_string(fixture_path("vfnova.csv")).unwrap();

    let body = serde_json::json!({
        "query": "mutation($n:String!, $d:String!) { importVarmecoCsv(name:$n, data:$d) { id name registers { name address kind dataType } } }",
        "variables": { "n": "vfnova", "d": csv },
    });

    let client = reqwest::Client::new();
    let resp: JsonValue = client
        .post(format!("http://127.0.0.1:{port}/graphql"))
        .json(&body)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert!(
        resp.get("errors").is_none(),
        "unexpected GraphQL errors: {resp:#?}"
    );
    let dt = &resp["data"]["importVarmecoCsv"];
    assert_eq!(dt["name"], "vfnova");
    let regs = dt["registers"].as_array().expect("registers array");
    // vfnova.csv has 56 data rows.
    assert!(
        regs.len() >= 50,
        "expected ~56 registers, got {}",
        regs.len()
    );
    // First row: SYS.CollectiveFaultSignal;0;coil;…
    let first = &regs[0];
    assert_eq!(first["name"], "SYS.CollectiveFaultSignal");
    assert_eq!(first["address"], 0);
    assert_eq!(first["kind"], "COIL");
}

#[tokio::test(flavor = "multi_thread")]
async fn importing_invalid_csv_returns_error_without_creating_device_type() {
    let port = start().await;
    let body = serde_json::json!({
        "query": "mutation($n:String!, $d:String!) { importVarmecoCsv(name:$n, data:$d) { id } }",
        "variables": { "n": "junk", "d": "not even a csv" },
    });
    let client = reqwest::Client::new();
    let resp: JsonValue = client
        .post(format!("http://127.0.0.1:{port}/graphql"))
        .json(&body)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(resp.get("errors").is_some(), "expected error: {resp:#?}");

    // Confirm no device type ended up persisted.
    let list: JsonValue = client
        .post(format!("http://127.0.0.1:{port}/graphql"))
        .json(&serde_json::json!({"query": "{ deviceTypes { id name } }"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        list["data"]["deviceTypes"]
            .as_array()
            .map(|a| a.len())
            .unwrap_or(0),
        0
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn importing_exm_compact_preserves_signed_int16_value_type() {
    let port = start().await;
    let csv = std::fs::read_to_string(fixture_path("exm_compact.csv")).unwrap();
    let body = serde_json::json!({
        "query": "mutation($n:String!, $d:String!) { importVarmecoCsv(name:$n, data:$d) { registers { name dataType } } }",
        "variables": { "n": "exm_compact", "d": csv },
    });
    let client = reqwest::Client::new();
    let resp: JsonValue = client
        .post(format!("http://127.0.0.1:{port}/graphql"))
        .json(&body)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(resp.get("errors").is_none(), "{resp:#?}");
    let regs = resp["data"]["importVarmecoCsv"]["registers"]
        .as_array()
        .unwrap();
    let tf_offset = regs
        .iter()
        .find(|r| r["name"] == "SYS.Config.TfOffset")
        .expect("TfOffset present");
    assert_eq!(tf_offset["dataType"], "I16");
}
