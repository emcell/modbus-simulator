//! HTTP server: serves GraphQL endpoint and embedded UI assets.

use std::sync::Arc;

use async_graphql::http::{playground_source, GraphQLPlaygroundConfig};
use async_graphql_axum::{GraphQLRequest, GraphQLResponse, GraphQLSubscription};
use axum::extract::State;
use axum::http::{header, StatusCode, Uri};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use rust_embed::Embed;

use crate::graphql::ApiSchema;
use crate::state::AppState;

#[derive(Embed)]
#[folder = "$CARGO_MANIFEST_DIR/../../frontend/dist/"]
#[include = "*"]
struct Assets;

pub fn router(state: Arc<AppState>, schema: ApiSchema) -> Router {
    Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/graphql", post(graphql_handler))
        .route_service("/graphql/ws", GraphQLSubscription::new(schema.clone()))
        .route("/playground", get(playground))
        .fallback(static_handler)
        .with_state(HttpState { app: state, schema })
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

async fn playground() -> impl IntoResponse {
    Html(playground_source(GraphQLPlaygroundConfig::new("/graphql")))
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
        None => {
            // SPA fallback
            if let Some(idx) = Assets::get("index.html") {
                ([(header::CONTENT_TYPE, "text/html")], idx.data.into_owned()).into_response()
            } else {
                (StatusCode::NOT_FOUND, "not found").into_response()
            }
        }
    }
}
