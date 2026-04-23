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
    tracing::info!("HTTP listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

// keep Arc import used
#[allow(dead_code)]
fn _type_hint(_s: Arc<AppState>) {}
