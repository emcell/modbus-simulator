import { useCallback, useEffect, useRef, useState, type FormEvent } from "react";
import { request } from "../graphql";
import {
  CreateDeviceMutation,
  DeleteDeviceMutation,
  SetRegisterValueMutation,
} from "../queries";
import type { WorldSnapshot } from "../App";
import { Modal } from "../components/Modal";
import { CopyButton } from "../components/CopyButton";
import { buildExamples } from "../mbpollExamples";

export function DevicesPage({
  world,
  onRefresh,
}: {
  world: WorldSnapshot;
  onRefresh: () => Promise<void>;
}) {
  const active = world.activeContext;
  const [selectedId, setSelectedId] = useState<string | null>(active?.devices[0]?.id ?? null);
  const [modalOpen, setModalOpen] = useState(false);

  const openCreateModal = useCallback(() => {
    if (world.deviceTypes.length === 0) {
      window.alert("Create a device type first.");
      return;
    }
    setModalOpen(true);
  }, [world.deviceTypes.length]);

  const handleCreated = useCallback(
    async (id: string) => {
      setSelectedId(id);
      setModalOpen(false);
      await onRefresh();
    },
    [onRefresh],
  );

  const deleteDevice = useCallback(
    async (id: string, name: string) => {
      if (!window.confirm(`Delete device "${name}"?`)) return;
      await request(DeleteDeviceMutation, { id });
      if (selectedId === id) setSelectedId(null);
      await onRefresh();
    },
    [selectedId, onRefresh],
  );

  if (!active) return <div className="panel muted">No active context.</div>;

  const selected = active.devices.find((d) => d.id === selectedId) ?? null;

  return (
    <div className="stack">
      <Modal open={modalOpen} title="New device" onClose={() => setModalOpen(false)}>
        <NewDeviceForm
          deviceTypes={world.deviceTypes}
          existingSlaveIds={active.devices.map((d) => d.slaveId)}
          onCancel={() => setModalOpen(false)}
          onCreated={handleCreated}
        />
      </Modal>
      <div className="panel">
        <div className="row">
          <h2 className="grow">Devices in “{active.name}”</h2>
          <button onClick={openCreateModal}>+ New</button>
        </div>
        <table>
          <thead>
            <tr>
              <th>Name</th>
              <th>Slave ID</th>
              <th>Type</th>
              <th>Overrides</th>
              <th>Last read</th>
              <th>Last write</th>
              <th className="narrow">Actions</th>
            </tr>
          </thead>
          <tbody>
            {active.devices.map((d) => {
              const dtName =
                world.deviceTypes.find((t) => t.id === d.deviceTypeId)?.name ?? "?";
              return (
                <tr
                  key={d.id}
                  onClick={() => setSelectedId(d.id)}
                  style={{
                    cursor: "pointer",
                    background: d.id === selectedId ? "rgba(76,196,255,0.08)" : undefined,
                  }}
                >
                  <td>{d.name}</td>
                  <td>{d.slaveId}</td>
                  <td>{dtName}</td>
                  <td>{d.hasBehaviorOverrides ? "yes" : "—"}</td>
                  <td>
                    <ActivityCell timestampMs={d.lastReadAtMs ?? null} kind="read" />
                  </td>
                  <td>
                    <ActivityCell timestampMs={d.lastWriteAtMs ?? null} kind="write" />
                  </td>
                  <td className="narrow">
                    <button
                      className="danger"
                      onClick={(e) => {
                        e.stopPropagation();
                        void deleteDevice(d.id, d.name);
                      }}
                    >
                      Delete
                    </button>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>

      {selected && <DeviceValueEditor device={selected} world={world} onRefresh={onRefresh} />}
    </div>
  );
}

function DeviceValueEditor({
  device,
  world,
  onRefresh,
}: {
  device: NonNullable<WorldSnapshot["activeContext"]>["devices"][number];
  world: WorldSnapshot;
  onRefresh: () => Promise<void>;
}) {
  const dt = world.deviceTypes.find((t) => t.id === device.deviceTypeId);
  const ctx = world.activeContext;
  const [exampleRegisterId, setExampleRegisterId] = useState<string | null>(null);

  if (!dt)
    return <div className="panel error">Device type not found for device {device.name}</div>;
  if (!ctx) return null;

  const valueFor = (registerId: string) => {
    const v = device.registerValues.find((rv) => rv.registerId === registerId);
    return v?.value;
  };

  const exampleRegister = dt.registers.find((r) => r.id === exampleRegisterId) ?? null;

  return (
    <div className="panel">
      <Modal
        open={exampleRegister !== null}
        title={exampleRegister ? `mbpoll examples — ${exampleRegister.name}` : ""}
        onClose={() => setExampleRegisterId(null)}
      >
        {exampleRegister && (
          <ExamplesView
            register={exampleRegister}
            slaveId={device.slaveId}
            tcp={ctx.tcp}
            rtu={ctx.rtu}
            virtualSerials={world.virtualSerials}
          />
        )}
      </Modal>
      <h2>Register values — {device.name}</h2>
      <p className="muted">
        Values shown are the current instance state. Editing here changes only this device
        instance; the device type's default is untouched. Click ℹ on any row for mbpoll read/write
        examples.
      </p>
      <table>
        <thead>
          <tr>
            <th>Kind</th>
            <th>Addr</th>
            <th>Name</th>
            <th>Type</th>
            <th>Encoding</th>
            <th>Value</th>
            <th>Default</th>
            <th>Activity</th>
            <th className="narrow"></th>
          </tr>
        </thead>
        <tbody>
          {dt.registers.map((r) => {
            const current = valueFor(r.id);
            const activity = device.registerActivity.find(
              (a) => a.registerId === r.id,
            );
            return (
              <ValueRow
                key={r.id}
                deviceId={device.id}
                register={r}
                current={current?.value ?? r.defaultValue.value}
                defaultStr={r.defaultValue.value}
                currentType={current?.dataType ?? r.defaultValue.dataType}
                lastReadAtMs={activity?.lastReadAtMs ?? null}
                lastWriteAtMs={activity?.lastWriteAtMs ?? null}
                onRefresh={onRefresh}
                onShowExamples={() => setExampleRegisterId(r.id)}
              />
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

function ExamplesView({
  register,
  slaveId,
  tcp,
  rtu,
  virtualSerials,
}: {
  register: WorldSnapshot["deviceTypes"][number]["registers"][number];
  slaveId: number;
  tcp: NonNullable<WorldSnapshot["activeContext"]>["tcp"];
  rtu: NonNullable<WorldSnapshot["activeContext"]>["rtu"];
  virtualSerials: WorldSnapshot["virtualSerials"];
}) {
  const [tab, setTab] = useState<"tcp" | "rtu">("tcp");

  // Pick a sensible default RTU device path: prefer an existing virtual
  // serial (symlink if available, else the slave path) over the configured
  // physical device, falling back to /dev/ttyUSB0.
  const firstVs = virtualSerials[0];
  const defaultDevice =
    firstVs?.symlinkPath ?? firstVs?.slavePath ?? rtu.device ?? "/dev/ttyUSB0";

  // RTU settings are tweakable in the modal so the generated commands match
  // whatever the user's mbpoll test setup actually uses. Defaults: 9600 8N1.
  const [rtuCfg, setRtuCfg] = useState({
    baudRate: rtu.baudRate > 0 ? rtu.baudRate : 9600,
    dataBits: rtu.dataBits > 0 ? rtu.dataBits : 8,
    stopBits: rtu.stopBits > 0 ? rtu.stopBits : 1,
    parity: (rtu.parity || "N").toUpperCase().charAt(0) as "N" | "E" | "O",
    device: defaultDevice,
  });

  const examples = buildExamples(register, slaveId, tcp, {
    enabled: true,
    device: rtuCfg.device,
    baudRate: rtuCfg.baudRate,
    dataBits: rtuCfg.dataBits,
    stopBits: rtuCfg.stopBits,
    parity: rtuCfg.parity,
    virtualSerialId: null,
  });
  const list = tab === "tcp" ? examples.tcp : examples.rtu;

  return (
    <div className="stack">
      <div className="row">
        <button
          className={tab === "tcp" ? "primary" : ""}
          onClick={() => setTab("tcp")}
        >
          Modbus TCP
        </button>
        <button
          className={tab === "rtu" ? "primary" : ""}
          onClick={() => setTab("rtu")}
        >
          Modbus RTU
        </button>
      </div>

      {tab === "rtu" && (
        <div className="panel" style={{ marginBottom: 0 }}>
          <div className="grid">
            <label>
              Device
              <input
                value={rtuCfg.device}
                onChange={(e) => setRtuCfg((c) => ({ ...c, device: e.target.value }))}
                placeholder="/dev/ttyUSB0"
              />
            </label>
            <label>
              Baud rate
              <select
                value={rtuCfg.baudRate}
                onChange={(e) =>
                  setRtuCfg((c) => ({ ...c, baudRate: parseInt(e.target.value, 10) }))
                }
              >
                {[1200, 2400, 4800, 9600, 19200, 38400, 57600, 115200].map((b) => (
                  <option key={b} value={b}>
                    {b}
                  </option>
                ))}
              </select>
            </label>
            <label>
              Data bits
              <select
                value={rtuCfg.dataBits}
                onChange={(e) =>
                  setRtuCfg((c) => ({ ...c, dataBits: parseInt(e.target.value, 10) }))
                }
              >
                {[7, 8].map((d) => (
                  <option key={d} value={d}>
                    {d}
                  </option>
                ))}
              </select>
            </label>
            <label>
              Parity
              <select
                value={rtuCfg.parity}
                onChange={(e) =>
                  setRtuCfg((c) => ({ ...c, parity: e.target.value as "N" | "E" | "O" }))
                }
              >
                <option value="N">None</option>
                <option value="E">Even</option>
                <option value="O">Odd</option>
              </select>
            </label>
            <label>
              Stop bits
              <select
                value={rtuCfg.stopBits}
                onChange={(e) =>
                  setRtuCfg((c) => ({ ...c, stopBits: parseInt(e.target.value, 10) }))
                }
              >
                {[1, 2].map((s) => (
                  <option key={s} value={s}>
                    {s}
                  </option>
                ))}
              </select>
            </label>
          </div>
        </div>
      )}

      <p className="muted">
        Replace <code>&lt;value&gt;</code> placeholders with the value you want to write. All
        examples use 0-based (PDU-style) addressing (<code>-0</code>) and exit after one poll
        (<code>-1</code>).
      </p>
      {list.map((ex, i) => (
        <div key={i} className="stack" style={{ gap: "0.25rem" }}>
          <div className="row">
            <strong className="grow">{ex.title}</strong>
            <CopyButton text={ex.command} />
          </div>
          <pre className="traffic" style={{ maxHeight: "none" }}>
            {ex.command}
          </pre>
          {ex.note && <div className="muted">{ex.note}</div>}
        </div>
      ))}

      <details className="mbpoll-legend">
        <summary>What do the flags mean?</summary>
        <table className="legend-table">
          <tbody>
            <tr>
              <th>
                <code>-m tcp|rtu</code>
              </th>
              <td>Protocol mode. RTU uses a serial port; TCP uses a hostname/port.</td>
            </tr>
            <tr>
              <th>
                <code>-a &lt;id&gt;</code>
              </th>
              <td>Slave / unit id (1–247 for RTU, 0–255 for TCP).</td>
            </tr>
            {tab === "tcp" && (
              <tr>
                <th>
                  <code>-p &lt;port&gt;</code>
                </th>
                <td>TCP port (Modbus default 502).</td>
              </tr>
            )}
            <tr>
              <th>
                <code>-0</code>
              </th>
              <td>
                0-based (PDU-style) addressing. Without it mbpoll subtracts 1, so{" "}
                <code>-r 1</code> would point at modbus address 0.
              </td>
            </tr>
            <tr>
              <th>
                <code>-1</code>
              </th>
              <td>Poll once and exit (omit for continuous polling on an interval).</td>
            </tr>
            <tr>
              <th>
                <code>-r &lt;addr&gt;</code>
              </th>
              <td>Start address of the register / coil range.</td>
            </tr>
            <tr>
              <th>
                <code>-c &lt;n&gt;</code>
              </th>
              <td>How many values to read (omitted when writing).</td>
            </tr>
            <tr>
              <th>
                <code>-t &lt;type&gt;</code>
              </th>
              <td>
                Register table + display format:
                <br />
                <code>0</code> coils (FC 01 / 05 / 15) &nbsp;·&nbsp;{" "}
                <code>1</code> discrete inputs (FC 02) &nbsp;·&nbsp;{" "}
                <code>3</code> input registers (FC 04) &nbsp;·&nbsp;{" "}
                <code>4</code> holding registers (FC 03 / 06 / 16)
                <br />
                Optional suffix: <code>:hex</code>, <code>:int</code> (32-bit signed, spans 2
                registers), <code>:float</code> (32-bit IEEE-754, spans 2 registers),{" "}
                <code>:string</code> (ASCII, spans N registers).
              </td>
            </tr>
            <tr>
              <th>
                <code>-B</code>
              </th>
              <td>
                Big-endian word order for 32-bit types. Without it, mbpoll uses word-swapped
                order — that's what “<code>BIG_ENDIAN_WORD_SWAP</code>” means in the simulator.
              </td>
            </tr>
            {tab === "rtu" && (
              <>
                <tr>
                  <th>
                    <code>-b &lt;baud&gt;</code>
                  </th>
                  <td>Serial baud rate.</td>
                </tr>
                <tr>
                  <th>
                    <code>-d &lt;n&gt;</code>
                  </th>
                  <td>Data bits per character (5–8; usually 8).</td>
                </tr>
                <tr>
                  <th>
                    <code>-s &lt;n&gt;</code>
                  </th>
                  <td>Stop bits (1 or 2).</td>
                </tr>
                <tr>
                  <th>
                    <code>-P none|even|odd</code>
                  </th>
                  <td>Parity.</td>
                </tr>
              </>
            )}
          </tbody>
        </table>
      </details>
    </div>
  );
}

function ValueRow({
  deviceId,
  register,
  current,
  defaultStr,
  currentType,
  lastReadAtMs,
  lastWriteAtMs,
  onRefresh,
  onShowExamples,
}: {
  deviceId: string;
  register: WorldSnapshot["deviceTypes"][number]["registers"][number];
  current: string;
  defaultStr: string;
  currentType: string;
  lastReadAtMs: string | null;
  lastWriteAtMs: string | null;
  onRefresh: () => Promise<void>;
  onShowExamples: () => void;
}) {
  const [value, setValue] = useState(current);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const save = useCallback(async () => {
    setBusy(true);
    setErr(null);
    try {
      const type = register.kind === "COIL" || register.kind === "DISCRETE" ? "BOOL" : register.dataType;
      await request(SetRegisterValueMutation, {
        deviceId,
        registerId: register.id,
        value: { dataType: type, value },
      });
      await onRefresh();
    } catch (e) {
      setErr((e as Error).message);
    } finally {
      setBusy(false);
    }
  }, [deviceId, register, value, onRefresh]);

  return (
    <tr>
      <td>{register.kind}</td>
      <td>{register.address}</td>
      <td>{register.name}</td>
      <td>{currentType}</td>
      <td>{register.encoding}</td>
      <td>
        <div className="row">
          <input value={value} onChange={(e) => setValue(e.target.value)} />
          <button disabled={busy || value === current} onClick={() => void save()}>
            Save
          </button>
        </div>
        {err && <div className="error">{err}</div>}
      </td>
      <td>
        <code className="muted">{defaultStr}</code>
      </td>
      <td>
        <div className="row" style={{ gap: "0.4rem" }}>
          <ActivityCell timestampMs={lastReadAtMs} kind="read" />
          <ActivityCell timestampMs={lastWriteAtMs} kind="write" />
        </div>
      </td>
      <td className="narrow">
        <button
          type="button"
          onClick={onShowExamples}
          title="Show mbpoll CLI examples"
          aria-label={`mbpoll examples for ${register.name}`}
        >
          ℹ
        </button>
      </td>
    </tr>
  );
}

function NewDeviceForm({
  deviceTypes,
  existingSlaveIds,
  onCancel,
  onCreated,
}: {
  deviceTypes: WorldSnapshot["deviceTypes"];
  existingSlaveIds: number[];
  onCancel: () => void;
  onCreated: (id: string) => Promise<void>;
}) {
  const [name, setName] = useState("");
  // Pick a free slave id by default.
  const defaultSlaveId = (() => {
    for (let n = 1; n <= 247; n++) {
      if (!existingSlaveIds.includes(n)) return n;
    }
    return 1;
  })();
  const [slaveId, setSlaveId] = useState(defaultSlaveId);
  const [deviceTypeId, setDeviceTypeId] = useState<string>(deviceTypes[0]?.id ?? "");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  // Focus the name field when the modal opens.
  useEffect(() => {
    const el = document.getElementById("new-device-name");
    (el as HTMLInputElement | null)?.focus();
  }, []);

  const slaveConflict = existingSlaveIds.includes(slaveId);
  const nameTrimmed = name.trim();
  const canSubmit =
    !busy && nameTrimmed.length > 0 && !slaveConflict && slaveId >= 1 && slaveId <= 247 && !!deviceTypeId;

  const submit = useCallback(
    async (e?: FormEvent) => {
      e?.preventDefault();
      if (!canSubmit) return;
      setBusy(true);
      setErr(null);
      try {
        const res = await request(CreateDeviceMutation, {
          input: { name: nameTrimmed, slaveId, deviceTypeId },
        });
        await onCreated(res.createDevice.id);
      } catch (ex) {
        setErr((ex as Error).message);
      } finally {
        setBusy(false);
      }
    },
    [canSubmit, nameTrimmed, slaveId, deviceTypeId, onCreated],
  );

  return (
    <form onSubmit={submit} className="stack">
      <label>
        Name
        <input
          id="new-device-name"
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="e.g. evse-1"
        />
      </label>
      <label>
        Slave ID
        <input
          type="number"
          min={1}
          max={247}
          value={slaveId}
          onChange={(e) => setSlaveId(parseInt(e.target.value, 10) || 0)}
        />
        {slaveConflict && (
          <span className="error">Slave ID {slaveId} is already used in this context.</span>
        )}
      </label>
      <label>
        Device type
        <select value={deviceTypeId} onChange={(e) => setDeviceTypeId(e.target.value)}>
          {deviceTypes.map((t) => (
            <option key={t.id} value={t.id}>
              {t.name} {t.description ? `— ${t.description}` : ""}
            </option>
          ))}
        </select>
      </label>
      {err && <div className="error">{err}</div>}
      <div className="modal-footer">
        <button type="button" onClick={onCancel} disabled={busy}>
          Cancel
        </button>
        <button type="submit" className="primary" disabled={!canSubmit}>
          {busy ? "Creating…" : "Create"}
        </button>
      </div>
    </form>
  );
}

function ActivityCell({
  timestampMs,
  kind,
}: {
  timestampMs: string | null;
  kind: "read" | "write";
}) {
  // Pulse briefly when the timestamp changes.
  const prev = useRef<string | null>(null);
  const [pulseSeq, setPulseSeq] = useState(0);
  useEffect(() => {
    if (timestampMs && timestampMs !== prev.current) {
      if (prev.current !== null) {
        setPulseSeq((n) => n + 1);
      }
      prev.current = timestampMs;
    }
  }, [timestampMs]);

  // Re-render every second so the relative age updates.
  const [, setTick] = useState(0);
  useEffect(() => {
    const id = window.setInterval(() => setTick((n) => n + 1), 1000);
    return () => window.clearInterval(id);
  }, []);

  if (!timestampMs) {
    return <span className="muted">—</span>;
  }
  const ms = Number(timestampMs);
  const ageMs = Date.now() - ms;
  return (
    <span title={new Date(ms).toLocaleTimeString()}>
      <span
        key={pulseSeq}
        className={`pulse-dot ${kind}`}
        aria-label={`${kind} activity`}
      />
      {formatAge(ageMs)}
    </span>
  );
}

function formatAge(ms: number): string {
  if (ms < 0) return "now";
  if (ms < 2000) return "just now";
  if (ms < 60_000) return `${Math.floor(ms / 1000)}s ago`;
  if (ms < 3_600_000) return `${Math.floor(ms / 60_000)}m ago`;
  return `${Math.floor(ms / 3_600_000)}h ago`;
}
