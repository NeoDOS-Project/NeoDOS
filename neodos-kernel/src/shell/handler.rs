use crate::shell::shell::DosShell;

pub struct CommandEntry {
    pub name: &'static str,
    pub category: &'static str,
    pub description: &'static str,
    pub usage: &'static str,
    pub handler: fn(&mut DosShell, &[&str]),
}

pub struct CommandRegistry {
    commands: &'static [CommandEntry],
}

impl CommandRegistry {
    pub const fn new(commands: &'static [CommandEntry]) -> Self {
        Self { commands }
    }

    pub fn dispatch(&self, cmd: &str, args: &[&str], shell: &mut DosShell) -> bool {
        for entry in self.commands {
            if cmd.eq_ignore_ascii_case(entry.name) {
                if !args.is_empty() && (args[0] == "/?" || args[0] == "-h" || args[0] == "--help") {
                    crate::println!("{}", entry.usage);
                } else {
                    (entry.handler)(shell, args);
                }
                return true;
            }
        }
        false
    }

    pub fn print_command_help(&self, name: &str) -> bool {
        for entry in self.commands {
            if name.eq_ignore_ascii_case(entry.name) {
                crate::println!("{}", entry.usage);
                return true;
            }
        }
        false
    }

    pub fn names_starting_with(&self, prefix: &str) -> alloc::vec::Vec<&'static str> {
        let mut result = alloc::vec::Vec::new();
        for entry in self.commands {
            if entry.name.len() >= prefix.len()
                && entry.name[..prefix.len()].eq_ignore_ascii_case(prefix)
                && !result.contains(&entry.name)
            {
                result.push(entry.name);
            }
        }
        result
    }

    pub fn print_help(&self) {
        use crate::println;

        let categories: [(&str, &str); 8] = [
            ("FILE",     "FILE MANAGEMENT"),
            ("DISK",     "NAVIGATION & DISKS"),
            ("INFO",     "SYSTEM INFO"),
            ("CONFIG",   "CONFIGURATION"),
            ("CTRL",     "SYSTEM CONTROL"),
            ("SHUTDOWN", "SHUTDOWN"),
            ("MISC",     "MISC"),
            ("",         ""),
        ];

        println!("========================================");
        println!("         N e o D O S  H E L P");
        println!("========================================\r\n");

        for (cat_key, cat_title) in &categories {
            if cat_key.is_empty() {
                continue;
            }
            let mut has = false;
            for e in self.commands {
                if e.category == *cat_key {
                    has = true;
                    break;
                }
            }
            if !has {
                continue;
            }

            println!("  == {} ==", cat_title);
            for e in self.commands {
                if e.category == *cat_key {
                    if let Some(pad) = 8usize.checked_sub(e.name.len()) {
                        let spaces = "                ";
                        println!("  {} {} {}", e.name, &spaces[..pad], e.description);
                    } else {
                        println!("  {}  {}", e.name, e.description);
                    }
                }
            }
            println!();
        }

        println!("========================================");
    }
}

