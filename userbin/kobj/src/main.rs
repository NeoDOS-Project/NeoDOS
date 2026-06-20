#![no_std]
#![no_main]

use libneodos::syscall::{self, KObjEntryRaw, sys_kobj_enum};

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn write_u64(mut v: u64) {
    if v == 0 {
        write_str(b"0");
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = 19;
    while v > 0 {
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        i -= 1;
    }
    write_str(&buf[i + 1..]);
}

fn write_u32(mut v: u32) {
    if v == 0 {
        write_str(b"0");
        return;
    }
    let mut buf = [0u8; 10];
    let mut i = 9;
    while v > 0 {
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        i -= 1;
    }
    write_str(&buf[i + 1..]);
}

fn pad_right(s: &[u8], width: usize) -> [u8; 32] {
    let mut buf = [0u8; 32];
    let len = s.len().min(width);
    buf[..len].copy_from_slice(&s[..len]);
    buf
}

#[used]
#[link_section = ".rodata"]
static KOBJ_HELP: &[u8] = b"::HELP::\
KOBJ\r\n\
  Lists all kernel objects in the registry.\r\n\
::END::";

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut entries = [KObjEntryRaw {
        id: 0,
        obj_type: 0,
        padding: 0,
        name: [0u8; 24],
        refcount: 0,
        native_id: 0,
    }; 64];

    match sys_kobj_enum(&mut entries) {
        Ok(count) if count > 0 => {
            write_str(b"\r\n");
            write_str(b" ID   TYPE         NAME                     REF      NATIVE    \r\n");
            write_str(b" ---- ------------ ------------------------ -------- ----------\r\n");
            let typ_width = 12;
            let name_width = 24;
            for i in 0..count.min(64) {
                let e = &entries[i];
                let typ_str = e.type_str();
                let name_str = e.name_str();

                write_u64(e.id);
                write_str(b"    ");
                let p = pad_right(typ_str.as_bytes(), typ_width);
                write_str(&p[..typ_width]);
                write_str(b" ");
                let n = pad_right(name_str.as_bytes(), name_width);
                write_str(&n[..name_width]);
                write_str(b" ");
                write_u32(e.refcount);
                write_str(b"       ");
                write_u64(e.native_id);
                write_str(b"\r\n");
            }
            write_str(b"\r\nTotal: ");
            write_u64(count as u64);
            write_str(b" objects\r\n\r\n");
        }
        Ok(_) => {
            write_str(b"\r\nNo kernel objects registered.\r\n\r\n");
        }
        Err(_) => {
            write_str(b"\r\nKOBJ: syscall failed\r\n\r\n");
        }
    }

    syscall::sys_exit(0)
}
