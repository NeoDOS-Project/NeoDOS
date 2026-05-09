use crate::println;
use crate::shell::shell::DosShell;

impl<'a> DosShell<'a> {
    pub fn cmd_label(&mut self, args: &[&str]) {
        let current_label = self.fs.get_volume_label();
        
        if args.is_empty() {
            // Mostrar etiqueta actual
            println!("Volume in drive C has label {}", if current_label.is_empty() { "no label" } else { current_label });
            return;
        }
        
        let arg = args[0];
        
        // Check if it's a drive letter (e.g., "C:")
        let label = if arg.len() == 2 && arg.as_bytes()[1] == b':' {
            // Just a drive letter, show current label
            println!("Volume in drive {} has label {}", arg.chars().next().unwrap(), if current_label.is_empty() { "no label" } else { current_label });
            return;
        } else if arg.len() == 3 && arg.as_bytes()[1] == b':' {
            // Drive letter + label
            &arg[2..]
        } else {
            arg
        };
        
        // Set new label
        if label.is_empty() {
            println!("Usage: LABEL [drive:] [newlabel]");
            return;
        }
        
        match self.fs.set_volume_label(label, self.cache, self.ata) {
            Ok(_) => {
                let _ = self.cache.flush(self.ata);
                println!("Volume label set to: {}", label);
            }
            Err(e) => {
                println!("Error: {:?}", e);
            }
        }
    }
}