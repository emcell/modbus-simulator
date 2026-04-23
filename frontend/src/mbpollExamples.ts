/**
 * Build ready-to-copy mbpoll CLI invocations for a given register + transport
 * setup. We generate both read and write variants (if the register kind
 * supports writes) for both TCP and RTU.
 */

import type { WorldSnapshot } from "./App";

type Register = WorldSnapshot["deviceTypes"][number]["registers"][number];
type Tcp = NonNullable<WorldSnapshot["activeContext"]>["tcp"];
type Rtu = NonNullable<WorldSnapshot["activeContext"]>["rtu"];

export interface CommandExample {
  title: string;
  command: string;
  note?: string;
}

export interface ExampleSet {
  tcp: CommandExample[];
  rtu: CommandExample[];
}

export function buildExamples(
  register: Register,
  slaveId: number,
  tcp: Tcp,
  rtu: Rtu,
): ExampleSet {
  return {
    tcp: buildFor("tcp", register, slaveId, tcp, rtu),
    rtu: buildFor("rtu", register, slaveId, tcp, rtu),
  };
}

function buildFor(
  transport: "tcp" | "rtu",
  register: Register,
  slaveId: number,
  tcp: Tcp,
  rtu: Rtu,
): CommandExample[] {
  const transportArgs = transportArgsFor(transport, tcp, rtu);
  const tail = transportTail(transport, tcp, rtu);
  const common = `mbpoll ${transportArgs} -a ${slaveId} -0 -1`;

  const addr = register.address;
  const kind = register.kind;
  const out: CommandExample[] = [];

  if (kind === "COIL") {
    out.push({
      title: "Read coil",
      command: `${common} -t 0 -r ${addr} -c 1 ${tail}`,
    });
    out.push({
      title: "Write coil = 1",
      command: `${common} -t 0 -r ${addr} ${tail} 1`,
    });
    out.push({
      title: "Write coil = 0",
      command: `${common} -t 0 -r ${addr} ${tail} 0`,
    });
    return out;
  }

  if (kind === "DISCRETE") {
    out.push({
      title: "Read discrete input (read-only)",
      command: `${common} -t 1 -r ${addr} -c 1 ${tail}`,
    });
    return out;
  }

  const isHolding = kind === "HOLDING";
  const tRead = isHolding ? "4" : "3";

  const words = wordCountFor(register);
  const dt = register.dataType;
  const enc = register.encoding;

  // Decide display type for mbpoll.
  if (dt === "STRING") {
    out.push({
      title: "Read string",
      command: `${common} -t ${tRead}:string -r ${addr} -c ${words} ${tail}`,
      note: `Fixed byte length = ${register.byteLength ?? "?"} → ${words} registers.`,
    });
    if (isHolding) {
      out.push({
        title: "Write string (one register at a time)",
        command: `${common} -t ${tRead}:hex -r ${addr} ${tail} 0xAABB`,
        note: "mbpoll's :string mode is read-only; write raw words (0xHHHH) via -t 4:hex.",
      });
    }
    return out;
  }

  if (dt === "U16" || dt === "I16" || dt === "F16") {
    out.push({
      title: `Read ${dt}`,
      command: `${common} -t ${tRead} -r ${addr} -c 1 ${tail}`,
    });
    if (isHolding) {
      out.push({
        title: `Write ${dt}`,
        command: `${common} -t ${tRead} -r ${addr} ${tail} <value>`,
      });
    }
    return out;
  }

  // Multi-register numeric: U32/I32/F32, U64/I64/F64.
  const is64 = dt === "U64" || dt === "I64" || dt === "F64";
  const mbpollType = mbpollMultiType(dt);
  const wordOrderFlag = mbpollWordOrderFlag(enc);

  if (is64) {
    // mbpoll doesn't have a native 64-bit display; read as 4 raw words.
    out.push({
      title: `Read ${dt} (as 4 raw hex words)`,
      command: `${common} -t ${tRead}:hex -r ${addr} -c 4 ${tail}`,
      note: "Reassemble the 64-bit value from the four 16-bit words on the client side.",
    });
    if (isHolding) {
      out.push({
        title: `Write ${dt} (four raw words)`,
        command: `${common} -t ${tRead} -r ${addr} ${tail} <w0> <w1> <w2> <w3>`,
        note: "Split the 64-bit value into four 16-bit decimal words in the target byte order.",
      });
    }
    return out;
  }

  // 32-bit path.
  if (wordOrderFlag === null) {
    // LittleEndian / LittleEndianWordSwap — mbpoll's :int/:float assume
    // BE-byte order within each word, so it can't render this directly.
    out.push({
      title: `Read ${dt} (raw 2 words — ${enc})`,
      command: `${common} -t ${tRead}:hex -r ${addr} -c 2 ${tail}`,
      note: "mbpoll doesn't support this byte-swapped encoding natively; read raw words and decode on the client.",
    });
    if (isHolding) {
      out.push({
        title: `Write ${dt} (two raw words)`,
        command: `${common} -t ${tRead} -r ${addr} ${tail} <low-word> <high-word>`,
      });
    }
    return out;
  }

  // BigEndian or BigEndianWordSwap → both renderable with mbpoll.
  const flag = wordOrderFlag === "B" ? " -B" : "";
  out.push({
    title: `Read ${dt}`,
    command: `${common} -t ${tRead}:${mbpollType}${flag} -r ${addr} -c 1 ${tail}`,
  });
  if (isHolding) {
    out.push({
      title: `Write ${dt}`,
      command: `${common} -t ${tRead}:${mbpollType}${flag} -r ${addr} ${tail} <value>`,
    });
  }
  return out;
}

function transportArgsFor(transport: "tcp" | "rtu", _tcp: Tcp, rtu: Rtu): string {
  if (transport === "tcp") {
    return `-m tcp`;
  }
  const parity =
    rtu.parity === "E" ? "even" : rtu.parity === "O" ? "odd" : "none";
  const baud = rtu.baudRate || 9600;
  const dataBits = rtu.dataBits || 8;
  const stopBits = rtu.stopBits || 1;
  return `-m rtu -b ${baud} -d ${dataBits} -s ${stopBits} -P ${parity}`;
}

function transportTail(transport: "tcp" | "rtu", tcp: Tcp, rtu: Rtu): string {
  if (transport === "tcp") {
    const host = tcp.bind && tcp.bind !== "0.0.0.0" ? tcp.bind : "127.0.0.1";
    const port = tcp.port || 502;
    return `-p ${port} ${host}`;
  }
  return rtu.device || "/dev/ttyUSB0";
}

function mbpollMultiType(dt: string): string {
  if (dt === "F32") return "float";
  return "int"; // U32 / I32
}

/**
 * Returns "B" for big-endian word order (mbpoll -B), "" for mbpoll default
 * (= BigEndianWordSwap), or `null` when mbpoll can't natively render.
 */
function mbpollWordOrderFlag(enc: string): "B" | "" | null {
  switch (enc) {
    case "BIG_ENDIAN":
      return "B";
    case "BIG_ENDIAN_WORD_SWAP":
      return "";
    default:
      return null;
  }
}

function wordCountFor(register: Register): number {
  if (register.kind === "COIL" || register.kind === "DISCRETE") return 1;
  switch (register.dataType) {
    case "U16":
    case "I16":
    case "F16":
      return 1;
    case "U32":
    case "I32":
    case "F32":
      return 2;
    case "U64":
    case "I64":
    case "F64":
      return 4;
    case "STRING":
      return Math.ceil((register.byteLength ?? 2) / 2);
    default:
      return 1;
  }
}
