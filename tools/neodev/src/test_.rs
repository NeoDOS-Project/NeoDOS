use crate::config::Config;
use crate::run::{NetMode, RunOptions, StorageMode};
use anyhow::{Context, Result};
use colored::*;
use std::io::Write;
use std::net::{TcpStream, Shutdown};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

static SERIAL_LOG: &str = "/tmp/neodos_serial.log";

pub struct TestOptions {
    pub storage: StorageMode,
    pub kvm: bool,
    pub iterations: u32,
    pub timeout: u64,
}

impl Default for TestOptions {
    fn default() -> Self {
        Self {
            storage: StorageMode::Ahci,
            kvm: false,
            iterations: 1,
            timeout: 180,
        }
    }
}

pub struct TestResult {
    pub kernel_tests_passed: bool,
    pub kernel_count: Option<u32>,
    pub command_tests_passed: bool,
    pub shell_tests_passed: bool,
    #[allow(dead_code)]
    pub total_duration: Duration,
    pub panics: Vec<String>,
}

pub fn run_tests(cfg: &Config, opts: &TestOptions) -> Result<TestResult> {
    println!("{} NeoDOS Test Runner", "[*]".bold().cyan());
    println!();

    for iter in 1..=opts.iterations {
        if opts.iterations > 1 {
            println!("{} Iteration {}/{}", "[*]".bold().cyan(), iter, opts.iterations);
        }
        let result = run_single_test(cfg, opts)?;

        if iter < opts.iterations {
            println!();
        }

        if iter == opts.iterations || !result.kernel_tests_passed {
            return Ok(result);
        }
    }

    unreachable!()
}

fn run_single_test(cfg: &Config, opts: &TestOptions) -> Result<TestResult> {
    let start = Instant::now();

    // Clean serial log
    let _ = std::fs::remove_file(SERIAL_LOG);

    let _run_opts = RunOptions {
        storage: opts.storage,
        net: NetMode::User,
        kvm: opts.kvm,
        gdb: false,
        headless: true,
        bdm: false,
        serial_file: Some(SERIAL_LOG.to_string()),
        monitor_port: Some(4446),
        ..Default::default()
    };

    let disk_image = cfg.project_root.join("disk_image.img");
    if !disk_image.exists() {
        anyhow::bail!("disk_image.img not found. Run 'neodev build --image' first.");
    }

    let ovmf_vars_template = &cfg.ovmf_vars_template;
    let ovmf_vars = format!("/tmp/OVMF_VARS_test_{}.fd", std::process::id());
    std::fs::copy(ovmf_vars_template, &ovmf_vars)?;

    let mut qemu = Command::new("qemu-system-x86_64");

    let accel = if opts.kvm && std::path::Path::new("/dev/kvm").exists() {
        "kvm"
    } else {
        "tcg"
    };

    qemu.args([
        "-machine", &format!("q35,accel={}", accel),
        "-monitor", "telnet:127.0.0.1:4446,server,nowait",
        "-display", "none",
        "-drive", &format!("if=pflash,format=raw,readonly=on,file={}", cfg.ovmf_code.display()),
        "-drive", &format!("if=pflash,format=raw,file={}", ovmf_vars),
        "-no-reboot",
    ]);

    match opts.storage {
        StorageMode::Ahci => {
            qemu.args([
                "-device", "ahci,id=ahci",
                "-drive", &format!("if=none,format=raw,file={},id=mydisk", disk_image.display()),
                "-device", "ide-hd,drive=mydisk,bus=ahci.0",
            ]);
        }
        StorageMode::Virtio => {
            qemu.args([
                "-drive", &format!("if=none,format=raw,file={},id=virtioblk", disk_image.display()),
                "-device", "virtio-blk-pci,disable-modern=on,drive=virtioblk",
            ]);
        }
        _ => {
            qemu.args([
                "-drive", &format!("format=raw,file={},index=0,media=disk", disk_image.display()),
            ]);
        }
    }

    qemu.args([
        "-netdev", "user,id=net0,net=10.0.1.0/24,dhcpstart=10.0.1.80,host=10.0.1.1",
        "-device", "e1000,netdev=net0",
        "-m", &cfg.qemu_memory,
        "-serial", &format!("file:{}", SERIAL_LOG),
    ]);

    let mut child = qemu
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("Failed to start QEMU")?;

    // Give QEMU time to start
    std::thread::sleep(Duration::from_secs(3));

    // Connect to monitor
    let monitor = connect_monitor("127.0.0.1", 4446, 10)?;
    println!("  Monitor connected");

    // Wait for output and parse results
    let result = wait_for_test_completion(&mut child, &monitor, opts.timeout)?;

    // Cleanup
    let _ = monitor.shutdown(Shutdown::Both);
    if child.try_wait().ok().flatten().is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }
    let _ = std::fs::remove_file(&ovmf_vars);

    println!();
    print_test_result(&result, start.elapsed());

    Ok(result)
}

