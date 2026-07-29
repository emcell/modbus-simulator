/**
 * The UI has to work when it is not served from the origin root. Two
 * deployment shapes are covered here:
 *
 *   1. The proxy strips its prefix (`proxy_pass http://backend/;`). The
 *      backend keeps serving at `/`, and only the browser sees the
 *      subpath — this is what the relative asset/API URLs are for. A
 *      minimal HTTP+WebSocket proxy below plays the nginx role.
 *   2. The proxy forwards the prefix verbatim (`proxy_pass
 *      http://backend;`). The backend itself mounts below the prefix via
 *      MODSIM_BASE_PATH, so no proxy is needed to reproduce it.
 *
 * Both cases assert that assets load, GraphQL over POST works, and the
 * subscription WebSocket comes up — the three things a subpath breaks.
 */
import { spawn, type ChildProcess } from "node:child_process";
import { createServer, request as httpRequest, type Server } from "node:http";
import { connect as netConnect } from "node:net";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { resolve } from "node:path";

import { test, expect, type Page } from "@playwright/test";

const PREFIX = "/tools/modsim";
const STRIPPING_BACKEND_PORT = 18090;
const PROXY_PORT = 18091;
const BASE_PATH_BACKEND_PORT = 18092;

const procs: ChildProcess[] = [];
const tmpRoots: string[] = [];
let proxy: Server | null = null;

