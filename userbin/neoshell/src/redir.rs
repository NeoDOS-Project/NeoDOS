use libneodos::syscall;
use neoshell_lib::tokenizer::{tokenize, TokenKind};

const OB_PREFIX: &[u8] = b"\\Global\\FileSystem\\";

fn to_ob_path<'a>(vfs: &[u8], buf: &'a mut [u8; 512]) -> Option<&'a str> {
    let t = OB_PREFIX.len() + vfs.len();
    if t > 510 { return None; }
    buf[..OB_PREFIX.len()].copy_from_slice(OB_PREFIX);
    buf[OB_PREFIX.len()..t].copy_from_slice(vfs);
    buf[t] = 0;
    Some(unsafe { core::str::from_utf8_unchecked(&buf[..t]) })
}

fn resolve_vfs_path<'a>(val: &[u8], out: &'a mut [u8; 260]) -> &'a str {
    let mut cwb = [0u8; 256];
    let cwd_len = syscall::sys_getcwd(&mut cwb).unwrap_or(0);
    let drive = if cwd_len > 1 && cwb[1] == b':' {
        let d = cwb[0];
        if d >= b'a' && d <= b'z' { d - 32 } else { d }
    } else {
        b'C'
    };

    let mut pos;

    if val.len() >= 2 && val[1] == b':' {
        let n = val.len().min(259);
        out[..n].copy_from_slice(&val[..n]);
        pos = n;
    } else {
        out[0] = drive;
        out[1] = b':';
        pos = 2;

        if val[0] != b'\\' && val[0] != b'/' && cwd_len > 2 {
            let cwd = &cwb[2..cwd_len];
            let n = cwd.len().min(258 - pos);
            out[pos..pos + n].copy_from_slice(cwd);
            pos += n;
            if out[pos - 1] != b'\\' && pos < 259 {
                out[pos] = b'\\';
                pos += 1;
            }
        }

        let n = val.len().min(259 - pos);
        out[pos..pos + n].copy_from_slice(&val[..n]);
        pos += n;
    }

    out[pos] = 0;
    unsafe { core::str::from_utf8_unchecked(&out[..pos]) }
}

fn close_fds(stdin: u8, stdout: u8, stderr: u8) {
    if stdin != 0xFF { let _ = syscall::sys_close(stdin); }
    if stdout != 0xFF { let _ = syscall::sys_close(stdout); }
    if stderr != 0xFF { let _ = syscall::sys_close(stderr); }
}

#[derive(Debug, Clone, Copy)]
pub struct RedirFds {
    pub stdin_fd: u8,
    pub stdout_fd: u8,
    pub stderr_fd: u8,
}

impl RedirFds {
    pub const fn none() -> Self {
        RedirFds { stdin_fd: 0xFF, stdout_fd: 0xFF, stderr_fd: 0xFF }
    }

