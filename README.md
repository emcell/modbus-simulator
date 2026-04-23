# Modbus Simulator

Ein Tool zum Simulieren von Modbus-Slaves und deren Verhalten für Labor- und Integrationstests.

## Motivation

Im Laboralltag möchte man häufig die Integration eines Modbus-Masters testen, ohne ein passendes physisches Gerät zur Hand zu haben. Dieses Tool stellt beliebig viele simulierte Modbus-Slaves bereit — inklusive realitätsnaher Eigenheiten echter Geräte (Function-Code-Einschränkungen, maximale Registeranzahl pro Request, spezifisches Fehlerverhalten bei nicht existierenden Registern).

## Features

### Gerätetypen & Geräte

Das Kernmodell kennt zwei Ebenen:

- **Gerätetyp (DeviceType)** — wiederverwendbare Schablone. Beschreibt einmal: Registerlayout, Datenpunkte mit Metadaten und das Verhalten (Function-Code-Einschränkungen, max. Register pro Request, Missing-Block-Verhalten, Antwortverzögerung). Gerätetypen lassen sich exportieren/importieren und sind kontext-übergreifend nutzbar.
- **Gerät (Device)** — konkrete Instanz eines Gerätetyps mit eigener Slave ID, eigenem Namen und eigenen *Laufzeit-Registerwerten*. Mehrere Geräte können sich denselben Gerätetyp teilen. Änderungen am Gerätetyp schlagen auf alle Instanzen durch (Registerlayout & Verhalten). Registerwerte sind pro Instanz.
- **Overrides (optional)** — pro Gerät können einzelne Verhaltensparameter (z. B. anderer Timeout) überschrieben werden, ohne einen neuen Typ anlegen zu müssen.

### Register (am Gerätetyp definiert)

- Register-Kinds: Holding Registers, Input Registers, Coils, Discrete Inputs
- Pro Datenpunkt:
  - **Name**
  - **Datentyp / Breite**: 16-bit, 32-bit (belegt 2 Registernummern), 64-bit, String (feste Byte-Anzahl)
  - **Encoding**: little-endian, big-endian (inkl. Word-Swap-Varianten), `f16`, `f32`, `f64`, `int16`, `uint16`, `int32`, `uint32`, `int64`, `uint64`, `string`
  - **Beschreibung** (Dokumentation des Datenpunkts)
  - **Startadresse**
  - **Default-Wert** (Ausgangswert für neue Instanzen; laufender Wert pro Geräteinstanz)

### Geräteverhalten (am Gerätetyp definiert, pro Gerät override-bar)

- **Function Codes deaktivieren** — echte Geräte beherrschen oft nur eine Teilmenge
- **Maximale Anzahl Register pro Request** — viele Geräte limitieren, wie viele Register gleichzeitig gelesen/geschrieben werden können
- **Verhalten bei nicht existierenden Registern**, getrennt konfigurierbar für:
  - Der komplette angefragte Block existiert nicht
  - Der Block überschneidet sich nur teilweise mit nicht existierenden Registern
  - Optionen: `IllegalDataAddress` (Exception 02), `IllegalFunction` (Exception 01), `SlaveDeviceFailure` (Exception 04), Timeout / keine Antwort, Rückgabe von `0x0000`
- **Antwortverzögerung** (optional, für Timeout-Tests)

### Transport

- **Modbus TCP**
  - Port konfigurierbar, Default `502`
  - Bind-Adresse konfigurierbar
- **Modbus RTU**
  - Serielle Schnittstelle konfigurierbar (Pfad, Baudrate, Parität, Stop-Bits)
  - **Virtuelle TTYs** unter Linux und macOS: Backend erzeugt selbstständig ein Paar verbundener PTYs (via `openpty`), symbolische Linknamen vom Nutzer vergebbar (z. B. `/tmp/modbus-sim-a` ↔ `/tmp/modbus-sim-b`) — eine Seite hält der Simulator, die andere die Testanwendung
  - Unter Windows wird das virtuelle TTY-Feature weggelassen, falls keine sinnvolle Bordmittel-Lösung existiert. Physische/bereits vorhandene COM-Ports bleiben nutzbar.

### UI & API

- **Browser-basiertes UI** — Anwendung öffnet einen HTTP/WebSocket-Port und liefert das UI selbst aus
- **GraphQL-API** für sämtliche Backend-Funktionen (Geräte, Register, Verhalten, Transporte, Kontexte)
- Frontend nutzt **TypeScript** und **gql.tada** für typisierte Queries

### Persistenz

- Alle Einstellungen werden auf Platte gespeichert (Format: JSON, Datei-basiert, versionierbar)
- **Import/Export** kompletter Konfigurationen
- **Kontext-System**: benannte Konfigurationsschnappschüsse, zwischen denen seamless umgeschaltet werden kann
  - Use-Case: Gestern getestete Kombination A,B,C, heute E,F,G, morgen wieder A,B,C — ohne Re-Import, per Klick/CLI-Kommando wechselbar
  - Aktiver Kontext ist sofort live: Transporte werden gegebenenfalls umgeschwenkt

### Plattform & Auslieferung

- **Linux** (x86_64), **Windows** (x86_64), **macOS** (x86_64 + aarch64) — macOS ist Entwicklungsplattform und damit First-Class-Target
- Auslieferung als **einzelne Executable** (statisch gelinkt wo möglich, UI-Assets eingebettet)

## Technologie