function startBackend(port: number, extraEnv: Record<string, string> = {}) {
  // Playwright runs tests with cwd = frontend/
  const bin = resolve(process.cwd(), "..", "target", "release", "modsim");
  const tmpRoot = mkdtempSync(resolve(tmpdir(), "modsim-proxy-e2e-"));
  tmpRoots.push(tmpRoot);
  const proc = spawn(bin, [], {
    env: {
      ...process.env,
      MODSIM_CONFIG_DIR: tmpRoot,
      MODSIM_HTTP_PORT: String(port),
      MODSIM_OPEN_BROWSER: "false",
      ...extraEnv,
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  procs.push(proc);
}

async function waitForHealth(url: string) {
  const deadline = Date.now() + 10_000;
  while (Date.now() < deadline) {
    try {
      const r = await fetch(url);
      if (r.ok) return;
    } catch {
      /* not up yet */
    }
    await new Promise((r) => setTimeout(r, 150));
  }
  throw new Error(`no /health response from ${url}`);
}

/** `/tools/modsim/graphql` → `/graphql`; `null` when outside the prefix. */
function stripPrefix(url: string): string | null {
  if (url === PREFIX) return "/";
  if (url.startsWith(`${PREFIX}/`)) return url.slice(PREFIX.length);
  return null;
}

/** Prefix-stripping reverse proxy, HTTP and WebSocket upgrades alike. */
function startProxy(): Promise<Server> {
  const server = createServer((req, res) => {
    const path = stripPrefix(req.url ?? "/");
    if (path === null) {
      res.writeHead(404).end("outside the proxied prefix");
      return;
    }
    const upstream = httpRequest(
      {
        host: "127.0.0.1",
        port: STRIPPING_BACKEND_PORT,
        path,
        method: req.method,
        headers: { ...req.headers, "x-forwarded-prefix": PREFIX },
      },
      (up) => {
        res.writeHead(up.statusCode ?? 502, up.headers);
        up.pipe(res);
      },
    );
    upstream.on("error", () => res.writeHead(502).end("upstream error"));
    req.pipe(upstream);
  });

  server.on("upgrade", (req, socket, head) => {
    const path = stripPrefix(req.url ?? "/");
    if (path === null) {
      socket.destroy();
      return;
    }
    const upstream = netConnect(STRIPPING_BACKEND_PORT, "127.0.0.1", () => {
      const headers = Object.entries(req.headers)
        .map(([k, v]) => `${k}: ${Array.isArray(v) ? v.join(", ") : v}`)
        .join("\r\n");
      upstream.write(`GET ${path} HTTP/1.1\r\n${headers}\r\n\r\n`);
      if (head?.length) upstream.write(head);
      upstream.pipe(socket);
      socket.pipe(upstream);
    });
    upstream.on("error", () => socket.destroy());
  });

  return new Promise((res) => server.listen(PROXY_PORT, "127.0.0.1", () => res(server)));
}

test.beforeAll(async () => {
  startBackend(STRIPPING_BACKEND_PORT);
  startBackend(BASE_PATH_BACKEND_PORT, { MODSIM_BASE_PATH: PREFIX });
  proxy = await startProxy();
  await waitForHealth(`http://127.0.0.1:${STRIPPING_BACKEND_PORT}/health`);
  await waitForHealth(`http://127.0.0.1:${BASE_PATH_BACKEND_PORT}${PREFIX}/health`);
});

test.afterAll(async () => {
  for (const p of procs) {
    if (p.pid != null) p.kill("SIGTERM");
  }
  proxy?.close();
  await new Promise((r) => setTimeout(r, 200));
  for (const root of tmpRoots) {
    try {
      rmSync(root, { recursive: true, force: true });
    } catch {
      /* ignore */
    }
  }
});

/**
 * Loads the UI, then asserts the three things a subpath deployment
 * breaks: assets, GraphQL POST and the subscription socket. Returns the
 * URLs the page requested so callers can check they carried the prefix.
 */
async function walkTheApp(page: Page, url: string) {
  const failures: string[] = [];
  const requested: string[] = [];
  page.on("request", (req) => requested.push(req.url()));
  page.on("requestfailed", (req) =>
    failures.push(`${req.method()} ${req.url()} — ${req.failure()?.errorText}`),
  );
  page.on("response", (resp) => {
    if (resp.status() >= 400) failures.push(`${resp.status()} ${resp.url()}`);
  });
  page.on("pageerror", (err) => failures.push(`pageerror: ${err.message}`));

  await page.goto(url);

  // Rendered at all → the JS/CSS assets resolved under the prefix.
  await expect(page.getByRole("heading", { name: /Modbus Simulator/ })).toBeVisible();
  // The devices page only replaces the "Loading…" placeholder once a
  // world snapshot arrived → the GraphQL POST resolved under the prefix.
  await expect(page.getByText(/Devices in |No active context/)).toBeVisible();

  // Subscriptions → the WebSocket upgrade resolved under the prefix.
  await page.getByRole("button", { name: "Traffic", exact: true }).click();
  await expect(page.getByText("● live")).toBeVisible({ timeout: 10_000 });

  expect(failures, `browser errors on ${url}`).toEqual([]);
  return requested;
}

test("works behind a proxy that strips the subpath", async ({ page }) => {
  const requested = await walkTheApp(page, `http://127.0.0.1:${PROXY_PORT}${PREFIX}/`);

  // Every app request must be prefixed — an unprefixed one only works
  // by accident when the app is served from the root.
  const unprefixed = requested.filter(
    (u) => u.startsWith(`http://127.0.0.1:${PROXY_PORT}/`) && !u.includes(`${PROXY_PORT}${PREFIX}/`),
  );
  expect(unprefixed, "requests that escaped the subpath").toEqual([]);
  expect(requested.some((u) => u.endsWith(`${PREFIX}/graphql`))).toBe(true);
});

test("works when the backend itself is mounted at the subpath", async ({ page }) => {
  await walkTheApp(page, `http://127.0.0.1:${BASE_PATH_BACKEND_PORT}${PREFIX}/`);
});

test("a bare subpath redirects to the trailing-slash form", async ({ page }) => {
  await page.goto(`http://127.0.0.1:${BASE_PATH_BACKEND_PORT}${PREFIX}`);
  await expect(page).toHaveURL(`http://127.0.0.1:${BASE_PATH_BACKEND_PORT}${PREFIX}/`);
  await expect(page.getByRole("heading", { name: /Modbus Simulator/ })).toBeVisible();
});
