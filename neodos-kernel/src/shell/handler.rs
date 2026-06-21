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
pub fn cmd_call(shell: &mut DosShell, args: &[&str]) { shell.cmd_call(args); }
pub fn cmd_cls(_shell: &mut DosShell, _args: &[&str]) { crate::console::clear_screen(); }
pub fn cmd_run(shell: &mut DosShell, args: &[&str]) { shell.cmd_run(args); }
pub fn cmd_ndreg(shell: &mut DosShell, args: &[&str]) { shell.cmd_ndreg(args); }
pub fn cmd_loadnem(shell: &mut DosShell, args: &[&str]) { shell.cmd_loadnem(args); }
pub fn cmd_nemlist(shell: &mut DosShell, _args: &[&str]) { shell.cmd_nemlist(); }
pub fn cmd_fsck(shell: &mut DosShell, args: &[&str]) { shell.cmd_fsck(args); }
pub fn cmd_crash(shell: &mut DosShell, args: &[&str]) { shell.cmd_crash(args); }

pub const COMMANDS: CommandRegistry = CommandRegistry::new(&[
    CommandEntry { name: "CALL",     category: "CTRL",     handler: cmd_call,    description: "Execute a .BAT batch file",
        usage: concat!("Syntax:  CALL file.bat [args]\n",
                       "  Execute a batch file from within another batch file.\n",
                       "  Returns control to the caller when the batch completes."), },
    CommandEntry { name: "RUN",      category: "CTRL",     handler: cmd_run,     description: "Run flat binary in Ring 3",
        usage: concat!("Syntax:  RUN file.nxe\n",
                       "  Load and execute a flat binary in user mode (Ring 3).\n",
                       "  RUN HELLO.NXE   runs the hello binary."), },
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
