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
        let count = self.fb.size / 4;
        unsafe {
            core::arch::asm!(
                "rep stosd",
                inout("rcx") count => _,
                inout("rdi") self.fb.base_address as *mut u32 => _,
                in("eax") color,
                options(nostack, preserves_flags)
            );
        }
    }
}

pub static mut RENDERER: Option<Renderer> = None;

pub fn init(fb: FramebufferInfo) {
    unsafe {
        if fb.base_address == 0 || fb.size == 0 {
            // No valid framebuffer — don't initialise the renderer.
            // draw_char() checks RENDERER.is_some() and will skip all screen output.
            // Serial output still works via serial_print! / println!.
            return;
        }
        let renderer = Renderer::new(fb);
        renderer.clear(0x000000); // Black
        RENDERER = Some(renderer);
    }
}
