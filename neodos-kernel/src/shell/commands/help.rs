use crate::println;
use crate::shell::shell::DosShell;

impl<'a> DosShell<'a> {
    pub(super) fn cmd_help(&mut self) {
        println!("Built-in commands:");
        println!("  HELP    - Show this help");
        println!("  CLS     - Clear screen");
        println!("  DIR     - List directory");
        println!("  TYPE    - Display file contents");
        println!("  COPY    - Copy file (COPY SRC DST)");
        println!("  MD      - Make directory (MD DIRNAME)");
        println!("  VOL     - Volume label (VOL [d:])");
        println!("  DRIVES  - List mounted drive letters");
        println!("  DEL     - Delete file");
        println!("  REN     - Rename file");
        println!("  SYNC    - Flush disk cache");
        println!("  TSR     - Load TSR (TSR FILE INT)");
        println!("  DEVICES - List TSRs");
        println!("  ECHO    - Print text");
        println!("  SET     - Set environment variables");
        println!("  CD      - Change directory / switch drive (CD d:)");
        println!("  CPUINFO - Show CPU vendor/brand");
        println!("  MEM     - Show memory usage");
        println!("  KEYB    - Change keyboard layout (KEYB US|SP)");
        println!("  VER     - Show version");
        println!("  EXIT    - Sync and halt");
    }
}
