import { useEffect, useRef, useState } from "react";
import { subscribe, type ConnectionStatus } from "../subscriptions";

interface DecodedField {
  registerName: string;
  address: number;
  dataType: string;
  value: string;
}

interface TrafficEvent {
  direction: string;
  transport: string;
  slaveId: number;
  functionCode: number;
  bytesHex: string;
  timestampMs: string;
  summary: string;
  decoded: DecodedField[];
}

const MAX_EVENTS = 500;

export function TrafficPage() {
  const [events, setEvents] = useState<TrafficEvent[]>([]);
  const [paused, setPaused] = useState(false);
  const [status, setStatus] = useState<ConnectionStatus>({ state: "connecting" });
  const pausedRef = useRef(false);
  pausedRef.current = paused;

  useEffect(() => {
    const handle = subscribe<{ traffic: TrafficEvent }>({
      query: `subscription {
        traffic {
          direction transport slaveId functionCode bytesHex timestampMs
          summary
          decoded { registerName address dataType value }
        }
      }`,
      onStatus: setStatus,
      onData: (data) => {
        if (pausedRef.current) return;
        setEvents((cur) => {
          const next = [data.traffic, ...cur];
          return next.length > MAX_EVENTS ? next.slice(0, MAX_EVENTS) : next;
        });
      },
    });
    return () => handle.close();
  }, []);

  const statusBadge = (() => {
    switch (status.state) {
      case "open":
        return <span className="tag active" title="WebSocket subscription open">● live</span>;
      case "connecting":
        return <span className="tag" title="Connecting to /graphql/ws">◌ connecting…</span>;
      case "closed":
        return (
          <span
            className="tag"
            title={`Socket closed: ${status.reason}`}
            style={{
              background: "rgba(255,107,107,0.18)",
              color: "#ff9e9e",
              borderColor: "#ff6b6b",
            }}
          >
            ✗ disconnected ({status.reason})
          </span>
        );
    }
  })();

  return (
    <div className="stack">
      <div className="panel">
        <div className="row">
          <h2 className="grow">Live Traffic</h2>
          {statusBadge}
          <span className="muted" style={{ marginLeft: "0.5rem" }}>
            {events.length} frame{events.length === 1 ? "" : "s"}
          </span>
          <button onClick={() => setPaused((p) => !p)}>
            {paused ? "▶ Resume" : "⏸ Pause"}
          </button>
          <button onClick={() => setEvents([])}>Clear</button>
        </div>
        <p className="muted">
          Every Modbus request/response flowing through TCP or RTU appears here in real time via
          GraphQL subscription. If the badge above shows <code>live</code> but nothing is arriving,
          check the Transport tab to make sure a listener is actually bound.
        </p>
        <pre className="traffic">
          {events.length === 0 ? (
            <span className="muted">
              {status.state === "open"
                ? "Connected — waiting for frames…"
                : status.state === "connecting"
                  ? "Opening subscription…"
                  : `Disconnected: ${status.state === "closed" ? status.reason : ""}`}
            </span>
          ) : (
            events.map((e, i) => (
              <div key={`${e.timestampMs}-${i}`} className={`traffic-line ${e.direction}`}>
                <div>
                  {formatTimestamp(e.timestampMs)} [{e.transport.toUpperCase()}] slave=
                  {e.slaveId}{" "}
                  {e.direction === "in" ? "→" : "←"}{" "}
                  <strong>{e.summary || `fc=${e.functionCode}`}</strong>
                </div>
                {e.decoded.length > 0 && (
                  <div className="traffic-decoded">
                    {e.decoded.map((d, j) => (
                      <span key={j} className="traffic-field">
                        {d.registerName}@{d.address} <em>({d.dataType})</em> ={" "}
                        <code>{d.value}</code>
                      </span>
                    ))}
                  </div>
                )}
                <div className="traffic-hex">{e.bytesHex}</div>
              </div>
            ))
          )}
        </pre>
      </div>
    </div>
  );
}

function formatTimestamp(ms: string): string {
  const n = Number(ms);
  if (!Number.isFinite(n)) return ms;
  const d = new Date(n);
  return d.toISOString().slice(11, 23);
}
