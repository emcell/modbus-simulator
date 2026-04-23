import { useCallback } from "react";
import { request } from "../graphql";
import {
  CreateContextMutation,
  DeleteContextMutation,
  ExportContextMutation,
  ImportContextMutation,
  SwitchContextMutation,
} from "../queries";
import type { WorldSnapshot } from "../App";

export function ContextsPage({
  world,
  onRefresh,
}: {
  world: WorldSnapshot;
  onRefresh: () => Promise<void>;
}) {
  const createContext = useCallback(async () => {
    const name = window.prompt("Context name?");
    if (!name) return;
    await request(CreateContextMutation, { name });
    await onRefresh();
  }, [onRefresh]);

  const switchTo = useCallback(
    async (id: string) => {
      await request(SwitchContextMutation, { id });
      await onRefresh();
    },
    [onRefresh],
  );

  const del = useCallback(
    async (id: string, name: string) => {
      if (!window.confirm(`Delete context "${name}"? This removes its devices and transport settings.`))
        return;
      await request(DeleteContextMutation, { id });
      await onRefresh();
    },
    [onRefresh],
  );

  const exportOne = useCallback(async (id: string, name: string) => {
    const result = await request(ExportContextMutation, { id });
    const blob = new Blob([result.exportContext], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `context-${name}.json`;
    a.click();
    URL.revokeObjectURL(url);
  }, []);

  const importContext = useCallback(async () => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = "application/json";
    input.onchange = async () => {
      const file = input.files?.[0];
      if (!file) return;
      const data = await file.text();
      await request(ImportContextMutation, { data });
      await onRefresh();
    };
    input.click();
  }, [onRefresh]);

  return (
    <div className="stack">
      <div className="panel">
        <div className="row">
          <h2 className="grow">Contexts</h2>
          <button onClick={() => void createContext()}>+ New</button>
          <button onClick={() => void importContext()}>Import…</button>
        </div>
        <p className="muted">
          A context bundles the devices, per-device behavior overrides and transport settings for one
          test scenario. Switch seamlessly between scenarios — the active transport reconfigures itself.
        </p>
        <table>
          <thead>
            <tr>
              <th>Name</th>
              <th>Status</th>
              <th className="narrow">Actions</th>
            </tr>
          </thead>
          <tbody>
            {world.contexts.map((c) => (
              <tr key={c.id}>
                <td>{c.name}</td>
                <td>
                  {c.active ? (
                    <span className="tag active">active</span>
                  ) : (
                    <span className="tag">idle</span>
                  )}
                </td>
                <td className="narrow">
                  <div className="row">
                    {!c.active && (
                      <button onClick={() => void switchTo(c.id)}>Switch</button>
                    )}
                    <button onClick={() => void exportOne(c.id, c.name)}>Export</button>
                    <button className="danger" onClick={() => void del(c.id, c.name)}>
                      Delete
                    </button>
                  </div>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
