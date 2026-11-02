use core::fmt;
use crate::arch::x86_64::io::{inb, outb};
use crate::sync::IrqSafeSpinlock;

pub const COM1_PORT: u16 = 0x3F8;

pub struct SerialPort {
    port: u16,
}

impl SerialPort {
    pub const fn new(port: u16) -> Self {
        Self { port }
    }

    /// Initialize the 16550 UART serial port.
    pub fn init(&self) {
        unsafe {
            outb(self.port + 1, 0x00); // Disable interrupts
            outb(self.port + 3, 0x80); // Enable DLAB (set baud rate divisor)
            outb(self.port + 0, 0x01); // Set divisor to 1 (lo byte) 115200 baud
            outb(self.port + 1, 0x00); //                  (hi byte)
            outb(self.port + 3, 0x03); // 8 bits, no parity, one stop bit
            outb(self.port + 2, 0xC7); // Enable FIFO, clear them, 14-byte threshold
            outb(self.port + 4, 0x0B); // IRQs enabled, RTS/DSR set
        }
    }

    pub fn is_transmit_empty(&self) -> bool {
        unsafe { (inb(self.port + 5) & 0x20) != 0 }
    }

    pub fn write_byte(&self, byte: u8) {
        while !self.is_transmit_empty() {
            core::hint::spin_loop();
        }
        unsafe {
            outb(self.port, byte);
        }
    }

    pub fn is_data_ready(&self) -> bool {
        unsafe { (inb(self.port + 5) & 0x01) != 0 }
    }

    pub fn read_byte(&self) -> u8 {
        while !self.is_data_ready() {
            core::hint::spin_loop();
        }
        unsafe { inb(self.port) }
    }
}

impl fmt::Write for SerialPort {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            if byte == b'\n' {
                self.write_byte(b'\r');
            }
            self.write_byte(byte);
        }
        Ok(())
    }
}

pub static SERIAL1: IrqSafeSpinlock<SerialPort> = IrqSafeSpinlock::new(SerialPort::new(COM1_PORT));

#[doc(hidden)]
pub fn _serial_print(args: fmt::Arguments) {
    use core::fmt::Write;
    let mut serial = SERIAL1.lock();
    let _ = serial.write_fmt(args);
}

#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => {
        $crate::arch::x86_64::serial::_serial_print(format_args!($($arg)*))
    };
}

#[macro_export]
macro_rules! serial_println {
    () => ($crate::serial_print!("\n"));
    ($($arg:tt)*) => ($crate::serial_print!("{}\n", format_args!($($arg)*)));
}
