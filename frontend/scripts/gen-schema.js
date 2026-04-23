// Regenerate `frontend/schema.graphql` from the Rust GraphQL schema.
//
// 1. Make sure `frontend/dist/` exists. rust-embed expands the
//    `#[folder = "…/frontend/dist/"]` path at macro expansion time and
//    errors out if the directory is missing. On a fresh CI checkout
//    there's nothing yet — `vite build` only runs later. An empty
//    directory is enough for rust-embed.
// 2. Invoke `cargo run --bin modsim-schema` through a shell so Windows
//    finds `cargo.exe` via PATH regardless of how npm wraps the script.
// 3. Write the captured SDL to `schema.graphql` via Node (no shell
//    redirects → no Windows BOM/CRLF surprises). Sanity-check that the
//    output actually looks like a GraphQL schema; bail loudly if not so
//    `gql.tada` doesn't silently produce empty types and leave `tsc`
//    with a pile of confusing `'world' is of type 'unknown'` errors.
import { mkdirSync, writeFileSync } from "node:fs";
import { execSync } from "node:child_process";

mkdirSync("dist", { recursive: true });

let sdl;
try {
  sdl = execSync(
    "cargo run --quiet --manifest-path=../Cargo.toml --bin modsim-schema",
    {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "inherit"],
      maxBuffer: 16 * 1024 * 1024,
    },
  );
} catch (e) {
  console.error("[gen-schema] cargo invocation failed:", e.message);
  if (e.stdout) {
    console.error("stdout (truncated):", e.stdout.toString().slice(0, 2000));
  }
  process.exit(e.status ?? 1);
}

if (!sdl || sdl.length === 0) {
  console.error(
    "[gen-schema] cargo produced an empty schema — refusing to overwrite schema.graphql",
  );
  process.exit(1);
}

// Strip a possible UTF-8 BOM that some Windows toolchains prepend.
if (sdl.charCodeAt(0) === 0xfeff) {
  sdl = sdl.slice(1);
}

if (!sdl.includes("type Query")) {
  console.error(
    "[gen-schema] output does not look like a GraphQL schema (no 'type Query'):",
  );
  console.error(sdl.slice(0, 2000));
  process.exit(1);
}

writeFileSync("schema.graphql", sdl);
console.log(`[gen-schema] wrote ${sdl.length} bytes to schema.graphql`);
