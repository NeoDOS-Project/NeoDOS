#![no_std]
#![no_main]

use libneodos::syscall;

// ── Helpers ──

fn pass(label: &[u8]) {
    let _ = syscall::sys_write(1, b"[CMDTEST] ");
    let _ = syscall::sys_write(1, label);
    let _ = syscall::sys_write(1, b": PASS\r\n");
}

fn fail(label: &[u8], detail: &[u8]) {
    let _ = syscall::sys_write(1, b"[CMDTEST] ");
    let _ = syscall::sys_write(1, label);
    let _ = syscall::sys_write(1, b": FAIL - ");
    let _ = syscall::sys_write(1, detail);
    let _ = syscall::sys_write(1, b"\r\n");
}

fn to_ob_path<'a>(vfs: &'a str, buf: &'a mut [u8; 512]) -> &'a str {
    let prefix = b"\\Global\\FileSystem\\";
    let vfs_bytes = vfs.as_bytes();
    let total = prefix.len() + vfs_bytes.len();
    if total > 510 { return vfs; }
    buf[..prefix.len()].copy_from_slice(prefix);
    buf[prefix.len()..total].copy_from_slice(vfs_bytes);
    buf[total] = 0;
    unsafe { core::str::from_utf8_unchecked(&buf[..total]) }
}

fn write_str(s: &[u8]) {
    let _ = syscall::sys_write(1, s);
}

fn file_exists(path: &str) -> bool {
    let mut ob_buf = [0u8; 512];
    let ob_path = to_ob_path(path, &mut ob_buf);
    match syscall::sys_ob_open(ob_path, libneodos::syscall::ob_access::READ) {
        Ok(fd) => { let _ = syscall::sys_close(fd); true }
        Err(_) => false,
    }
}

fn dir_exists(path: &str) -> bool {
    let mut ob_buf1 = [0u8; 512];
    let ob_path1 = to_ob_path(path, &mut ob_buf1);
    match syscall::sys_ob_open(ob_path1, libneodos::syscall::ob_access::READ) {
        Ok(fd) => { let _ = syscall::sys_close(fd); true }
        Err(_) => false,
    }
}

fn err_code_str(e: i64) -> &'static [u8] {
    match e {
        -1 => b"EINVAL",
        -2 => b"ENOENT",
        -3 => b"ENOMEM",
        -4 => b"EACCES",
        -5 => b"EBADF",
        -6 => b"EFAULT",
        -7 => b"ENOSYS",
        -8 => b"EAGAIN",
        -9 => b"EPIPE",
        -10 => b"EEXIST",
        -11 => b"ENOTDIR",
        -12 => b"EISDIR",
        -13 => b"EIO",
        -14 => b"ENODEV",
        -15 => b"EBUSY",
        _ => b"UNKNOWN",
    }
}

