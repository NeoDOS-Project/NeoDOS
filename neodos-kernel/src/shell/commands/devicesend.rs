// src/shell/commands/devicesend.rs
//
// DEVICESEND <device_id> <cmd>
//
// Sends a command to a registered device handler, waking it up.

use crate::println;
use crate::drivers::signal_device_event;
use crate::shell::shell::DosShell;

impl DosShell {
    pub fn cmd_devicesend(&mut self, args: &[&str]) {
        if args.len() < 2 {
            println!("Usage: DEVICESEND <device_id> <command>");
            println!("  Sends a command to a device handler.");
            println!("  Example: DEVICESEND 0 1");
            return;
        }

        let device_id: u32 = match args[0].parse() {
            Ok(n) => n,
            Err(_) => {
                println!("Invalid device ID: {}", args[0]);
                return;
            }
        };

        let cmd: u32 = match args[1].parse() {
            Ok(n) => n,
            Err(_) => {
                println!("Invalid command: {}", args[1]);
                return;
            }
        };

        println!("Sending cmd {} to device {}", cmd, device_id);
        signal_device_event(device_id);
        println!("Device {} signaled.", device_id);
    }
}