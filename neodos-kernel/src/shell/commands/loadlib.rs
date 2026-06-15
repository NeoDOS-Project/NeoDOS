use crate::println;
use crate::shell::shell::DosShell;

impl DosShell {
    pub fn cmd_loadlib(&mut self, args: &[&str]) {
        if args.is_empty() {
            println!("Usage: LOADLIB <path>");
            println!("  Load a shared library (NXL) from the filesystem.");
            println!("  The NXL is loaded into a free slot in the DLL region");
            println!("  and its export table becomes accessible at the slot base address.");
            println!();
            println!("  LOADLIB C:\\System\\Libraries\\math.nxl");
            return;
        }

        let filename = args[0];
        let full_path = self.resolve_absolute_path(filename);

        println!("Loading NXL '{}'...", full_path);

        match crate::nxl::nxl_load(&full_path) {
            Some(base) => {
                println!("NXL loaded at 0x{:x}", base);
                println!("Export table at 0x{:x}", base);
            }
            None => {
                println!("Error: Failed to load NXL '{}'.", filename);
                println!("Check that the file exists and is a valid ELF binary.");
            }
        }
    }
}
