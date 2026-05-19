// src/drivers/rtc.rs

pub struct Rtc {
    addr_port: u16,
    data_port: u16,
}

#[derive(Debug, Clone, Copy)]
pub struct DateTime {
    pub second: u8,
    pub minute: u8,
    pub hour: u8,
    pub day: u8,
    pub month: u8,
    pub year: u8,
}

impl Rtc {
    pub fn new() -> Self {
        Rtc {
            addr_port: 0x70,
            data_port: 0x71,
        }
    }

    fn read_register(&mut self, reg: u8) -> u8 {
        crate::hal::outb(self.addr_port, reg);
        crate::hal::inb(self.data_port)
    }

    pub fn get_datetime(&mut self) -> DateTime {
        // Disable NMI while reading
        let mut second = self.read_register(0x00);
        let mut minute = self.read_register(0x02);
        let mut hour = self.read_register(0x04);
        let mut day = self.read_register(0x07);
        let mut month = self.read_register(0x08);
        let mut year = self.read_register(0x09);

        // Check if BCD format is used
        let reg_b = self.read_register(0x0B);
        if (reg_b & 0x04) == 0 {
            // Convert BCD to binary
            second = ((second & 0xF0) >> 4) * 10 + (second & 0x0F);
            minute = ((minute & 0xF0) >> 4) * 10 + (minute & 0x0F);
            hour = ((hour & 0xF0) >> 4) * 10 + (hour & 0x0F);
            day = ((day & 0xF0) >> 4) * 10 + (day & 0x0F);
            month = ((month & 0xF0) >> 4) * 10 + (month & 0x0F);
            year = ((year & 0xF0) >> 4) * 10 + (year & 0x0F);
        }

        DateTime {
            second,
            minute,
            hour,
            day,
            month,
            year,
        }
    }
}