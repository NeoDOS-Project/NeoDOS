pub const MAX_TOKENS: usize = 32;
pub const CLEAN_BUF_SIZE: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Word,
    RedirectStdout,
    RedirectAppend,
    RedirectStdin,
    RedirectStderr,
    Pipe,
    Semicolon,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseError {
    UnmatchedDoubleQuote,
    UnmatchedSingleQuote,
}

pub struct TokenizeResult {
    pub tokens: [Token; MAX_TOKENS],
    pub token_count: usize,
    pub clean: [u8; CLEAN_BUF_SIZE],
    pub clean_len: usize,
    pub error: Option<ParseError>,
}

impl TokenizeResult {
    pub fn new() -> Self {
        Self {
            tokens: [Token { kind: TokenKind::Word, start: 0, end: 0 }; MAX_TOKENS],
            token_count: 0,
            clean: [0u8; CLEAN_BUF_SIZE],
            clean_len: 0,
            error: None,
        }
    }

    pub fn get(&self, index: usize) -> Option<(TokenKind, &[u8])> {
        if index >= self.token_count {
            return None;
        }
        let t = self.tokens[index];
        Some((t.kind, &self.clean[t.start..t.end]))
    }

    fn push(&mut self, kind: TokenKind, start: usize, end: usize) {
        if self.token_count < MAX_TOKENS {
            self.tokens[self.token_count] = Token { kind, start, end };
            self.token_count += 1;
        }
    }
}

pub fn tokenize(input: &[u8]) -> TokenizeResult {
    let mut result = TokenizeResult::new();
    let mut i = 0;
    let len = input.len();

    while i < len && result.token_count < MAX_TOKENS {
        while i < len && matches!(input[i], b' ' | b'\t') {
            i += 1;
        }
        if i >= len {
            break;
        }

        match input[i] {
            b'|' => {
                let s = result.clean_len;
                result.push(TokenKind::Pipe, s, s);
                i += 1;
            }
            b';' => {
                let s = result.clean_len;
                result.push(TokenKind::Semicolon, s, s);
                i += 1;
            }
            b'>' => {
                i += 1;
                let kind = if i < len && input[i] == b'>' {
                    i += 1;
                    TokenKind::RedirectAppend
                } else {
                    TokenKind::RedirectStdout
                };
                while i < len && matches!(input[i], b' ' | b'\t') {
                    i += 1;
                }
                let start = result.clean_len;
                read_word(input, &mut i, &mut result.clean, &mut result.clean_len, &mut result.error);
                result.push(kind, start, result.clean_len);
            }
            b'<' => {
                i += 1;
                while i < len && matches!(input[i], b' ' | b'\t') {
                    i += 1;
                }
                let start = result.clean_len;
                read_word(input, &mut i, &mut result.clean, &mut result.clean_len, &mut result.error);
                result.push(TokenKind::RedirectStdin, start, result.clean_len);
            }
            b'2' if i + 1 < len && input[i + 1] == b'>' => {
                i += 2;
                while i < len && matches!(input[i], b' ' | b'\t') {
                    i += 1;
                }
                let start = result.clean_len;
                read_word(input, &mut i, &mut result.clean, &mut result.clean_len, &mut result.error);
                result.push(TokenKind::RedirectStderr, start, result.clean_len);
            }
            _ => {
                let start = result.clean_len;
                read_word(input, &mut i, &mut result.clean, &mut result.clean_len, &mut result.error);
                result.push(TokenKind::Word, start, result.clean_len);
            }
        }
    }

    result
}

