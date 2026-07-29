//! Serving the API + UI below a subpath, for reverse proxies that forward
//! their prefix instead of stripping it (`MODSIM_BASE_PATH=/modsim`).

use std::path::PathBuf;

use modsim_core::model::World;
use modsim_server::graphql::build_schema;
use modsim_server::http::router_with_base;
use modsim_server::persistence::{AppSettings, Store};
use modsim_server::state::AppState;

fn tmp_root() -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "modsim-basepath-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}

async fn start(base_path: &str) -> u16 {
    let store = Store::with_root(tmp_root()).unwrap();
    let state = AppState::new(World::default(), AppSettings::default(), store);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let schema = build_schema(state.clone());
    let app = router_with_base(state, schema, base_path);
    tokio::spawn(async move {
        axum::serve(listener, app).await.ok();
    });
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    port
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn routes_and_ui_are_served_below_the_base_path() {
    let port = start("/modsim").await;
    let c = client();
    let url = |p: &str| format!("http://127.0.0.1:{port}{p}");

    let health = c.get(url("/modsim/health")).send().await.unwrap();
    assert_eq!(health.status(), 200);
    assert_eq!(health.text().await.unwrap(), "ok");

    // GraphQL over POST.
    let body = serde_json::json!({ "query": "{ contexts { id } }" });
    let gql = c
        .post(url("/modsim/graphql"))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(gql.status(), 200);
    assert!(gql.text().await.unwrap().contains("\"data\""));

    // The UI itself, served by the SPA fallback below the prefix.
    let index = c.get(url("/modsim/")).send().await.unwrap();
    assert_eq!(index.status(), 200);
    assert!(index.text().await.unwrap().contains("<div id=\"root\">"));

    // Nothing is reachable at the root any more.
    assert_eq!(c.get(url("/health")).send().await.unwrap().status(), 404);
}

#[tokio::test(flavor = "multi_thread")]
async fn bare_base_path_redirects_to_trailing_slash() {
    let port = start("modsim/").await; // normalization also handles this shape
    let resp = client()
        .get(format!("http://127.0.0.1:{port}/modsim"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 308);
    assert_eq!(
        resp.headers().get("location").unwrap().to_str().unwrap(),
        "/modsim/"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn missing_assets_below_the_base_path_are_404_not_index_html() {
    let port = start("/modsim").await;
    let resp = client()
        .get(format!("http://127.0.0.1:{port}/modsim/assets/nope.js"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 404);
}

#[tokio::test(flavor = "multi_thread")]
async fn playground_points_at_the_prefixed_graphql_endpoint() {
    let port = start("/modsim").await;
    let c = client();

    // Prefix forwarded by the proxy → visible in the request path.
    let html = c
        .get(format!("http://127.0.0.1:{port}/modsim/playground"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        html.contains(&format!("http://127.0.0.1:{port}/modsim/graphql")),
        "playground should target the prefixed endpoint"
    );
    assert!(html.contains(&format!("ws://127.0.0.1:{port}/modsim/graphql/ws")));

    // Prefix stripped by the proxy → the server serves at its root and
    // only the forwarded headers know where the client sees it.
    let root_port = start("").await;
    let html = c
        .get(format!("http://127.0.0.1:{root_port}/playground"))
        .header("x-forwarded-proto", "https")
        .header("x-forwarded-host", "sim.example.com")
        .header("x-forwarded-prefix", "/tools/modsim")
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(html.contains("https://sim.example.com/tools/modsim/graphql"));
    assert!(html.contains("wss://sim.example.com/tools/modsim/graphql/ws"));
}

#[tokio::test(flavor = "multi_thread")]
async fn empty_base_path_serves_at_the_root() {
    let port = start("").await;
    let c = client();
    let health = c
        .get(format!("http://127.0.0.1:{port}/health"))
        .send()
        .await
        .unwrap();
    assert_eq!(health.status(), 200);
}
