use spin::Mutex;
use lazy_static::lazy_static;

#[allow(dead_code)]
pub struct Pic {
    offset: u8,
    command_port: u16,
    data_port: u16,
}

impl Pic {
    pub const fn new(offset: u8, command: u16, data: u16) -> Self {
        Pic {
            offset,
            command_port: command,
            data_port: data,
        }
    }

    pub fn handles_interrupt(&self, interrupt_id: u8) -> bool {
        self.offset <= interrupt_id && interrupt_id < self.offset + 8
    }

    pub unsafe fn end_of_interrupt(&mut self) {
        self.write_command(0x20);
    }

    pub unsafe fn write_command(&mut self, cmd: u8) {
        crate::hal::outb(self.command_port, cmd);
    }

    pub unsafe fn write_data(&mut self, data: u8) {
        crate::hal::outb(self.data_port, data);
    }

    #[allow(dead_code)]
    pub unsafe fn read_data(&mut self) -> u8 {
        crate::hal::inb(self.data_port)
    }
}

pub struct ChainedPics {
    pics: [Pic; 2],
}

#[allow(dead_code)]
impl ChainedPics {
    pub const fn new(offset1: u8, offset2: u8) -> Self {
        ChainedPics {
            pics: [
                Pic::new(offset1, 0x20, 0x21),
                Pic::new(offset2, 0xA0, 0xA1),
            ],
        }
    }

    pub unsafe fn initialize(&mut self) {
        let wait = || {
            crate::hal::outb(0x80, 0);
        };

        // ICW1: Start initialization in cascade mode
        self.pics[0].write_command(0x11);
        wait();
        self.pics[1].write_command(0x11);
        wait();

        // ICW2: Set vector offsets
        self.pics[0].write_data(self.pics[0].offset);
        wait();
        self.pics[1].write_data(self.pics[1].offset);
        wait();

        // ICW3: Cascade communication
        self.pics[0].write_data(4);
        wait();
        self.pics[1].write_data(2);
        wait();

        // ICW4: 8086 mode
        self.pics[0].write_data(0x01);
        wait();
        self.pics[1].write_data(0x01);
        wait();

        // Unmask all interrupts
        self.pics[0].write_data(0x00);
        self.pics[1].write_data(0x00);
    }

    pub fn handles_interrupt(&self, interrupt_id: u8) -> bool {
        self.pics.iter().any(|p| p.handles_interrupt(interrupt_id))
    }

    pub unsafe fn notify_end_of_interrupt(&mut self, interrupt_id: u8) {
        if self.handles_interrupt(interrupt_id) {
            if self.pics[1].handles_interrupt(interrupt_id) {
                self.pics[1].end_of_interrupt();
            }
            self.pics[0].end_of_interrupt();
        }
    }
}

lazy_static! {
    pub static ref PICS: Mutex<ChainedPics> = Mutex::new(ChainedPics::new(32, 40));
}

pub fn init() {
    unsafe {
        PICS.lock().initialize();
    }
}