// Command shims
pub fn cmd_help(shell: &mut DosShell, _args: &[&str]) { shell.cmd_help(_args); }
pub fn cmd_dir(shell: &mut DosShell, args: &[&str]) { shell.cmd_dir(args); }
pub fn cmd_type(shell: &mut DosShell, args: &[&str]) { shell.cmd_type(args); }
pub fn cmd_echo(shell: &mut DosShell, args: &[&str]) { shell.cmd_echo(args); }
pub fn cmd_set(shell: &mut DosShell, args: &[&str]) { shell.cmd_set(args); }
pub fn cmd_keyb(shell: &mut DosShell, args: &[&str]) { shell.cmd_keyb(args); }
pub fn cmd_cpuinfo(shell: &mut DosShell, _args: &[&str]) { shell.cmd_cpuinfo(); }
pub fn cmd_mem(shell: &mut DosShell, args: &[&str]) { shell.cmd_mem(args); }
pub fn cmd_cd(shell: &mut DosShell, args: &[&str]) { shell.cmd_cd(args); }
pub fn cmd_call(shell: &mut DosShell, args: &[&str]) { shell.cmd_call(args); }
pub fn cmd_copy(shell: &mut DosShell, args: &[&str]) { shell.cmd_copy(args); }
pub fn cmd_md(shell: &mut DosShell, args: &[&str]) { shell.cmd_md(args); }
pub fn cmd_vol(shell: &mut DosShell, args: &[&str]) { shell.cmd_vol(args); }
pub fn cmd_drives(shell: &mut DosShell, _args: &[&str]) { shell.cmd_drives(); }
pub fn cmd_label(shell: &mut DosShell, args: &[&str]) { shell.cmd_label(args); }
pub fn cmd_sync(_shell: &mut DosShell, _args: &[&str]) {
    crate::println!("Syncing disk...");
    crate::globals::NEED_CACHE_FLUSH.store(true, core::sync::atomic::Ordering::Relaxed);
    crate::globals::flush_cache_if_needed();
}
pub fn cmd_del(shell: &mut DosShell, args: &[&str]) { shell.cmd_del(args); }
pub fn cmd_ren(shell: &mut DosShell, args: &[&str]) { shell.cmd_rename(args); }
pub fn cmd_rd(shell: &mut DosShell, args: &[&str]) { shell.cmd_rd(args); }
pub fn cmd_ver(_shell: &mut DosShell, _args: &[&str]) { crate::println!("NeoDOS v{}", env!("CARGO_PKG_VERSION")); }
pub fn cmd_tsr(shell: &mut DosShell, args: &[&str]) { shell.cmd_tsr(args); }
pub fn cmd_devices(shell: &mut DosShell, _args: &[&str]) { shell.cmd_devices(); }
pub fn cmd_test(shell: &mut DosShell, args: &[&str]) { shell.cmd_test(args); }
pub fn cmd_date(shell: &mut DosShell, args: &[&str]) { shell.cmd_date(args); }
pub fn cmd_time(shell: &mut DosShell, args: &[&str]) { shell.cmd_time(args); }
pub fn cmd_attrib(shell: &mut DosShell, args: &[&str]) { shell.cmd_attrib(args); }
pub fn cmd_ps(shell: &mut DosShell, _args: &[&str]) { shell.cmd_ps(); }
pub fn cmd_kill(shell: &mut DosShell, args: &[&str]) { shell.cmd_kill(args); }
pub fn cmd_cls(_shell: &mut DosShell, _args: &[&str]) { crate::console::clear_screen(); }
pub fn cmd_run(shell: &mut DosShell, args: &[&str]) { shell.cmd_run(args); }
pub fn cmd_load(shell: &mut DosShell, args: &[&str]) { shell.cmd_load(args); }
pub fn cmd_devicesend(shell: &mut DosShell, args: &[&str]) { shell.cmd_devicesend(args); }
pub fn cmd_exit(shell: &mut DosShell, args: &[&str]) { shell.cmd_shutdown(args); }
pub fn cmd_shutdown(shell: &mut DosShell, args: &[&str]) { shell.cmd_shutdown(args); }

