use crate::println;
use crate::shell::shell::DosShell;

impl<'a> DosShell<'a> {
    pub fn cmd_cpuinfo(&mut self) {
        let info = crate::cpu::get_cpu_info();
        println!("CPU Vendor: {}", info.vendor_str());
        println!("CPU Brand:  {}", info.brand_str());
    }
}

