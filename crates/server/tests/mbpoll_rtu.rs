//! End-to-end RTU test driven by `mbpoll` over a `socat`-bridged PTY pair.
//!
//!   socat pty,link=/tmp/modsim-sim pty,link=/tmp/modsim-client
//!
//! The simulator opens the `-sim` side, mbpoll opens the `-client` side.
//! Skipped automatically if either tool is missing.

#![cfg(unix)]
#![allow(unsafe_code)]
#![allow(clippy::await_holding_lock)] // intentional: rtu_lock serializes the entire async test

use std::collections::BTreeMap;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
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
use modsim_server::transport::rtu;
use tokio::time::sleep;

const SLAVE_ID: u8 = 7;

fn have_tool(name: &str) -> bool {
    Command::new(name)
        .arg("-h")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

fn tmp_root() -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "modsim-mbpoll-rtu-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn holding(
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

fn bit(kind: RegisterKind, addr: u16, value: bool) -> RegisterPoint {
    RegisterPoint {
        id: RegisterId(uuid::Uuid::new_v4()),
        kind,
        address: addr,
        name: format!("{kind:?}-{addr}"),
        description: String::new(),
        data_type: DataType::U16,
        encoding: Encoding::BigEndian,
        byte_length: None,
        default_value: Value::Bool(value),
    }
}

fn input(addr: u16, v: u16) -> RegisterPoint {
    RegisterPoint {
        id: RegisterId(uuid::Uuid::new_v4()),
        kind: RegisterKind::Input,
        address: addr,
        name: format!("i{addr}"),
        description: String::new(),
        data_type: DataType::U16,
        encoding: Encoding::BigEndian,
        byte_length: None,
        default_value: Value::U16(v),
    }
}

fn seed_world() -> World {
    let mut registers = Vec::new();
    // Holding U16 block 0..4
    for (i, v) in [0x1234_u16, 0x5678, 0x9ABC, 0xDEF0].iter().enumerate() {
        registers.push(holding(
            i as u16,
            DataType::U16,
            Encoding::BigEndian,
            Value::U16(*v),
            None,
        ));
    }
    // 32-bit encodings
    registers.push(holding(
        10,
        DataType::U32,
        Encoding::BigEndian,
        Value::U32(0x1122_3344),
        None,
    ));
    registers.push(holding(
        12,
        DataType::U32,
        Encoding::BigEndianWordSwap,
        Value::U32(0x1122_3344),
        None,
    ));
    registers.push(holding(
        14,
        DataType::U32,
        Encoding::LittleEndian,
        Value::U32(0x1122_3344),
        None,
    ));
    registers.push(holding(
        16,
        DataType::U32,
        Encoding::LittleEndianWordSwap,
        Value::U32(0x1122_3344),
        None,
    ));
    registers.push(holding(
        20,
        DataType::I32,
        Encoding::BigEndian,
        Value::I32(-123_456),
        None,
    ));
    registers.push(holding(
        30,
        DataType::F32,
        Encoding::BigEndian,
        Value::F32(3.5),
        None,
    ));
    registers.push(holding(
        40,
        DataType::U64,
        Encoding::BigEndian,
        Value::U64(0x0102_0304_0506_0708),
        None,
    ));
    registers.push(holding(
        50,
        DataType::I64,
        Encoding::BigEndian,
        Value::I64(-9),
        None,
    ));
    registers.push(holding(
        60,
        DataType::String,
        Encoding::BigEndian,
        Value::String("ABCDefgh".into()),
        Some(8),
    ));
    // input registers
    registers.push(input(0, 0xAAAA));
    registers.push(input(1, 0xBBBB));
    registers.push(input(2, 0xCCCC));
    // coils 0..8: T F T F T F T F
    for i in 0..8u16 {
        registers.push(bit(RegisterKind::Coil, i, i % 2 == 0));
    }
    // discrete inputs 0..8: F T F T F T F T
    for i in 0..8u16 {
        registers.push(bit(RegisterKind::Discrete, i, i % 2 == 1));
    }

    let dt_id = DeviceTypeId(uuid::Uuid::new_v4());
    let dt = DeviceType {
        id: dt_id,
        name: "RtuDev".into(),
        description: String::new(),
        registers,
        behavior: DeviceBehavior::default(),
    };
    let device = Device {
        id: DeviceId(uuid::Uuid::new_v4()),
        name: "d".into(),
        slave_id: SLAVE_ID,
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

struct Bridge {
    _child: Child,
    pub sim_link: PathBuf,
    pub client_link: PathBuf,
    _tmp: PathBuf,
}

impl Drop for Bridge {
    fn drop(&mut self) {
        let _ = self._child.kill();
        let _ = self._child.wait();
        let _ = std::fs::remove_file(&self.sim_link);
        let _ = std::fs::remove_file(&self.client_link);
        let _ = std::fs::remove_dir_all(&self._tmp);
    }
}

fn spawn_socat_bridge() -> Bridge {
    let tmp = std::env::temp_dir().join(format!(
        "modsim-bridge-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&tmp).unwrap();
    let sim_link = tmp.join("sim");
    let client_link = tmp.join("client");
    let child = Command::new("socat")
        .arg("-d")
        .arg(format!(
            "pty,raw,echo=0,link={}",
            sim_link.to_string_lossy()
        ))
        .arg(format!(
            "pty,raw,echo=0,link={}",
            client_link.to_string_lossy()
        ))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn socat");
    // Wait for both symlinks to appear.
    for _ in 0..50 {
        if sim_link.exists() && client_link.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(sim_link.exists(), "socat did not create sim link");
    assert!(client_link.exists(), "socat did not create client link");
    Bridge {
        _child: child,
        sim_link,
        client_link,
        _tmp: tmp,
    }
}

async fn start_simulator(bridge_path: PathBuf) -> Arc<AppState> {
    let store = Store::with_root(tmp_root()).unwrap();
    let state = AppState::new(seed_world(), AppSettings::default(), store);
    let st = Arc::clone(&state);
    let stream = open_pty_async(&bridge_path).expect("open pty");
    tokio::spawn(async move {
        match rtu::run_on_stream(st, stream).await {
            Ok(()) => eprintln!("rtu serve returned"),
            Err(e) => eprintln!("rtu serve error: {e}"),
        }
    });
    sleep(Duration::from_millis(300)).await;
    state
}

/// Open a PTY path as a nonblocking async stream that can be used with
/// `AsyncRead + AsyncWrite`. Sets termios to raw mode so binary data
/// isn't mangled.
fn open_pty_async(path: &std::path::Path) -> std::io::Result<pty_async::PtyStream> {
    use std::os::fd::{AsRawFd, BorrowedFd, OwnedFd};
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)?;
    // Set raw mode.
    {
        let raw = file.as_raw_fd();
        let borrow = unsafe { BorrowedFd::borrow_raw(raw) };
        if let Ok(mut t) = nix::sys::termios::tcgetattr(borrow) {
            nix::sys::termios::cfmakeraw(&mut t);
            let _ = nix::sys::termios::tcsetattr(borrow, nix::sys::termios::SetArg::TCSANOW, &t);
        }
    }
    let owned: OwnedFd = file.into();
    pty_async::PtyStream::new(owned)
}

mod pty_async {
    use std::io;
    use std::os::fd::{AsRawFd, OwnedFd};
    use std::pin::Pin;
    use std::task::{Context, Poll};

    use tokio::io::unix::AsyncFd;
    use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

    pub struct PtyStream {
        inner: AsyncFd<OwnedFd>,
    }

    impl PtyStream {
        pub fn new(fd: OwnedFd) -> io::Result<Self> {
            Ok(Self {
                inner: AsyncFd::new(fd)?,
            })
        }
    }

    impl AsyncRead for PtyStream {
        fn poll_read(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            loop {
                let mut guard = match self.inner.poll_read_ready(cx) {
                    Poll::Ready(Ok(g)) => g,
                    Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                    Poll::Pending => return Poll::Pending,
                };
                let fd = guard.get_inner().as_raw_fd();
                let n = unsafe {
                    libc::read(
                        fd,
                        buf.initialize_unfilled().as_mut_ptr().cast(),
                        buf.remaining(),
                    )
                };
                if n < 0 {
                    let e = io::Error::last_os_error();
                    if e.kind() == io::ErrorKind::WouldBlock {
                        guard.clear_ready();
                        continue;
                    }
                    return Poll::Ready(Err(e));
                }
                let n = n as usize;
                buf.advance(n);
                return Poll::Ready(Ok(()));
            }
        }
    }

    impl AsyncWrite for PtyStream {
        fn poll_write(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            loop {
                let mut guard = match self.inner.poll_write_ready(cx) {
                    Poll::Ready(Ok(g)) => g,
                    Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                    Poll::Pending => return Poll::Pending,
                };
                let fd = guard.get_inner().as_raw_fd();
                let n = unsafe { libc::write(fd, buf.as_ptr().cast(), buf.len()) };
                if n < 0 {
                    let e = io::Error::last_os_error();
                    if e.kind() == io::ErrorKind::WouldBlock {
                        guard.clear_ready();
                        continue;
                    }
                    return Poll::Ready(Err(e));
                }
                return Poll::Ready(Ok(n as usize));
            }
        }

        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
        fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }
}

/// Runs `mbpoll -m rtu` against a device path with a consistent base
/// config (19200 8N1, slave SLAVE_ID, 0-based addressing, one-shot). The
/// device path is the positional "host" argument, so it goes *before* any
/// write values.
fn mbpoll_rtu(device: &std::path::Path, opts: &[&str], values: &[&str]) -> std::process::Output {
    let mut args: Vec<String> = vec![
        "-m".into(),
        "rtu".into(),
        "-b".into(),
        "19200".into(),
        "-d".into(),
        "8".into(),
        "-s".into(),
        "1".into(),
        "-P".into(),
        "none".into(),
        "-a".into(),
        SLAVE_ID.to_string(),
        "-0".into(),
        "-1".into(),
        "-o".into(),
        "2".into(),
    ];
    for e in opts {
        args.push((*e).to_string());
    }
    args.push(device.to_string_lossy().into_owned());
    for v in values {
        args.push((*v).to_string());
    }
    Command::new("mbpoll").args(&args).output().expect("mbpoll")
}

fn mbpoll_rtu_success(device: &std::path::Path, opts: &[&str], values: &[&str]) -> String {
    let out = mbpoll_rtu(device, opts, values);
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(
        out.status.success(),
        "mbpoll rtu failed. opts={opts:?} values={values:?}\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    stdout
}

fn assert_slot(stdout: &str, idx: u32, value: &str) {
    let ok = stdout.lines().any(|l| {
        let l = l.trim_end();
        l.starts_with(&format!("[{idx}]:")) && l.contains(value)
    });
    assert!(ok, "expected [{idx}] = {value} in:\n{stdout}");
}

fn tools_available() -> bool {
    have_tool("mbpoll") && have_tool("socat")
}

/// macOS PTY allocation + socat + tokio's AsyncFd don't scale well when
/// many of these tests hammer the system in parallel — tests start before
/// their bridges are fully wired up and mbpoll hits the read timeout. We
/// serialize RTU tests with a shared mutex. TCP tests in the sibling file
/// still run fully in parallel.
fn rtu_lock() -> std::sync::MutexGuard<'static, ()> {
    static M: std::sync::Mutex<()> = std::sync::Mutex::new(());
    M.lock().unwrap_or_else(|e| e.into_inner())
}

// ===========================================================================
// Read function codes
// ===========================================================================

#[tokio::test(flavor = "multi_thread")]
async fn rtu_fc01_read_coils() {
    let _g = rtu_lock();
    if !tools_available() {
        return;
    }
    let bridge = spawn_socat_bridge();
    let _state = start_simulator(bridge.sim_link.clone()).await;
    let client = bridge.client_link.clone();
    let stdout = tokio::task::spawn_blocking(move || {
        mbpoll_rtu_success(&client, &["-t", "0", "-r", "0", "-c", "8"], &[])
    })
    .await
    .unwrap();
    for i in 0..8 {
        let expected = if i % 2 == 0 { "1" } else { "0" };
        assert_slot(&stdout, i, expected);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn rtu_fc02_read_discrete_inputs() {
    let _g = rtu_lock();
    if !tools_available() {
        return;
    }
    let bridge = spawn_socat_bridge();
    let _state = start_simulator(bridge.sim_link.clone()).await;
    let client = bridge.client_link.clone();
    let stdout = tokio::task::spawn_blocking(move || {
        mbpoll_rtu_success(&client, &["-t", "1", "-r", "0", "-c", "8"], &[])
    })
    .await
    .unwrap();
    for i in 0..8 {
        let expected = if i % 2 == 0 { "0" } else { "1" };
        assert_slot(&stdout, i, expected);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn rtu_fc03_read_holding_registers() {
    let _g = rtu_lock();
    if !tools_available() {
        return;
    }
    let bridge = spawn_socat_bridge();
    let _state = start_simulator(bridge.sim_link.clone()).await;
    let client = bridge.client_link.clone();
    let stdout = tokio::task::spawn_blocking(move || {
        mbpoll_rtu_success(&client, &["-t", "4:hex", "-r", "0", "-c", "4"], &[])
    })
    .await
    .unwrap();
    for v in ["0x1234", "0x5678", "0x9ABC", "0xDEF0"] {
        assert!(stdout.contains(v), "missing {v}:\n{stdout}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn rtu_fc04_read_input_registers() {
    let _g = rtu_lock();
    if !tools_available() {
        return;
    }
    let bridge = spawn_socat_bridge();
    let _state = start_simulator(bridge.sim_link.clone()).await;
    let client = bridge.client_link.clone();
    let stdout = tokio::task::spawn_blocking(move || {
        mbpoll_rtu_success(&client, &["-t", "3:hex", "-r", "0", "-c", "3"], &[])
    })
    .await
    .unwrap();
    for v in ["0xAAAA", "0xBBBB", "0xCCCC"] {
        assert!(stdout.contains(v), "missing {v}:\n{stdout}");
    }
}

// ===========================================================================
// Write function codes
// ===========================================================================

#[tokio::test(flavor = "multi_thread")]
async fn rtu_fc05_write_single_coil() {
    let _g = rtu_lock();
    if !tools_available() {
        return;
    }
    let bridge = spawn_socat_bridge();
    let _state = start_simulator(bridge.sim_link.clone()).await;
    let client = bridge.client_link.clone();
    let c = client.clone();
    tokio::task::spawn_blocking(move || mbpoll_rtu_success(&c, &["-t", "0", "-r", "1"], &["1"]))
        .await
        .unwrap();
    let stdout = tokio::task::spawn_blocking(move || {
        mbpoll_rtu_success(&client, &["-t", "0", "-r", "1", "-c", "1"], &[])
    })
    .await
    .unwrap();
    assert_slot(&stdout, 1, "1");
}

#[tokio::test(flavor = "multi_thread")]
async fn rtu_fc06_write_single_register() {
    let _g = rtu_lock();
    if !tools_available() {
        return;
    }
    let bridge = spawn_socat_bridge();
    let _state = start_simulator(bridge.sim_link.clone()).await;
    let client = bridge.client_link.clone();
    let c = client.clone();
    tokio::task::spawn_blocking(move || {
        mbpoll_rtu_success(&c, &["-t", "4", "-r", "0"], &["54321"])
    })
    .await
    .unwrap();
    let stdout = tokio::task::spawn_blocking(move || {
        mbpoll_rtu_success(&client, &["-t", "4", "-r", "0", "-c", "1"], &[])
    })
    .await
    .unwrap();
    assert_slot(&stdout, 0, "54321");
}

#[tokio::test(flavor = "multi_thread")]
async fn rtu_fc15_write_multiple_coils() {
    let _g = rtu_lock();
    if !tools_available() {
        return;
    }
    let bridge = spawn_socat_bridge();
    let _state = start_simulator(bridge.sim_link.clone()).await;
    let client = bridge.client_link.clone();
    let c = client.clone();
    tokio::task::spawn_blocking(move || {
        mbpoll_rtu_success(
            &c,
            &["-t", "0", "-r", "0"],
            &["0", "1", "1", "0", "0", "1", "1", "1"],
        )
    })
    .await
    .unwrap();
    let stdout = tokio::task::spawn_blocking(move || {
        mbpoll_rtu_success(&client, &["-t", "0", "-r", "0", "-c", "8"], &[])
    })
    .await
    .unwrap();
    for (i, v) in [0, 1, 1, 0, 0, 1, 1, 1].iter().enumerate() {
        assert_slot(&stdout, i as u32, &v.to_string());
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn rtu_fc16_write_multiple_registers() {
    let _g = rtu_lock();
    if !tools_available() {
        return;
    }
    let bridge = spawn_socat_bridge();
    let _state = start_simulator(bridge.sim_link.clone()).await;
    let client = bridge.client_link.clone();
    let c = client.clone();
    tokio::task::spawn_blocking(move || {
        mbpoll_rtu_success(&c, &["-t", "4", "-r", "0"], &["100", "200", "300", "400"])
    })
    .await
    .unwrap();
    let stdout = tokio::task::spawn_blocking(move || {
        mbpoll_rtu_success(&client, &["-t", "4", "-r", "0", "-c", "4"], &[])
    })
    .await
    .unwrap();
    for (i, v) in [100, 200, 300, 400].iter().enumerate() {
        assert_slot(&stdout, i as u32, &v.to_string());
    }
}

// ===========================================================================
// Encoding variants — byte/word order correctness
// ===========================================================================

#[tokio::test(flavor = "multi_thread")]
async fn rtu_u32_big_endian() {
    let _g = rtu_lock();
    if !tools_available() {
        return;
    }
    let bridge = spawn_socat_bridge();
    let _state = start_simulator(bridge.sim_link.clone()).await;
    let client = bridge.client_link.clone();
    let stdout = tokio::task::spawn_blocking(move || {
        mbpoll_rtu_success(&client, &["-t", "4:int", "-B", "-r", "10", "-c", "1"], &[])
    })
    .await
    .unwrap();
    assert!(stdout.contains("287454020"), "{stdout}");
}

#[tokio::test(flavor = "multi_thread")]
async fn rtu_u32_big_endian_word_swap() {
    let _g = rtu_lock();
    if !tools_available() {
        return;
    }
    let bridge = spawn_socat_bridge();
    let _state = start_simulator(bridge.sim_link.clone()).await;
    let client = bridge.client_link.clone();
    // address 12 holds the same value with BigEndianWordSwap; mbpoll's
    // default (without -B) reads this as 287454020
    let stdout = tokio::task::spawn_blocking(move || {
        mbpoll_rtu_success(&client, &["-t", "4:int", "-r", "12", "-c", "1"], &[])
    })
    .await
    .unwrap();
    assert!(stdout.contains("287454020"), "{stdout}");
}

#[tokio::test(flavor = "multi_thread")]
async fn rtu_i32_negative() {
    let _g = rtu_lock();
    if !tools_available() {
        return;
    }
    let bridge = spawn_socat_bridge();
    let _state = start_simulator(bridge.sim_link.clone()).await;
    let client = bridge.client_link.clone();
    let stdout = tokio::task::spawn_blocking(move || {
        mbpoll_rtu_success(&client, &["-t", "4:int", "-B", "-r", "20", "-c", "1"], &[])
    })
    .await
    .unwrap();
    assert!(stdout.contains("-123456"), "{stdout}");
}

#[tokio::test(flavor = "multi_thread")]
async fn rtu_f32_big_endian() {
    let _g = rtu_lock();
    if !tools_available() {
        return;
    }
    let bridge = spawn_socat_bridge();
    let _state = start_simulator(bridge.sim_link.clone()).await;
    let client = bridge.client_link.clone();
    let stdout = tokio::task::spawn_blocking(move || {
        mbpoll_rtu_success(
            &client,
            &["-t", "4:float", "-B", "-r", "30", "-c", "1"],
            &[],
        )
    })
    .await
    .unwrap();
    assert!(stdout.contains("3.5"), "{stdout}");
}

#[tokio::test(flavor = "multi_thread")]
async fn rtu_string_read_raw_words() {
    let _g = rtu_lock();
    if !tools_available() {
        return;
    }
    let bridge = spawn_socat_bridge();
    let _state = start_simulator(bridge.sim_link.clone()).await;
    let client = bridge.client_link.clone();
    let stdout = tokio::task::spawn_blocking(move || {
        mbpoll_rtu_success(&client, &["-t", "4:string", "-r", "60", "-c", "4"], &[])
    })
    .await
    .unwrap();
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
    assert_eq!(assembled, "ABCDefgh", "got {assembled:?} stdout:\n{stdout}");
}

#[tokio::test(flavor = "multi_thread")]
async fn rtu_u64_big_endian_raw_words() {
    let _g = rtu_lock();
    if !tools_available() {
        return;
    }
    let bridge = spawn_socat_bridge();
    let _state = start_simulator(bridge.sim_link.clone()).await;
    let client = bridge.client_link.clone();
    let stdout = tokio::task::spawn_blocking(move || {
        mbpoll_rtu_success(&client, &["-t", "4:hex", "-r", "40", "-c", "4"], &[])
    })
    .await
    .unwrap();
    let up = stdout.to_uppercase();
    for v in ["0X0102", "0X0304", "0X0506", "0X0708"] {
        assert!(up.contains(v), "missing {v}:\n{stdout}");
    }
}

// ===========================================================================
// Error paths
// ===========================================================================

#[tokio::test(flavor = "multi_thread")]
async fn rtu_unknown_slave_times_out() {
    let _g = rtu_lock();
    if !tools_available() {
        return;
    }
    let bridge = spawn_socat_bridge();
    let _state = start_simulator(bridge.sim_link.clone()).await;
    // Query slave 200 — simulator is silent, mbpoll times out quickly.
    let client = bridge.client_link.clone();
    let out = tokio::task::spawn_blocking(move || {
        Command::new("mbpoll")
            .args([
                "-m",
                "rtu",
                "-b",
                "19200",
                "-P",
                "none",
                "-a",
                "200",
                "-t",
                "4",
                "-0",
                "-r",
                "0",
                "-c",
                "1",
                "-1",
                "-o",
                "0.3",
                &client.to_string_lossy(),
            ])
            .output()
            .expect("mbpoll")
    })
    .await
    .unwrap();
    let lower = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
    .to_lowercase();
    assert!(
        lower.contains("timed out") || lower.contains("timeout") || !out.status.success(),
        "expected timeout, got:\n{lower}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn rtu_missing_address_reports_exception() {
    let _g = rtu_lock();
    if !tools_available() {
        return;
    }
    let bridge = spawn_socat_bridge();
    let _state = start_simulator(bridge.sim_link.clone()).await;
    let client = bridge.client_link.clone();
    let out = tokio::task::spawn_blocking(move || {
        mbpoll_rtu(&client, &["-t", "4", "-r", "500", "-c", "2"], &[])
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
