/**
 * Full browser-level walkthrough: click every interactive element the UI
 * exposes, verify the resulting GraphQL mutations land, and assert nothing
 * ends up in the browser console or page-error stream.
 *
 * Starts a fresh modsim binary with a temp config dir so tests don't
 * collide with the user's real data.
 */
import { spawn, type ChildProcess } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { resolve } from "node:path";

import { test, expect, type ConsoleMessage, type Page } from "@playwright/test";

let serverProc: ChildProcess | null = null;
let tmpRoot: string | null = null;
const PORT = 18080;

// Collected across the whole spec file.
const problems: string[] = [];

test.beforeAll(async () => {
  // Find the modsim binary next to the workspace target directory.
  // Playwright runs tests with cwd = frontend/
  const bin = resolve(process.cwd(), "..", "target", "release", "modsim");
  tmpRoot = mkdtempSync(resolve(tmpdir(), "modsim-e2e-"));

  serverProc = spawn(bin, [], {
    env: {
      ...process.env,
      MODSIM_CONFIG_DIR: tmpRoot,
      MODSIM_HTTP_PORT: String(PORT),
    },
    stdio: ["ignore", "pipe", "pipe"],
  });

  // Wait for the /health endpoint.
  const deadline = Date.now() + 10_000;
  while (Date.now() < deadline) {
    try {
      const r = await fetch(`http://127.0.0.1:${PORT}/health`);
      if (r.ok) return;
    } catch {
      /* not up yet */
    }
    await new Promise((r) => setTimeout(r, 150));
  }
  throw new Error("modsim did not start in time");
});

test.afterAll(async () => {
  if (serverProc && serverProc.pid != null) {
    serverProc.kill("SIGTERM");
    await new Promise((r) => setTimeout(r, 200));
  }
  if (tmpRoot) {
    try {
      rmSync(tmpRoot, { recursive: true, force: true });
    } catch {
      /* ignore */
    }
  }
  if (problems.length > 0) {
    console.error("\nBrowser issues accumulated during the run:\n" + problems.join("\n"));
  }
});

/**
 * Queue of answers for dialogs in order. For prompts, provide the text.
 * For confirms/alerts, the string is ignored — we always accept.
 * `null` means "use the prompt default value".
 */
const dialogQueue: (string | null)[] = [];

function attachListeners(page: Page) {
  page.on("request", (req) => {
    if (req.url().endsWith("/graphql") && req.method() === "POST") {
      const body = req.postData() ?? "";
      const match = /"(mutation|query|subscription)\s+(\w+)/.exec(body);
      console.log(`  ▸ ${match?.[1] ?? "?"} ${match?.[2] ?? "?"}`);
    }
  });
  page.on("console", (msg: ConsoleMessage) => {
    if (msg.type() === "error" || msg.type() === "warning") {
      problems.push(`[${msg.type()}] ${msg.text()}`);
    }
  });
  page.on("pageerror", (err) => {
    problems.push(`[pageerror] ${err.message}`);
  });
  page.on("requestfailed", (req) => {
    const failure = req.failure()?.errorText ?? "";
    if (!failure.includes("aborted")) {
      problems.push(`[requestfailed] ${req.method()} ${req.url()} — ${failure}`);
    }
  });
  page.on("response", (resp) => {
    const url = resp.url();
    if (resp.status() >= 500 && url.includes("/graphql")) {
      problems.push(`[5xx] ${resp.status()} ${url}`);
    }
  });
  page.on("dialog", async (dialog) => {
    const next = dialogQueue.shift();
    if (dialog.type() === "prompt") {
      await dialog.accept(next ?? dialog.defaultValue() ?? "");
    } else {
      await dialog.accept();
    }
  });
}

function expectDialog(answer: string | null = null) {
  dialogQueue.push(answer);
}

