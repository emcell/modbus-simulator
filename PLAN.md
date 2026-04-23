# Umsetzungsplan

Inkrementelle Umsetzung in Meilensteinen. Jeder Meilenstein ist lauffähig, testbar und liefert einen sichtbaren Mehrwert. Nach jedem Meilenstein: `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test`, `tsc --noEmit`, `prettier --check`.

## M0 — Projekt-Gerüst

- Cargo-Workspace aufsetzen
  - `crates/core` — Domänenmodell (pure), Modbus-Encoding/Decoding, Verhaltens-Engine
  - `crates/server` — axum, async-graphql, Transport-Layer, Persistenz, CLI
  - `crates/frontend` (oder `frontend/`) — TS + gql.tada + Vite
- Toolchain festnageln (`rust-toolchain.toml`)
- CI-Grundlagen: fmt, clippy (-D warnings), test, tsc, prettier
- Lizenz, `.editorconfig`, `.gitignore`

**Deliverable**: `cargo run` startet einen leeren Server auf Port 8080, `/health` antwortet.

## M1 — Domänenmodell (pure core)

- Typen:
  - `DeviceType` (Schablone): `registers: Vec<RegisterPoint>`, `behavior: DeviceBehavior`
  - `Device` (Instanz): `device_type_id`, `slave_id`, `name`, `register_values`, optional `behavior_overrides`
  - `RegisterPoint`, `DataType`, `Encoding`, `DeviceBehavior`, `DeviceBehaviorOverrides`, `RegisterKind`
- Effektives Verhalten pro Instanz = `DeviceBehavior` aus Typ, überlagert durch `Overrides` (pure merge-Funktion)
- Encoder/Decoder: Register-Words ↔ typisierte Werte (alle Encodings)
- Adress-Map pro Gerätetyp: welche Adresse existiert, welcher Register-Kind gehört dazu, überlappende/fehlende Blöcke erkennen
- Verhaltens-Engine als reine Funktion: `(Request, DeviceInstanceState, EffectiveBehavior) -> Response | Exception | Silence`
- Referenzielle Integrität: `DeviceType` darf nicht gelöscht werden, solange `Device`-Instanzen existieren
- Unit-Tests für jedes Encoding, jedes Missing-Block-Szenario, und für Override-Merge

**Deliverable**: Getesteter, transport-freier Kern.

## M2 — Persistenz & Kontexte

- Config-Verzeichnis ermitteln (`directories` crate)
- Serialisierung (serde + JSON) aller Kontext-Daten
- Kontext-Operationen: list, create, switch, delete, export, import
- Atomic writes (temp-file + rename)

**Deliverable**: Geräte-Konfigurationen überleben Neustart, Kontexte wechselbar (CLI-Befehl zum Testen).

## M3 — Modbus TCP

- `tokio-modbus` als Server, aber eigener Handler, damit wir gezielt "falsch" antworten können
- Request-Parsing → Verhaltens-Engine → Response-Serialisierung
- Konfigurierbarer Port/Bind
- Integrationstests: echter Client (tokio-modbus client) gegen simulierten Slave
  - Happy path pro Function Code
  - Deaktivierte Function Codes → Exception 01
  - Überschreitung `maxRegistersPerRequest`
  - Missing-Block-Varianten (full/partial) gegen alle Verhalten
  - `responseDelayMs`

**Deliverable**: Über TCP erreichbarer Simulator, voll testabgedeckt.

## M4 — GraphQL-API

- `async-graphql` Schema gemäß README
- Queries: deviceTypes, devices, contexts, transport
- Mutations: CRUD für DeviceType (inkl. clone), Register (am Typ), Behavior (am Typ), Device-Instanzen, Overrides, Context, Import/Export
- Subscriptions: `registerChanged`, `traffic` (Live-Traffic-Stream)
- Änderungen persistieren und Live-Server rekonfigurieren (read → compute → update)
- Schema-Export als `.graphql`-Datei (für gql.tada)

**Deliverable**: Vollständige Backend-API; manuelles Testen per GraphQL Playground.

