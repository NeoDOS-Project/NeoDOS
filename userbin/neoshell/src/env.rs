#[derive(Copy, Clone)]
pub struct EnvVar {
    pub key: [u8; 32],
    pub key_len: usize,
    pub val: [u8; 128],
    pub val_len: usize,
}

pub const MAX_ENV: usize = 16;
