use crate::arch::x86_64::io::{inb, outb};

const CMOS_ADDR: u16 = 0x70;
const CMOS_DATA: u16 = 0x71;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

fn get_update_in_progress_flag() -> bool {
    unsafe {
        outb(CMOS_ADDR, 0x0A);
        (inb(CMOS_DATA) & 0x80) != 0
    }
}

fn read_rtc_register(reg: u8) -> u8 {
    unsafe {
        outb(CMOS_ADDR, reg);
        inb(CMOS_DATA)
    }
}

fn bcd_to_bin(bcd: u8) -> u8 {
    ((bcd >> 4) * 10) + (bcd & 0x0F)
}

/// Read current UTC date and time from the CMOS Real-Time Clock.
pub fn read_datetime() -> DateTime {
    while get_update_in_progress_flag() {
        core::hint::spin_loop();
    }

    let mut second = read_rtc_register(0x00);
    let mut minute = read_rtc_register(0x02);
    let mut hour = read_rtc_register(0x04);
    let mut day = read_rtc_register(0x07);
    let mut month = read_rtc_register(0x08);
    let mut year = read_rtc_register(0x09) as u16;

    let register_b = read_rtc_register(0x0B);

    // Convert BCD to binary if needed
    if (register_b & 0x04) == 0 {
        second = bcd_to_bin(second);
        minute = bcd_to_bin(minute);
        hour = ((hour & 0x7F) >> 4) * 10 + (hour & 0x0F);
        day = bcd_to_bin(day);
        month = bcd_to_bin(month);
        year = bcd_to_bin(year as u8) as u16;
    }

    // Convert 12 hour clock to 24 hour if needed
    if (register_b & 0x02) == 0 && (hour & 0x80) != 0 {
        hour = ((hour & 0x7F) + 12) % 24;
    }

    // Calculate full year (assuming 21st century)
    year += 2000;

    DateTime {
        year,
        month,
        day,
        hour,
        minute,
        second,
    }
}
