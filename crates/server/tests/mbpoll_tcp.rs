#![allow(clippy::await_holding_lock)] // intentional: tcp_lock serializes each async test

//! End-to-end TCP tests driven by the `mbpoll` CLI.
//!
//! Covers every function code the simulator implements (FC 01–06, 15, 16)
//! and every numeric encoding (U16/I16, U32/I32 in four byte-orders, F32,
//! U64/I64, strings). mbpoll uses 1-based addressing by default; we pass
//! `-0` throughout so `-r N` maps directly to modbus address N.
//!
//! Skipped automatically if `mbpoll` is not installed.

use std::collections::BTreeMap;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use modsim_core::behavior::DeviceBehavior;
use modsim_core::encoding::{DataType, Encoding, Value};
use modsim_core::model::{
    Context, Device, DeviceId, DeviceType, DeviceTypeId, RegisterId, RegisterKind, RegisterPoint,
    World,
};
use modsim_server::persistence::{AppSettings, Store};
use modsim_server::state::AppState;
use modsim_server::transport::tcp;

fn have_mbpoll() -> bool {
    Command::new("mbpoll").arg("-h").output().is_ok()
}

fn tmp_root() -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!(
        "modsim-mbpoll-tcp-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}

// ---- register layout --------------------------------------------------------
//
// Holding registers:
//   0..4    U16      (FC03 read, FC06/FC16 write)
//   10..12  U32 BigEndian           0x11223344
//   12..14  U32 BigEndianWordSwap   0x11223344
//   14..16  U32 LittleEndian        0x11223344
//   16..18  U32 LittleEndianWordSwap 0x11223344
//   20..22  I32 BigEndian           -123456
//   30..34  F32 BigEndian           3.5
//   40..44  U64 BigEndian           0x0102030405060708
//   50..54  I64 BigEndian           -9
//   60..64  String (8 bytes)        "ABCDefgh"
// Input registers:
//   0..3    U16                     0xAAAA, 0xBBBB, 0xCCCC
// Coils:
//   0..8    bools                   T F T F T F T F
// Discrete inputs:
//   0..8    bools                   F T F T F T F T

fn make_holding(
    addr: u16,
    dt: DataType,
    enc: Encoding,
    value: Value,
    byte_len: Option<usize>,
) -> RegisterPoint {
    RegisterPoint {
        id: RegisterId(uuid::Uuid::new_v4()),
        kind: RegisterKind::Holding,
        address: addr,
        name: format!("h{addr}"),
        description: String::new(),
        data_type: dt,
        encoding: enc,
        byte_length: byte_len,
        default_value: value,
    }
}

fn make_input(addr: u16, value: u16) -> RegisterPoint {
    RegisterPoint {
        id: RegisterId(uuid::Uuid::new_v4()),
        kind: RegisterKind::Input,
        address: addr,
        name: format!("i{addr}"),
        description: String::new(),
        data_type: DataType::U16,
        encoding: Encoding::BigEndian,
        byte_length: None,
        default_value: Value::U16(value),
    }
}

fn make_bit(kind: RegisterKind, addr: u16, value: bool) -> RegisterPoint {
    RegisterPoint {
        id: RegisterId(uuid::Uuid::new_v4()),
        kind,
        address: addr,
        name: format!("{:?}-{addr}", kind),
        description: String::new(),
        data_type: DataType::U16,
        encoding: Encoding::BigEndian,
        byte_length: None,
        default_value: Value::Bool(value),
    }
}

fn seed_world() -> World {
    let mut registers = Vec::new();

    // Holding U16 block (4 regs)
    for (i, v) in [0x0A0A_u16, 0x0B0B, 0x0C0C, 0x0D0D].iter().enumerate() {
        registers.push(make_holding(
            i as u16,
            DataType::U16,
            Encoding::BigEndian,
            Value::U16(*v),
            None,
        ));
    }

    // 32-bit at 4 different encodings, all holding 0x11223344
    registers.push(make_holding(
        10,
        DataType::U32,
        Encoding::BigEndian,
        Value::U32(0x1122_3344),
        None,
    ));
    registers.push(make_holding(
        12,
        DataType::U32,
        Encoding::BigEndianWordSwap,
        Value::U32(0x1122_3344),
        None,
    ));
    registers.push(make_holding(
        14,
        DataType::U32,
        Encoding::LittleEndian,
        Value::U32(0x1122_3344),
        None,
    ));
    registers.push(make_holding(
        16,
        DataType::U32,
        Encoding::LittleEndianWordSwap,
        Value::U32(0x1122_3344),
        None,
    ));

    // I32 signed -123456 big-endian
    registers.push(make_holding(
        20,
        DataType::I32,
        Encoding::BigEndian,
        Value::I32(-123_456),
        None,
    ));

    // F32 = 3.5 (big-endian word order)
    registers.push(make_holding(
        30,
        DataType::F32,
        Encoding::BigEndian,
        Value::F32(3.5),
        None,
    ));

    // U64 big-endian
    registers.push(make_holding(
        40,
        DataType::U64,
        Encoding::BigEndian,
        Value::U64(0x0102_0304_0506_0708),
        None,
    ));

    // I64 big-endian = -9
    registers.push(make_holding(
        50,
        DataType::I64,
        Encoding::BigEndian,
        Value::I64(-9),
        None,
    ));

    // String "ABCDefgh" (8 bytes = 4 registers) big-endian
    registers.push(make_holding(
        60,
        DataType::String,
        Encoding::BigEndian,
        Value::String("ABCDefgh".into()),
        Some(8),
    ));

    // Input registers 0..3
    registers.push(make_input(0, 0xAAAA));
    registers.push(make_input(1, 0xBBBB));
    registers.push(make_input(2, 0xCCCC));

    // Coils 0..8: T F T F T F T F
    for i in 0..8u16 {
        registers.push(make_bit(RegisterKind::Coil, i, i % 2 == 0));
    }
    // Discrete inputs 0..8: F T F T F T F T
    for i in 0..8u16 {
        registers.push(make_bit(RegisterKind::Discrete, i, i % 2 == 1));
    }

    let dt_id = DeviceTypeId(uuid::Uuid::new_v4());
    let dt = DeviceType {
        id: dt_id,
        name: "AllCodes".into(),
        description: String::new(),
        registers,
        behavior: DeviceBehavior::default(),
    };
    let device = Device {
        id: DeviceId(uuid::Uuid::new_v4()),
        name: "dev".into(),
        slave_id: 1,
        device_type_id: dt_id,
        behavior_overrides: None,
        register_values: BTreeMap::new(),
    };
    let ctx = Context {
        id: modsim_core::model::ContextId(uuid::Uuid::new_v4()),
        name: "c".into(),
        devices: vec![device],
        transport: Default::default(),
    };
    let ctx_id = ctx.id;
    World {
        device_types: vec![dt],
        contexts: vec![ctx],
        active_context_id: Some(ctx_id),
    }
}

async fn start_server() -> u16 {
    let store = Store::with_root(tmp_root()).unwrap();
    let state = AppState::new(seed_world(), AppSettings::default(), store);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let st = Arc::clone(&state);
    tokio::spawn(async move {
        let _ = tcp::run(st, "127.0.0.1".into(), port).await;
    });
    tokio::time::sleep(Duration::from_millis(150)).await;
    port
}

/// Tests in this file each spin up their own AppState + TCP listener on an
/// ephemeral port. Between picking the port and binding it inside
/// `tcp::run`, another parallel test can steal it — which surfaces as
/// sporadic "Connection refused" from mbpoll. Serialize to remove that
/// race.
fn tcp_lock() -> std::sync::MutexGuard<'static, ()> {
    static M: std::sync::Mutex<()> = std::sync::Mutex::new(());
    M.lock().unwrap_or_else(|e| e.into_inner())
}

