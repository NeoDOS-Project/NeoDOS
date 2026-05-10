use crate::shell::shell::DosShell;

pub struct CommandEntry {
    pub name: &'static str,
    pub category: &'static str,
    pub description: &'static str,
    pub handler: fn(&mut DosShell, &[&str]),
}

pub struct CommandRegistry {
    commands: &'static [CommandEntry],
}

impl CommandRegistry {
    pub const fn new(commands: &'static [CommandEntry]) -> Self {
        Self { commands }
    }

    pub fn dispatch(&self, cmd: &str, args: &[&str], shell: &mut DosShell) {
        for entry in self.commands {
            if cmd.eq_ignore_ascii_case(entry.name) {
                (entry.handler)(shell, args);
                return;
            }
        }
        crate::println!("Bad command or file name");
    }

    pub fn print_help(&self) {
        use crate::println;

        let mut categories: [(&str, &str); 8] = [
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
pub fn cmd_help(shell: &mut DosShell, _args: &[&str]) { shell.cmd_help(); }
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
pub fn cmd_sync(shell: &mut DosShell, _args: &[&str]) {
    crate::println!("Syncing disk...");
    let _ = shell.fs.sync(shell.cache, shell.ata);
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
pub fn cmd_cls(shell: &mut DosShell, _args: &[&str]) { crate::console::clear_screen(); }
pub fn cmd_run(shell: &mut DosShell, args: &[&str]) { shell.cmd_run(args); }
pub fn cmd_exit(shell: &mut DosShell, _args: &[&str]) { shell.cmd_shutdown(); }
pub fn cmd_shutdown(shell: &mut DosShell, _args: &[&str]) { shell.cmd_shutdown(); }

pub const COMMANDS: CommandRegistry = CommandRegistry::new(&[
    CommandEntry { name: "HELP",     category: "CTRL",     handler: cmd_help,    description: "Show this help", },
    CommandEntry { name: "CLS",      category: "CTRL",     handler: cmd_cls,     description: "Clear screen", },
    CommandEntry { name: "DIR",      category: "DISK",     handler: cmd_dir,     description: "List directory contents", },
    CommandEntry { name: "TYPE",     category: "FILE",     handler: cmd_type,    description: "Display file contents", },
    CommandEntry { name: "ECHO",     category: "CONFIG",   handler: cmd_echo,    description: "Print text with %VAR% expansion", },
    CommandEntry { name: "SET",      category: "CONFIG",   handler: cmd_set,     description: "Display/set environment variables", },
    CommandEntry { name: "KEYB",     category: "CONFIG",   handler: cmd_keyb,    description: "Change layout (KEYB US|SP)", },
    CommandEntry { name: "CPUINFO",  category: "INFO",     handler: cmd_cpuinfo, description: "Show CPU vendor and brand", },
    CommandEntry { name: "MEM",      category: "INFO",     handler: cmd_mem,     description: "Show memory usage", },
    CommandEntry { name: "PS",       category: "INFO",     handler: cmd_ps,      description: "Show process list", },
    CommandEntry { name: "DATE",     category: "INFO",     handler: cmd_date,    description: "Display current date", },
    CommandEntry { name: "TIME",     category: "INFO",     handler: cmd_time,    description: "Display current time", },
    CommandEntry { name: "VER",      category: "INFO",     handler: cmd_ver,     description: "Show kernel version", },
    CommandEntry { name: "CD",       category: "DISK",     handler: cmd_cd,      description: "Change directory / switch drive", },
    CommandEntry { name: "VOL",      category: "DISK",     handler: cmd_vol,     description: "Show volume label", },
    CommandEntry { name: "LABEL",    category: "DISK",     handler: cmd_label,   description: "Display or set volume label", },
    CommandEntry { name: "DRIVES",   category: "DISK",     handler: cmd_drives,  description: "List mounted drive letters", },
    CommandEntry { name: "CALL",     category: "CTRL",     handler: cmd_call,    description: "Execute a .BAT batch file", },
    CommandEntry { name: "COPY",     category: "FILE",     handler: cmd_copy,    description: "Copy file (COPY SRC DST)", },
    CommandEntry { name: "MD",       category: "FILE",     handler: cmd_md,      description: "Create directory", },
    CommandEntry { name: "DEL",      category: "FILE",     handler: cmd_del,     description: "Delete file", },
    CommandEntry { name: "REN",      category: "FILE",     handler: cmd_ren,     description: "Rename file", },
    CommandEntry { name: "RENAME",   category: "FILE",     handler: cmd_ren,     description: "Rename file", },
    CommandEntry { name: "RD",       category: "FILE",     handler: cmd_rd,      description: "Remove empty directory", },
    CommandEntry { name: "RMDIR",    category: "FILE",     handler: cmd_rd,      description: "Remove empty directory", },
    CommandEntry { name: "ATTRIB",   category: "FILE",     handler: cmd_attrib,  description: "Display/modify file attributes", },
    CommandEntry { name: "SYNC",     category: "CTRL",     handler: cmd_sync,    description: "Flush disk cache to disk", },
    CommandEntry { name: "TSR",      category: "CTRL",     handler: cmd_tsr,     description: "Load TSR (TSR FILE INT)", },
    CommandEntry { name: "DEVICES",  category: "CTRL",     handler: cmd_devices, description: "List installed TSRs", },
    CommandEntry { name: "TEST",     category: "CTRL",     handler: cmd_test,    description: "Run kernel self-tests", },
    CommandEntry { name: "RUN",      category: "CTRL",     handler: cmd_run,     description: "Run flat binary in Ring 3 (RUN FILE.BIN)", },
    CommandEntry { name: "EXIT",     category: "SHUTDOWN", handler: cmd_exit,    description: "Sync disk and halt", },
    CommandEntry { name: "SHUTDOWN", category: "SHUTDOWN", handler: cmd_shutdown,description: "Power off the system", },
    CommandEntry { name: "POWEROFF", category: "SHUTDOWN", handler: cmd_shutdown,description: "Power off the system", },
]);
