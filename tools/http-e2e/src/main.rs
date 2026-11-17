//! `http-e2e` — host-side driver for the NeoDOS HTTP acceptance test.
//!
//! Starts a fixture HTTP/1.1 server, boots the VM headless, types
//! `httptest <base-url>` into the guest console over the serial log, and
//! reports the `HTTP-E2E:` summary line.
//!
//! ```text
//! cargo run --release --manifest-path tools/http-e2e/Cargo.toml -- \
//!     --neodev /path/to/neodev --backend virtualbox
//! ```
//!
//! The fixture implements the endpoints `httptest` expects (`/hello`,
//! `/chunked`, `/post`, `/status/NNN`, `/close`, `/big`, `/huge`, `/slow`), so
//! no Python server or network access to the host's public IP is needed.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

const BIG: usize = 65_536;
const HUGE: usize = 1_048_576;
const SLOW_DELAY: Duration = Duration::from_millis(3000);
const VM_NAME: &str = "NeoDOS";

fn main() {
    let opts = match Options::parse() {
        Ok(opts) => opts,
        Err(err) => {
            eprintln!("http-e2e: {err}");
            std::process::exit(2);
        }
    };
    match run(&opts) {
        Ok(true) => std::process::exit(0),
        Ok(false) => std::process::exit(1),
        Err(err) => {
            eprintln!("http-e2e: {err}");
            std::process::exit(2);
        }
    }
}

struct Options {
    port: u16,
    guest_host: String,
    backend: String,
    neodev: String,
    repo: PathBuf,
    serial: PathBuf,
    boot_timeout: Duration,
    test_timeout: Duration,
    wait_sync: bool,
    keep: bool,
}

impl Options {
    fn parse() -> Result<Self, String> {
        let mut opts = Options {
            port: 18_080,
            guest_host: "10.0.2.2".into(),
            backend: "virtualbox".into(),
            neodev: "neodev".into(),
            repo: std::env::current_dir().map_err(|e| e.to_string())?,
            serial: std::env::temp_dir().join("http-e2e-serial.log"),
            boot_timeout: Duration::from_secs(240),
            test_timeout: Duration::from_secs(300),
            wait_sync: false,
            keep: false,
        };
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            let mut value = || args.next().ok_or_else(|| format!("{arg} needs a value"));
            match arg.as_str() {
                "--port" => opts.port = value()?.parse().map_err(|_| "invalid --port")?,
                "--guest-host" => opts.guest_host = value()?,
                "--backend" => opts.backend = value()?,
                "--neodev" => opts.neodev = value()?,
                "--repo" => opts.repo = PathBuf::from(value()?),
                "--serial" => opts.serial = PathBuf::from(value()?),
                "--boot-timeout" => {
                    opts.boot_timeout = Duration::from_secs(value()?.parse().map_err(|_| "invalid")?)
                }
                "--test-timeout" => {
                    opts.test_timeout = Duration::from_secs(value()?.parse().map_err(|_| "invalid")?)
                }
                "--wait-sync" => opts.wait_sync = true,
                "--keep" => opts.keep = true,
                "--help" | "-h" => {
                    println!(
                        "usage: http-e2e [--port N] [--guest-host IP] [--backend vbox] \
                         [--neodev PATH] [--repo DIR] [--serial FILE] [--boot-timeout S] \
                         [--test-timeout S] [--wait-sync] [--keep]"
                    );
                    std::process::exit(0);
                }
                other => return Err(format!("unknown option: {other}")),
            }
        }
        Ok(opts)
    }
}

