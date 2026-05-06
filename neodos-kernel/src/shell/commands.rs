// src/shell/commands.rs

use crate::print;
use crate::println;
use crate::shell::shell::{vfs_path_from_drive_manager, DosShell, ShellPathError};

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
            }
            "CD" => self.cmd_cd(args),
            "CALL" => self.cmd_call(args),
            "COPY" => self.cmd_copy(args),
            "MD" => self.cmd_md(args),
            "VOL" => self.cmd_vol(args),
            "DRIVES" => self.cmd_drives(),
            "SYNC" => {
                println!("Syncing disk...");
                let _ = self.fs.sync(self.cache, self.ata);
            }
            "DEL" => println!("DEL not yet implemented"),
            "REN" => println!("REN not yet implemented"),
            "VER" => println!("NeoDOS v0.6"),
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
        println!("  VER     - Show version");
        println!("  EXIT    - Sync and halt");
    }

    fn cmd_echo(&mut self, args: &[&str]) {
        for (i, arg) in args.iter().enumerate() {
            if i > 0 {
                print!(" ");
            }
            if arg.starts_with('%') && arg.ends_with('%') && arg.len() > 2 {
                let var = &arg[1..arg.len() - 1];
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

        let mut found_eq = false;
        for arg in args {
            if let Some(pos) = arg.find('=') {
                let key = arg[..pos].trim();
                let val = arg[pos + 1..].trim();
                if !key.is_empty() {
                    self.environment.set(key, val);
                    found_eq = true;
                }
                break;
            }
        }

        if !found_eq && args.len() >= 2 {
            println!("Usage: SET VAR=VALUE");
        }
    }

    fn cmd_vol(&mut self, args: &[&str]) {
        let drive_char = if args.is_empty() {
            self.current_drive as char
        } else {
            let a = args[0];
            let b = a.as_bytes();
            if b.len() != 2 || b[1] != b':' {
                println!("Usage: VOL [d:]");
                return;
            }
            match a.chars().next() {
                Some(c) => c.to_ascii_uppercase(),
                None => {
                    println!("Invalid drive specification");
                    return;
                }
            }
        };

        match self.drive_manager.get(drive_char) {
            Some(_) => {
                println!(
                    " Volume in drive {} is {}",
                    drive_char,
                    self.volume_label()
                );
                println!(" NeoDOS filesystem");
            }
            None => println!("Invalid drive specification"),
        }
    }

    fn cmd_drives(&mut self) {
        println!("Mounted drives:");
        let mut any = false;
        for i in 0..26u8 {
            let c = (b'A' + i) as char;
            if let Some(d) = self.drive_manager.get(c) {
                println!("  {}:  FsInstance {}", d.letter as char, d.fs.0);
                any = true;
            }
        }
        if !any {
            println!("  (none)");
        }
    }

    fn cmd_dir(&mut self, args: &[&str]) {
        let path_arg = args.first().copied();
        let (dir_inode, dir_path, dir_path_len) = if args.is_empty() {
            let mut path = [0u8; 128];
            path[..self.current_dir_len].copy_from_slice(&self.current_dir[..self.current_dir_len]);
            (self.current_dir_inode, path, self.current_dir_len)
        } else {
            match self.resolve_directory_arg(args[0]) {
                Ok(resolved) => resolved,
                Err(_) => {
                    println!("The system cannot find the path specified");
                    return;
                }
            }
        };

        let drive_letter = self.dir_display_drive(path_arg);
        let path_str = core::str::from_utf8(&dir_path[..dir_path_len]).unwrap_or("\\");
        println!(" Directory of {}:{}", drive_letter, path_str);
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
        match self.resolve_file_inode(filename) {
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
            println!(
                "{}",
                core::str::from_utf8(&self.current_dir[..self.current_dir_len]).unwrap_or("\\")
            );
            return;
        }

        let path = args[0];
        let b = path.as_bytes();

        if b.len() == 2 && b[1] == b':' {
            let letter = match path.chars().next() {
                Some(c) => c,
                None => {
                    println!("Invalid drive specification");
                    return;
                }
            };
            match self.drive_manager.get(letter) {
                Some(d) => {
                    self.current_drive = d.letter;
                }
                None => println!("Invalid drive specification"),
            }
            return;
        }

        let dm = self.drive_manager;
        let vfs = match vfs_path_from_drive_manager(&dm, path) {
            Err(ShellPathError::UnsupportedVolume) => {
                println!("Drive not ready");
                return;
            }
            Err(_) => {
                println!("Invalid path");
                return;
            }
            Ok(v) => v,
        };

        match self.resolve_directory_arg_from_vfs(vfs) {
            Ok((new_inode, new_path, new_path_len)) => {
                if b.len() >= 2 && b[1] == b':' {
                    if let Some(c) = path.chars().next() {
                        if let Some(d) = self.drive_manager.get(c) {
                            self.current_drive = d.letter;
                        }
                    }
                }
                self.current_dir_inode = new_inode;
                self.current_dir.fill(0);
                self.current_dir[..new_path_len].copy_from_slice(&new_path[..new_path_len]);
                self.current_dir_len = new_path_len;
            }
            Err(_) => println!("The system cannot find the path specified"),
        }
    }

    fn cmd_call(&mut self, args: &[&str]) {
        if args.is_empty() {
            println!("Usage: CALL FILENAME.BAT");
            return;
        }

        let filename = args[0];
        match self.resolve_file_inode(filename) {
            Ok(inode_num) => {
                let mut buf = [0u8; 4096];
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

        match self.resolve_file_inode(src) {
            Ok(src_inode_num) => {
                let mut buf = [0u8; 16384];
                match self.fs.read_file_to_buf(src_inode_num, &mut buf, self.cache, self.ata) {
                    Ok(read) => {
                        let (parent_path, leaf) = self.split_parent_and_leaf(dst);
                        if leaf.is_empty() || leaf == "." || leaf == ".." {
                            println!("Invalid destination path");
                            return;
                        }

                        let parent_inode = if parent_path.is_empty() {
                            self.current_dir_inode
                        } else {
                            match self.resolve_directory_arg(parent_path) {
                                Ok((inode, _, _)) => inode,
                                Err(_) => {
                                    println!("The system cannot find the destination path");
                                    return;
                                }
                            }
                        };

                        match self
                            .fs
                            .create_file_at(parent_inode, leaf, self.cache, self.ata)
                        {
                            Ok(dst_inode_num) => {
                                if let Ok(written) =
                                    self.fs
                                        .write_file(dst_inode_num, &buf[..read], self.cache, self.ata)
                                {
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
        let (parent_path, leaf) = self.split_parent_and_leaf(dirname);
        if leaf.is_empty() || leaf == "." || leaf == ".." {
            println!("Invalid directory name");
            return;
        }

        let parent_inode = if parent_path.is_empty() {
            self.current_dir_inode
        } else {
            match self.resolve_directory_arg(parent_path) {
                Ok((inode, _, _)) => inode,
                Err(_) => {
                    println!("The system cannot find the path specified");
                    return;
                }
            }
        };

        match self
            .fs
            .create_directory_at(parent_inode, leaf, self.cache, self.ata)
        {
            Ok(_) => println!("Directory created"),
            Err(e) => println!("Error creating directory: {:?}", e),
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
