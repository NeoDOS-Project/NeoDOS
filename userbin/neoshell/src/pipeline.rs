pub const MAX_PIPELINE: usize = 16;

/// Parse a command line and find all pipe (`|`) positions.
pub fn parse_pipeline(line: &[u8], pos: &mut [usize; MAX_PIPELINE]) -> usize {
    let mut c = 0;
    for i in 0..line.len() {
        if line[i] == b'|' && c < MAX_PIPELINE {
            pos[c] = i;
            c += 1;
        }
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipeline_no_pipe() {
        let mut pos = [0usize; MAX_PIPELINE];
        let n = parse_pipeline(b"echo hello", &mut pos);
        assert_eq!(n, 0);
    }

    #[test]
    fn pipeline_single_pipe() {
        let mut pos = [0usize; MAX_PIPELINE];
        let n = parse_pipeline(b"cmd1 | cmd2", &mut pos);
        assert_eq!(n, 1);
        assert_eq!(pos[0], 5);
    }

    #[test]
    fn pipeline_multiple_pipes() {
        let mut pos = [0usize; MAX_PIPELINE];
        let n = parse_pipeline(b"a | b | c | d", &mut pos);
        assert_eq!(n, 3);
        assert_eq!(pos[0], 2);
        assert_eq!(pos[1], 6);
        assert_eq!(pos[2], 10);
    }

    #[test]
    fn pipeline_empty_input() {
        let mut pos = [0usize; MAX_PIPELINE];
        let n = parse_pipeline(b"", &mut pos);
        assert_eq!(n, 0);
    }

    #[test]
    fn pipeline_pipe_at_start() {
        let mut pos = [0usize; MAX_PIPELINE];
        let n = parse_pipeline(b"| cmd", &mut pos);
        assert_eq!(n, 1);
        assert_eq!(pos[0], 0);
    }

    #[test]
    fn pipeline_pipe_at_end() {
        let mut pos = [0usize; MAX_PIPELINE];
        let n = parse_pipeline(b"cmd |", &mut pos);
        assert_eq!(n, 1);
        assert_eq!(pos[0], 4);
    }
}

