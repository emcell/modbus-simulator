// Regenerate `frontend/schema.graphql` from the Rust GraphQL schema.
//
// Steps:
//   1. Ensure `frontend/dist/` exists. rust-embed expands the
//      `#[folder = "…/frontend/dist/"]` path at macro expansion time and
//      errors out if the directory is missing. On a fresh CI checkout
//      there's nothing yet, so the schema binary would fail to compile
//      before `vite build` ever runs. An empty directory satisfies
//      rust-embed.
//   2. Spawn `cargo run --bin modsim-schema`, capture stdout and write
//      it to `schema.graphql`. Using Node's spawnSync + writeFileSync
//      instead of a shell redirect keeps the behaviour identical across
//      Unix and Windows runners (cmd.exe's `>` redirects can add a BOM
//      or CRLF depending on the PowerShell wrapper).
import { mkdirSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";

mkdirSync("dist", { recursive: true });

const result = spawnSync(
  "cargo",
  ["run", "--quiet", "--manifest-path=../Cargo.toml", "--bin", "modsim-schema"],
  { encoding: "utf8", stdio: ["ignore", "pipe", "inherit"] },
);

if (result.error) {
  console.error(result.error);
  process.exit(1);
}
if (result.status !== 0) {
  process.exit(result.status ?? 1);
}

writeFileSync("schema.graphql", result.stdout);
