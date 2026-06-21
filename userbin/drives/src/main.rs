#![no_std]
#![no_main]

use libneodos::syscall;

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

#[used]
#[link_section = ".rodata"]
static DRIVES_HELP: &[u8] = b"::HELP::\
DRIVES\r\n\
  Lists all mounted drives.\r\n\
::END::";

#[repr(C)]
#[derive(Clone, Copy)]
struct DriveInfo {
    letter: u8,
    present: u8,
    fs_type: [u8; 16],
    label: [u8; 32],
    total_sectors: u64,
}

fn sys_get_drives(buf: &mut [DriveInfo]) -> Result<usize, i64> {
    let ptr = buf.as_mut_ptr() as *mut u8;
    let max = buf.len() as u64;
    let r: i64;
    unsafe {
        core::arch::asm!(
            "push rbx",
            "push rcx",
            "mov rax, 33",
            "mov rbx, {ptr}",
            "mov rcx, {max}",
            "int 0x80",
            "pop rcx",
            "pop rbx",
            ptr = in(reg) ptr as u64,
            max = in(reg) max,
            out("rax") r,
            options(nostack),
        );
    }
    if r < 0 { Err(r) } else { Ok(r as usize) }
}

fn fs_type_str(fs_type: &[u8; 16]) -> &str {
    let end = fs_type.iter().position(|&b| b == 0).unwrap_or(16);
    core::str::from_utf8(&fs_type[..end]).unwrap_or("Unknown")
}

fn label_str(label: &[u8; 32]) -> &str {
    let end = label.iter().position(|&b| b == 0).unwrap_or(32);
    if end == 0 { return "(no label)"; }
    core::str::from_utf8(&label[..end]).unwrap_or("")
}

fn write_num(n: u64) {
    if n == 0 {
        write_str(b"0");
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = 20;
    let mut v = n;
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    write_str(&buf[i..]);
}

fn write_size(sectors: u64) {
    let bytes = sectors * 512;
    if bytes >= 1024 * 1024 * 1024 {
        let gb = bytes / (1024 * 1024 * 1024);
        let rem = (bytes % (1024 * 1024 * 1024)) * 100 / (1024 * 1024 * 1024);
        write_num(gb);
        write_str(b".");
        if rem < 10 { write_str(b"0"); }
        write_num(rem);
        write_str(b" GB");
    } else if bytes >= 1024 * 1024 {
        let mb = bytes / (1024 * 1024);
        write_num(mb);
        write_str(b" MB");
    } else if bytes >= 1024 {
        let kb = bytes / 1024;
        write_num(kb);
        write_str(b" KB");
    } else {
        write_str(b"0 B");
    }
}

fn print_help() {
    write_str(b"\r\nDRIVES\r\n  Lists all mounted drives.\r\n\r\n");
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    if libneodos::args::is_help_flag(&libneodos::args::read_args()) {
        print_help();
        syscall::sys_exit(0);
    }
    let mut drives = [DriveInfo {
        letter: 0,
        present: 0,
        fs_type: [0u8; 16],
        label: [0u8; 32],
        total_sectors: 0,
    }; 26];

    match sys_get_drives(&mut drives) {
        Ok(count) => {
            write_str(b"\r\nMounted drives:\r\n");
            for i in 0..count {
                let d = &drives[i];
                if d.present == 0 {
                    continue;
                }
                let letter = d.letter as char;
                let fstype = fs_type_str(&d.fs_type);
                let label = label_str(&d.label);

                write_str(b"  ");
                write_str(&[letter as u8, b':']);
                write_str(b"  ");
                write_str(fstype.as_bytes());
                write_str(b"  ");
                write_str(label.as_bytes());
                write_str(b"  ");
                write_size(d.total_sectors);
                write_str(b"\r\n");
            }
            write_str(b"\r\n");
        }
        Err(_) => {
            write_str(b"\r\nError listing drives\r\n\r\n");
        }
    }
    syscall::sys_exit(0)
}
