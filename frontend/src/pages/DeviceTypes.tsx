import { useCallback, useEffect, useRef, useState } from "react";
import { request } from "../graphql";
import {
  CreateDeviceTypeMutation,
  DeleteDeviceTypeMutation,
  DeleteRegisterMutation,
  ExportDeviceTypeMutation,
  ImportDeviceTypeMutation,
  ImportVarmecoCsvMutation,
  RenameDeviceTypeMutation,
  UpdateBehaviorMutation,
  UpsertRegisterMutation,
} from "../queries";
import type { WorldSnapshot } from "../App";

type DataType = "U16" | "I16" | "U32" | "I32" | "U64" | "I64" | "F16" | "F32" | "F64" | "STRING";
type Encoding = "BIG_ENDIAN" | "LITTLE_ENDIAN" | "BIG_ENDIAN_WORD_SWAP" | "LITTLE_ENDIAN_WORD_SWAP";
type Kind = "HOLDING" | "INPUT" | "COIL" | "DISCRETE";
type Missing =
  | "ILLEGAL_DATA_ADDRESS"
  | "ILLEGAL_FUNCTION"
  | "SLAVE_DEVICE_FAILURE"
  | "TIMEOUT"
  | "ZERO_FILL";

const DATA_TYPES: DataType[] = ["U16", "I16", "U32", "I32", "U64", "I64", "F16", "F32", "F64", "STRING"];
const ENCODINGS: Encoding[] = [
  "BIG_ENDIAN",
  "LITTLE_ENDIAN",
  "BIG_ENDIAN_WORD_SWAP",
  "LITTLE_ENDIAN_WORD_SWAP",
];
const KINDS: Kind[] = ["HOLDING", "INPUT", "COIL", "DISCRETE"];
const MISSING: Missing[] = [
  "ILLEGAL_DATA_ADDRESS",
  "ILLEGAL_FUNCTION",
  "SLAVE_DEVICE_FAILURE",
  "TIMEOUT",
  "ZERO_FILL",
];

