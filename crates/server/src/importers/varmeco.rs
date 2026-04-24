//! Importer for Varmeco's per-product Modbus register CSV files (e.g.
//! `vfnova.csv`, `exm_compact.csv`, `vc380_substation.csv`).
//!
//! Format: semicolon-separated, UTF-8, optional UTF-8 BOM, CRLF or LF line
//! endings, first row is the header. Required columns: `Id`, `Register`,
//! `Type`, `Name`. Optional columns: `ValueType`, `ValueHandling`, `Min`,
//! `Max`, `Format`, `Multiplicator`, `Divisor`, `DigiVisuFormat`,
//! `SecurityLevel`, `DefaultValue`, `DefaultExtModbusIndex`.
//!
//! The file's per-row `ValueHandling`, scaling, suffix and security level
//! don't have a 1:1 home in the simulator's data model, so we drop them
//! during import. They show up in the human-readable `description` we
//! synthesise so a user looking at the imported type can still see them.

use modsim_core::encoding::{DataType, Encoding, Value};
use modsim_core::model::{RegisterId, RegisterKind, RegisterPoint};

/// One row's worth of error context — file line number + message.
#[derive(Debug, Clone)]
pub struct ImportError {
    pub line: usize,
    pub message: String,
}

#[derive(Debug, Default)]
pub struct ImportResult {
    pub registers: Vec<RegisterPoint>,
    pub errors: Vec<ImportError>,
    pub processed_lines: usize,
}

/// Parse a Varmeco-style CSV into a list of register points.
///
/// Returns the registers it could parse plus a list of per-row errors. A
/// totally unparseable file (missing required headers, no header row at
/// all) yields a single header-level error and an empty register list.
pub fn parse(input: &str) -> ImportResult {
    let mut result = ImportResult::default();
    // Skip optional UTF-8 BOM.
    let trimmed = input.strip_prefix('\u{FEFF}').unwrap_or(input);
    let mut lines = trimmed.lines();
    let Some(header_line) = lines.next() else {
        result.errors.push(ImportError {
            line: 0,
            message: "empty file".into(),
        });
        return result;
    };
    let header = match Header::parse(header_line) {
        Ok(h) => h,
        Err(msg) => {
            result.errors.push(ImportError {
                line: 1,
                message: msg,
            });
            return result;
        }
    };

    for (idx, raw_line) in lines.enumerate() {
        let line_no = idx + 2; // header was line 1
        let line = raw_line.trim_end_matches('\r');
        if line.trim().is_empty() {
            continue;
        }
        result.processed_lines += 1;
        match parse_row(&header, line, line_no) {
            Ok(rp) => result.registers.push(rp),
            Err(e) => result.errors.push(e),
        }
    }
    result
}

/// Indices of every column we know about. -1 (`None`) means the column
/// wasn't in the file at all.
struct Header {
    id: usize,
    register: usize,
    kind: usize,
    name: usize,
    value_type: Option<usize>,
    value_handling: Option<usize>,
    format: Option<usize>,
    multiplicator: Option<usize>,
    divisor: Option<usize>,
    digi_visu_format: Option<usize>,
    security_level: Option<usize>,
    default_value: Option<usize>,
}

impl Header {
    fn parse(line: &str) -> Result<Self, String> {
        let trimmed = line.strip_prefix('\u{FEFF}').unwrap_or(line);
        let cells: Vec<&str> = trimmed
            .trim_end_matches('\r')
            .split(';')
            .map(str::trim)
            .collect();
        let find = |name: &str| -> Option<usize> {
            cells.iter().position(|c| c.eq_ignore_ascii_case(name))
        };
        let required = |name: &str| -> Result<usize, String> {
            find(name).ok_or_else(|| format!("required header '{name}' is missing"))
        };
        Ok(Self {
            id: required("Id")?,
            register: required("Register")?,
            kind: required("Type")?,
            name: required("Name")?,
            value_type: find("ValueType"),
            value_handling: find("ValueHandling"),
            format: find("Format"),
            multiplicator: find("Multiplicator"),
            divisor: find("Divisor"),
            digi_visu_format: find("DigiVisuFormat"),
            security_level: find("SecurityLevel"),
            default_value: find("DefaultValue"),
        })
    }
}

