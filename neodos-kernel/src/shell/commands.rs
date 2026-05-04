// src/shell/commands.rs

use crate::shell::shell::DosShell;
use crate::println;
use crate::print;

impl<'a> DosShell<'a> {
    pub fn dispatch_command(&mut self, cmd: &str, args: &[&str]) {
        match cmd {
            "HELP" => self.cmd_help(),
            "CLS" => crate::vga::clear_screen(),
            "DIR" => self.cmd_dir(args),
            "TYPE" => self.cmd_type(args),
            "ECHO" => self.cmd_echo(args),
            "SET" => self.cmd_set(args),
            "EXIT" => {
                let _ = self.fs.sync(self.cache, self.ata);
                self.running = false;
            },
            "CD" => self.cmd_cd(args),
            "CALL" => self.cmd_call(args),
            "COPY" => self.cmd_copy(args),
            "MD" => self.cmd_md(args),
            "SYNC" => {
                println!("Syncing disk...");
                let _ = self.fs.sync(self.cache, self.ata);
            },
            "DEL" => println!("DEL not yet implemented"),
            "REN" => println!("REN not yet implemented"),
            "VER" => println!("NeoDOS v0.5"),
            "TSR" => self.cmd_tsr(args),
            "DEVICES" => self.cmd_devices(),
            _ => println!("Bad command or file name"),
        }
    }

    fn cmd_help(&mut self) {
        println!("Built-in commands:");
        println!("  HELP    - Show this help");
        println!("  CLS     - Clear screen");
        println!("  DIR     - List directory");
        println!("  TYPE    - Display file contents");
        println!("  COPY    - Copy file (COPY SRC DST)");
        println!("  MD      - Make directory (MD DIRNAME)");
        println!("  DEL     - Delete file");
        println!("  REN     - Rename file");
        println!("  SYNC    - Flush disk cache");
        println!("  TSR     - Load TSR (TSR FILE INT)");
        println!("  DEVICES - List TSRs");
        println!("  ECHO    - Print text");
        println!("  SET     - Set environment variables");
        println!("  CD      - Change directory");
        println!("  VER     - Show version");
        println!("  EXIT    - Sync and halt");
    }

    fn cmd_echo(&mut self, args: &[&str]) {
        for (i, arg) in args.iter().enumerate() {
            if i > 0 { print!(" "); }
            if arg.starts_with('%') && arg.ends_with('%') && arg.len() > 2 {
                let var = &arg[1..arg.len()-1];
                if let Some(val) = self.environment.get(var) {
                    print!("{}", val);
                } else {
                    print!("{}", arg);
                }
            } else {
                print!("{}", arg);
            }
        }
        println!();
    }

    fn cmd_set(&mut self, args: &[&str]) {
        if args.is_empty() {
            for i in 0..self.environment.count {
                if let Ok(k) = core::str::from_utf8(&self.environment.keys[i]) {
                    if let Ok(v) = core::str::from_utf8(&self.environment.values[i]) {
                        println!("{}={}", k.trim_matches('\0'), v.trim_matches('\0'));
                    }
                }
            }
            return;
        }

        // Search for '=' in args
        let mut found_eq = false;
        for arg in args {
            if let Some(pos) = arg.find('=') {
                let key = arg[..pos].trim();
                let val = arg[pos+1..].trim();
                if !key.is_empty() {
                    self.environment.set(key, val);
                    found_eq = true;
                }
                break;
            }
        }

        if !found_eq && args.len() >= 2 {
            // Support SET VAR VALUE (no space around =)
            // But usually DOS is SET VAR=VAL
            println!("Usage: SET VAR=VALUE");
        }
    }

    fn cmd_dir(&mut self, args: &[&str]) {
        let dir_inode = if args.is_empty() {
            // List current directory
            self.current_dir_inode
        } else {
            // List specified directory - for now, just support current dir
            // TODO: Support relative paths like "DIR SUBDIR" or absolute paths
            self.current_dir_inode
        };
        
        // Display the path
        let path_str = core::str::from_utf8(&self.current_dir[..self.current_dir_len]).unwrap_or("\\");
        println!(" Directory of C:{}", path_str);
        println!();
        
        if let Err(e) = self.fs.list_directory(dir_inode, self.cache, self.ata) {
            println!("Error reading directory: {:?}", e);
        }
    }

    fn cmd_type(&mut self, args: &[&str]) {
        if args.is_empty() {
            println!("Usage: TYPE FILENAME");
            return;
        }

        let filename = args[0];
        match self.fs.find_file(filename, self.cache, self.ata) {
            Ok(inode_num) => {
                if let Err(e) = self.fs.read_file(inode_num, self.cache, self.ata) {
                    println!("Error reading file: {:?}", e);
                }
                println!();
            }
            Err(_) => println!("File not found"),
        }
    }