- **Backend**: Rust
  - Async-Runtime: Tokio
  - Modbus-Stack: `tokio-modbus` (bzw. manuelle Implementierung wo das Verhalten zu starr ist — wir brauchen bewusst "fehlerhafte" Geräte)
  - GraphQL: `async-graphql`
  - HTTP/WebSocket: `axum`
  - Serielle Schnittstelle: `tokio-serial`
  - PTY unter Linux: `nix` / `openpty`
  - Eingebettete Assets: `rust-embed`
- **Frontend**: TypeScript + gql.tada; Framework-Wahl offen (Vorschlag: React + Vite, da gängig mit gql.tada)

## Code-Qualität (verpflichtend)

- Code wird bei jeder Änderung automatisch formatiert (`rustfmt`, `prettier`)
- Keine Warnungen, keine Errors — `cargo clippy -- -D warnings`, `tsc --noEmit` clean
- Bevorzugt **pure functions** und ein **read → compute → update** Modell
- Seiteneffekte an den Rändern, Kern deterministisch und testbar

## GraphQL-API (Skizze)

```graphql
type DeviceType {
  id: ID!
  name: String!
  description: String
  registers: [RegisterPoint!]!
  behavior: DeviceBehavior!
  instances: [Device!]!
}

type Device {
  id: ID!
  name: String!
  slaveId: Int!
  deviceType: DeviceType!
  behaviorOverrides: DeviceBehaviorOverrides  # null = Typ-Default
  registerValues: [RegisterValue!]!           # Laufzeitwerte dieser Instanz
}

type RegisterPoint {
  id: ID!
  kind: RegisterKind!        # HOLDING | INPUT | COIL | DISCRETE
  address: Int!
  name: String!
  description: String
  dataType: DataType!        # U16, I16, U32, I32, U64, I64, F16, F32, F64, STRING
  encoding: Encoding!        # BIG_ENDIAN, LITTLE_ENDIAN, BE_WORD_SWAP, LE_WORD_SWAP
  byteLength: Int            # nur für STRING
  defaultValue: Value!
}

type RegisterValue {
  registerId: ID!
  value: Value!
}

type DeviceBehavior {
  disabledFunctionCodes: [Int!]!
  maxRegistersPerRequest: Int
  missingFullBlock: MissingBlockBehavior!
  missingPartialBlock: MissingBlockBehavior!
  responseDelayMs: Int
}

enum MissingBlockBehavior {
  ILLEGAL_DATA_ADDRESS
  ILLEGAL_FUNCTION
  SLAVE_DEVICE_FAILURE
  TIMEOUT
  ZERO_FILL
}

type Transport {
  tcp: TcpTransport
  rtu: RtuTransport
}

type Context {
  id: ID!
  name: String!
  active: Boolean!
}

type Query {
  deviceTypes: [DeviceType!]!
  deviceType(id: ID!): DeviceType
  devices: [Device!]!
  device(id: ID!): Device
  transport: Transport!
  contexts: [Context!]!
  activeContext: Context!
}

type Mutation {
  createDeviceType(input: CreateDeviceTypeInput!): DeviceType!
  updateDeviceType(id: ID!, input: UpdateDeviceTypeInput!): DeviceType!
  deleteDeviceType(id: ID!): Boolean!        # scheitert, wenn noch Instanzen existieren
  cloneDeviceType(id: ID!, name: String!): DeviceType!

  upsertRegister(deviceTypeId: ID!, input: RegisterInput!): RegisterPoint!
  deleteRegister(id: ID!): Boolean!
  updateBehavior(deviceTypeId: ID!, input: BehaviorInput!): DeviceBehavior!

  createDevice(input: CreateDeviceInput!): Device!     # braucht deviceTypeId + slaveId + name
  updateDevice(id: ID!, input: UpdateDeviceInput!): Device!
  deleteDevice(id: ID!): Boolean!
  setBehaviorOverrides(deviceId: ID!, input: BehaviorOverridesInput): Device!
  setRegisterValue(deviceId: ID!, registerId: ID!, value: ValueInput!): RegisterValue!

  configureTransport(input: TransportInput!): Transport!

  createContext(name: String!): Context!
  switchContext(id: ID!): Context!
  deleteContext(id: ID!): Boolean!
  exportContext(id: ID!): String!           # JSON
  importContext(name: String!, data: String!): Context!

  createVirtualSerialPair(input: VirtualSerialInput!): VirtualSerialPair!
  removeVirtualSerialPair(id: ID!): Boolean!
}

type Subscription {
  registerChanged(deviceId: ID): RegisterValue!
  traffic: ModbusFrame!       # Live-Mitschnitt aller Requests/Responses
}
```

## Persistenz-Layout

```
<config-dir>/modbus-simulator/
  active.json               # { "contextId": "..." }
  device-types/
    <uuid>.json             # Gerätetyp-Schablonen (kontext-übergreifend, referenzierbar)
  contexts/
    <uuid>.json             # ein Kontext = Geräte-Instanzen (Referenz auf DeviceType) +
                            #   Laufzeitwerte + Behavior-Overrides + Transport-Settings
  settings.json             # UI-Port, Log-Level, etc.
```

Import/Export: Ein exportierter Kontext enthält die benötigten Gerätetypen eingebettet, damit er auf anderen Maschinen in sich konsistent ist.

`<config-dir>` folgt plattformkonvention (XDG unter Linux, `%APPDATA%` unter Windows).

## Nicht-Ziele (explizit)

- Kein Modbus-Master-Mode
- Keine Cloud-Sync oder Multi-User-Funktionalität
- Kein Schreiben auf echte Geräte