fn parse_row(h: &Header, line: &str, line_no: usize) -> Result<RegisterPoint, ImportError> {
    let cells: Vec<&str> = line.split(';').collect();
    let cell = |idx: usize| -> &str { cells.get(idx).copied().unwrap_or("") };
    let opt_cell = |idx: Option<usize>| -> &str { idx.map(cell).unwrap_or("") };
    let err = |msg: String| ImportError {
        line: line_no,
        message: msg,
    };

    let id = cell(h.id).trim();
    if id.is_empty() {
        return Err(err("Id is empty".into()));
    }

    let address: u16 = cell(h.register)
        .trim()
        .parse()
        .map_err(|_| err(format!("Register '{}' is not a u16", cell(h.register))))?;

    let kind = parse_kind(cell(h.kind).trim())
        .ok_or_else(|| err(format!("Type '{}' is unknown", cell(h.kind))))?;

    let name = cell(h.name).trim().to_string();
    if name.is_empty() {
        return Err(err("Name is empty".into()));
    }

    let value_type = parse_value_type(opt_cell(h.value_type).trim());

    let (data_type, default_value) = match kind {
        RegisterKind::Coil | RegisterKind::Discrete => {
            let v = parse_bool_default(opt_cell(h.default_value).trim());
            (DataType::U16, Value::Bool(v))
        }
        RegisterKind::Holding | RegisterKind::Input => match value_type {
            ValueType::U16 => {
                let v = opt_cell(h.default_value).trim().parse::<u16>().unwrap_or(0);
                (DataType::U16, Value::U16(v))
            }
            ValueType::I16 => {
                let v = opt_cell(h.default_value).trim().parse::<i16>().unwrap_or(0);
                (DataType::I16, Value::I16(v))
            }
        },
    };

    // Build a description that surfaces the bits the simulator's data
    // model doesn't carry natively (label, scaling, suffix).
    let description = build_description(h, &cells, &name);

    Ok(RegisterPoint {
        id: RegisterId::new(),
        kind,
        address,
        name: id.to_string(),
        description,
        data_type,
        encoding: Encoding::BigEndian,
        byte_length: None,
        default_value,
    })
}

fn build_description(h: &Header, cells: &[&str], name: &str) -> String {
    let cell = |idx: Option<usize>| -> &str {
        idx.and_then(|i| cells.get(i).copied()).unwrap_or("").trim()
    };
    let mut parts = vec![name.to_string()];
    let mult = cell(h.multiplicator);
    let div = cell(h.divisor);
    if (!mult.is_empty() && mult != "1") || (!div.is_empty() && div != "1") {
        let m = if mult.is_empty() { "1" } else { mult };
        let d = if div.is_empty() { "1" } else { div };
        parts.push(format!("scale ×{m}/{d}"));
    }
    let suffix = cell(h.format);
    if !suffix.is_empty() {
        parts.push(format!("unit {suffix}"));
    }
    let handling = cell(h.value_handling);
    if !handling.is_empty() && !handling.eq_ignore_ascii_case("ON_CONFLICT_MASTER_HAS_PRIORITY") {
        parts.push(handling.to_string());
    }
    let security = cell(h.security_level);
    if !security.is_empty() && !security.eq_ignore_ascii_case("ALL") {
        parts.push(format!("auth: {security}"));
    }
    let visu = cell(h.digi_visu_format);
    if !visu.is_empty() {
        parts.push(format!("fmt {visu}"));
    }
    parts.join(" · ")
}

fn parse_kind(s: &str) -> Option<RegisterKind> {
    if let Ok(n) = s.parse::<i32>() {
        return match n {
            0 => Some(RegisterKind::Coil),
            1 => Some(RegisterKind::Discrete),
            2 => Some(RegisterKind::Holding),
            3 => Some(RegisterKind::Input),
            _ => None,
        };
    }
    match s.to_ascii_lowercase().as_str() {
        "coil" => Some(RegisterKind::Coil),
        "discrete-input" | "discrete_input" => Some(RegisterKind::Discrete),
        "holding-register" | "holding_register" => Some(RegisterKind::Holding),
        "input-register" | "input_register" => Some(RegisterKind::Input),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy)]
enum ValueType {
    U16,
    I16,
}

fn parse_value_type(s: &str) -> ValueType {
    match s.to_ascii_lowercase().as_str() {
        "int16" | "int16_t" => ValueType::I16,
        // "", "uint16", "uint16_t", anything else → default
        _ => ValueType::U16,
    }
}