    pub fn close_all(&self) {
        close_fds(self.stdin_fd, self.stdout_fd, self.stderr_fd);
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ParsedLine {
    pub fds: RedirFds,
    pub cmd: [u8; 32],
    pub cmd_len: usize,
    pub args: [u8; 256],
    pub args_len: usize,
    pub error: Option<&'static str>,
}

fn make_error(e: &'static str) -> ParsedLine {
    ParsedLine {
        fds: RedirFds::none(),
        cmd: [0; 32], cmd_len: 0,
        args: [0; 256], args_len: 0,
        error: Some(e),
    }
}

pub fn parse_line(line: &[u8]) -> ParsedLine {
    let tr = tokenize(line);
    if tr.error.is_some() {
        return make_error("parse error");
    }

    let mut stdin_fd = 0xFFu8;
    let mut stdout_fd = 0xFFu8;
    let mut stderr_fd = 0xFFu8;

    for i in 0..tr.token_count {
        let (kind, val) = match tr.get(i) {
            Some(v) => v,
            None => break,
        };
        if val.is_empty() {
            continue;
        }

        match kind {
            TokenKind::RedirectStdin => {
                let mut path_buf = [0u8; 260];
                let vfs = resolve_vfs_path(val, &mut path_buf);
                let mut ob = [0u8; 512];
                match to_ob_path(vfs.as_bytes(), &mut ob) {
                    Some(p) => match syscall::sys_ob_open(p, syscall::ob_access::READ) {
                        Ok(fd) => {
                            if stdin_fd != 0xFF { let _ = syscall::sys_close(stdin_fd); }
                            stdin_fd = fd;
                        }
                        Err(_) => {
                            close_fds(stdin_fd, stdout_fd, stderr_fd);
                            return make_error("input file not found");
                        }
                    },
                    None => {
                        close_fds(stdin_fd, stdout_fd, stderr_fd);
                        return make_error("path too long");
                    }
                }
            }
            TokenKind::RedirectStdout => {
                let mut path_buf = [0u8; 260];
                let vfs = resolve_vfs_path(val, &mut path_buf);
                match syscall::ob_file_create(vfs) {
                    Ok(fd) => {
                        if stdout_fd != 0xFF { let _ = syscall::sys_close(stdout_fd); }
                        stdout_fd = fd;
                    }
                    Err(_) => {
                        close_fds(stdin_fd, stdout_fd, stderr_fd);
                        return make_error("cannot create output file");
                    }
                }
            }
            TokenKind::RedirectAppend => {
                let mut path_buf = [0u8; 260];
                let vfs = resolve_vfs_path(val, &mut path_buf);
                let mut ob = [0u8; 512];
                let fd = match to_ob_path(vfs.as_bytes(), &mut ob) {
                    Some(p) => {
                        match syscall::sys_ob_open(p, syscall::ob_access::WRITE) {
                            Ok(f) => f,
                            Err(_) => match syscall::ob_file_create(vfs) {
                                Ok(f) => f,
                                Err(_) => {
                                    close_fds(stdin_fd, stdout_fd, stderr_fd);
                                    return make_error("cannot open append file");
                                }
                            }
                        }
                    }
                    None => {
                        close_fds(stdin_fd, stdout_fd, stderr_fd);
                        return make_error("path too long");
                    }
                };
                if stdout_fd != 0xFF { let _ = syscall::sys_close(stdout_fd); }
                stdout_fd = fd;
            }
            TokenKind::RedirectStderr => {
                let mut path_buf = [0u8; 260];
                let vfs = resolve_vfs_path(val, &mut path_buf);
                match syscall::ob_file_create(vfs) {
                    Ok(fd) => {
                        if stderr_fd != 0xFF { let _ = syscall::sys_close(stderr_fd); }
                        stderr_fd = fd;
                    }
                    Err(_) => {
                        close_fds(stdin_fd, stdout_fd, stderr_fd);
                        return make_error("cannot create stderr file");
                    }
                }
            }
            _ => {}
        }
    }

    let mut cmd = [0u8; 32];
    let mut cmd_len = 0;
    let mut args = [0u8; 256];
    let mut args_len = 0;
    let mut first_word = true;

    for i in 0..tr.token_count {
        let (kind, val) = match tr.get(i) {
            Some(v) => v,
            None => break,
        };
        if kind == TokenKind::Word {
            if first_word {
                cmd_len = val.len().min(31);
                cmd[..cmd_len].copy_from_slice(&val[..cmd_len]);
                first_word = false;
            } else {
                if args_len > 0 && args_len < 255 {
                    args[args_len] = b' ';
                    args_len += 1;
                }
                let n = val.len().min(255 - args_len);
                args[args_len..args_len + n].copy_from_slice(&val[..n]);
                args_len += n;
            }
        }
    }

    ParsedLine {
        fds: RedirFds { stdin_fd, stdout_fd, stderr_fd },
        cmd, cmd_len,
        args, args_len,
        error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neoshell_lib::tokenizer::tokenize;

    #[test]
    fn parse_simple_command() {
        let p = parse_line(b"dir");
        assert!(p.error.is_none());
        assert_eq!(&p.cmd[..p.cmd_len], b"dir");
        assert_eq!(p.args_len, 0);
        assert!(p.fds.is_none());
    }

    #[test]
    fn parse_command_with_args() {
        let p = parse_line(b"echo hello world");
        assert!(p.error.is_none());
        assert_eq!(&p.cmd[..p.cmd_len], b"echo");
        assert_eq!(&p.args[..p.args_len], b"hello world");
        assert!(p.fds.is_none());
    }

    #[test]
    fn token_extract_cmd_and_args() {
        let tr = tokenize(b"echo hello > out.txt");
        assert_eq!(tr.token_count, 3);
        let (k0, v0) = tr.get(0).unwrap();
        assert_eq!(k0, TokenKind::Word);
        assert_eq!(v0, b"echo");
        let (k1, v1) = tr.get(1).unwrap();
        assert_eq!(k1, TokenKind::Word);
        assert_eq!(v1, b"hello");
        let (k2, v2) = tr.get(2).unwrap();
        assert_eq!(k2, TokenKind::RedirectStdout);
        assert_eq!(v2, b"out.txt");
    }

    #[test]
    fn token_redirect_append() {
        let tr = tokenize(b"cmd >> log.txt");
        assert_eq!(tr.token_count, 2);
        let (k0, _) = tr.get(0).unwrap();
        assert_eq!(k0, TokenKind::Word);
        let (k1, v1) = tr.get(1).unwrap();
        assert_eq!(k1, TokenKind::RedirectAppend);
        assert_eq!(v1, b"log.txt");
    }

    #[test]
    fn token_redirect_stderr() {
        let tr = tokenize(b"cmd 2> err.txt");
        assert_eq!(tr.token_count, 2);
        let (k1, v1) = tr.get(1).unwrap();
        assert_eq!(k1, TokenKind::RedirectStderr);
        assert_eq!(v1, b"err.txt");
    }

    #[test]
    fn token_multiple_redirects() {
        let tr = tokenize(b"sort < in.txt > out.txt");
        assert_eq!(tr.token_count, 3);
        let (k0, v0) = tr.get(0).unwrap();
        assert_eq!(k0, TokenKind::Word);
        assert_eq!(v0, b"sort");
        let (k1, v1) = tr.get(1).unwrap();
        assert_eq!(k1, TokenKind::RedirectStdin);
        assert_eq!(v1, b"in.txt");
        let (k2, v2) = tr.get(2).unwrap();
        assert_eq!(k2, TokenKind::RedirectStdout);
        assert_eq!(v2, b"out.txt");
    }

    #[test]
    fn parse_error_on_unmatched_quote() {
        let p = parse_line(b"echo \"hello");
        assert!(p.error.is_some());
    }
}