fn run(opts: &Options) -> Result<bool, String> {
    let listener = TcpListener::bind(("0.0.0.0", opts.port))
        .map_err(|e| format!("cannot bind fixture port {}: {e}", opts.port))?;
    listener
        .set_nonblocking(false)
        .map_err(|e| e.to_string())?;
    let fixture = Arc::new(listener);
    {
        let fixture = Arc::clone(&fixture);
        thread::spawn(move || serve(fixture));
    }
    println!("fixture: http://0.0.0.0:{}", opts.port);

    let _ = Command::new("VBoxManage")
        .args(["controlvm", VM_NAME, "poweroff"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    thread::sleep(Duration::from_secs(2));
    let _ = std::fs::remove_file(&opts.serial);

    let log = opts.serial.with_extension("neodev.log");
    let log_file = std::fs::File::create(&log).map_err(|e| e.to_string())?;
    let mut child = Command::new(&opts.neodev)
        .current_dir(&opts.repo)
        .args([
            "run",
            "--backend",
            &opts.backend,
            "--net",
            "user",
            "--headless",
            "--serial",
        ])
        .arg(&opts.serial)
        .stdout(log_file.try_clone().map_err(|e| e.to_string())?)
        .stderr(log_file)
        .spawn()
        .map_err(|e| format!("cannot start {}: {e}", opts.neodev))?;

    let result = (|| -> Result<bool, String> {
        wait_for_boot(&opts.serial, opts.boot_timeout)?;
        thread::sleep(Duration::from_secs(2));
        if opts.wait_sync {
            wait_for_sync(&opts.serial, Duration::from_secs(120))?;
        }
        let command = format!("httptest http://{}:{}", opts.guest_host, opts.port);
        println!("typing: {command}");
        type_text(&command)?;
        let summary = wait_for_summary(&opts.serial, opts.test_timeout)?;
        println!("{summary}");
        Ok(summary.contains("HTTP-E2E: PASS"))
    })();

    if !opts.keep {
        let _ = Command::new("VBoxManage")
            .args(["controlvm", VM_NAME, "poweroff"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.wait();
    result
}

fn read(path: &Path) -> String {
    std::fs::read(path)
        .map(|raw| String::from_utf8_lossy(&raw).into_owned())
        .unwrap_or_default()
}

fn prompts(path: &Path) -> usize {
    read(path).matches("[READB] blocking").count()
}

fn wait_for_boot(path: &Path, timeout: Duration) -> Result<(), String> {
    let start = Instant::now();
    while start.elapsed() < timeout {
        let text = read(path);
        if prompts(path) >= 1 && text.contains("C:\\>") {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(500));
    }
    Err(format!(
        "guest did not reach the shell prompt in {}s (serial: {})",
        timeout.as_secs(),
        path.display()
    ))
}

/// Wait until `ntpd` reports a successful step. The test does not depend on it,
/// but clock-policy checks (`CLOCK: trusted`) do.
fn wait_for_sync(path: &Path, timeout: Duration) -> Result<(), String> {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if read(path).contains("[ntpd] synced") {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(500));
    }
    Err(format!(
        "ntpd did not report a sync in {}s (serial: {})",
        timeout.as_secs(),
        path.display()
    ))
}

fn wait_for_summary(path: &Path, timeout: Duration) -> Result<String, String> {
    let start = Instant::now();
    while start.elapsed() < timeout {
        for line in read(path).lines() {
            if let Some(pos) = line.find("HTTP-E2E:") {
                return Ok(line[pos..].trim().to_string());
            }
        }
        thread::sleep(Duration::from_millis(500));
    }
    Err(format!(
        "no HTTP-E2E summary within {}s (serial: {})",
        timeout.as_secs(),
        path.display()
    ))
}

/// Type ASCII text into the guest with set-1 scancodes.
fn type_text(text: &str) -> Result<(), String> {
    for ch in text.chars() {
        let codes = scancodes(ch)?;
        let mut cmd = Command::new("VBoxManage");
        cmd.args(["controlvm", VM_NAME, "keyboardputscancode"]);
        for code in &codes {
            cmd.arg(format!("{code:02x}"));
        }
        let status = cmd.stdout(Stdio::null()).stderr(Stdio::null()).status();
        if !status.map(|s| s.success()).unwrap_or(false) {
            return Err(format!("keyboardputscancode failed for {ch:?}"));
        }
        thread::sleep(Duration::from_millis(30));
    }
    let status = Command::new("VBoxManage")
        .args([
            "controlvm",
            VM_NAME,
            "keyboardputscancode",
            "1c",
            "9c",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if !status.map(|s| s.success()).unwrap_or(false) {
        return Err("keyboardputscancode failed for Enter".into());
    }
    Ok(())
}

const SHIFT_MAKE: u8 = 0x2A;
const SHIFT_BREAK: u8 = 0xAA;

fn scancodes(ch: char) -> Result<Vec<u8>, String> {
    let (code, shift) = match ch {
        '1'..='9' => (0x02 + (ch as u8 - b'1'), false),
        '0' => (0x0B, false),
        'a'..='z' => (letter(ch), false),
        'A'..='Z' => (letter(ch.to_ascii_lowercase()), true),
        ' ' => (0x39, false),
        '.' => (0x34, false),
        '/' => (0x35, false),
        '-' => (0x0C, false),
        '=' => (0x0D, false),
        '\\' => (0x2B, false),
        ':' => (0x27, true),
        '_' => (0x0C, true),
        other => return Err(format!("unsupported character {other:?}")),
    };
    let mut out = Vec::with_capacity(4);
    if shift {
        out.extend([SHIFT_MAKE, code, code | 0x80, SHIFT_BREAK]);
    } else {
        out.extend([code, code | 0x80]);
    }
    Ok(out)
}

fn letter(ch: char) -> u8 {
    match ch {
        'q' => 0x10,
        'w' => 0x11,
        'e' => 0x12,
        'r' => 0x13,
        't' => 0x14,
        'y' => 0x15,
        'u' => 0x16,
        'i' => 0x17,
        'o' => 0x18,
        'p' => 0x19,
        'a' => 0x1E,
        's' => 0x1F,
        'd' => 0x20,
        'f' => 0x21,
        'g' => 0x22,
        'h' => 0x23,
        'j' => 0x24,
        'k' => 0x25,
        'l' => 0x26,
        'z' => 0x2C,
        'x' => 0x2D,
        'c' => 0x2E,
        'v' => 0x2F,
        'b' => 0x30,
        'n' => 0x31,
        'm' => 0x32,
        _ => 0x1E,
    }
}

// ── Fixture server ──

fn serve(listener: Arc<TcpListener>) {
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        thread::spawn(move || {
            let _ = handle(stream);
        });
    }
}

fn handle(mut stream: TcpStream) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let request = read_request(&mut stream)?;
    let Some((method, path)) = request_line(&request) else {
        return respond(&mut stream, 400, CT_TEXT, b"bad request", true);
    };
    let body = request_body(&request);

    match (method.as_str(), path.as_str()) {
        ("GET", "/hello") => respond(&mut stream, 200, CT_TEXT, b"hello-from-host\n", true),
        ("HEAD", "/hello") => respond(&mut stream, 200, CT_TEXT, b"", true),
        ("GET", "/chunked") => respond_chunked(&mut stream),
        ("GET", "/close") => respond(&mut stream, 200, CT_TEXT, b"close-delimited-body\n", false),
        ("GET", "/big") => {
            let body = vec![b'x'; BIG];
            respond(&mut stream, 200, CT_BIN, &body, true)
        }
        ("GET", "/huge") => {
            let body = vec![b'y'; HUGE];
            respond(&mut stream, 200, CT_BIN, &body, true)
        }
        ("GET", "/slow") => {
            thread::sleep(SLOW_DELAY);
            respond(&mut stream, 200, CT_TEXT, b"slow-response\n", true)
        }
        ("POST", "/post") => {
            let mut echo = format!("POST {} ", body.len()).into_bytes();
            echo.extend_from_slice(&body);
            respond(&mut stream, 200, CT_TEXT, &echo, true)
        }
        ("GET", p) if p.starts_with("/status/") => {
            let code = p[8..].parse::<u16>().unwrap_or(404);
            let mut body = format!("status-{code}").into_bytes();
            respond(&mut stream, code, CT_TEXT, &body, true)
        }
        _ => {
            let mut body = b"status-404".to_vec();
            respond(&mut stream, 404, CT_TEXT, &body, true)
        }
    }
}

const CT_TEXT: &str = "text/plain";
const CT_BIN: &str = "application/octet-stream";

fn read_request(stream: &mut TcpStream) -> std::io::Result<String> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 512];
    loop {
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..pos + 4]).into_owned();
            let len = content_length(&head);
            let mut body = buf[pos + 4..].to_vec();
            while body.len() < len {
                let n = stream.read(&mut chunk)?;
                if n == 0 {
                    break;
                }
                body.extend_from_slice(&chunk[..n]);
            }
            let mut out = head;
            out.push_str(&String::from_utf8_lossy(&body[..len.min(body.len())]));
            return Ok(out);
        }
        if buf.len() > 64 * 1024 {
            break;
        }
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn content_length(head: &str) -> usize {
    for line in head.lines() {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-length") {
            return value.trim().parse().unwrap_or(0);
        }
    }
    0
}