fn parse_bool_default(s: &str) -> bool {
    match s.trim() {
        "" => false,
        "0" => false,
        "1" => true,
        other => other.eq_ignore_ascii_case("true") || other.eq_ignore_ascii_case("on"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        let path = format!(
            "{}/../../varmeco_csv_format/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"))
    }

    #[test]
    fn parses_vfnova_fixture() {
        let result = parse(&fixture("vfnova.csv"));
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        assert!(
            result.registers.len() > 50,
            "got {}",
            result.registers.len()
        );
        // Sanity-check the first row matches the file: it's `SYS.CollectiveFaultSignal;0;coil;…`
        let first = &result.registers[0];
        assert_eq!(first.name, "SYS.CollectiveFaultSignal");
        assert_eq!(first.address, 0);
        assert_eq!(first.kind, RegisterKind::Coil);
        assert_eq!(first.default_value, Value::Bool(false));
    }

    #[test]
    fn parses_exm_compact_fixture() {
        let result = parse(&fixture("exm_compact.csv"));
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        assert!(
            result.registers.len() > 100,
            "got {}",
            result.registers.len()
        );
        // The 4th data row in exm_compact.csv has a negative min and i16 type
        // (`SYS.Config.TfOffset;3;input-register;READ_ONLY;-30;30;…;int16_t;°C;1;10;####,# °C;…`).
        let tf_offset = result
            .registers
            .iter()
            .find(|r| r.name == "SYS.Config.TfOffset")
            .expect("TfOffset register present");
        assert_eq!(tf_offset.kind, RegisterKind::Input);
        assert_eq!(tf_offset.data_type, DataType::I16);
        assert_eq!(tf_offset.address, 3);
        assert!(
            tf_offset.description.contains("°C"),
            "description should carry the unit suffix: {}",
            tf_offset.description
        );
        assert!(
            tf_offset.description.contains("scale ×1/10"),
            "description should carry the scaling: {}",
            tf_offset.description
        );
    }

    #[test]
    fn all_sample_files_parse_without_errors() {
        for name in [
            "vfnova_kaskade.csv",
            "vsn.csv",
            "vfnova.csv",
            "exm_compact.csv",
            "vc380_substation.csv",
            "vfnova_plus.csv",
            "vff.csv",
        ] {
            let result = parse(&fixture(name));
            assert!(
                result.errors.is_empty(),
                "{name}: errors {:?}",
                result.errors
            );
            assert!(!result.registers.is_empty(), "{name}: no registers");
        }
    }

    #[test]
    fn header_with_missing_required_column_fails_cleanly() {
        let csv = "Id;Register;Name\nfoo;0;bar\n";
        let result = parse(csv);
        assert_eq!(result.registers.len(), 0);
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].message.contains("Type"));
    }

    #[test]
    fn empty_input_yields_an_error() {
        let result = parse("");
        assert!(result.registers.is_empty());
        assert_eq!(result.errors.len(), 1);
    }

    #[test]
    fn handles_utf8_bom() {
        let csv = "\u{FEFF}Id;Register;Type;Name\na;0;coil;label\n";
        let result = parse(csv);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.registers.len(), 1);
        assert_eq!(result.registers[0].name, "a");
    }

    #[test]
    fn default_values_for_coils_parse() {
        let csv = "Id;Register;Type;Name;DefaultValue\nx;5;coil;label;1\ny;6;coil;label;0\nz;7;coil;label;\n";
        let result = parse(csv);
        assert!(result.errors.is_empty());
        assert_eq!(result.registers[0].default_value, Value::Bool(true));
        assert_eq!(result.registers[1].default_value, Value::Bool(false));
        assert_eq!(result.registers[2].default_value, Value::Bool(false));
    }

    #[test]
    fn signed_value_type_default() {
        let csv = "Id;Register;Type;Name;ValueType;DefaultValue\nx;5;holding-register;label;int16_t;-42\n";
        let result = parse(csv);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.registers[0].data_type, DataType::I16);
        assert_eq!(result.registers[0].default_value, Value::I16(-42));
    }

    #[test]
    fn numeric_kind_codes_supported() {
        let csv = "Id;Register;Type;Name\nc;0;0;coil_test\nd;1;1;discrete_test\nh;2;2;holding_test\ni;3;3;input_test\n";
        let result = parse(csv);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.registers[0].kind, RegisterKind::Coil);
        assert_eq!(result.registers[1].kind, RegisterKind::Discrete);
        assert_eq!(result.registers[2].kind, RegisterKind::Holding);
        assert_eq!(result.registers[3].kind, RegisterKind::Input);
    }
}
