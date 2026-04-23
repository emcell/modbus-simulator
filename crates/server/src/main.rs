use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use modsim_server::graphql::build_schema;
use modsim_server::http::router;
use modsim_server::persistence::Store;
use modsim_server::state::AppState;
use modsim_server::transport::{rtu as modbus_rtu, tcp as modbus_tcp};
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
    let state = AppState::new(world, settings.clone(), store);

    // Kick off modbus TCP from the active context if enabled.
    if let Some(ctx) = state.world.read().active_context().cloned() {
        if ctx.transport.tcp.enabled {
            let bind = if ctx.transport.tcp.bind.is_empty() {
                "0.0.0.0".to_string()
            } else {
                ctx.transport.tcp.bind.clone()
            };
            let port = if ctx.transport.tcp.port == 0 {
                502
            } else {
                ctx.transport.tcp.port
            };
            let st = state.clone();
            tokio::spawn(async move {
                if let Err(e) = modbus_tcp::run(st, bind, port).await {
                    tracing::error!("modbus tcp: {e}");
                }
            });
        }
        if ctx.transport.rtu.enabled && !ctx.transport.rtu.device.is_empty() {
            let rtu = ctx.transport.rtu.clone();
            let st = state.clone();
            tokio::spawn(async move {
                let cfg = modbus_rtu::SerialConfig {
                    device: &rtu.device,
                    baud_rate: if rtu.baud_rate == 0 {
                        9600
                    } else {
                        rtu.baud_rate
                    },
                    data_bits: if rtu.data_bits == 0 { 8 } else { rtu.data_bits },
                    stop_bits: if rtu.stop_bits == 0 { 1 } else { rtu.stop_bits },
                    parity: if rtu.parity.is_empty() {
                        "N"
                    } else {
                        &rtu.parity
                    },
                };
                if let Err(e) = modbus_rtu::run(st, cfg).await {
                    tracing::error!("modbus rtu: {e}");
                }
            });
        }
    }

    let schema = build_schema(state.clone());
    let app = router(state.clone(), schema);

    let addr: SocketAddr = format!("{}:{}", settings.http_bind, settings.http_port).parse()?;
    tracing::info!("HTTP listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

// keep Arc import used
#[allow(dead_code)]
fn _type_hint(_s: Arc<AppState>) {}
