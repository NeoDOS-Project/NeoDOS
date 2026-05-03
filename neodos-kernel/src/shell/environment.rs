// src/shell/environment.rs

pub struct Environment {
    pub keys: [[u8; 32]; 64],
    pub values: [[u8; 128]; 64],
    pub count: usize,
}

impl Environment {
    pub const fn new() -> Self {
        Environment {
            keys: [[0; 32]; 64],
            values: [[0; 128]; 64],
            count: 0,
        }
    }
    
    pub fn set(&mut self, key: &str, value: &str) {
        // Find if already exists
        for i in 0..self.count {
            if let Ok(k) = core::str::from_utf8(&self.keys[i]) {
                if k.trim_matches('\0') == key {
                    self.update_entry(i, value);
                    return;
                }
            }
        }
        
        // Add new
        if self.count < 64 {
            let idx = self.count;
            self.update_key(idx, key);
            self.update_entry(idx, value);
            self.count += 1;
        }
    }
    
    fn update_key(&mut self, idx: usize, key: &str) {
        let bytes = key.as_bytes();
        let len = if bytes.len() > 31 { 31 } else { bytes.len() };
        self.keys[idx][..len].copy_from_slice(&bytes[..len]);
        for i in len..32 { self.keys[idx][i] = 0; }
    }
    
    fn update_entry(&mut self, idx: usize, value: &str) {
        let bytes = value.as_bytes();
        let len = if bytes.len() > 127 { 127 } else { bytes.len() };
        self.values[idx][..len].copy_from_slice(&bytes[..len]);
        for i in len..128 { self.values[idx][i] = 0; }
    }
    
    pub fn get(&self, key: &str) -> Option<&str> {
        for i in 0..self.count {
            if let Ok(k) = core::str::from_utf8(&self.keys[i]) {
                if k.trim_matches('\0') == key {
                    if let Ok(v) = core::str::from_utf8(&self.values[i]) {
                        return Some(v.trim_matches('\0'));
                    }
                }
            }
        }
        None
    }
}
