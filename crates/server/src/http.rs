//! HTTP server: serves GraphQL endpoint and embedded UI assets.

use std::sync::Arc;

use async_graphql::http::{playground_source, GraphQLPlaygroundConfig};
use async_graphql_axum::{GraphQLRequest, GraphQLResponse, GraphQLSubscription};
use axum::extract::{OriginalUri, State};
use axum::http::{header, HeaderMap, StatusCode, Uri};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::Router;
use rust_embed::Embed;

use crate::graphql::ApiSchema;
use crate::state::AppState;

#[derive(Embed)]
#[folder = "$CARGO_MANIFEST_DIR/../../frontend/dist/"]
#[include = "*"]
struct Assets;

/// Router mounted at the server root.
pub fn router(state: Arc<AppState>, schema: ApiSchema) -> Router {
    router_with_base(state, schema, "")
}

/// Same as [`router`], but mounted below `base_path`.
///
/// Only needed for reverse proxies that forward the subpath verbatim
/// (`proxy_pass http://backend;`). Proxies that strip it
/// (`proxy_pass http://backend/;`) need no configuration at all — the UI
/// builds every URL relative to the document it was loaded from.
///
/// An empty / `"/"` base path mounts at the root, unchanged.
pub fn router_with_base(state: Arc<AppState>, schema: ApiSchema, base_path: &str) -> Router {
    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/graphql", post(graphql_handler))
        .route_service("/graphql/ws", GraphQLSubscription::new(schema.clone()))
        .route("/playground", get(playground))
        .fallback(static_handler)
        .with_state(HttpState { app: state, schema });

    let Some(base) = normalize_base_path(base_path) else {
        return app;
    };

    let with_slash = format!("{base}/");
    let redirect_target = with_slash.clone();
    Router::new()
        // `/modsim` without the trailing slash would make the browser
        // resolve the UI's relative asset URLs against the parent
        // directory, so bounce those requests to `/modsim/` first.
        .route(
            &base,
            get(move || {
                let target = redirect_target.clone();
                async move { Redirect::permanent(&target) }
            }),
        )
        // `nest_service` (rather than `nest`) so the SPA fallback that
        // serves index.html is prefixed along with the routes.
        .nest_service(&with_slash, app)
}

/// `"/modsim"` for anything shaped like `modsim`, `/modsim`, `/modsim/`;
/// `None` when no base path is configured (mount at the root).
///
/// Path parameters (`{id}`) would make axum panic when nesting, so a base
/// path containing braces is rejected rather than trusted.
pub fn normalize_base_path(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_matches('/').trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.contains(['{', '}', '?', '#']) {
        tracing::warn!("ignoring invalid base path '{raw}' (must be a plain path like /modsim)");
        return None;
    }
    Some(format!("/{trimmed}"))
}

#[derive(Clone)]
struct HttpState {
    app: Arc<AppState>,
    schema: ApiSchema,
}

async fn graphql_handler(State(s): State<HttpState>, req: GraphQLRequest) -> GraphQLResponse {
    let _ = &s.app; // schema already owns Arc<AppState>
    s.schema.execute(req.into_inner()).await.into()
}

/// GraphQL Playground needs absolute URLs (it derives the subscription
/// socket from them), so they are reconstructed from the request: the
/// path the client actually asked for plus, behind a prefix-stripping
/// proxy, the standard `X-Forwarded-*` headers.
async fn playground(headers: HeaderMap, OriginalUri(uri): OriginalUri) -> impl IntoResponse {
    let header = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
    let scheme = header("x-forwarded-proto").unwrap_or("http");
    let host = header("x-forwarded-host")
        .or_else(|| header(header::HOST.as_str()))
        .unwrap_or("localhost");
    let prefix = header("x-forwarded-prefix")
        .unwrap_or_default()
        .trim_end_matches('/');

    // "/modsim/playground" → "/modsim/". The prefix is only prepended when
    // the proxy stripped it; if it forwarded the prefix it is already part
    // of the path.
    let dir = uri.path().strip_suffix("playground").unwrap_or("/");
    let dir = if prefix.is_empty() || dir.starts_with(prefix) {
        dir.to_string()
    } else {
        format!("{prefix}{dir}")
    };

    let endpoint = format!("{scheme}://{host}{dir}graphql");
    let ws_scheme = if scheme == "https" { "wss" } else { "ws" };
    let subscription = format!("{ws_scheme}://{host}{dir}graphql/ws");
    Html(playground_source(
        GraphQLPlaygroundConfig::new(&endpoint).subscription_endpoint(&subscription),
    ))
}

async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    match Assets::get(path) {
        Some(f) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            (
                [(header::CONTENT_TYPE, mime.as_ref().to_string())],
                f.data.into_owned(),
            )
                .into_response()
        }
        // SPA fallback — but only for document-ish requests. Answering a
        // missing `/assets/index-abc.js` with index.html would leave the
        // browser reporting a MIME-type error instead of the 404 that
        // points at the actual problem (usually a proxy prefix that isn't
        // being stripped).
        None if !looks_like_file(path) => match Assets::get("index.html") {
            Some(idx) => {
                ([(header::CONTENT_TYPE, "text/html")], idx.data.into_owned()).into_response()
            }
            None => (StatusCode::NOT_FOUND, "not found").into_response(),
        },
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

/// Whether the last path segment carries a file extension.
fn looks_like_file(path: &str) -> bool {
    path.rsplit('/').next().is_some_and(|seg| seg.contains('.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_path_normalization() {
        assert_eq!(normalize_base_path(""), None);
        assert_eq!(normalize_base_path("/"), None);
        assert_eq!(normalize_base_path("  "), None);
        assert_eq!(normalize_base_path("modsim").as_deref(), Some("/modsim"));
        assert_eq!(normalize_base_path("/modsim").as_deref(), Some("/modsim"));
        assert_eq!(normalize_base_path("/modsim/").as_deref(), Some("/modsim"));
        assert_eq!(
            normalize_base_path(" /tools/modbus/ ").as_deref(),
            Some("/tools/modbus")
        );
        assert_eq!(normalize_base_path("/{id}"), None);
    }

    #[test]
    fn file_like_paths() {
        assert!(looks_like_file("assets/index-abc.js"));
        assert!(looks_like_file("favicon.ico"));
        assert!(!looks_like_file("devices"));
        assert!(!looks_like_file("some.dir/devices"));
    }
}