fn request_line(request: &str) -> Option<(String, String)> {
    let line = request.lines().next()?;
    let mut parts = line.split_whitespace();
    let method = parts.next()?.to_string();
    let target = parts.next()?;
    let path = target.split('?').next().unwrap_or(target).to_string();
    Some((method, path))
}

fn request_body(request: &str) -> Vec<u8> {
    let Some(pos) = request.find("\r\n\r\n") else {
        return Vec::new();
    };
    request.as_bytes()[pos + 4..].to_vec()
}

fn respond(
    stream: &mut TcpStream,
    code: u16,
    kind: &str,
    body: &[u8],
    with_length: bool,
) -> std::io::Result<()> {
    let mut head = format!(
        "HTTP/1.1 {code} {}\r\nContent-Type: {kind}\r\nConnection: close\r\n",
        reason(code)
    );
    if with_length {
        head.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()?;
    Ok(())
}

fn respond_chunked(stream: &mut TcpStream) -> std::io::Result<()> {
    let head = "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\
                Transfer-Encoding: chunked\r\nConnection: close\r\n\r\n";
    stream.write_all(head.as_bytes())?;
    for chunk in [&b"chunk-one-"[..], &b"chunk-two-"[..], &b"chunk-three\n"[..]] {
        stream.write_all(format!("{:x}\r\n", chunk.len()).as_bytes())?;
        stream.write_all(chunk)?;
        stream.write_all(b"\r\n")?;
    }
    stream.write_all(b"0\r\nX-Fixed: 1\r\n\r\n")?;
    stream.flush()
}

fn reason(code: u16) -> &'static str {
    match code {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Status",
    }
}
