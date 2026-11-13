use crate::arch::x86_64::io::{inb, outb};
use crate::sync::IrqSafeSpinlock;

const CMOS_ADDR: u16 = 0x70;
const CMOS_DATA: u16 = 0x71;

/// Anchor for NTP/system time synchronization: (unix_seconds, pit_ticks_at_sync).
static SYSTEM_TIME_ANCHOR: IrqSafeSpinlock<Option<(u64, u64)>> = IrqSafeSpinlock::new(None);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

pub fn is_leap_year(year: u16) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

pub fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        1 => 31,
        2 => if is_leap_year(year) { 29 } else { 28 },
        3 => 31,
        4 => 30,
        5 => 31,
        6 => 30,
        7 => 31,
        8 => 31,
        9 => 30,
        10 => 31,
        11 => 30,
        12 => 31,
        _ => 30,
    }
}

/// Convert a DateTime struct to UNIX timestamp (seconds since 1970-01-01 00:00:00 UTC).
pub fn datetime_to_unix(dt: &DateTime) -> u64 {
    let mut days = 0u64;
    for y in 1970..dt.year {
        days += if is_leap_year(y) { 366 } else { 365 };
    }
    for m in 1..dt.month {
        days += days_in_month(dt.year, m) as u64;
    }
    days += (dt.day.max(1) - 1) as u64;
    days * 86400 + (dt.hour as u64) * 3600 + (dt.minute as u64) * 60 + (dt.second as u64)
}

/// Convert a UNIX timestamp (seconds since 1970-01-01 UTC) into a DateTime struct.
pub fn unix_to_datetime(unix_secs: u64) -> DateTime {
    let mut days = (unix_secs / 86400) as u64;
    let rem_secs = (unix_secs % 86400) as u32;
    let hour = (rem_secs / 3600) as u8;
    let minute = ((rem_secs % 3600) / 60) as u8;
    let second = (rem_secs % 60) as u8;

    let mut year = 1970u16;
    loop {
        let days_in_cur_year = if is_leap_year(year) { 366 } else { 365 };
        if days >= days_in_cur_year {
            days -= days_in_cur_year;
            year += 1;
        } else {
            break;
        }
    }

    let mut month = 1u8;
    loop {
        let dim = days_in_month(year, month) as u64;
        if days >= dim && month < 12 {
            days -= dim;
            month += 1;
        } else {
            break;
        }
    }
    let day = (days + 1) as u8;

    DateTime {
        year,
        month,
        day,
        hour,
        minute,
        second,
    }
}

/// Set the system time anchor (e.g. from NTP or manual configuration).
pub fn set_system_time(unix_secs: u64) {
    let current_ticks = crate::arch::x86_64::pit::get_ticks();
    let mut anchor = SYSTEM_TIME_ANCHOR.lock();
    *anchor = Some((unix_secs, current_ticks));
}

/// Retrieve current UNIX timestamp (in seconds), utilizing the NTP/system anchor if available.
pub fn get_system_time_unix() -> u64 {
    let anchor = SYSTEM_TIME_ANCHOR.lock();
    if let Some((anchor_unix, anchor_tick)) = *anchor {
        let current_ticks = crate::arch::x86_64::pit::get_ticks();
        let elapsed_secs = current_ticks.saturating_sub(anchor_tick) / 1000;
        anchor_unix + elapsed_secs
    } else {
        drop(anchor);
        datetime_to_unix(&read_datetime())
    }
}

/// Retrieve current system DateTime (in UTC), using synchronized time if available.
pub fn get_system_datetime() -> DateTime {
    unix_to_datetime(get_system_time_unix())
}

/// Check if the system time has been synchronized with an external time source (e.g. NTP).
pub fn is_time_synchronized() -> bool {
    SYSTEM_TIME_ANCHOR.lock().is_some()
}

fn get_update_in_progress_flag() -> bool {
    unsafe {
        outb(CMOS_ADDR, 0x0A);
        let val = inb(CMOS_DATA);
        // On Apple hardware / modern UEFI, port 0x71 may return 0xFF (floating bus)
        if val == 0xFF {
            return false;
        }
        (val & 0x80) != 0
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

/// Read current UTC date and time from the CMOS Real-Time Clock with bounded spin limit.
pub fn read_datetime() -> DateTime {
    let mut spins = 0;
    while get_update_in_progress_flag() {
        core::hint::spin_loop();
        spins += 1;
        if spins > 1000 {
            break;
        }
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
