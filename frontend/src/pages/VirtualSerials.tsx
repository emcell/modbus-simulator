import { useCallback, useState } from "react";
import { request } from "../graphql";
import { CreateVirtualSerialMutation, RemoveVirtualSerialMutation } from "../queries";
import type { WorldSnapshot } from "../App";

export function VirtualSerialsPage({
  world,
  onRefresh,
}: {
  world: WorldSnapshot;
  onRefresh: () => Promise<void>;
}) {
  const [symlink, setSymlink] = useState("");

  const create = useCallback(async () => {
    const path = symlink.trim() || null;
    await request(CreateVirtualSerialMutation, { symlinkPath: path });
    setSymlink("");
    await onRefresh();
  }, [symlink, onRefresh]);

  const remove = useCallback(
    async (id: string) => {
      await request(RemoveVirtualSerialMutation, { id });
      await onRefresh();
    },
    [onRefresh],
  );

  return (
    <div className="stack">
      <div className="panel">
        <h2>Virtual Serial Ports (PTY pairs)</h2>
        <p className="muted">
          Only available on Linux and macOS. The simulator creates an `openpty(3)` pair — it holds
          the master fd internally and exposes the slave device path (optionally aliased via a
          symlink you choose). Point your test app at the slave path, and configure the simulator's
          RTU transport to talk to the same path.
        </p>
        <div className="row">
          <input
            className="grow"
            value={symlink}
            onChange={(e) => setSymlink(e.target.value)}
            placeholder="Optional symlink path, e.g. /tmp/modsim-a"
          />
          <button className="primary" onClick={() => void create()}>
            + Create PTY
          </button>
        </div>
      </div>
      <div className="panel">
        <table>
          <thead>
            <tr>
              <th>Slave path</th>
              <th>Symlink</th>
              <th>Status</th>
              <th className="narrow"></th>
            </tr>
          </thead>
          <tbody>
            {world.virtualSerials.length === 0 && (
              <tr>
                <td colSpan={4} className="muted">
                  No virtual serial ports yet.
                </td>
              </tr>
            )}
            {world.virtualSerials.map((v) => (
              <tr key={v.id}>
                <td>
                  <code>{v.slavePath}</code>
                </td>
                <td>
                  {v.symlinkPath ? <code>{v.symlinkPath}</code> : <span className="muted">—</span>}
                </td>
                <td>
                  {v.inUse ? (
                    <span className="tag active">in use by RTU</span>
                  ) : (
                    <span className="tag">idle</span>
                  )}
                </td>
                <td className="narrow">
                  <button className="danger" onClick={() => void remove(v.id)}>
                    Remove
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <div className="panel">
        <h3>Windows users</h3>
        <p className="muted">
          Windows has no userspace equivalent to <code>openpty(3)</code>, so this simulator cannot
          create virtual serial ports on Windows directly. Use{" "}
          <a href="https://com0com.sourceforge.net/" target="_blank" rel="noreferrer">
            com0com
          </a>{" "}
          (open source, signed kernel driver) to create a pair of linked virtual COM ports.
        </p>
        <ol className="muted">
          <li>
            Download and install com0com from{" "}
            <a href="https://com0com.sourceforge.net/" target="_blank" rel="noreferrer">
              com0com.sourceforge.net
            </a>{" "}
            (requires administrator rights).
          </li>
          <li>
            Open the <code>Setup Command Prompt</code> shipped with com0com and run{" "}
            <code>install PortName=COM10 PortName=COM11</code> to create the pair{" "}
            <code>COM10</code> ↔ <code>COM11</code>.
          </li>
          <li>
            Configure this simulator's RTU transport to use one side (e.g. <code>COM10</code>) and
            point your client application at the other side (<code>COM11</code>).
          </li>
        </ol>
      </div>
    </div>
  );
}