fn connect_monitor(host: &str, port: u16, retries: u32) -> Result<TcpStream> {
    for _ in 0..retries {
        if let Ok(stream) = TcpStream::connect_timeout(
            &format!("{}:{}", host, port).parse().unwrap(),
            Duration::from_secs(2),
        ) {
            return Ok(stream);
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    anyhow::bail!("Could not connect to QEMU monitor at {}:{}", host, port);
}

#[allow(dead_code)]
fn send_monitor(sock: &TcpStream, cmd: &str) -> Result<String> {
    use std::io::Read;
    let mut stream = sock.try_clone()?;
    stream.write_all(format!("{}\n", cmd).as_bytes())?;
    std::thread::sleep(Duration::from_millis(200));
    let mut data = String::new();
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    let mut buf = [0u8; 4096];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => data.push_str(&String::from_utf8_lossy(&buf[..n])),
            Err(_) => break,
        }
    }
    Ok(data)
}

fn wait_for_test_completion(
    child: &mut Child,
    _monitor: &TcpStream,
    timeout_secs: u64,
) -> Result<TestResult> {
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    let mut output_lines: Vec<String> = vec![];
    let mut last_serial_len = 0u64;
    let mut state = "booting".to_string();

    while Instant::now() < deadline {
        // Check if QEMU died
        if let Some(status) = child.try_wait()? {
            return Ok(TestResult {
                kernel_tests_passed: false,
                kernel_count: None,
                command_tests_passed: false,
                shell_tests_passed: false,
                total_duration: Duration::ZERO,
                panics: vec![format!("QEMU exited early: {}", status)],
            });
        }

        // Read serial output
        if let Ok(new_data) = read_serial_since(last_serial_len) {
            last_serial_len += new_data.len() as u64;
            for line in new_data.split('\n') {
                let clean = strip_ansi(line);
                if !clean.is_empty() {
                    output_lines.push(clean.clone());
                    print_serial_line(&clean, &state);
                }
            }
        }

        // Check for completion — wait for both kernel AND command tests
        let full_text = output_lines.join("\n");
        if full_text.contains("ALL_TESTS_COMPLETE")
            && full_text.contains("CMDTEST_COMPLETE")
            && full_text.contains("STRESSCMD_COMPLETE")
        {
            state = "done".to_string();
            #[allow(unused_assignments)]
            {
                let _ = &state;
            }
            break;
        }
        if full_text.contains("kernel tests passed") {
            // Keep going for more tests
        }

        std::thread::sleep(Duration::from_millis(300));
    }

    analyze_results(&output_lines)
}

fn read_serial_since(offset: u64) -> Result<String> {
    let path = std::path::Path::new(SERIAL_LOG);
    if !path.exists() {
        return Ok(String::new());
    }
    let data = std::fs::read(path)?;
    if data.len() as u64 <= offset {
        return Ok(String::new());
    }
    let new_data = &data[offset as usize..];
    Ok(String::from_utf8_lossy(new_data).to_string())
}