export function DeviceTypesPage({
  world,
  onRefresh,
}: {
  world: WorldSnapshot;
  onRefresh: () => Promise<void>;
}) {
  const [selectedId, setSelectedId] = useState<string | null>(
    world.deviceTypes[0]?.id ?? null,
  );
  const selected = world.deviceTypes.find((t) => t.id === selectedId) ?? null;

  const createType = useCallback(async () => {
    const name = window.prompt("Device type name?");
    if (!name) return;
    const res = await request(CreateDeviceTypeMutation, { input: { name } });
    setSelectedId(res.createDeviceType.id);
    await onRefresh();
  }, [onRefresh]);

  const deleteType = useCallback(
    async (id: string, name: string) => {
      if (!window.confirm(`Delete device type "${name}"?`)) return;
      try {
        await request(DeleteDeviceTypeMutation, { id });
      } catch (e) {
        window.alert((e as Error).message);
        return;
      }
      if (selectedId === id) setSelectedId(null);
      await onRefresh();
    },
    [selectedId, onRefresh],
  );

  const exportType = useCallback(async (id: string, name: string) => {
    const result = await request(ExportDeviceTypeMutation, { id });
    const blob = new Blob([result.exportDeviceType], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `device-type-${name.replace(/[^A-Za-z0-9._-]/g, "_")}.json`;
    a.click();
    URL.revokeObjectURL(url);
  }, []);

  const importType = useCallback(async () => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = "application/json,.json";
    input.onchange = async () => {
      const file = input.files?.[0];
      if (!file) return;
      try {
        const data = await file.text();
        const res = await request(ImportDeviceTypeMutation, { data });
        setSelectedId(res.importDeviceType.id);
        await onRefresh();
      } catch (e) {
        window.alert(`Import failed: ${(e as Error).message}`);
      }
    };
    input.click();
  }, [onRefresh]);

  const importVarmecoCsv = useCallback(async () => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = "text/csv,.csv";
    input.onchange = async () => {
      const file = input.files?.[0];
      if (!file) return;
      try {
        const data = await file.text();
        // Default the type's name to the file's stem (e.g. `vfnova.csv`
        // → `vfnova`); the user can rename afterwards in the editor.
        const stem = file.name.replace(/\.csv$/i, "");
        const res = await request(ImportVarmecoCsvMutation, {
          name: stem,
          description: `Imported from ${file.name}`,
          data,
        });
        setSelectedId(res.importVarmecoCsv.id);
        await onRefresh();
      } catch (e) {
        window.alert(`Varmeco CSV import failed: ${(e as Error).message}`);
      }
    };
    input.click();
  }, [onRefresh]);

  return (
    <div className="stack">
      <div className="panel">
        <div className="row">
          <h2 className="grow">Device Types</h2>
          <button onClick={() => void createType()}>+ New</button>
          <button onClick={() => void importType()} title="Import a device type from JSON">
            Import…
          </button>
          <button
            onClick={() => void importVarmecoCsv()}
            title="Import a Varmeco-format CSV (vfnova.csv, exm_compact.csv, …)"
          >
            Import Varmeco CSV…
          </button>
        </div>
        <p className="muted">
          Device types are reusable templates shared across all contexts. Export a type to share it
          between machines; import to drop one in without re-creating it by hand.
        </p>
        <table>
          <thead>
            <tr>
              <th>Name</th>
              <th>Description</th>
              <th># Registers</th>
              <th className="narrow">Actions</th>
            </tr>
          </thead>
          <tbody>
            {world.deviceTypes.map((t) => (
              <tr
                key={t.id}
                onClick={() => setSelectedId(t.id)}
                style={{
                  cursor: "pointer",
                  background: t.id === selectedId ? "rgba(76,196,255,0.08)" : undefined,
                }}
              >
                <td>{t.name}</td>
                <td>{t.description}</td>
                <td>{t.registers.length}</td>
                <td className="narrow">
                  <div className="row">
                    <button
                      onClick={(e) => {
                        e.stopPropagation();
                        void exportType(t.id, t.name);
                      }}
                      title="Download this type as JSON"
                    >
                      Export
                    </button>
                    <button
                      className="danger"
                      onClick={(e) => {
                        e.stopPropagation();
                        void deleteType(t.id, t.name);
                      }}
                    >
                      Delete
                    </button>
                  </div>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      {selected && <DeviceTypeEditor key={selected.id} type={selected} onRefresh={onRefresh} />}
    </div>
  );
}

function DeviceTypeEditor({
  type,
  onRefresh,
}: {
  type: WorldSnapshot["deviceTypes"][number];
  onRefresh: () => Promise<void>;
}) {
  const [name, setName] = useState(type.name);
  const [description, setDescription] = useState(type.description);

  const rename = useCallback(async () => {
    await request(RenameDeviceTypeMutation, { id: type.id, name, description });
    await onRefresh();
  }, [name, description, type.id, onRefresh]);

  const [behavior, setBehavior] = useState({
    disabledFunctionCodes: type.behavior.disabledFunctionCodes.join(", "),
    maxRegistersPerRequest: type.behavior.maxRegistersPerRequest?.toString() ?? "",
    missingFullBlock: type.behavior.missingFullBlock as Missing,
    missingPartialBlock: type.behavior.missingPartialBlock as Missing,
    responseDelayMs: type.behavior.responseDelayMs,
  });

  const saveBehavior = useCallback(async () => {
    const codes = behavior.disabledFunctionCodes
      .split(/[,;\s]+/)
      .map((s) => s.trim())
      .filter(Boolean)
      .map((s) => parseInt(s, 10))
      .filter((n) => !Number.isNaN(n));
    const max = behavior.maxRegistersPerRequest.trim();
    await request(UpdateBehaviorMutation, {
      deviceTypeId: type.id,
      input: {
        disabledFunctionCodes: codes,
        maxRegistersPerRequest: max === "" ? null : parseInt(max, 10),
        missingFullBlock: behavior.missingFullBlock,
        missingPartialBlock: behavior.missingPartialBlock,
        responseDelayMs: behavior.responseDelayMs,
      },
    });
    await onRefresh();
  }, [behavior, type.id, onRefresh]);

  return (
    <>
      <div className="panel">
        <h2>Edit “{type.name}”</h2>
        <div className="grid">
          <label>
            Name
            <input value={name} onChange={(e) => setName(e.target.value)} />
          </label>
          <label>
            Description
            <input value={description} onChange={(e) => setDescription(e.target.value)} />
          </label>
        </div>
        <div style={{ marginTop: "0.5rem" }}>
          <button onClick={() => void rename()}>Save name/description</button>
        </div>
      </div>

      <div className="panel">
        <h2>Behavior</h2>
        <p className="muted">
          Simulate real-device quirks. Disable function codes the device wouldn't support, cap the
          request size, pick how the device reacts when addresses are missing.
        </p>
        <div className="grid">
          <label>
            Disabled function codes (comma-separated)
            <input
              value={behavior.disabledFunctionCodes}
              onChange={(e) =>
                setBehavior((b) => ({ ...b, disabledFunctionCodes: e.target.value }))
              }
              placeholder="e.g. 5, 15"
            />
          </label>
          <label>
            Max registers per request (empty = unlimited)
            <input
              value={behavior.maxRegistersPerRequest}
              onChange={(e) =>
                setBehavior((b) => ({ ...b, maxRegistersPerRequest: e.target.value }))
              }
              placeholder="125"
            />
          </label>
          <label>
            Missing block — full miss
            <select
              value={behavior.missingFullBlock}
              onChange={(e) =>
                setBehavior((b) => ({ ...b, missingFullBlock: e.target.value as Missing }))
              }
            >
              {MISSING.map((m) => (
                <option key={m}>{m}</option>
              ))}
            </select>
          </label>
          <label>
            Missing block — partial overlap
            <select
              value={behavior.missingPartialBlock}
              onChange={(e) =>
                setBehavior((b) => ({ ...b, missingPartialBlock: e.target.value as Missing }))
              }
            >
              {MISSING.map((m) => (
                <option key={m}>{m}</option>
              ))}
            </select>
          </label>
          <label>
            Response delay (ms)
            <input
              type="number"
              value={behavior.responseDelayMs}
              onChange={(e) =>
                setBehavior((b) => ({ ...b, responseDelayMs: parseInt(e.target.value, 10) || 0 }))
              }
            />
          </label>
        </div>
        <div style={{ marginTop: "0.5rem" }}>
          <button className="primary" onClick={() => void saveBehavior()}>
            Save behavior
          </button>
        </div>
      </div>

      <RegisterEditor type={type} onRefresh={onRefresh} />
    </>
  );
}

function RegisterEditor({
  type,
  onRefresh,
}: {
  type: WorldSnapshot["deviceTypes"][number];
  onRefresh: () => Promise<void>;
}) {
  const [draft, setDraft] = useState({
    kind: "HOLDING" as Kind,
    address: "0",
    name: "",
    description: "",
    dataType: "U16" as DataType,
    encoding: "BIG_ENDIAN" as Encoding,
    byteLength: "",
    defaultValueType: "U16",
    defaultValueRaw: "0",
  });

  const addRegister = useCallback(async () => {
    const byteLen = draft.byteLength.trim();
    // Coil + discrete values are semantically boolean; always send BOOL so
    // the backend's scalar parser accepts "true"/"false"/"1"/"0".
    const isBit = draft.kind === "COIL" || draft.kind === "DISCRETE";
    const valueType = isBit ? "BOOL" : draft.dataType;
    const effectiveByteLen =
      draft.dataType !== "STRING" || byteLen === "" ? null : parseInt(byteLen, 10);
    await request(UpsertRegisterMutation, {
      deviceTypeId: type.id,
      input: {
        kind: draft.kind,
        address: parseInt(draft.address, 10) || 0,
        name: draft.name || "unnamed",
        description: draft.description,
        dataType: draft.dataType,
        encoding: draft.encoding,
        byteLength: effectiveByteLen,
        defaultValue: {
          dataType: valueType,
          value: draft.defaultValueRaw,
        },
      },
    });
    setDraft((d) => ({ ...d, name: "", address: String((parseInt(d.address, 10) || 0) + 1) }));
    await onRefresh();
  }, [draft, type.id, onRefresh]);

  const delRegister = useCallback(
    async (id: string) => {
      await request(DeleteRegisterMutation, { id });
      await onRefresh();
    },
    [onRefresh],
  );

  return (
    <div className="panel">
      <h2>Registers</h2>
      <table>
        <thead>
          <tr>
            <th>Kind</th>
            <th>Addr</th>
            <th>Name</th>
            <th>Type</th>
            <th>Encoding</th>
            <th>Bytes</th>
            <th>Default</th>
            <th>Description</th>
            <th className="narrow"></th>
          </tr>
        </thead>
        <tbody>
          {type.registers.map((r) => (
            <ExistingRegisterRow
              key={r.id}
              deviceTypeId={type.id}
              register={r}
              onRefresh={onRefresh}
              onDelete={() => void delRegister(r.id)}
            />
          ))}
          <tr>
            <td>
              <select
                value={draft.kind}
                onChange={(e) => setDraft((d) => ({ ...d, kind: e.target.value as Kind }))}
              >
                {KINDS.map((k) => (
                  <option key={k}>{k}</option>
                ))}
              </select>
            </td>
            <td>
              <input
                value={draft.address}
                onChange={(e) => setDraft((d) => ({ ...d, address: e.target.value }))}
              />
            </td>
            <td>
              <input
                value={draft.name}
                placeholder="name"
                onChange={(e) => setDraft((d) => ({ ...d, name: e.target.value }))}
              />
            </td>
            <td>
              <select
                value={draft.dataType}
                disabled={draft.kind === "COIL" || draft.kind === "DISCRETE"}
                title={
                  draft.kind === "COIL" || draft.kind === "DISCRETE"
                    ? "Coils and discrete inputs are single bits; data type is fixed"
                    : undefined
                }
                onChange={(e) =>
                  setDraft((d) => ({ ...d, dataType: e.target.value as DataType }))
                }
              >
                {DATA_TYPES.map((t) => (
                  <option key={t}>{t}</option>
                ))}
              </select>
            </td>
            <td>
              <select
                value={draft.encoding}
                disabled={
                  draft.dataType === "STRING" ||
                  draft.kind === "COIL" ||
                  draft.kind === "DISCRETE"
                }
                title={
                  draft.kind === "COIL" || draft.kind === "DISCRETE"
                    ? "Coils and discrete inputs are single bits; encoding is N/A"
                    : draft.dataType === "STRING"
                      ? "Strings are packed byte-wise; encoding is fixed"
                      : undefined
                }
                onChange={(e) =>
                  setDraft((d) => ({ ...d, encoding: e.target.value as Encoding }))
                }
              >
                {ENCODINGS.map((enc) => (
                  <option key={enc}>{enc}</option>
                ))}
              </select>
            </td>
            <td>
              <input
                value={draft.dataType === "STRING" ? draft.byteLength : ""}
                placeholder={draft.dataType === "STRING" ? "bytes" : "—"}
                disabled={draft.dataType !== "STRING"}
                title={draft.dataType === "STRING" ? undefined : "Only used for STRING"}
                onChange={(e) => setDraft((d) => ({ ...d, byteLength: e.target.value }))}
              />
            </td>
            <td>
              <input
                value={draft.defaultValueRaw}
                onChange={(e) =>
                  setDraft((d) => ({ ...d, defaultValueRaw: e.target.value }))
                }
              />
            </td>
            <td>
              <input
                value={draft.description}
                placeholder="optional"
                onChange={(e) => setDraft((d) => ({ ...d, description: e.target.value }))}
              />
            </td>
            <td className="narrow">
              <button className="primary" onClick={() => void addRegister()}>
                + Add
              </button>
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  );
}

function ExistingRegisterRow({
  deviceTypeId,
  register,
  onRefresh,
  onDelete,
}: {
  deviceTypeId: string;
  register: WorldSnapshot["deviceTypes"][number]["registers"][number];
  onRefresh: () => Promise<void>;
  onDelete: () => void;
}) {
  const [form, setForm] = useState({
    kind: register.kind as Kind,
    address: String(register.address),
    name: register.name,
    description: register.description,
    dataType: register.dataType as DataType,
    encoding: register.encoding as Encoding,
    byteLength: register.byteLength != null ? String(register.byteLength) : "",
    defaultValue: register.defaultValue.value,
  });
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const dirty =
    form.kind !== register.kind ||
    form.address !== String(register.address) ||
    form.name !== register.name ||
    form.description !== register.description ||
    form.dataType !== register.dataType ||
    form.encoding !== register.encoding ||
    form.byteLength !== (register.byteLength != null ? String(register.byteLength) : "") ||
    form.defaultValue !== register.defaultValue.value;

  // Sync form from upstream register only when the user has no pending edits.
  // Without this, the 2 s world-refresh polling would re-render with a new
  // `register` object reference every cycle and overwrite the user's in-
  // progress selection.
  const dirtyRef = useRef(dirty);
  dirtyRef.current = dirty;
  useEffect(() => {
    if (dirtyRef.current) return;
    setForm({
      kind: register.kind as Kind,
      address: String(register.address),
      name: register.name,
      description: register.description,
      dataType: register.dataType as DataType,
      encoding: register.encoding as Encoding,
      byteLength: register.byteLength != null ? String(register.byteLength) : "",
      defaultValue: register.defaultValue.value,
    });
  }, [
    register.kind,
    register.address,
    register.name,
    register.description,
    register.dataType,
    register.encoding,
    register.byteLength,
    register.defaultValue.value,
  ]);

  const save = useCallback(async () => {
    setBusy(true);
    setErr(null);
    try {
      const isBit = form.kind === "COIL" || form.kind === "DISCRETE";
      const valueType = isBit ? "BOOL" : form.dataType;
      const byteLen = form.byteLength.trim();
      const effectiveByteLen =
        form.dataType !== "STRING" || byteLen === "" ? null : parseInt(byteLen, 10);
      await request(UpsertRegisterMutation, {
        deviceTypeId,
        input: {
          id: register.id,
          kind: form.kind,
          address: parseInt(form.address, 10) || 0,
          name: form.name || "unnamed",
          description: form.description,
          dataType: form.dataType,
          encoding: form.encoding,
          byteLength: effectiveByteLen,
          defaultValue: { dataType: valueType, value: form.defaultValue },
        },
      });
      await onRefresh();
    } catch (e) {
      setErr((e as Error).message);
    } finally {
      setBusy(false);
    }
  }, [deviceTypeId, register.id, form, onRefresh]);

  return (
    <tr>
      <td>
        <select
          value={form.kind}
          onChange={(e) => setForm((f) => ({ ...f, kind: e.target.value as Kind }))}
        >
          {KINDS.map((k) => (
            <option key={k}>{k}</option>
          ))}
        </select>
      </td>
      <td>
        <input
          value={form.address}
          onChange={(e) => setForm((f) => ({ ...f, address: e.target.value }))}
        />
      </td>
      <td>
        <input
          value={form.name}
          onChange={(e) => setForm((f) => ({ ...f, name: e.target.value }))}
        />
      </td>
      <td>
        <select
          value={form.dataType}
          disabled={form.kind === "COIL" || form.kind === "DISCRETE"}
          title={
            form.kind === "COIL" || form.kind === "DISCRETE"
              ? "Coils and discrete inputs are single bits; data type is fixed"
              : undefined
          }
          onChange={(e) => setForm((f) => ({ ...f, dataType: e.target.value as DataType }))}
        >
          {DATA_TYPES.map((t) => (
            <option key={t}>{t}</option>
          ))}
        </select>
      </td>
      <td>
        <select
          value={form.encoding}
          disabled={
            form.dataType === "STRING" ||
            form.kind === "COIL" ||
            form.kind === "DISCRETE"
          }
          title={
            form.kind === "COIL" || form.kind === "DISCRETE"
              ? "Coils and discrete inputs are single bits; encoding is N/A"
              : form.dataType === "STRING"
                ? "Strings are packed byte-wise; encoding is fixed"
                : undefined
          }
          onChange={(e) => setForm((f) => ({ ...f, encoding: e.target.value as Encoding }))}
        >
          {ENCODINGS.map((enc) => (
            <option key={enc}>{enc}</option>
          ))}
        </select>
      </td>
      <td>
        <input
          value={form.dataType === "STRING" ? form.byteLength : ""}
          placeholder={form.dataType === "STRING" ? "bytes" : "—"}
          disabled={form.dataType !== "STRING"}
          title={form.dataType === "STRING" ? undefined : "Only used for STRING"}
          onChange={(e) => setForm((f) => ({ ...f, byteLength: e.target.value }))}
        />
      </td>
      <td>
        <input
          value={form.defaultValue}
          onChange={(e) => setForm((f) => ({ ...f, defaultValue: e.target.value }))}
        />
      </td>
      <td>
        <input
          value={form.description}
          onChange={(e) => setForm((f) => ({ ...f, description: e.target.value }))}
        />
      </td>
      <td className="narrow">
        <div className="row">
          <button
            className="primary"
            disabled={busy || !dirty}
            onClick={() => void save()}
            title={dirty ? "Save changes" : "No changes"}
          >
            Save
          </button>
          <button className="danger" onClick={onDelete} title="Delete register">
            ✕
          </button>
        </div>
        {err && <div className="error">{err}</div>}
      </td>
    </tr>
  );
}
