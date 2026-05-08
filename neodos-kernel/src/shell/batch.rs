// src/shell/batch.rs

use crate::shell::shell::DosShell;

impl<'a> DosShell<'a> {
    pub fn execute_batch(&mut self, content: &str) {
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("REM") || trimmed.starts_with(':') {
                continue;
            }
            
            // Very basic support for simple commands in batch
            //println!("Executing: {}", trimmed);
            self.execute_line(trimmed);
        }
    }
}