fn read_word(
    input: &[u8],
    i: &mut usize,
    clean: &mut [u8],
    clean_len: &mut usize,
    error: &mut Option<ParseError>,
) {
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let len = input.len();

    while *i < len {
        let c = input[*i];

        if in_single_quote {
            if c == b'\'' {
                in_single_quote = false;
                *i += 1;
            } else {
                if *clean_len < clean.len() {
                    clean[*clean_len] = c;
                    *clean_len += 1;
                }
                *i += 1;
            }
        } else if in_double_quote {
            match c {
                b'"' => {
                    in_double_quote = false;
                    *i += 1;
                }
                b'^' => {
                    *i += 1;
                    if *i < len {
                        if *clean_len < clean.len() {
                            clean[*clean_len] = input[*i];
                            *clean_len += 1;
                        }
                        *i += 1;
                    }
                }
                _ => {
                    if *clean_len < clean.len() {
                        clean[*clean_len] = c;
                        *clean_len += 1;
                    }
                    *i += 1;
                }
            }
        } else {
            match c {
                b'^' => {
                    *i += 1;
                    if *i < len {
                        if *clean_len < clean.len() {
                            clean[*clean_len] = input[*i];
                            *clean_len += 1;
                        }
                        *i += 1;
                    }
                }
                b'"' => {
                    in_double_quote = true;
                    *i += 1;
                }
                b'\'' => {
                    in_single_quote = true;
                    *i += 1;
                }
                b'|' | b';' | b'>' | b'<' => {
                    break;
                }
                c if c == b' ' || c == b'\t' => {
                    break;
                }
                _ => {
                    if *clean_len < clean.len() {
                        clean[*clean_len] = c;
                        *clean_len += 1;
                    }
                    *i += 1;
                }
            }
        }
    }

    if in_single_quote {
        *error = Some(ParseError::UnmatchedSingleQuote);
    } else if in_double_quote {
        *error = Some(ParseError::UnmatchedDoubleQuote);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! assert_token {
        ($r:expr, $i:expr, $kind:expr, $val:expr) => {
            let t = $r.get($i).unwrap();
            assert_eq!(t.0, $kind, "token[{}] kind mismatch", $i);
            assert_eq!(t.1, $val, "token[{}] value mismatch", $i);
        };
    }

    #[test]
    fn tokenizer_pipe() {
        let r = tokenize(b"cmd1 | cmd2");
        assert_eq!(r.error, None);
        assert_eq!(r.token_count, 3);
        assert_token!(r, 0, TokenKind::Word, b"cmd1");
        assert_token!(r, 1, TokenKind::Pipe, b"");
        assert_token!(r, 2, TokenKind::Word, b"cmd2");
    }

    #[test]
    fn tokenizer_redirect() {
        let r = tokenize(b"dir > out.txt");
        assert_eq!(r.error, None);
        assert_eq!(r.token_count, 2);
        assert_token!(r, 0, TokenKind::Word, b"dir");
        assert_token!(r, 1, TokenKind::RedirectStdout, b"out.txt");
    }

    #[test]
    fn tokenizer_redirect_append() {
        let r = tokenize(b"echo hi >> log.txt");
        assert_eq!(r.error, None);
        assert_eq!(r.token_count, 3);
        assert_token!(r, 0, TokenKind::Word, b"echo");
        assert_token!(r, 1, TokenKind::Word, b"hi");
        assert_token!(r, 2, TokenKind::RedirectAppend, b"log.txt");
    }

    #[test]
    fn tokenizer_redirect_stdin() {
        let r = tokenize(b"sort < input.txt");
        assert_eq!(r.error, None);
        assert_eq!(r.token_count, 2);
        assert_token!(r, 0, TokenKind::Word, b"sort");
        assert_token!(r, 1, TokenKind::RedirectStdin, b"input.txt");
    }

    #[test]
    fn tokenizer_redirect_stderr() {
        let r = tokenize(b"cmd 2> err.txt");
        assert_eq!(r.error, None);
        assert_eq!(r.token_count, 2);
        assert_token!(r, 0, TokenKind::Word, b"cmd");
        assert_token!(r, 1, TokenKind::RedirectStderr, b"err.txt");
    }

    #[test]
    fn tokenizer_multiple_redirects() {
        let r = tokenize(b"cmd < in.txt > out.txt");
        assert_eq!(r.error, None);
        assert_eq!(r.token_count, 3);
        assert_token!(r, 0, TokenKind::Word, b"cmd");
        assert_token!(r, 1, TokenKind::RedirectStdin, b"in.txt");
        assert_token!(r, 2, TokenKind::RedirectStdout, b"out.txt");
    }

    #[test]
    fn tokenizer_quoted_arg() {
        let r = tokenize(b"echo 'hello %PATH%'");
        assert_eq!(r.error, None);
        assert_eq!(r.token_count, 2);
        assert_token!(r, 0, TokenKind::Word, b"echo");
        assert_token!(r, 1, TokenKind::Word, b"hello %PATH%");
    }

    #[test]
    fn tokenizer_double_quotes() {
        let r = tokenize(b"echo \"hello world\"");
        assert_eq!(r.error, None);
        assert_eq!(r.token_count, 2);
        assert_token!(r, 0, TokenKind::Word, b"echo");
        assert_token!(r, 1, TokenKind::Word, b"hello world");
    }

    #[test]
    fn tokenizer_escape_char() {
        let r = tokenize(b"echo hello^| more");
        assert_eq!(r.error, None);
        assert_eq!(r.token_count, 3);
        assert_token!(r, 0, TokenKind::Word, b"echo");
        assert_token!(r, 1, TokenKind::Word, b"hello|");
        assert_token!(r, 2, TokenKind::Word, b"more");
    }

    #[test]
    fn tokenizer_semicolon() {
        let r = tokenize(b"cd src; dir");
        assert_eq!(r.error, None);
        assert_eq!(r.token_count, 4);
        assert_token!(r, 0, TokenKind::Word, b"cd");
        assert_token!(r, 1, TokenKind::Word, b"src");
        assert_token!(r, 2, TokenKind::Semicolon, b"");
        assert_token!(r, 3, TokenKind::Word, b"dir");
    }

    #[test]
    fn tokenizer_unmatched_double_quote() {
        let r = tokenize(b"echo \"hello");
        assert_eq!(r.error, Some(ParseError::UnmatchedDoubleQuote));
        assert_eq!(r.token_count, 2);
        assert_token!(r, 0, TokenKind::Word, b"echo");
        assert_token!(r, 1, TokenKind::Word, b"hello");
    }

    #[test]
    fn tokenizer_empty() {
        let r = tokenize(b"");
        assert_eq!(r.error, None);
        assert_eq!(r.token_count, 0);
    }

    #[test]
    fn tokenizer_escape_in_double_quote() {
        let r = tokenize(b"echo \"hello ^\"world\"");
        assert_eq!(r.error, None);
        assert_eq!(r.token_count, 2);
        assert_token!(r, 0, TokenKind::Word, b"echo");
        assert_token!(r, 1, TokenKind::Word, b"hello \"world");
    }

    #[test]
    fn tokenizer_multiple_spaces() {
        let r = tokenize(b"  cmd1   |   cmd2  ");
        assert_eq!(r.error, None);
        assert_eq!(r.token_count, 3);
        assert_token!(r, 0, TokenKind::Word, b"cmd1");
        assert_token!(r, 1, TokenKind::Pipe, b"");
        assert_token!(r, 2, TokenKind::Word, b"cmd2");
    }
}
