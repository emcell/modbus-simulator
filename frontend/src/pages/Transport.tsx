import { useCallback, useState } from "react";
import { request } from "../graphql";
import { ConfigureRtuMutation, ConfigureTcpMutation } from "../queries";
import type { WorldSnapshot } from "../App";

type StatusView = WorldSnapshot["transportStatus"]["tcp"];

export function TransportPage({
  world,
  onRefresh,
}: {
  world: WorldSnapshot;
  onRefresh: () => Promise<void>;
}) {
  const active = world.activeContext;
  if (!active) return <div className="panel muted">No active context.</div>;

  return (
    <div className="stack">
      <TcpPanel
        tcp={active.tcp}
        status={world.transportStatus.tcp}
        onRefresh={onRefresh}
      />
      <RtuPanel
        rtu={active.rtu}
        status={world.transportStatus.rtu}
        virtualSerials={world.virtualSerials}
        onRefresh={onRefresh}
      />
    </div>
  );
}

type Feedback = { kind: "ok"; message: string } | { kind: "err"; message: string } | null;

function StatusBadge({ status }: { status: StatusView }) {
  switch (status.state) {
    case "RUNNING":
      return (
        <span className="tag active" title={status.description}>
          ● running — {status.description}
        </span>
      );
    case "DISABLED":
      return <span className="tag">○ disabled</span>;
    case "ERROR":
      return (
        <span
          className="tag"
          style={{ background: "rgba(255,107,107,0.18)", color: "#ff9e9e", borderColor: "#ff6b6b" }}
          title={status.error ?? ""}
        >
          ✗ error — {status.description}
        </span>
      );
  }
}

function FeedbackBanner({ feedback }: { feedback: Feedback }) {
  if (!feedback) return null;
  const color = feedback.kind === "ok" ? "var(--accent-2)" : "var(--danger)";
  return (
    <div
      className="muted"
      style={{
        padding: "0.5rem 0.75rem",
        borderLeft: `3px solid ${color}`,
        background: "rgba(255,255,255,0.02)",
        color: color,
      }}
    >
      {feedback.kind === "ok" ? "✓" : "✗"} {feedback.message}
    </div>
  );
}

function TcpPanel({
  tcp,
  status,
  onRefresh,
}: {
  tcp: NonNullable<WorldSnapshot["activeContext"]>["tcp"];
  status: StatusView;
  onRefresh: () => Promise<void>;
}) {
  const [enabled, setEnabled] = useState(tcp.enabled);
  const [bind, setBind] = useState(tcp.bind || "0.0.0.0");
  const [port, setPort] = useState(tcp.port || 502);
  const [feedback, setFeedback] = useState<Feedback>(null);
  const [busy, setBusy] = useState(false);

  const save = useCallback(async () => {
    setBusy(true);
    setFeedback(null);
    try {
      await request(ConfigureTcpMutation, { enabled, bind, port });
      await onRefresh();
      // Refresh brings back the latest transportStatus — but we also want
      // immediate feedback even before the world snapshot propagates.
      setFeedback({ kind: "ok", message: "Settings saved and applied." });
    } catch (e) {
      setFeedback({ kind: "err", message: (e as Error).message });
    } finally {
      setBusy(false);
    }
  }, [enabled, bind, port, onRefresh]);

  return (
    <div className="panel">
      <div className="row" style={{ marginBottom: "0.5rem" }}>
        <h2 className="grow" style={{ margin: 0 }}>
          Modbus TCP
        </h2>
        <StatusBadge status={status} />
      </div>
      {status.state === "ERROR" && status.error && (
        <FeedbackBanner feedback={{ kind: "err", message: status.error }} />
      )}
      <p className="muted">
        Changes are applied live — no restart needed. The badge above reflects whether the port is
        currently bound.
      </p>
      <div className="grid">
        <label>
          <span>
            <input
              type="checkbox"
              checked={enabled}
              onChange={(e) => setEnabled(e.target.checked)}
            />{" "}
            Enabled
          </span>
        </label>
        <label>
          Bind address
          <input value={bind} onChange={(e) => setBind(e.target.value)} placeholder="0.0.0.0" />
        </label>
        <label>
          Port
          <input
            type="number"
            value={port}
            onChange={(e) => setPort(parseInt(e.target.value, 10) || 0)}
          />
        </label>
      </div>
      <div style={{ marginTop: "0.5rem" }} className="row">
        <button className="primary" onClick={() => void save()} disabled={busy}>
          {busy ? "Saving…" : "Save"}
        </button>
        {feedback && <FeedbackBanner feedback={feedback} />}
      </div>
    </div>
  );
}

