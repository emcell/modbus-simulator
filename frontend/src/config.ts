/**
 * Runtime URL resolution.
 *
 * The UI must work both when it is served from the server root
 * (`http://host:8080/`) and when a reverse proxy publishes it under a
 * subpath (`https://host/tools/modsim/`). Since the frontend is baked
 * into the release binary we cannot bake the subpath in at build time —
 * every URL the app builds is therefore derived at runtime from the
 * document it was loaded with.
 *
 * `import.meta.env.BASE_URL` is `"/"` for the dev server and `"./"` for
 * production builds (see `base` in vite.config.ts), so resolving it
 * against `document.baseURI` yields the directory the UI was served
 * from — no configuration, no rebuild.
 *
 * Note that the proxy location needs a trailing slash (`/modsim/`);
 * without one the browser resolves relative URLs against the parent
 * directory. The README documents the proxy snippets.
 */
const appBase = new URL(import.meta.env.BASE_URL, document.baseURI);

/** Absolute http(s) URL for a path relative to the UI root, e.g. `graphql`. */
export function httpUrl(path: string): string {
  return new URL(path, appBase).toString();
}

/** Same as {@link httpUrl}, but with the matching ws:/wss: scheme. */
export function wsUrl(path: string): string {
  const url = new URL(path, appBase);
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  return url.toString();
}