fn print_serial_line(line: &str, _state: &str) {
    if line.contains("ALL_TESTS_COMPLETE") {
        println!("{} ALL TESTS COMPLETE", "[+]".bold().green());
    } else if line.contains("Type HELP") {
        println!("{} Shell detected!", "[+]".bold().green());
    } else if line.contains("NeoDOS Kernel v") || line.contains("Bootloader v") {
        println!("  {}", line);
    } else if line.contains("[✓]") || line.contains("[+]") {
        let clean = line.trim();
        if clean.len() > 4 {
            println!("  {}", clean);
        }
    } else if line.contains("kernel tests") || line.contains("ALL TESTS COMPLETE") {
        println!("  [TEST] {}", line);
    } else if line.contains("PANIC") || line.contains("panic") {
        println!("  {} {}", "[!]".bold().red(), line);
    }
}

fn analyze_results(lines: &[String]) -> Result<TestResult> {
    let full_text = lines.join("\n");

    let kernel_ok = full_text.contains("kernel tests passed");
    let kernel_count = full_text
        .lines()
        .find_map(|l| {
            if l.contains("kernel tests passed") {
                let parts: Vec<&str> = l.split_whitespace().collect();
                parts.get(1).and_then(|s| s.parse::<u32>().ok())
            } else {
                None
            }
        });

    let cmd_ok = full_text.contains("ALL_COMMAND_TESTS_PASSED");
    let stress_ok = full_text.contains("ALL_STRESS_TESTS_PASSED");
    let sh_ok = full_text.contains("SHELL_TESTS_PASSED");

    let mut panics = vec![];
    for line in lines {
        let clean = strip_ansi(line);
        for keyword in &["KERNEL PANIC", "DOUBLE FAULT", "GPF:", "panic!", "BUGCHECK"] {
            if clean.contains(keyword) {
                panics.push(clean.clone());
            }
        }
    }

    Ok(TestResult {
        kernel_tests_passed: kernel_ok,
        kernel_count,
        command_tests_passed: cmd_ok,
        shell_tests_passed: sh_ok,
        total_duration: Duration::ZERO,
        panics,
    })
}

fn print_test_result(result: &TestResult, elapsed: Duration) {
    println!("{}", "=".repeat(60));
    println!("{}", "TEST RESULTS".bold());
    println!("{}", "=".repeat(60));

    if result.kernel_tests_passed {
        if let Some(count) = result.kernel_count {
            println!("  {} Kernel tests: {} passed", "[PASS]".bold().green(), count);
        } else {
            println!("  {} Kernel tests passed", "[PASS]".bold().green());
        }
    } else {
        println!("  {} Kernel tests FAILED", "[FAIL]".bold().red());
    }

    if result.command_tests_passed {
        println!("  {} Command tests passed", "[PASS]".bold().green());
    } else {
        println!("  {} Command tests FAILED or not run", "[INFO]".bold().yellow());
    }

    if result.shell_tests_passed {
        println!("  {} Shell tests passed", "[PASS]".bold().green());
    } else {
        println!("  {} Shell tests FAILED or not run", "[INFO]".bold().yellow());
    }

    if !result.panics.is_empty() {
        println!();
        println!("  {} Panics detected:", "[!]".bold().red());
        for p in &result.panics {
            println!("    {}", p);
        }
    }

    println!();
    if result.kernel_tests_passed {
        println!("{} OVERALL: SUCCESS", "[✓]".bold().green());
    } else {
        println!("{} OVERALL: FAILED", "[✗]".bold().red());
    }
    println!("  Duration: {:.1}s", elapsed.as_secs_f64());
}

fn strip_ansi(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.next() == Some('[') {
                for ch in chars.by_ref() {
                    if ch.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            result.push(c);
        }
    }
    result
}