    fn cmd_cd(&mut self, args: &[&str]) {
        if args.is_empty() {
            println!("{}", core::str::from_utf8(&self.current_dir[..self.current_dir_len]).unwrap_or("\\"));
            return;
        }

        let path = args[0];
        if path == "\\" {
            self.current_dir_len = 1;
            self.current_dir[0] = b'\\';
            self.current_dir_inode = 0;  // Root inode is always 0
        } else if path == ".." {
            if self.current_dir_len > 1 {
                // Find last backslash
                let mut last_bs = 0;
                for i in 0..self.current_dir_len - 1 {
                    if self.current_dir[i] == b'\\' {
                        last_bs = i;
                    }
                }
                self.current_dir_len = if last_bs == 0 { 1 } else { last_bs };
                self.current_dir_inode = 0;  // Go back to root for now (simplified)
            }
        } else {
            // Very basic CD: just append for now, no validation
            if self.current_dir_len > 1 {
                self.current_dir[self.current_dir_len] = b'\\';
                self.current_dir_len += 1;
            }
            let bytes = path.as_bytes();
            let to_copy = if bytes.len() > (128 - self.current_dir_len) { 128 - self.current_dir_len } else { bytes.len() };
            self.current_dir[self.current_dir_len..self.current_dir_len+to_copy].copy_from_slice(&bytes[..to_copy]);
            self.current_dir_len += to_copy;
            // TODO: Validate directory exists and get its inode
        }
    }

    fn cmd_call(&mut self, args: &[&str]) {
        if args.is_empty() {
            println!("Usage: CALL FILENAME.BAT");
            return;
        }

        let filename = args[0];
        match self.fs.find_file(filename, self.cache, self.ata) {
            Ok(inode_num) => {
                let mut buf = [0u8; 4096]; // Max 4KB batch file
                match self.fs.read_file_to_buf(inode_num, &mut buf, self.cache, self.ata) {
                    Ok(read) => {
                        if let Ok(content) = core::str::from_utf8(&buf[..read]) {
                            self.execute_batch(content);
                        } else {
                            println!("Error: Batch file is not valid UTF-8");
                        }
                    }
                    Err(e) => println!("Error reading batch file: {:?}", e),
                }
            }
            Err(_) => println!("File not found"),
        }
    }

    fn cmd_copy(&mut self, args: &[&str]) {
        if args.len() < 2 {
            println!("Usage: COPY SRC DST");
            return;
        }

        let src = args[0];
        let dst = args[1];

        match self.fs.find_file(src, self.cache, self.ata) {
            Ok(src_inode_num) => {
                let mut buf = [0u8; 16384]; // Max 16KB copy for now
                match self.fs.read_file_to_buf(src_inode_num, &mut buf, self.cache, self.ata) {
                    Ok(read) => {
                        match self.fs.create_file(dst, self.cache, self.ata) {
                            Ok(dst_inode_num) => {
                                if let Ok(written) = self.fs.write_file(dst_inode_num, &buf[..read], self.cache, self.ata) {
                                    println!("{} bytes copied", written);
                                } else {
                                    println!("Error writing destination file");
                                }
                            }
                            Err(e) => println!("Error creating destination file: {:?}", e),
                        }
                    }
                    Err(e) => println!("Error reading source file: {:?}", e),
                }
            }
            Err(_) => println!("Source file not found"),
        }
    }

    fn cmd_md(&mut self, args: &[&str]) {
        if args.is_empty() {
            println!("Usage: MD DIRNAME");
            return;
        }

        let dirname = args[0];
        // Simplified: create an inode with MODE_DIR and add to root
        match self.fs.find_free_inode(self.cache, self.ata) {
            Ok(new_inode_num) => {
                let new_inode = crate::fs::neodos_fs::Inode {
                    inode_num: new_inode_num,
                    mode: crate::fs::neodos_fs::MODE_DIR,
                    size: 0,
                    atime: 0, mtime: 0, ctime: 0,
                    link_count: 1,
                    owner_uid: 0, owner_gid: 0,
                    direct_blocks: [0; 12],
                    indirect_block: 0,
                    padding: [0; 160],
                };
                
                if let Ok(()) = self.fs.write_inode(new_inode_num as usize, &new_inode, self.cache, self.ata) {
                    if let Err(e) = self.fs.add_directory_entry(0, dirname, new_inode_num, 2, self.cache, self.ata) {
                        println!("Error adding directory entry: {:?}", e);
                    } else {
                        println!("Directory created");
                    }
                } else {
                    println!("Error writing inode");
                }
            }
            Err(e) => println!("Error finding free inode: {:?}", e),
        }
    }

    fn cmd_tsr(&mut self, args: &[&str]) {
        if args.len() < 2 {
            println!("Usage: TSR FILENAME INT");
            println!("Example: TSR CLOCK.BIN 1C");
            return;
        }

        let filename = args[0];
        let int_hex = args[1];
        
        // Simple hex parser
        let mut int_num = 0;
        for b in int_hex.as_bytes() {
            let digit = match *b {
                b'0'..=b'9' => *b - b'0',
                b'a'..=b'f' => *b - b'a' + 10,
                b'A'..=b'F' => *b - b'A' + 10,
                _ => 0,
            };
            int_num = int_num * 16 + digit;
        }

        match crate::tsr::install_tsr(filename, int_num as u8, self.fs, self.cache, self.ata) {
            Ok(addr) => println!("TSR installed @ 0x{:x} (INT 0x{:x})", addr, int_num),
            Err(_) => println!("Error installing TSR"),
        }
    }

    fn cmd_devices(&mut self) {
        println!("Installed TSRs:");
        let registry = crate::tsr::TSR_REGISTRY.lock();
        let mut found = false;
        for prog in &registry.programs {
            if let Some(info) = prog {
                if let Ok(name) = core::str::from_utf8(&info.name) {
                    println!("  {}  @ 0x{:x}  INT 0x{:x}", name.trim_matches('\0'), info.base_address, info.interrupt_num);
                    found = true;
                }
            }
        }
        if !found {
            println!("  None");
        }
    }
}