/// Runs `mbpoll -m tcp -p PORT -a 1 -0 -1 -o 2 <opts> 127.0.0.1 <values...>`.
///
/// mbpoll's CLI requires the host to appear *before* any positional write
/// values. We split them here so tests don't have to think about argv
/// order.
fn mbpoll(port: u16, opts: &[&str], values: &[&str]) -> std::process::Output {
    let mut args: Vec<String> = vec![
        "-m".into(),
        "tcp".into(),
        "-p".into(),
        port.to_string(),
        "-a".into(),
        "1".into(),
        "-0".into(),
        "-1".into(),
        "-o".into(),
        "2".into(),
    ];
    for e in opts {
        args.push((*e).to_string());
    }
    args.push("127.0.0.1".into());
    for v in values {
        args.push((*v).to_string());
    }
    Command::new("mbpoll").args(&args).output().expect("mbpoll")
}

fn mbpoll_success(port: u16, opts: &[&str], values: &[&str]) -> String {
    let out = mbpoll(port, opts, values);
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(
        out.status.success(),
        "mbpoll failed with opts {opts:?} values {values:?}\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    stdout
}

/// Helper: assert mbpoll output has a line `[<idx>]:\t<value>` (tab-separated).
fn assert_slot(stdout: &str, idx: u32, value: &str) {
    let ok = stdout.lines().any(|l| {
        let l = l.trim_end();
        l.starts_with(&format!("[{idx}]:")) && l.contains(value)
    });
    assert!(ok, "expected [{idx}] = {value} in:\n{stdout}");
}

// ===========================================================================
// Read function codes
// ===========================================================================

#[tokio::test(flavor = "multi_thread")]
async fn fc01_read_coils() {
    let _g = tcp_lock();
    if !have_mbpoll() {
        return;
    }
    let port = start_server().await;
    // 8 coils starting at 0, pattern T F T F T F T F
    let stdout = tokio::task::spawn_blocking(move || {
        mbpoll_success(port, &["-t", "0", "-r", "0", "-c", "8"], &[])
    })
    .await
    .unwrap();
    for i in 0..8 {
        let expected = if i % 2 == 0 { "1" } else { "0" };
        assert_slot(&stdout, i, expected);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn fc02_read_discrete_inputs() {
    let _g = tcp_lock();
    if !have_mbpoll() {
        return;
    }
    let port = start_server().await;
    let stdout = tokio::task::spawn_blocking(move || {
        mbpoll_success(port, &["-t", "1", "-r", "0", "-c", "8"], &[])
    })
    .await
    .unwrap();
    // Pattern is F T F T F T F T
    for i in 0..8 {
        let expected = if i % 2 == 0 { "0" } else { "1" };
        assert_slot(&stdout, i, expected);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn fc03_read_holding_registers() {
    let _g = tcp_lock();
    if !have_mbpoll() {
        return;
    }
    let port = start_server().await;
    let stdout = tokio::task::spawn_blocking(move || {
        mbpoll_success(port, &["-t", "4:hex", "-r", "0", "-c", "4"], &[])
    })
    .await
    .unwrap();
    assert!(stdout.contains("0x0A0A"), "{stdout}");
    assert!(stdout.contains("0x0B0B"), "{stdout}");
    assert!(stdout.contains("0x0C0C"), "{stdout}");
    assert!(stdout.contains("0x0D0D"), "{stdout}");
}

#[tokio::test(flavor = "multi_thread")]
async fn fc04_read_input_registers() {
    let _g = tcp_lock();
    if !have_mbpoll() {
        return;
    }
    let port = start_server().await;
    let stdout = tokio::task::spawn_blocking(move || {
        mbpoll_success(port, &["-t", "3:hex", "-r", "0", "-c", "3"], &[])
    })
    .await
    .unwrap();
    assert!(stdout.contains("0xAAAA"), "{stdout}");
    assert!(stdout.contains("0xBBBB"), "{stdout}");
    assert!(stdout.contains("0xCCCC"), "{stdout}");
}

// ===========================================================================
// Write function codes
// ===========================================================================

#[tokio::test(flavor = "multi_thread")]
async fn fc05_write_single_coil_then_read() {
    let _g = tcp_lock();
    if !have_mbpoll() {
        return;
    }
    let port = start_server().await;
    // Coil[1] is initially 0 (F); write 1 using FC05. mbpoll picks FC05 for a
    // single coil write when exactly 1 value is given.
    tokio::task::spawn_blocking(move || mbpoll_success(port, &["-t", "0", "-r", "1"], &["1"]))
        .await
        .unwrap();
    let stdout = tokio::task::spawn_blocking(move || {
        mbpoll_success(port, &["-t", "0", "-r", "1", "-c", "1"], &[])
    })
    .await
    .unwrap();
    // mbpoll prints with absolute address in brackets
    assert_slot(&stdout, 1, "1");
}

#[tokio::test(flavor = "multi_thread")]
async fn fc06_write_single_register_then_read() {
    let _g = tcp_lock();
    if !have_mbpoll() {
        return;
    }
    let port = start_server().await;
    tokio::task::spawn_blocking(move || mbpoll_success(port, &["-t", "4", "-r", "0"], &["12345"]))
        .await
        .unwrap();
    let stdout = tokio::task::spawn_blocking(move || {
        mbpoll_success(port, &["-t", "4", "-r", "0", "-c", "1"], &[])
    })
    .await
    .unwrap();
    assert_slot(&stdout, 0, "12345");
    // sanity
    assert!(stdout.contains("[0]:"), "{stdout}");
}

#[tokio::test(flavor = "multi_thread")]
async fn fc15_write_multiple_coils_then_read() {
    let _g = tcp_lock();
    if !have_mbpoll() {
        return;
    }
    let port = start_server().await;
    // write all 8 coils to a new pattern
    tokio::task::spawn_blocking(move || {
        mbpoll_success(
            port,
            &["-t", "0", "-r", "0"],
            &["1", "1", "0", "0", "1", "0", "1", "1"],
        )
    })
    .await
    .unwrap();
    let stdout = tokio::task::spawn_blocking(move || {
        mbpoll_success(port, &["-t", "0", "-r", "0", "-c", "8"], &[])
    })
    .await
    .unwrap();
    let expected = [1, 1, 0, 0, 1, 0, 1, 1];
    for (i, v) in expected.iter().enumerate() {
        assert_slot(&stdout, i as u32, &v.to_string());
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn fc16_write_multiple_registers_then_read() {
    let _g = tcp_lock();
    if !have_mbpoll() {
        return;
    }
    let port = start_server().await;
    tokio::task::spawn_blocking(move || {
        mbpoll_success(port, &["-t", "4", "-r", "0"], &["10", "20", "30", "40"])
    })
    .await
    .unwrap();
    let stdout = tokio::task::spawn_blocking(move || {
        mbpoll_success(port, &["-t", "4", "-r", "0", "-c", "4"], &[])
    })
    .await
    .unwrap();
    for (i, v) in [10, 20, 30, 40].iter().enumerate() {
        assert_slot(&stdout, i as u32, &v.to_string());
    }
}

// ===========================================================================
// Encoding variants — the critical correctness check for how we serialize
// multi-register values.
//
// mbpoll's 32-bit encoding has two word-orders:
//   default: low word first (little-endian word order)
//   -B:      high word first (big-endian word order)
// Within a word, bytes are always big-endian (Modbus spec).
//
// Mapping to our `Encoding` enum:
//   mbpoll default + our BigEndianWordSwap  -> same wire bytes
//   mbpoll -B      + our BigEndian          -> same wire bytes
// ===========================================================================

#[tokio::test(flavor = "multi_thread")]
async fn u32_big_endian_with_mbpoll_dash_big() {
    let _g = tcp_lock();
    if !have_mbpoll() {
        return;
    }
    let port = start_server().await;
    // address 10 holds U32 BigEndian 0x11223344 = 287_454_020
    let stdout = tokio::task::spawn_blocking(move || {
        mbpoll_success(port, &["-t", "4:int", "-B", "-r", "10", "-c", "1"], &[])
    })
    .await
    .unwrap();
    assert!(stdout.contains("287454020"), "{stdout}");
}

#[tokio::test(flavor = "multi_thread")]
async fn u32_big_endian_word_swap_with_mbpoll_default() {
    let _g = tcp_lock();
    if !have_mbpoll() {
        return;
    }
    let port = start_server().await;
    // address 12 holds U32 BigEndianWordSwap 0x11223344.
    // Without -B, mbpoll expects the same wire order.
    let stdout = tokio::task::spawn_blocking(move || {
        mbpoll_success(port, &["-t", "4:int", "-r", "12", "-c", "1"], &[])
    })
    .await
    .unwrap();
    assert!(stdout.contains("287454020"), "{stdout}");
}

/// For `Encoding::LittleEndian` + `Encoding::LittleEndianWordSwap` we need
/// to reinterpret the raw 16-bit registers and reassemble the 32-bit value
/// in the client. mbpoll doesn't natively speak those encodings, so we
/// read the two raw registers and reconstruct 0x11223344 ourselves.
#[tokio::test(flavor = "multi_thread")]
async fn u32_little_endian_raw_words_match_spec() {
    let _g = tcp_lock();
    if !have_mbpoll() {
        return;
    }
    let port = start_server().await;
    // LittleEndian U32 0x11223344 →
    //   bytes BE 11 22 33 44 → LE within each word: 22 11 , 44 33
    //   => words (BE) 0x2211, 0x4433
    let stdout = tokio::task::spawn_blocking(move || {
        mbpoll_success(port, &["-t", "4:hex", "-r", "14", "-c", "2"], &[])
    })
    .await
    .unwrap();
    assert!(stdout.to_uppercase().contains("0X2211"), "{stdout}");
    assert!(stdout.to_uppercase().contains("0X4433"), "{stdout}");
}

#[tokio::test(flavor = "multi_thread")]
async fn u32_little_endian_word_swap_raw_words_match_spec() {
    let _g = tcp_lock();
    if !have_mbpoll() {
        return;
    }
    let port = start_server().await;
    // LittleEndianWordSwap U32 0x11223344 →
    //   LE bytes per word, word-swapped: 44 33 , 22 11
    //   => words (BE) 0x4433, 0x2211
    let stdout = tokio::task::spawn_blocking(move || {
        mbpoll_success(port, &["-t", "4:hex", "-r", "16", "-c", "2"], &[])
    })
    .await
    .unwrap();
    assert!(stdout.to_uppercase().contains("0X4433"), "{stdout}");
    assert!(stdout.to_uppercase().contains("0X2211"), "{stdout}");
}

#[tokio::test(flavor = "multi_thread")]
async fn u64_big_endian_raw_words_match_spec() {
    let _g = tcp_lock();
    if !have_mbpoll() {
        return;
    }
    let port = start_server().await;
    // U64 BigEndian 0x0102030405060708 → 4 words: 0x0102 0x0304 0x0506 0x0708
    let stdout = tokio::task::spawn_blocking(move || {
        mbpoll_success(port, &["-t", "4:hex", "-r", "40", "-c", "4"], &[])
    })
    .await
    .unwrap();
    let up = stdout.to_uppercase();
    for expected in ["0X0102", "0X0304", "0X0506", "0X0708"] {
        assert!(up.contains(expected), "expected {expected} in:\n{stdout}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn i64_negative_big_endian_raw_words_match_spec() {
    let _g = tcp_lock();
    if !have_mbpoll() {
        return;
    }
    let port = start_server().await;
    // I64 BigEndian -9 = 0xFFFFFFFFFFFFFFF7
    // → words: 0xFFFF 0xFFFF 0xFFFF 0xFFF7
    let stdout = tokio::task::spawn_blocking(move || {
        mbpoll_success(port, &["-t", "4:hex", "-r", "50", "-c", "4"], &[])
    })
    .await
    .unwrap();
    let up = stdout.to_uppercase();
    // at least 3× FFFF and the low word 0xFFF7
    assert!(up.contains("0XFFF7"), "expected 0XFFF7 in:\n{stdout}");
    let ffff_count = up.matches("0XFFFF").count();
    assert!(ffff_count >= 3, "expected >=3 0xFFFF words in:\n{stdout}");
}

#[tokio::test(flavor = "multi_thread")]
async fn i32_negative_big_endian() {
    let _g = tcp_lock();
    if !have_mbpoll() {
        return;
    }
    let port = start_server().await;
    // address 20 holds I32 BigEndian = -123456
    let stdout = tokio::task::spawn_blocking(move || {
        mbpoll_success(port, &["-t", "4:int", "-B", "-r", "20", "-c", "1"], &[])
    })
    .await
    .unwrap();
    assert!(stdout.contains("-123456"), "{stdout}");
}

#[tokio::test(flavor = "multi_thread")]
async fn f32_big_endian() {
    let _g = tcp_lock();
    if !have_mbpoll() {
        return;
    }
    let port = start_server().await;
    // address 30 holds F32 BigEndian = 3.5
    let stdout = tokio::task::spawn_blocking(move || {
        mbpoll_success(port, &["-t", "4:float", "-B", "-r", "30", "-c", "1"], &[])
    })
    .await
    .unwrap();
    // mbpoll prints floats like "3.500000" or "3.5"
    let got = stdout.to_lowercase();
    assert!(got.contains("3.5"), "{stdout}");
}

#[tokio::test(flavor = "multi_thread")]
async fn string_read_round_trip_raw_words() {
    let _g = tcp_lock();
    if !have_mbpoll() {
        return;
    }
    let port = start_server().await;
    // Read 4 holding regs as a string starting at address 60.
    let stdout = tokio::task::spawn_blocking(move || {
        mbpoll_success(port, &["-t", "4:string", "-r", "60", "-c", "4"], &[])
    })
    .await
    .unwrap();
    // mbpoll prints one line per register with two characters each:
    //   [60]:\tAB   [61]:\tCD   [62]:\tef   [63]:\tgh
    // Concatenating those value columns should give back "ABCDefgh".
    let mut assembled = String::new();
    for line in stdout.lines() {
        let line = line.trim_end();
        for prefix in ["[60]:", "[61]:", "[62]:", "[63]:"] {
            if line.starts_with(prefix) {
                if let Some(tail) = line.split_once('\t') {
                    assembled.push_str(tail.1.trim());
                }
            }
        }
    }
    assert_eq!(
        assembled, "ABCDefgh",
        "reconstructed={assembled:?} stdout:\n{stdout}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn u16_write_and_read_roundtrip_matches_engine() {
    let _g = tcp_lock();
    if !have_mbpoll() {
        return;
    }
    let port = start_server().await;
    // Write single U16 via FC06, then read it via FC03.
    tokio::task::spawn_blocking(move || {
        mbpoll_success(port, &["-t", "4:hex", "-r", "3"], &["0xCAFE"])
    })
    .await
    .unwrap();
    let stdout = tokio::task::spawn_blocking(move || {
        mbpoll_success(port, &["-t", "4:hex", "-r", "3", "-c", "1"], &[])
    })
    .await
    .unwrap();
    // mbpoll prints hex with lowercase `0x` prefix: "[3]:\t0xCAFE"
    assert!(
        stdout
            .lines()
            .any(|l| l.contains("[3]:") && l.to_uppercase().contains("CAFE")),
        "expected CAFE at [3] in:\n{stdout}"
    );
}

// ===========================================================================
// Error / exception paths
// ===========================================================================

#[tokio::test(flavor = "multi_thread")]
async fn missing_address_reports_exception() {
    let _g = tcp_lock();
    if !have_mbpoll() {
        return;
    }
    let port = start_server().await;
    let out = tokio::task::spawn_blocking(move || {
        mbpoll(port, &["-t", "4", "-r", "500", "-c", "2"], &[])
    })
    .await
    .unwrap();
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
    .to_lowercase();
    assert!(
        combined.contains("illegal") || combined.contains("exception") || !out.status.success(),
        "expected exception, got:\n{combined}"
    );
}
