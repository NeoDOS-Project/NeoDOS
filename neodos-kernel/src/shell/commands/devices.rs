use crate::println;
use crate::shell::shell::DosShell;

impl<'a> DosShell<'a> {
    pub(super) fn cmd_devices(&mut self) {
        println!("Installed TSRs:");
        let registry = crate::tsr::TSR_REGISTRY.lock();
        let mut found = false;
        for prog in &registry.programs {
            if let Some(info) = prog {
                if let Ok(name) = core::str::from_utf8(&info.name) {
                    println!(
                        "  {}  @ 0x{:x}  INT 0x{:x}",
                        name.trim_matches('\0'),
                        info.base_address,
                        info.interrupt_num
                    );
                    found = true;
                }
            }
        }
        if !found {
            println!("  None");
        }
    }
}

