use crate::shell::shell::DosShell;

pub trait CommandHandler {
    fn name(&self) -> &str;
    fn execute(&mut self, args: &[&str]);
    fn help(&self) -> Option<&str> {
        None
    }
}

pub struct CommandRegistry {
    commands: &'static [(&'static str, fn(&mut DosShell, &[&str]))],
}

impl CommandRegistry {
    pub const fn new(commands: &'static [(&'static str, fn(&mut DosShell, &[&str]))]) -> Self {
        Self { commands }
    }

    pub fn dispatch(&self, cmd: &str, args: &[&str], shell: &mut DosShell) {
        for (name, handler) in self.commands {
            if cmd.eq_ignore_ascii_case(name) {
                handler(shell, args);
                return;
            }
        }
        crate::println!("Bad command or file name");
    }
}

// Command implementations
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
pub fn cmd_ver(shell: &mut DosShell, _args: &[&str]) { crate::println!("NeoDOS v0.6"); }
pub fn cmd_tsr(shell: &mut DosShell, args: &[&str]) { shell.cmd_tsr(args); }
pub fn cmd_devices(shell: &mut DosShell, _args: &[&str]) { shell.cmd_devices(); }
pub fn cmd_test(shell: &mut DosShell, args: &[&str]) { shell.cmd_test(args); }
pub fn cmd_date(shell: &mut DosShell, args: &[&str]) { shell.cmd_date(args); }
pub fn cmd_time(shell: &mut DosShell, args: &[&str]) { shell.cmd_time(args); }
pub fn cmd_attrib(shell: &mut DosShell, args: &[&str]) { shell.cmd_attrib(args); }
pub fn cmd_cls(shell: &mut DosShell, _args: &[&str]) { crate::vga::clear_screen(); }
pub fn cmd_exit(shell: &mut DosShell, _args: &[&str]) { shell.cmd_shutdown(); }
pub fn cmd_shutdown(shell: &mut DosShell, _args: &[&str]) { shell.cmd_shutdown(); }

pub const COMMANDS: CommandRegistry = CommandRegistry::new(&[
    ("HELP", cmd_help),
    ("CLS", cmd_cls),
    ("DIR", cmd_dir),
    ("TYPE", cmd_type),
    ("ECHO", cmd_echo),
    ("SET", cmd_set),
    ("KEYB", cmd_keyb),
    ("CPUINFO", cmd_cpuinfo),
    ("MEM", cmd_mem),
    ("EXIT", cmd_exit),
    ("SHUTDOWN", cmd_shutdown),
    ("POWEROFF", cmd_shutdown),
    ("CD", cmd_cd),
    ("CALL", cmd_call),
    ("COPY", cmd_copy),
    ("MD", cmd_md),
    ("VOL", cmd_vol),
    ("DRIVES", cmd_drives),
    ("LABEL", cmd_label),
    ("SYNC", cmd_sync),
    ("DEL", cmd_del),
    ("REN", cmd_ren),
    ("RENAME", cmd_ren),
    ("RD", cmd_rd),
    ("RMDIR", cmd_rd),
    ("VER", cmd_ver),
    ("TSR", cmd_tsr),
    ("DEVICES", cmd_devices),
    ("TEST", cmd_test),
    ("DATE", cmd_date),
    ("TIME", cmd_time),
    ("ATTRIB", cmd_attrib),
]);