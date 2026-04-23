import { useCallback, useEffect, useMemo, useState } from "react";
import { request } from "./graphql";
import {
  ActiveContextQuery,
  CreateContextMutation,
  SwitchContextMutation,
} from "./queries";
import { DeviceTypesPage } from "./pages/DeviceTypes";
import { DevicesPage } from "./pages/Devices";
import { TransportPage } from "./pages/Transport";
import { VirtualSerialsPage } from "./pages/VirtualSerials";
import { TrafficPage } from "./pages/Traffic";
import { ContextsPage } from "./pages/Contexts";

export type WorldSnapshot = Awaited<ReturnType<typeof fetchWorld>>;

async function fetchWorld() {
  return await request(ActiveContextQuery);
}

type Tab =
  | "devices"
  | "device-types"
  | "contexts"
  | "transport"
  | "virtual-serials"
  | "traffic";

const TABS: { id: Tab; label: string }[] = [
  { id: "devices", label: "Devices" },
  { id: "device-types", label: "Device Types" },
  { id: "contexts", label: "Contexts" },
  { id: "transport", label: "Transport" },
  { id: "virtual-serials", label: "Virtual Serials" },
  { id: "traffic", label: "Traffic" },
];

export function App() {
  const [tab, setTab] = useState<Tab>("devices");
  const [world, setWorld] = useState<WorldSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const w = await fetchWorld();
      setWorld(w);
      setError(null);
    } catch (e) {
      setError((e as Error).message);
    }
  }, []);

  useEffect(() => {
    void refresh();
    const t = window.setInterval(() => void refresh(), 2000);
    return () => window.clearInterval(t);
  }, [refresh]);

  const activeContext = world?.activeContext ?? null;
  const contexts = world?.contexts ?? [];

  const onSwitch = useCallback(
    async (id: string) => {
      await request(SwitchContextMutation, { id });
      await refresh();
    },
    [refresh],
  );

  const onCreateContext = useCallback(async () => {
    const name = window.prompt("New context name?");
    if (!name) return;
    await request(CreateContextMutation, { name });
    await refresh();
  }, [refresh]);

  const page = useMemo(() => {
    if (!world) return null;
    switch (tab) {
      case "devices":
        return <DevicesPage world={world} onRefresh={refresh} />;
      case "device-types":
        return <DeviceTypesPage world={world} onRefresh={refresh} />;
      case "contexts":
        return <ContextsPage world={world} onRefresh={refresh} />;
      case "transport":
        return <TransportPage world={world} onRefresh={refresh} />;
      case "virtual-serials":
        return <VirtualSerialsPage world={world} onRefresh={refresh} />;
      case "traffic":
        return <TrafficPage />;
    }
  }, [tab, world, refresh]);

  return (
    <>
      <header className="app">
        <h1>⚙️ Modbus Simulator</h1>
        <select
          value={activeContext?.id ?? ""}
          onChange={(e) => void onSwitch(e.target.value)}
          title="Active context"
        >
          {contexts.length === 0 && <option value="">(no contexts)</option>}
          {contexts.map((c) => (
            <option key={c.id} value={c.id}>
              {c.name}
            </option>
          ))}
        </select>
        <button onClick={() => void onCreateContext()}>+ Context</button>
        <div className="spacer" />
        <nav>
          {TABS.map((t) => (
            <button
              key={t.id}
              className={t.id === tab ? "active" : ""}
              onClick={() => setTab(t.id)}
            >
              {t.label}
            </button>
          ))}
        </nav>
      </header>
      <main>
        {error && <div className="panel error">Error: {error}</div>}
        {!world && !error && <div className="panel muted">Loading…</div>}
        {page}
      </main>
    </>
  );
}
