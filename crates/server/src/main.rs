use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use modsim_server::graphql::build_schema;
use modsim_server::http::router;
use modsim_server::persistence::Store;
use modsim_server::state::AppState;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env from CWD (walks up the tree). Silent when not present —
    // release users don't need one; developers copy `.env.example` once.
    let _ = dotenvy::dotenv();

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let store = Store::new_default()?;
    let settings = store.load_settings()?;
    let world = store.load_world()?;
    let persisted_vpty = store.load_virtual_serials().unwrap_or_default();
    let state = AppState::new(world, settings.clone(), store);

    // Re-create persisted virtual PTYs. The fd itself can't survive a
    // restart, but we restore the same id (so RTU configs that reference
    // it keep working) and the symlink (so the user's test app sees the
    // same path).
    #[cfg(unix)]
    for intent in persisted_vpty {
        match state
            .ptys
            .create_with_id(intent.id.clone(), intent.symlink_path.clone())
        {
            Ok(v) => tracing::info!(
                "restored virtual serial {} at {}",
                v.id,
                v.symlink_path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| v.slave_path.display().to_string())
            ),
            Err(e) => tracing::warn!("could not restore virtual serial {}: {e}", intent.id),
        }
    }
    #[cfg(not(unix))]
    let _ = persisted_vpty;

    // Bind TCP / RTU according to the active context. Subsequent config
    // changes trigger `state.supervisor.reconfigure(&state)` from the
    // GraphQL mutation handlers, so no process restart is required.
    state.supervisor.reconfigure(&state).await;

    let schema = build_schema(state.clone());
    let app = router(state.clone(), schema);

    // Env var override for tests / ad-hoc runs.
    let port = std::env::var("MODSIM_HTTP_PORT")
        .ok()
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(settings.http_port);
    let bind = std::env::var("MODSIM_HTTP_BIND").unwrap_or_else(|_| settings.http_bind.clone());
    let addr: SocketAddr = format!("{bind}:{port}").parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("HTTP listening on http://{addr}");

    maybe_open_browser(&bind, port);

    axum::serve(listener, app).await?;
    Ok(())
}

/// Spawn the default browser pointing at the UI, unless
/// `MODSIM_OPEN_BROWSER` is set to a falsy value. Silent on failure —
/// the URL is already in the log line above so the user can still click
/// it.
fn maybe_open_browser(bind: &str, port: u16) {
    let raw = std::env::var("MODSIM_OPEN_BROWSER").unwrap_or_default();
    let enabled = match raw.trim().to_ascii_lowercase().as_str() {
        "" | "1" | "true" | "yes" | "on" => true,
        "0" | "false" | "no" | "off" => false,
        other => {
            tracing::warn!(
                "unrecognized MODSIM_OPEN_BROWSER value '{other}', using default (enabled)"
            );
            true
        }
    };
    if !enabled {
        tracing::info!("Browser auto-open disabled (MODSIM_OPEN_BROWSER={raw})");
        return;
    }
    // Browsers can't navigate to a wildcard bind; point at loopback instead.
    let host = match bind {
        "0.0.0.0" | "::" | "[::]" => "127.0.0.1",
        other => other,
    };
    let url = format!("http://{host}:{port}/");
    tracing::info!("Opening {url} in browser");
    if let Err(e) = open::that_detached(&url) {
        tracing::warn!("failed to open browser for {url}: {e}");
    }
}

// keep Arc import used
#[allow(dead_code)]
fn _type_hint(_s: Arc<AppState>) {}