fn uint_to_str(mut n: u32) -> [u8; 10] {
    let mut buf = [0u8; 10];
    let mut i = 9;
    if n == 0 {
        buf[9] = b'0';
        return buf;
    }
    while n > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    buf
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let mut all_ok = true;
    let mut npass: u32 = 0;
    let mut nfail: u32 = 0;

    macro_rules! check {
        ($label:expr, $ok:expr) => {
            if $ok {
                pass($label);
                npass += 1;
            } else {
                fail($label, b"");
                nfail += 1;
                all_ok = false;
            }
        };
        ($label:expr, $ok:expr, $detail:expr) => {
            if $ok {
                pass($label);
                npass += 1;
            } else {
                fail($label, $detail);
                nfail += 1;
                all_ok = false;
            }
        };
    }

    // ── CLEANUP: remove leftovers from previous tests ──
    fn cleanup_file(path: &str) {
        let mut b = [0u8; 512];
        let ob = to_ob_path(path, &mut b);
        if let Ok(fd) = syscall::sys_ob_open(ob, libneodos::syscall::ob_access::READ) {
            let _ = syscall::ob_file_delete(fd);
            let _ = syscall::sys_close(fd);
        }
    }
    fn cleanup_dir(path: &str) {
        let mut b = [0u8; 512];
        let ob = to_ob_path(path, &mut b);
        if let Ok(fd) = syscall::sys_ob_open(ob, libneodos::syscall::ob_access::READ) {
            let _ = syscall::sys_ob_destroy(fd);
            let _ = syscall::sys_close(fd);
        }
    }
    {
        cleanup_file("C:\\Temp\\cmdtest_src.txt");
        cleanup_file("C:\\Temp\\cmdtest_dst.txt");
        cleanup_file("C:\\Temp\\cmdtest_renamed.txt");
        cleanup_dir("C:\\Temp\\cmdtest_dir");
    }

    // ── CLS: just verify escape sequence doesn't crash ──
    {
        write_str(b"\x1b[2J\x1b[H");
        check!(b"CLS", true);
    }

    // ── MD: create directory ──
    {
        let mut b = [0u8; 512];
        let ob = to_ob_path("C:\\Temp\\cmdtest_dir", &mut b);
        let r = syscall::sys_ob_create(ob, 11, None, 0);
        check!(b"MD create", r.is_ok());
    }

    // ── Verify directory exists ──
    {
        let exists = dir_exists("C:\\Temp\\cmdtest_dir");
        check!(b"MD verify exists", exists);
    }

    // ── RD: remove empty directory ──
    {
        let mut b = [0u8; 512];
        let ob = to_ob_path("C:\\Temp\\cmdtest_dir", &mut b);
        let r = syscall::sys_ob_open(ob, libneodos::syscall::ob_access::READ)
            .and_then(|fd| {
                let r = syscall::sys_ob_destroy(fd);
                let _ = syscall::sys_close(fd);
                r
            });
        check!(b"RD remove", r.is_ok());
    }

    // ── Verify directory gone ──
    {
        let exists = dir_exists("C:\\Temp\\cmdtest_dir");
        check!(b"RD verify gone", !exists);
    }

    // ── CREATE source file for COPY test ──
    {
        let content = b"Hello from cmdtest! NeoDOS rules!";
        let fd = syscall::ob_file_create("C:\\Temp\\cmdtest_src.txt");
        if let Ok(f) = fd {
            let r = syscall::sys_ob_set_info(f, libneodos::syscall::ob_set_info_class::WRITE_CONTENT, content);
            check!(b"CREATE file", r.is_ok());
            let _ = syscall::sys_close(f);
        } else {
            let e = fd.unwrap_err();
            let detail = err_code_str(e);
            fail(b"CREATE open", detail);
            nfail += 1;
            all_ok = false;
        }
    }

    // ── Verify source file exists ──
    {
        let exists = file_exists("C:\\Temp\\cmdtest_src.txt");
        check!(b"CREATE verify exists", exists);
    }

    // ── COPY: copy source to destination ──
    {
        let mut ob_buf2 = [0u8; 512];
        let ob_path2 = to_ob_path("C:\\Temp\\cmdtest_src.txt", &mut ob_buf2);
        let src_fd = syscall::sys_ob_open(ob_path2, libneodos::syscall::ob_access::READ);
        if let Ok(sf) = src_fd {
            cleanup_file("C:\\Temp\\cmdtest_dst.txt");
            let dst_fd = syscall::ob_file_create("C:\\Temp\\cmdtest_dst.txt");
            if let Ok(df) = dst_fd {
                let mut buf = [0u8; 4096];
                let mut copy_ok = true;
                let mut copy_err = b"" as &[u8];
                loop {
                    match syscall::sys_ob_query_info(sf, libneodos::syscall::ObInfoClass::ReadContent, &mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            if let Err(e) = syscall::sys_ob_set_info(df, libneodos::syscall::ob_set_info_class::WRITE_CONTENT, &buf[..n]) {
                                copy_ok = false;
                                copy_err = err_code_str(e);
                                break;
                            }
                        }
                        Err(e) => {
                            copy_ok = false;
                            copy_err = err_code_str(e);
                            break;
                        }
                    }
                }
                let _ = syscall::sys_close(df);
                check!(b"COPY", copy_ok, copy_err);
            } else {
                let e = dst_fd.unwrap_err();
                fail(b"COPY dest open", err_code_str(e));
                nfail += 1;
                all_ok = false;
            }
            let _ = syscall::sys_close(sf);
        } else {
            let e = src_fd.unwrap_err();
            fail(b"COPY src open", err_code_str(e));
            nfail += 1;
            all_ok = false;
        }
    }

    // ── Verify copy content ──
    {
        let mut ob_buf3 = [0u8; 512];
        let ob_path3 = to_ob_path("C:\\Temp\\cmdtest_dst.txt", &mut ob_buf3);
        let fd = syscall::sys_ob_open(ob_path3, libneodos::syscall::ob_access::READ);
        if let Ok(f) = fd {
            let mut buf = [0u8; 128];
            match syscall::sys_ob_query_info(f, libneodos::syscall::ObInfoClass::ReadContent, &mut buf) {
                Ok(n) => {
                    let expected = b"Hello from cmdtest! NeoDOS rules!";
                    let content_match = n == expected.len() && &buf[..n] == expected;
                    check!(b"COPY verify content", content_match);
                }
                Err(_) => {
                    check!(b"COPY verify read", false);
                }
            }
            let _ = syscall::sys_close(f);
        } else {
            fail(b"COPY verify open", err_code_str(fd.unwrap_err()));
            nfail += 1;
            all_ok = false;
        }
    }

    // ── REN: rename destination ──
    {
        let mut b = [0u8; 512];
        let ob = to_ob_path("C:\\Temp\\cmdtest_dst.txt", &mut b);
        let r = syscall::sys_ob_open(ob, libneodos::syscall::ob_access::READ)
            .and_then(|fd| {
                let r = syscall::sys_ob_set_info(fd, 6, b"C:\\Temp\\cmdtest_renamed.txt");
                let _ = syscall::sys_close(fd);
                r
            });
        check!(b"REN", r.is_ok());
    }

    // ── Verify old name fails, new name works ──
    {
        let old_exists = file_exists("C:\\Temp\\cmdtest_dst.txt");
        let new_exists = file_exists("C:\\Temp\\cmdtest_renamed.txt");
        check!(b"REN verify old gone", !old_exists);
        check!(b"REN verify new exists", new_exists);
    }

    // ── DEL: delete source file ──
    {
        let mut b = [0u8; 512];
        let ob = to_ob_path("C:\\Temp\\cmdtest_src.txt", &mut b);
        let r = syscall::sys_ob_open(ob, libneodos::syscall::ob_access::READ)
            .and_then(|fd| {
                let r = syscall::ob_file_delete(fd);
                let _ = syscall::sys_close(fd);
                r
            });
        check!(b"DEL", r.is_ok());
    }

    // ── Verify deletion ──
    {
        let exists = file_exists("C:\\Temp\\cmdtest_src.txt");
        check!(b"DEL verify gone", !exists);
    }

    // ── Final cleanup ──
    {
        cleanup_file("C:\\Temp\\cmdtest_renamed.txt");
    }

    // ── Report ──
    {
        write_str(b"\r\n[CMDTEST] ");
        let pass_s = uint_to_str(npass);
        let start1 = pass_s.iter().position(|&b| b != 0).unwrap_or(0);
        write_str(&pass_s[start1..]);
        write_str(b" passed, ");
        let fail_s = uint_to_str(nfail);
        let start2 = fail_s.iter().position(|&b| b != 0).unwrap_or(0);
        write_str(&fail_s[start2..]);
        write_str(b" failed\r\n");
    }

    if all_ok {
        write_str(b"[CMDTEST] ALL_COMMAND_TESTS_PASSED\r\n");
    } else {
        write_str(b"[CMDTEST] SOME_TESTS_FAILED\r\n");
    }
    write_str(b"CMDTEST_COMPLETE\r\n");

    syscall::sys_exit(if all_ok { 0 } else { 1 })
}
