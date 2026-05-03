// src/graphics.rs

#[derive(Clone, Copy)]
#[repr(C)]
pub struct FramebufferInfo {
    pub base_address: u64,
    pub size: usize,
    pub width: usize,
    pub height: usize,
    pub stride: usize,
}

pub struct Renderer {
    pub fb: FramebufferInfo,
}

impl Renderer {
    pub fn new(fb: FramebufferInfo) -> Self {
        Renderer { fb }
    }

    pub fn put_pixel(&self, x: usize, y: usize, color: u32) {
        if x >= self.fb.width || y >= self.fb.height { return; }
        
        unsafe {
            let pixel_ptr = (self.fb.base_address as *mut u32)
                .add(y * self.fb.stride + x);
            core::ptr::write_volatile(pixel_ptr, color);
        }
    }

    pub fn clear(&self, color: u32) {
        for y in 0..self.fb.height {
            for x in 0..self.fb.width {
                self.put_pixel(x, y, color);
            }
        }
    }
}

pub static mut RENDERER: Option<Renderer> = None;

pub fn init(fb: FramebufferInfo) {
    unsafe {
        let renderer = Renderer::new(fb);
        renderer.clear(0x000000); // Black
        RENDERER = Some(renderer);
    }
}
