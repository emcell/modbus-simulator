export function ConverterPage() {
  return (
    <div className="stack">
      <div className="panel">
        <h2>Converter</h2>
        <p className="muted">
          Für viele reale Modbus-Geräte existieren bereits gepflegte, maschinenlesbare
          Register-Definitionen in Open-Source-Projekten. Statt jedes Gerät von Hand
          nachzubauen, wollen wir diese Quellen direkt in unser Gerätetyp-Format
          übersetzen können.
        </p>
        <p className="muted">
          Die Konverter sind <strong>noch nicht implementiert</strong>. Diese Seite
          dokumentiert die geplanten Quellen und dient als Einstiegspunkt, sobald die
          Import-Pfade gebaut sind. Bis dahin können Gerätetypen manuell aus den unten
          verlinkten Quellen übernommen werden.
        </p>
      </div>

      <div className="panel">
        <h3>SunSpec Alliance — offizielle Modell-Definitionen</h3>
        <p>
          De-facto-Standard für PV-Wechselrichter, Batteriespeicher und Energiezähler.
          Modelle sind als JSON (und XML) maschinenlesbar gepflegt und enthalten
          Register-Layout, Datentypen, Skalierungsfaktoren und Enums.
        </p>
        <ul>
          <li>
            Modelle:{" "}
            <a
              href="https://github.com/sunspec/models"
              target="_blank"
              rel="noreferrer"
            >
              github.com/sunspec/models
            </a>{" "}
            (Apache-2.0)
          </li>
          <li>
            Spezifikation:{" "}
            <a
              href="https://sunspec.org/specifications/"
              target="_blank"
              rel="noreferrer"
            >
              sunspec.org/specifications
            </a>
          </li>
          <li>
            Beispiele: Model 103 (3-phasiger Inverter), Model 203 (3-phasiger Zähler),
            Model 802 (Batterie-Bank)
          </li>
        </ul>
        <p className="muted">
          <strong>Geplanter Konverter:</strong> SunSpec-Model-JSON → <code>DeviceType</code>.
          Sonderfälle: Scale-Factor-Register, Repeating Blocks, „pending"-Semantik.
        </p>
      </div>

      <div className="panel">
        <h3>EVCC — Wechselrichter-, Zähler- und Wallbox-Templates</h3>
        <p>
          EVCC pflegt strukturierte YAML-Templates für Hunderte von Energie-Geräten
          (SMA, Fronius, Kostal, SolarEdge, Huawei, Victron, SDM-Zähler, …) mit
          Registeradresse, Funktionscode, Datentyp und Skalierung.
        </p>
        <ul>
          <li>
            Templates:{" "}
            <a
              href="https://github.com/evcc-io/evcc/tree/master/templates/definition"
              target="_blank"
              rel="noreferrer"
            >
              github.com/evcc-io/evcc — templates/definition
            </a>{" "}
            (MIT)
          </li>
          <li>
            Geräteliste:{" "}
            <a
              href="https://docs.evcc.io/docs/devices/meters"
              target="_blank"
              rel="noreferrer"
            >
              docs.evcc.io/docs/devices
            </a>
          </li>
        </ul>
        <p className="muted">
          <strong>Geplanter Konverter:</strong> EVCC-YAML (<code>registers:</code>-Sektion)
          → <code>DeviceType</code>. Herausforderung: EVCC-Templates mischen Modbus mit
          Helfer-Formeln, nur die reine Register-Sektion ist mappbar.
        </p>
      </div>

      <div className="panel">
        <h3>Home Assistant — Modbus-Integration & Community-YAMLs</h3>
        <p>
          Die Home-Assistant-Modbus-Integration wird pro Gerät typischerweise über
          YAML-Snippets konfiguriert. Die Community pflegt umfangreiche Sammlungen für
          reale Geräte (Sungrow, Huawei SUN2000, EPEVER, Solax, Deye, …).
        </p>
        <ul>
          <li>
            Integrations-Doku:{" "}
            <a
              href="https://www.home-assistant.io/integrations/modbus/"
              target="_blank"
              rel="noreferrer"
            >
              home-assistant.io/integrations/modbus
            </a>
          </li>
          <li>
            Community-Forum (YAML-Snippets, nach Geräten durchsuchbar):{" "}
            <a
              href="https://community.home-assistant.io/tag/modbus"
              target="_blank"
              rel="noreferrer"
            >
              community.home-assistant.io/tag/modbus
            </a>
          </li>
          <li>
            Populäre Pakete: <code>mkaiser/Sungrow-SHx-Inverter-Modbus-Home-Assistant</code>,{" "}
            <code>wills106/homeassistant-solax-modbus</code>,{" "}
            <code>wills106/homeassistant-modbus-solaredge-powercontrol</code>
          </li>
        </ul>
        <p className="muted">
          <strong>Geplanter Konverter:</strong> HA-Modbus-YAML → <code>DeviceType</code>.
          Lizenzlage ist pro Snippet unterschiedlich (oft unspezifiziert), Import-Pfad
          wird deshalb „Datei hochladen" statt „aus Repo ziehen".
        </p>
      </div>

      <div className="panel">
        <h3>Weitere Quellen</h3>
        <ul>
          <li>
            <strong>OpenEMS</strong> —{" "}
            <a
              href="https://github.com/OpenEMS/openems"
              target="_blank"
              rel="noreferrer"
            >
              github.com/OpenEMS/openems
            </a>
            : Register-Definitionen liegen in Java-Code (<code>*.Nature</code>) vor,
            weniger maschinenlesbar, aber sehr breit abgedeckt.
          </li>
          <li>
            <strong>ioBroker Modbus-Adapter</strong> —{" "}
            <a
              href="https://github.com/ioBroker/ioBroker.modbus"
              target="_blank"
              rel="noreferrer"
            >
              github.com/ioBroker/ioBroker.modbus
            </a>
            : Community-JSON-Konfigurationen, heterogenes Format.
          </li>
          <li>
            <strong>pymodbus / Node-RED Flows</strong>: viele einzelne
            Geräte-Beispiele, verstreut über Gists und Forenposts.
          </li>
          <li>
            <strong>Hersteller-Dokumentation</strong>: PDFs (SMA Modbus-Protokoll,
            Victron VE.Bus, Huawei SUN2000). Nicht maschinenlesbar, aber autoritativ —
            Fallback wenn Community-Definitionen lückenhaft sind.
          </li>
        </ul>
      </div>

      <div className="panel">
        <h3>Roadmap</h3>
        <ol>
          <li>
            <strong>Import-Mutation</strong> in der GraphQL-API: nimmt eine Datei +
            Quelltyp (<code>SUNSPEC | EVCC | HOME_ASSISTANT</code>) entgegen und gibt
            einen <code>DeviceType</code> zurück.
          </li>
          <li>
            <strong>SunSpec-Konverter</strong> zuerst (formal strikt, klare Semantik).
          </li>
          <li>
            <strong>EVCC-Konverter</strong> für die reine <code>registers:</code>-
            Sektion der Templates.
          </li>
          <li>
            <strong>Home-Assistant-Konverter</strong> als Best-Effort (YAML-Schema der
            Modbus-Integration).
          </li>
          <li>
            <strong>Kuratierte Library</strong> im Repo (<code>device-library/</code>)
            mit vorbereiteten Beispiel-Gerätetypen als Startpunkt.
          </li>
        </ol>
        <p className="muted">
          Status dieser Schritte wird in <code>PLAN.md</code> gepflegt.
        </p>
      </div>
    </div>
  );
}