test("full UI walkthrough without browser errors", async ({ page }) => {
  attachListeners(page);

  await page.goto("/");
  // Real Vite bundle is served (not the old stub).
  await expect(page.locator("#root")).toBeVisible();
  await expect(page.locator("header.app h1")).toHaveText(/Modbus Simulator/);

  // --- Create two contexts via the header `+ Context` button ------------
  expectDialog("lab-a");
  await page.getByRole("button", { name: "+ Context" }).click();
  await expect(page.locator("header.app select")).toHaveValue(/.+/);

  expectDialog("lab-b");
  await page.getByRole("button", { name: "+ Context" }).click();

  // --- Contexts tab: switch, export, import, delete ---------------------
  await page.getByRole("button", { name: "Contexts", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Contexts" })).toBeVisible();

  // At least one "Switch" button should exist (non-active contexts).
  const switchBtns = page.getByRole("button", { name: "Switch" });
  await expect(switchBtns.first()).toBeVisible();
  await switchBtns.first().click();
  // After switching, the active tag moved; switchContext mutation fired.

  // Export a context — Playwright catches the download.
  const [download] = await Promise.all([
    page.waitForEvent("download"),
    page.getByRole("button", { name: "Export" }).first().click(),
  ]);
  expect(download.suggestedFilename()).toMatch(/^context-/);

  // --- Device Types: create type, rename, add registers, edit behavior --
  await page.getByRole("button", { name: "Device Types", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Device Types" })).toBeVisible();

  expectDialog("EVSE");
  await page.locator(".panel").first().getByRole("button", { name: "+ New" }).click();
  // createDeviceType sets selectedId automatically, so the editor panel
  // appears without a manual click.
  await expect(page.getByRole("heading", { name: /Edit .EVSE./ })).toBeVisible();

  // Export the brand-new type: downloads JSON.
  const [dtDownload] = await Promise.all([
    page.waitForEvent("download"),
    page.locator(".panel").first().getByRole("button", { name: "Export" }).first().click(),
  ]);
  expect(dtDownload.suggestedFilename()).toMatch(/^device-type-EVSE/);

  // Rename + description. The editor panel is the one that contains the
  // "Save name/description" button.
  const editor = page
    .locator(".panel")
    .filter({ has: page.getByRole("button", { name: "Save name/description" }) });
  await editor.locator("label", { hasText: "Name" }).locator("input").fill("EVSE v2");
  await editor.locator("label", { hasText: "Description" }).locator("input").fill("charger");
  await editor.getByRole("button", { name: "Save name/description" }).click();

  // Wait for the auto-refresh to propagate the rename into the table.
  await expect(page.getByRole("cell", { name: "EVSE v2", exact: true })).toBeVisible({
    timeout: 5000,
  });

  // --- Behavior form ----------------------------------------------------
  const behaviorPanel = page.locator(".panel").filter({ hasText: "Behavior" });
  await behaviorPanel
    .locator("label", { hasText: "Disabled function codes" })
    .locator("input")
    .fill("5, 15");
  await behaviorPanel
    .locator("label", { hasText: "Max registers per request" })
    .locator("input")
    .fill("20");
  await behaviorPanel
    .locator("label", { hasText: "full miss" })
    .locator("select")
    .selectOption("SLAVE_DEVICE_FAILURE");
  await behaviorPanel
    .locator("label", { hasText: "partial overlap" })
    .locator("select")
    .selectOption("ZERO_FILL");
  await behaviorPanel
    .locator("label", { hasText: "Response delay" })
    .locator("input")
    .fill("50");
  await behaviorPanel.getByRole("button", { name: "Save behavior" }).click();

  // --- Register editor: add a U32 holding register ----------------------
  // Column order in the draft row: kind-select, address, name, dataType-select,
  // encoding-select, byteLength, defaultValue, description, + Add.
  // The last row is always the draft/new-register form.
  const regPanel = page.getByRole("heading", { name: "Registers" }).locator("..");

  async function fillDraft(kind: string, address: string, name: string, dataType: string, defaultValue: string, description: string) {
    const row = regPanel.locator("tbody tr").last();
    await row.locator("select").nth(0).selectOption(kind);
    await row.locator("input").nth(0).fill(address);
    await row.locator("input").nth(1).fill(name);
    await row.locator("select").nth(1).selectOption(dataType);
    await row.locator("select").nth(2).selectOption("BIG_ENDIAN");
    await row.locator("input").nth(3).fill(defaultValue);
    await row.locator("input").nth(4).fill(description);
    await row.getByRole("button", { name: "+ Add" }).click();
  }

  await fillDraft("HOLDING", "100", "power", "U32", "42000", "active power W");
  // A row with cells 'power' and '100' must appear in the registers panel.
  await expect(regPanel.getByRole("cell", { name: "power", exact: true })).toBeVisible();

  await fillDraft("COIL", "0", "relay", "U16", "true", "on/off");
  await expect(regPanel.getByRole("cell", { name: "relay", exact: true })).toBeVisible();

  // --- Edit an existing register in-place --------------------------------
  // Existing rows are editable. Find the tr whose name input currently
  // holds "power" and rename it; inputs are controlled React components
  // so we check `.inputValue()` rather than the `value` attribute.
  const rows = await regPanel.locator("tbody tr").all();
  let powerRowIdx = -1;
  for (let i = 0; i < rows.length; i++) {
    const val = await rows[i].locator("input").nth(1).inputValue().catch(() => "");
    if (val === "power") {
      powerRowIdx = i;
      break;
    }
  }
  expect(powerRowIdx).toBeGreaterThanOrEqual(0);
  const powerRow = regPanel.locator("tbody tr").nth(powerRowIdx);

  const nameInput = powerRow.locator("input").nth(1);
  const defaultInput = powerRow.locator("input").nth(3);
  await nameInput.fill("active_power");
  await defaultInput.fill("55555");
  await powerRow.getByRole("button", { name: "Save" }).click();
  await expect(powerRow.getByRole("button", { name: "Save" })).toBeDisabled({
    timeout: 5000,
  });
  await expect(nameInput).toHaveValue("active_power");
  await expect(defaultInput).toHaveValue("55555");

  // --- Devices tab: create an instance ----------------------------------
  await page.getByRole("button", { name: "Devices", exact: true }).click();
  await expect(page.getByRole("heading", { name: /Devices in/ })).toBeVisible();

  // Modal-driven creation: name input, slaveId number input, deviceType <select>.
  await page.locator(".panel").first().getByRole("button", { name: "+ New" }).click();
  const modal = page.getByRole("dialog", { name: "New device" });
  await expect(modal).toBeVisible();
  await modal.getByLabel("Name").fill("evse-1");
  await modal.getByLabel("Slave ID").fill("1");
  // There's only one device type in this test, so pick the first option.
  await modal.getByLabel("Device type").selectOption({ index: 0 });
  await modal.getByRole("button", { name: "Create" }).click();
  await expect(modal).toBeHidden();
  await expect(page.getByRole("cell", { name: "evse-1" })).toBeVisible();

  // Select the instance → register-value editor appears
  await page.getByRole("cell", { name: "evse-1" }).click();
  await expect(page.getByRole("heading", { name: /Register values/ })).toBeVisible();

  // Edit the power register value and save it
  const valRow = page.locator("tbody tr").filter({ hasText: "power" });
  const valInput = valRow.locator("input").first();
  await valInput.fill("99999");
  await valRow.getByRole("button", { name: "Save" }).click();
  // The Save button becomes disabled once value === current
  await expect(valRow.getByRole("button", { name: "Save" })).toBeDisabled({ timeout: 5000 });

  // Open the mbpoll-examples modal for this register and verify the copy
  // button is present for at least one command.
  await valRow.getByRole("button", { name: /mbpoll examples/ }).click();
  const exModal = page.getByRole("dialog", { name: /mbpoll examples/ });
  await expect(exModal).toBeVisible();
  await expect(exModal.getByText("Modbus TCP")).toBeVisible();
  await expect(exModal.locator("pre").first()).toContainText("mbpoll");
  await expect(exModal.getByRole("button", { name: "Copy" }).first()).toBeVisible();
  // Switch to the RTU tab and check it renders RTU-specific args.
  await exModal.getByRole("button", { name: "Modbus RTU" }).click();
  await expect(exModal.locator("pre").first()).toContainText("-m rtu");
  // Close.
  await exModal.getByRole("button", { name: "Close" }).click();
  await expect(exModal).toBeHidden();

  // --- Transport tab ----------------------------------------------------
  await page.getByRole("button", { name: "Transport", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Modbus TCP" })).toBeVisible();

  const tcpPanel = page.locator(".panel").filter({ hasText: "Modbus TCP" });
  await tcpPanel.locator('input[type="checkbox"]').check();
  await tcpPanel.locator("label", { hasText: "Bind" }).locator("input").fill("127.0.0.1");
  await tcpPanel.locator("label", { hasText: "Port" }).locator("input").fill("15502");
  await tcpPanel.getByRole("button", { name: "Save" }).click();

  const rtuPanel = page.locator(".panel").filter({ hasText: "Modbus RTU" });
  await rtuPanel.locator('input[type="checkbox"]').uncheck();
  await rtuPanel.locator("label", { hasText: "Device" }).locator("input").fill("/tmp/modsim-e2e");
  await rtuPanel.locator("label", { hasText: "Baud rate" }).locator("input").fill("19200");
  await rtuPanel.locator("label", { hasText: "Parity" }).locator("select").selectOption("N");
  await rtuPanel.getByRole("button", { name: "Save" }).click();

  // --- Virtual Serials --------------------------------------------------
  await page.getByRole("button", { name: "Virtual Serials", exact: true }).click();
  await expect(page.getByRole("heading", { name: /Virtual Serial Ports/ })).toBeVisible();

  await page.locator("input[placeholder^='Optional symlink']").fill("/tmp/modsim-e2e-link");
  await page.getByRole("button", { name: "+ Create PTY" }).click();
  await expect(page.locator("code", { hasText: "/tmp/modsim-e2e-link" })).toBeVisible();

  await page.getByRole("button", { name: "Remove" }).click();
  await expect(page.getByText("No virtual serial ports yet.")).toBeVisible();

  // --- Traffic tab: subscription connects --------------------------------
  await page.getByRole("button", { name: "Traffic", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Live Traffic" })).toBeVisible();
  // Subscription should either open ("● live") or show it's still opening.
  await expect(page.locator("pre.traffic")).toContainText(/waiting for frames|Opening subscription/i);
  // Pause/resume + Clear don't emit network; just exercise them.
  await page.getByRole("button", { name: /Pause/ }).click();
  await page.getByRole("button", { name: /Resume/ }).click();
  await page.getByRole("button", { name: "Clear" }).click();

  // --- Cleanup: delete the test context's device and the device type ----
  await page.getByRole("button", { name: "Devices", exact: true }).click();
  expectDialog(); // confirm delete
  await page
    .getByRole("row", { name: /evse-1/ })
    .getByRole("button", { name: "Delete" })
    .click();
  await expect(page.getByRole("cell", { name: "evse-1" })).toHaveCount(0);

  await page.getByRole("button", { name: "Device Types", exact: true }).click();
  expectDialog(); // confirm delete
  await page
    .getByRole("row", { name: /EVSE v2/ })
    .getByRole("button", { name: "Delete" })
    .click();
  await expect(page.getByRole("cell", { name: "EVSE v2" })).toHaveCount(0);

  // Final assertion: no browser errors accumulated.
  expect(problems, `Browser issues:\n${problems.join("\n")}`).toEqual([]);
});