## M5 — Frontend-Grundgerüst

- Vite + React + TypeScript
- gql.tada gegen exportiertes Schema
- Pages:
  - Gerätetypen-Liste & -Editor (Registerlayout, Verhalten, Klonen)
  - Geräte-Liste (Instanzen): Auswahl des Typs, Slave ID, Name, Overrides
  - Register-Werte-Tabelle pro Geräteinstanz mit Inline-Edit
  - Transport-Einstellungen
  - Kontext-Umschalter (prominent in Header)
  - Live-Traffic-Ansicht (Subscription)
- `rust-embed` bindet das gebaute Frontend in die Server-Binary

**Deliverable**: Einzelne Binary startet Server + UI; komplette Verwaltung im Browser.

## M6 — Modbus RTU

- `tokio-serial` Integration
- RTU-Frame-Parser/-Serializer (CRC16)
- Gleiche Verhaltens-Engine wiederverwenden
- Mehrere Slave IDs auf einem seriellen Bus
- Konfiguration per GraphQL

**Deliverable**: Simulator spricht RTU auf vorhandener serieller Schnittstelle.

## M7 — Virtuelle TTYs (Linux & macOS)

- `openpty` via `nix` — funktioniert sowohl unter Linux als auch macOS identisch
- Symlink-Namen vom Nutzer vergebbar (unter macOS ebenfalls möglich: Symlink auf `/dev/ttys00X`)
- Paar-Verwaltung: erzeugen, anzeigen, löschen
- Lifecycle an Prozess gebunden, aufräumen bei Shutdown
- Unter Windows: Feature `cfg`-gated, UI zeigt "nicht verfügbar"

**Deliverable**: Lokales Ende-zu-Ende-RTU-Testen ohne Hardware (Dev-Maschine macOS!).

## M8 — Distribution

- Release-Builds:
  - Linux x86_64 (musl für portable Binary)
  - Windows x86_64
  - macOS x86_64 + aarch64 (Universal Binary via `lipo` oder getrennte Artefakte)
- GitHub Actions Release-Workflow (Matrix-Build über alle Targets)
- Versionierung / Changelog
- Erste getaggte Version `v0.1.0`

**Deliverable**: Download-fähige Binaries.

## Querschnittsthemen (kontinuierlich)

- **Formatierung**: pre-commit Hook (`cargo fmt`, `prettier --write`)
- **Lints**: CI bricht bei Warnung
- **Reinheit**: Neuer Code soll dem read-compute-update-Muster folgen; Seiteneffekte nur in klar benannten Modulen (`io::`, `transport::`, `persistence::`)
- **Tests**: Unit-Tests im Core, Integration-Tests für Transport-Layer, E2E gegen GraphQL
- **Logs**: `tracing` strukturiert; Traffic-Log pro Frame für UI-Subscription

## Status

Alle Meilensteine M0–M8 umgesetzt:

- Rust-Workspace (`modsim-core` pure + `modsim-server` I/O)
- Modbus TCP + RTU inkl. virtueller PTYs (Linux/macOS)
- Vollständige GraphQL-API mit Queries / Mutations / Subscriptions (`traffic`, `worldChanged`)
- React + TS + gql.tada Frontend, als Vite-Bundle ins Binary eingebettet
- GitHub-Actions: CI (fmt/clippy/test über Linux/macOS/Windows + Frontend-Build) + Release-Workflow mit Matrix-Build
- **71 Tests grün**: 19 core + 9 server unit + 4 TCP-Integration + 4 RTU-Dispatch + 19 mbpoll-TCP (alle FCs + Encodings) + 16 mbpoll-RTU

## Offene Fragen für später

- Scripted Verhalten (z. B. Wert ändert sich zyklisch, Rampen, Rauschen) als späteres Feature?
- Authentifizierung des UIs (bisher: lokal, keine) — falls später remote zugänglich, nötig.
- Mitgelieferte Gerätetypen-Bibliothek für gängige echte Geräte (SMA, Victron, …) als späterer Komfort?