function RtuPanel({
  rtu,
  status,
  virtualSerials,
  onRefresh,
}: {
  rtu: NonNullable<WorldSnapshot["activeContext"]>["rtu"];
  status: StatusView;
  virtualSerials: WorldSnapshot["virtualSerials"];
  onRefresh: () => Promise<void>;
}) {
  const [enabled, setEnabled] = useState(rtu.enabled);
  const [device, setDevice] = useState(rtu.device);
  const [baudRate, setBaudRate] = useState(rtu.baudRate || 9600);
  const [parity, setParity] = useState(rtu.parity || "N");
  const [dataBits, setDataBits] = useState(rtu.dataBits || 8);
  const [stopBits, setStopBits] = useState(rtu.stopBits || 1);
  const [virtualSerialId, setVirtualSerialId] = useState<string>(
    rtu.virtualSerialId ?? "",
  );
  const useVirtual = virtualSerialId !== "";
  const [feedback, setFeedback] = useState<Feedback>(null);
  const [busy, setBusy] = useState(false);

  const save = useCallback(async () => {
    setBusy(true);
    setFeedback(null);
    try {
      await request(ConfigureRtuMutation, {
        enabled,
        device,
        baudRate,
        parity,
        dataBits,
        stopBits,
        virtualSerialId: useVirtual ? virtualSerialId : null,
      });
      await onRefresh();
      setFeedback({ kind: "ok", message: "Settings saved and applied." });
    } catch (e) {
      setFeedback({ kind: "err", message: (e as Error).message });
    } finally {
      setBusy(false);
    }
  }, [
    enabled,
    device,
    baudRate,
    parity,
    dataBits,
    stopBits,
    useVirtual,
    virtualSerialId,
    onRefresh,
  ]);

  return (
    <div className="panel">
      <div className="row" style={{ marginBottom: "0.5rem" }}>
        <h2 className="grow" style={{ margin: 0 }}>
          Modbus RTU
        </h2>
        <StatusBadge status={status} />
      </div>
      {status.state === "ERROR" && status.error && (
        <FeedbackBanner feedback={{ kind: "err", message: status.error }} />
      )}
      <p className="muted">
        Point the simulator at a physical serial port or a virtual PTY created on the “Virtual
        Serials” tab. Applied live on Save.
      </p>
      <div className="grid">
        <label>
          <span>
            <input
              type="checkbox"
              checked={enabled}
              onChange={(e) => setEnabled(e.target.checked)}
            />{" "}
            Enabled
          </span>
        </label>
        <label>
          Virtual serial
          <select
            value={virtualSerialId}
            onChange={(e) => setVirtualSerialId(e.target.value)}
          >
            <option value="">— none (use device path below) —</option>
            {virtualSerials.map((v) => (
              <option key={v.id} value={v.id}>
                {v.symlinkPath ?? v.slavePath}
              </option>
            ))}
          </select>
        </label>
        <label>
          Device
          <input
            value={device}
            onChange={(e) => setDevice(e.target.value)}
            disabled={useVirtual}
            title={useVirtual ? "Ignored while a virtual serial is selected" : undefined}
            placeholder="/dev/ttyUSB0 or /tmp/modsim-link"
          />
        </label>
        <label>
          Baud rate
          <input
            type="number"
            value={baudRate}
            onChange={(e) => setBaudRate(parseInt(e.target.value, 10) || 0)}
          />
        </label>
        <label>
          Parity
          <select value={parity} onChange={(e) => setParity(e.target.value)}>
            <option value="N">None</option>
            <option value="E">Even</option>
            <option value="O">Odd</option>
          </select>
        </label>
        <label>
          Data bits
          <select value={dataBits} onChange={(e) => setDataBits(parseInt(e.target.value, 10))}>
            <option>5</option>
            <option>6</option>
            <option>7</option>
            <option>8</option>
          </select>
        </label>
        <label>
          Stop bits
          <select value={stopBits} onChange={(e) => setStopBits(parseInt(e.target.value, 10))}>
            <option>1</option>
            <option>2</option>
          </select>
        </label>
      </div>
      <div style={{ marginTop: "0.5rem" }} className="row">
        <button className="primary" onClick={() => void save()} disabled={busy}>
          {busy ? "Saving…" : "Save"}
        </button>
        {feedback && <FeedbackBanner feedback={feedback} />}
      </div>
    </div>
  );
}