pub const COMMANDS: CommandRegistry = CommandRegistry::new(&[
    CommandEntry { name: "HELP",     category: "CTRL",     handler: cmd_help,    description: "Show this help",
        usage: "HELP [command]\r\n  Show general help or detailed help for a specific command.\r\n  HELP DIR   shows detailed help for DIR.", },
    CommandEntry { name: "CLS",      category: "CTRL",     handler: cmd_cls,     description: "Clear screen",
        usage: "CLS\r\n  Clear the screen.", },
    CommandEntry { name: "DIR",      category: "DISK",     handler: cmd_dir,     description: "List directory contents",
        usage: "DIR [path]\r\n  List files and directories. Shows name, size, attributes, permissions, and timestamps.\r\n  DIR  C:\\BIN   lists contents of C:\\BIN.", },
    CommandEntry { name: "TYPE",     category: "FILE",     handler: cmd_type,    description: "Display file contents",
        usage: "TYPE [drive:][path]filename\r\n  Display the contents of a text file.", },
    CommandEntry { name: "ECHO",     category: "CONFIG",   handler: cmd_echo,    description: "Print text with %VAR% expansion",
        usage: "ECHO [text]\r\n  Print text, expanding %VAR% environment variables.\r\n  ECHO %PATH%  shows the current PATH.", },
    CommandEntry { name: "SET",      category: "CONFIG",   handler: cmd_set,     description: "Display/set environment variables",
        usage: "SET [var[=value]]\r\n  SET              lists all variables.\r\n  SET PATH=C:\\BIN  sets PATH to C:\\BIN.\r\n  SET PATH=        removes PATH.", },
    CommandEntry { name: "KEYB",     category: "CONFIG",   handler: cmd_keyb,    description: "Change layout (KEYB US|SP)",
        usage: "KEYB US|SP\r\n  Change keyboard layout. US = English US, SP = Spanish.", },
    CommandEntry { name: "CPUINFO",  category: "INFO",     handler: cmd_cpuinfo, description: "Show CPU vendor and brand",
        usage: "CPUINFO\r\n  Show CPU vendor string and brand name from CPUID.", },
    CommandEntry { name: "MEM",      category: "INFO",     handler: cmd_mem,     description: "Show memory usage",
        usage: "MEM [/H]\r\n  Show memory usage. /H shows human-readable sizes (KB/MB).", },
    CommandEntry { name: "PS",       category: "INFO",     handler: cmd_ps,      description: "Show process list",
        usage: "PS\r\n  List all processes with PID, state, and name.", },
    CommandEntry { name: "DATE",     category: "INFO",     handler: cmd_date,    description: "Display current date",
        usage: "DATE [YYYY-MM-DD]\r\n  Display or set the current date.", },
    CommandEntry { name: "TIME",     category: "INFO",     handler: cmd_time,    description: "Display current time",
        usage: "TIME [HH:MM:SS]\r\n  Display or set the current time.", },
    CommandEntry { name: "VER",      category: "INFO",     handler: cmd_ver,     description: "Show kernel version",
        usage: "VER\r\n  Show the NeoDOS kernel version.", },
    CommandEntry { name: "CD",       category: "DISK",     handler: cmd_cd,      description: "Change directory / switch drive",
        usage: "CD [path]\r\n  CD C:\\BIN    changes to C:\\BIN.\r\n  CD ..        goes up one level.\r\n  CD           shows current directory.", },
    CommandEntry { name: "VOL",      category: "DISK",     handler: cmd_vol,     description: "Show volume label",
        usage: "VOL [drive:]\r\n  Show the volume label of the specified or current drive.", },
    CommandEntry { name: "LABEL",    category: "DISK",     handler: cmd_label,   description: "Display or set volume label",
        usage: "LABEL [drive:][label]\r\n  Display or change the volume label.", },
    CommandEntry { name: "DRIVES",   category: "DISK",     handler: cmd_drives,  description: "List mounted drive letters",
        usage: "DRIVES\r\n  List all mounted drives and their filesystem types.", },
    CommandEntry { name: "CALL",     category: "CTRL",     handler: cmd_call,    description: "Execute a .BAT batch file",
        usage: "CALL file.bat [args]\r\n  Execute a batch file from within another batch file.", },
    CommandEntry { name: "COPY",     category: "FILE",     handler: cmd_copy,    description: "Copy file (COPY SRC DST)",
        usage: "COPY source destination\r\n  COPY C:\\readme.txt A:\\readme.txt", },
    CommandEntry { name: "MD",       category: "FILE",     handler: cmd_md,      description: "Create directory",
        usage: "MD directory\r\n  Create a new directory.\r\n  MD C:\\DATA  creates C:\\DATA.", },
    CommandEntry { name: "DEL",      category: "FILE",     handler: cmd_del,     description: "Delete file",
        usage: "DEL file\r\n  Delete a file.\r\n  DEL C:\\TEMP\\OLD.TXT", },
    CommandEntry { name: "REN",      category: "FILE",     handler: cmd_ren,     description: "Rename file",
        usage: "REN oldname newname\r\n  Rename a file.", },
    CommandEntry { name: "RENAME",   category: "FILE",     handler: cmd_ren,     description: "Rename file",
        usage: "RENAME oldname newname\r\n  Alias for REN.", },
    CommandEntry { name: "RD",       category: "FILE",     handler: cmd_rd,      description: "Remove empty directory",
        usage: "RD directory\r\n  Remove an empty directory.", },
    CommandEntry { name: "RMDIR",    category: "FILE",     handler: cmd_rd,      description: "Remove empty directory",
        usage: "RMDIR directory\r\n  Alias for RD.", },
    CommandEntry { name: "ATTRIB",   category: "FILE",     handler: cmd_attrib,  description: "Display/modify file attributes",
        usage: "ATTRIB [file]\r\n  Display or modify file attributes (R, H, S, A).", },
    CommandEntry { name: "SYNC",     category: "CTRL",     handler: cmd_sync,    description: "Flush disk cache to disk",
        usage: "SYNC\r\n  Flush all disk caches to physical media.", },
    CommandEntry { name: "TSR",      category: "CTRL",     handler: cmd_tsr,     description: "Load TSR (TSR FILE INT)",
        usage: "TSR file intnum\r\n  Load a terminate-and-stay-resident module.\r\n  TSR DRIVER.NDM 0x60", },
    CommandEntry { name: "DEVICES",  category: "CTRL",     handler: cmd_devices, description: "List installed TSRs",
        usage: "DEVICES\r\n  List all installed TSR modules.", },
    CommandEntry { name: "TEST",     category: "CTRL",     handler: cmd_test,    description: "Run kernel self-tests",
        usage: "TEST\r\n  Run all kernel self-tests (120 tests across 12 suites).\r\n  Then runs HELLO.BIN, SYSTEST.BIN, FILETEST.BIN, ALLTEST.BIN.", },
    CommandEntry { name: "RUN",      category: "CTRL",     handler: cmd_run,     description: "Run flat binary in Ring 3 (RUN FILE.BIN)",
        usage: "RUN file.bin\r\n  Load and execute a flat binary in user mode (Ring 3).", },
    CommandEntry { name: "LOAD",      category: "CTRL",     handler: cmd_load,   description: "Load and run flat binary (LOAD FILE.BIN)",
        usage: "LOAD file.bin [args]\r\n  Load and execute a flat binary. Accepts arguments.", },
    CommandEntry { name: "DEVICESEND",category: "CTRL",     handler: cmd_devicesend, description: "Send cmd to device (DEVICESEND id cmd)",
        usage: "DEVICESEND id command\r\n  Send a command to a loaded TSR device.", },
    CommandEntry { name: "KILL",     category: "CTRL",     handler: cmd_kill,    description: "Terminate a process by PID",
        usage: "KILL pid\r\n  Terminate a running process by its PID number.", },
    CommandEntry { name: "EXIT",     category: "SHUTDOWN", handler: cmd_exit,    description: "Sync disk and halt",
        usage: "EXIT\r\n  Sync disk cache and halt the system.", },
    CommandEntry { name: "SHUTDOWN", category: "SHUTDOWN", handler: cmd_shutdown,description: "Power off the system",
        usage: "SHUTDOWN\r\n  Power off the system.", },
    CommandEntry { name: "POWEROFF", category: "SHUTDOWN", handler: cmd_shutdown,description: "Power off the system",
        usage: "POWEROFF\r\n  Alias for SHUTDOWN.", },
]);
