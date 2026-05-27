use crate::println;
use crate::kobj;
use crate::shell::shell::DosShell;

impl DosShell {
    pub fn cmd_kobj(&mut self) {
        let snapshot = kobj::kobj_iter_snapshot();
        if snapshot.is_empty() {
            println!("No kernel objects registered.");
            return;
        }
        println!(" {:<4} {:<12} {:<24} {:<8} {:<10}", "ID", "TYPE", "NAME", "REF", "NATIVE");
        println!(" {:-<4} {:-<12} {:-<24} {:-<8} {:-<10}", "-", "-", "-", "-", "-");
        for (id, typ, name, refcount, native_id) in snapshot.iter() {
            let name_str = {
                let len = name.iter().position(|&b| b == 0).unwrap_or(24);
                core::str::from_utf8(&name[..len]).unwrap_or("<?>")
            };
            println!(" {:<4} {:<12} {:<24} {:<8} {:<10}", id, typ.to_str(), name_str, refcount, native_id);
        }
        println!("Total: {} objects", snapshot.len());
    }
}
