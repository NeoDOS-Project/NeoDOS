use crate::println;
use crate::shell::shell::DosShell;

impl DosShell {
    pub fn cmd_loadlib(&mut self, args: &[&str]) {
        if args.is_empty() {
            println!("Usage: LOADLIB <path>");
            println!("  Load a shared library (DLL) from the filesystem.");
            println!("  The DLL is loaded into a free slot in the DLL region");
            println!("  and its export table becomes accessible at the slot base address.");
            println!();
            println!("  LOADLIB C:\\SYSTEM\\LIB\\LIBMATH.DLL");
            return;
        }

        let filename = args[0];
        let full_path = self.resolve_absolute_path(filename);

        println!("Loading DLL '{}'...", full_path);

        match crate::dll::dll_load(&full_path) {
            Some(base) => {
                println!("DLL loaded at 0x{:x}", base);
                println!("Export table at 0x{:x}", base);
            }
            None => {
                println!("Error: Failed to load DLL '{}'.", filename);
                println!("Check that the file exists and is a valid ELF binary.");
            }
        }
    }
}
