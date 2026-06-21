use crate::println;
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
                    print_usage(entry);
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
                print_usage(entry);
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
        println!("========================================");
        println!("         N e o D O S  H E L P");
        println!("========================================");
        println!("  HELP <command>  or  <command> /?");
        println!("  for detailed help on a specific command.");
        println!();

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
                    if let Some(pad) = 9usize.checked_sub(e.name.len()) {
                        let spaces = "                 ";
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

fn print_usage(entry: &CommandEntry) {
    println!("========================================");
    println!("  {} — {}", entry.name, entry.description);
    println!("========================================");
    println!("{}", entry.usage);
    println!();
}

// Command shims
pub fn cmd_help(shell: &mut DosShell, _args: &[&str]) { shell.cmd_help(_args); }
#[allow(deprecated)]
pub fn cmd_dir(shell: &mut DosShell, args: &[&str]) { shell.cmd_dir(args); }
pub fn cmd_set(shell: &mut DosShell, args: &[&str]) { shell.cmd_set(args); }
pub fn cmd_keyb(shell: &mut DosShell, args: &[&str]) { shell.cmd_keyb(args); }
pub fn cmd_call(shell: &mut DosShell, args: &[&str]) { shell.cmd_call(args); }
pub fn cmd_copy(shell: &mut DosShell, args: &[&str]) { shell.cmd_copy(args); }
pub fn cmd_md(shell: &mut DosShell, args: &[&str]) { shell.cmd_md(args); }
pub fn cmd_drives(shell: &mut DosShell, _args: &[&str]) { shell.cmd_drives(); }
pub fn cmd_label(shell: &mut DosShell, args: &[&str]) { shell.cmd_label(args); }
pub fn cmd_del(shell: &mut DosShell, args: &[&str]) { shell.cmd_del(args); }
pub fn cmd_ren(shell: &mut DosShell, args: &[&str]) { shell.cmd_rename(args); }
pub fn cmd_rd(shell: &mut DosShell, args: &[&str]) { shell.cmd_rd(args); }
pub fn cmd_ps(shell: &mut DosShell, _args: &[&str]) { shell.cmd_ps(); }
pub fn cmd_pri(shell: &mut DosShell, args: &[&str]) { shell.cmd_pri(args); }
pub fn cmd_kill(shell: &mut DosShell, args: &[&str]) { shell.cmd_kill(args); }
pub fn cmd_cls(_shell: &mut DosShell, _args: &[&str]) { crate::console::clear_screen(); }
pub fn cmd_run(shell: &mut DosShell, args: &[&str]) { shell.cmd_run(args); }
pub fn cmd_loadlib(shell: &mut DosShell, args: &[&str]) { shell.cmd_loadlib(args); }
pub fn cmd_exit(shell: &mut DosShell, args: &[&str]) { shell.cmd_shutdown(args); }
pub fn cmd_ndreg(shell: &mut DosShell, args: &[&str]) { shell.cmd_ndreg(args); }
pub fn cmd_loadnem(shell: &mut DosShell, args: &[&str]) { shell.cmd_loadnem(args); }
pub fn cmd_nemlist(shell: &mut DosShell, _args: &[&str]) { shell.cmd_nemlist(); }
pub fn cmd_fsck(shell: &mut DosShell, args: &[&str]) { shell.cmd_fsck(args); }
pub fn cmd_crash(shell: &mut DosShell, args: &[&str]) { shell.cmd_crash(args); }

pub const COMMANDS: CommandRegistry = CommandRegistry::new(&[
    CommandEntry { name: "HELP",     category: "CTRL",     handler: cmd_help,    description: "Show this help",
        usage: concat!("Syntax:  HELP [command]\n",
                       "  Show general help listing or detailed help for a\n",
                       "  specific command.\n",
                       "  HELP DIR     shows detailed help for DIR."), },
    CommandEntry { name: "CLS",      category: "CTRL",     handler: cmd_cls,     description: "Clear screen",
        usage: "Syntax:  CLS\n  Clear the screen and reset cursor to top-left.", },
    CommandEntry { name: "DIR",      category: "DISK",     handler: cmd_dir,     description: "[DEPRECATED] List directory contents — use Ring 3 shell",
        usage: concat!("Syntax:  DIR [path]\n",
                       "  [DEPRECATED] Use the Ring 3 shell (neoshell) for DIR.\n",
                       "  Shows name, permissions (RWXSD), and size.\n",
                        "  DIR C:\\Programs   lists the contents of C:\\Programs."), },
    CommandEntry { name: "SET",      category: "CONFIG",   handler: cmd_set,     description: "Display/set environment variables",
        usage: concat!("Syntax:  SET [var[=value]]\n",
                       "  SET                lists all environment variables.\n",
                        "  SET PATH=C:\\Programs    sets PATH to C:\\Programs.\n",
                       "  SET PATH=          removes the PATH variable."), },
    CommandEntry { name: "KEYB",     category: "CONFIG",   handler: cmd_keyb,    description: "Change keyboard layout",
        usage: concat!("Syntax:  KEYB US|SP\n",
                       "  Change the active keyboard layout.\n",
                       "  US = English (United States)\n",
                       "  SP = Spanish"), },
    CommandEntry { name: "PS",       category: "INFO",     handler: cmd_ps,      description: "Show process list",
        usage: concat!("Syntax:  PS\n",
                       "  List all running processes with PID, current state,\n",
                       "  and name. States: Running, Ready, Blocked, Terminated."), },
    CommandEntry { name: "PRI",      category: "CTRL",     handler: cmd_pri,     description: "Set process priority",
        usage: concat!("Syntax:  PRI <pid> <priority>\n",
                       "  Set the scheduling priority of a running process.\n",
                       "  Priority levels:\n",
                       "    0 = HIGH (400 ticks)\n",
                       "    1 = ABOVE_NORMAL (200 ticks)\n",
                       "    2 = NORMAL (100 ticks) — default\n",
                       "    3 = IDLE (50 ticks)\n",
                       "  PRI 2 0   boosts PID 2 to HIGH priority."), },
    CommandEntry { name: "LABEL",    category: "DISK",     handler: cmd_label,   description: "Display or set volume label",
        usage: concat!("Syntax:  LABEL [drive:][label]\n",
                       "  Display or change the volume label of a drive.\n",
                       "  LABEL C:MYDISK   sets C: label to MYDISK."), },
    CommandEntry { name: "DRIVES",   category: "DISK",     handler: cmd_drives,  description: "List mounted drive letters",
        usage: concat!("Syntax:  DRIVES\n",
                       "  List all mounted drives, their letters, filesystem types,\n",
                       "  and volume labels."), },
    CommandEntry { name: "CALL",     category: "CTRL",     handler: cmd_call,    description: "Execute a .BAT batch file",
        usage: concat!("Syntax:  CALL file.bat [args]\n",
                       "  Execute a batch file from within another batch file.\n",
                       "  Returns control to the caller when the batch completes."), },
    CommandEntry { name: "COPY",     category: "FILE",     handler: cmd_copy,    description: "Copy a file",
        usage: concat!("Syntax:  COPY source destination\n",
                       "  Copy a file from source path to destination path.\n",
                        "  COPY C:\\Data\\file.txt A:\\file.txt"), },
    CommandEntry { name: "MD",       category: "FILE",     handler: cmd_md,      description: "Create a directory",
        usage: concat!("Syntax:  MD directory\n",
                       "  Create a new directory.\n",
                        "  MD C:\\Data   creates the C:\\Data directory."), },
    CommandEntry { name: "DEL",      category: "FILE",     handler: cmd_del,     description: "Delete a file",
        usage: concat!("Syntax:  DEL file\n",
                       "  Delete a file.\n",
                        "  DEL C:\\Temp\\old.txt   deletes the file."), },
    CommandEntry { name: "REN",      category: "FILE",     handler: cmd_ren,     description: "Rename a file",
        usage: concat!("Syntax:  REN oldname newname\n",
                       "  Rename a file. Both names are relative to the same\n",
                       "  directory (REN does not move files across directories).\n",
                       "  REN report.txt report_old.txt"), },
    CommandEntry { name: "RENAME",   category: "FILE",     handler: cmd_ren,     description: "Rename a file",
        usage: concat!("Syntax:  RENAME oldname newname\n",
                       "  Alias for REN."), },
    CommandEntry { name: "RD",       category: "FILE",     handler: cmd_rd,      description: "Remove empty directory",
        usage: concat!("Syntax:  RD directory\n",
                       "  Remove an empty directory.\n",
                        "  RD C:\\Temp\\emptydir   removes the directory."), },
    CommandEntry { name: "RMDIR",    category: "FILE",     handler: cmd_rd,      description: "Remove empty directory",
        usage: concat!("Syntax:  RMDIR directory\n",
                       "  Alias for RD."), },
    CommandEntry { name: "RUN",      category: "CTRL",     handler: cmd_run,     description: "Run flat binary in Ring 3",
        usage: concat!("Syntax:  RUN file.nxe\n",
                       "  Load and execute a flat binary in user mode (Ring 3).\n",
                       "  RUN HELLO.NXE   runs the hello binary."), },
    CommandEntry { name: "LOADLIB",   category: "CTRL",     handler: cmd_loadlib, description: "Load a shared library (NXL)",
        usage: concat!("Syntax:  LOADLIB path\n",
                       "  Load a shared library (NXL) from the filesystem.\n",
                       "  The NXL is loaded into a free slot in the DLL region\n",
                       "  and its export table becomes accessible at the slot base.\n",
                        "  LOADLIB C:\\System\\Libraries\\fs.nxl"), },
    CommandEntry { name: "KILL",     category: "CTRL",     handler: cmd_kill,    description: "Terminate a process by PID",
        usage: concat!("Syntax:  KILL pid\n",
                       "  Terminate a running process by its PID number.\n",
                       "  Use PS to list running processes and their PIDs."), },
    CommandEntry { name: "EXIT",     category: "SHUTDOWN", handler: cmd_exit,    description: "Sync disk and halt",
        usage: concat!("Syntax:  EXIT\n",
                       "  Sync disk cache and halt the system.\n",
                       "  Equivalent to SYNC followed by HLT."), },
    CommandEntry { name: "NDREG",    category: "INFO",     handler: cmd_ndreg,   description: "Driver Registry CLI",
        usage: concat!("Syntax:  NDREG <subcommand> [args]\n",
                       "  NeoDOS Driver Registry — inspect driver metadata.\n",
                       "  Subcommands:\n",
                       "    NDREG LIST [path]     List drivers with parsed metadata\n",
                       "    NDREG SHOW <name>     Show full driver details\n",
                       "    NDREG QUERY            Summarize driver registry\n",
                       "    NDREG RUNTIME          Show runtime state snapshot\n",
                       "    NDREG HEALTH           Validate driver metadata integrity\n",
                       "  All data is read-only from NeoFS + runtime registry."), },
    CommandEntry { name: "LOADNEM", category: "CTRL",     handler: cmd_loadnem, description: "Load a .nem driver",
        usage: concat!("Syntax:  LOADNEM <path>\n",
                       "  Load and register a .nem driver from the filesystem.\n",
                       "  Validates NEM header, ABI version, and registers with\n",
                       "  the Driver Runtime. The built-in dispatcher handles\n",
                       "  event delivery to the loaded driver.\n",
                        "  LOADNEM C:\\System\\Drivers\\disk.nem"), },
    CommandEntry { name: "NEMLIST",  category: "INFO",     handler: cmd_nemlist, description: "List loaded .nem drivers",
        usage: concat!("Syntax:  NEMLIST\n",
                       "  List all currently loaded .nem drivers with their\n",
                       "  IDs, names, states, event counts, and tick counts."), },
    CommandEntry { name: "CRASH",    category: "CTRL",     handler: cmd_crash,  description: "Crash dump management",
        usage: concat!("Syntax:  CRASH [DUMP|STATUS|TRIGGER]\n",
                       "  Manage crash dump buffers.\n",
                       "  CRASH              - show crash dump status\n",
                       "  CRASH DUMP          - write full crash dump to serial\n",
                       "  CRASH STATUS        - show crash dump area status\n",
                       "  CRASH TRIGGER       - trigger a test crash dump (safe)"), },
    CommandEntry { name: "FSCK", category: "CTRL", handler: cmd_fsck, description: "Check filesystem integrity",
        usage: concat!("Syntax:  FSCK [drive:] [/F]\n",
                       "  Check filesystem integrity on a NeoDOS volume.\n",
                       "  Without /F, only checks and reports errors.\n",
                       "  With /F, attempts to repair detected issues.\n\n",
                       "  Checks performed:\n",
                       "    1. Superblock (magic, block_size, label)\n",
                       "    2. Inode table (mode, block pointers, cross-links)\n",
                       "    3. Directory tree walk (orphans, dangling entries)\n",
                       "    4. Block allocation consistency\n",
                       "  FSCK C:             check-only on C:\n",
                       "  FSCK C: /F          check and repair C:"), },
]);
