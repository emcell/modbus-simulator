//! Dumps the GraphQL SDL to stdout. Used by the frontend's gql.tada
//! toolchain to generate typed queries.

use std::sync::Arc;

use modsim_server::graphql::build_schema;
use modsim_server::persistence::{AppSettings, Store};
use modsim_server::state::AppState;

fn main() {
    // Create a throwaway state — the schema definition doesn't depend on
    // runtime data, but `build_schema` takes the state for resolvers.
    let tmp = std::env::temp_dir().join("modsim-schema-dump");
    let _ = std::fs::create_dir_all(&tmp);
    let store = Store::with_root(tmp).expect("tmp store");
    let state = AppState::new(Default::default(), AppSettings::default(), store);
    let schema = build_schema(Arc::clone(&state));
    print!("{}", schema.sdl());
}
