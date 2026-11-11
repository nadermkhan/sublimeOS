extern crate alloc;
use alloc::collections::VecDeque;
use core::fmt;
use crate::arch::x86_64::idt::{register_irq_handler, InterruptContext};
use crate::arch::x86_64::io::{inb, outb};
use crate::arch::x86_64::pic::unmask;
use crate::fs::vfs::{FileType, InodeOperations, Stat};
use crate::sync::IrqSafeSpinlock;
use crate::kinfo;

// Standard 16550 UART I/O Ports
pub const COM1_PORT: u16 = 0x3F8;
pub const COM2_PORT: u16 = 0x2F8;
pub const COM3_PORT: u16 = 0x3E8;
pub const COM4_PORT: u16 = 0x2E8;

// Register Offsets (relative to base port)
#[allow(dead_code)]
const REG_DATA: u16 = 0;        // RBR (Read) / THR (Write)
#[allow(dead_code)]
const REG_IER: u16 = 1;         // Interrupt Enable Register
#[allow(dead_code)]
const REG_IIR_FCR: u16 = 2;     // IIR (Read) / FCR (Write)
#[allow(dead_code)]
const REG_LCR: u16 = 3;         // Line Control Register
#[allow(dead_code)]
const REG_MCR: u16 = 4;         // Modem Control Register
#[allow(dead_code)]
const REG_LSR: u16 = 5;         // Line Status Register
#[allow(dead_code)]
const REG_MSR: u16 = 6;         // Modem Status Register
#[allow(dead_code)]
const REG_SCRATCH: u16 = 7;     // Scratch Register

// Line Control Register Bits
#[allow(dead_code)]
const LCR_8_BITS: u8 = 0x03;
#[allow(dead_code)]
const LCR_1_STOP_BIT: u8 = 0x00;
#[allow(dead_code)]
const LCR_NO_PARITY: u8 = 0x00;
#[allow(dead_code)]
const LCR_DLAB: u8 = 0x80;

// Modem Control Register Bits
#[allow(dead_code)]
const MCR_DTR: u8 = 0x01;
#[allow(dead_code)]
const MCR_RTS: u8 = 0x02;
#[allow(dead_code)]
const MCR_OUT1: u8 = 0x04;
#[allow(dead_code)]
const MCR_OUT2: u8 = 0x08;      // Enables UART interrupt generation on PC motherboard
#[allow(dead_code)]
const MCR_LOOPBACK: u8 = 0x10;

// Line Status Register Bits
#[allow(dead_code)]
const LSR_DATA_READY: u8 = 0x01;
#[allow(dead_code)]
const LSR_THRE: u8 = 0x20;      // Transmitter Holding Register Empty
#[allow(dead_code)]
const LSR_TEMT: u8 = 0x40;      // Transmitter Empty

// Interrupt Enable Register Bits
#[allow(dead_code)]
const IER_RX_AVAILABLE: u8 = 0x01;
#[allow(dead_code)]
const IER_TX_EMPTY: u8 = 0x02;
#[allow(dead_code)]
const IER_LINE_STATUS: u8 = 0x04;
#[allow(dead_code)]
const IER_MODEM_STATUS: u8 = 0x08;

// FIFO Control Register Bits
#[allow(dead_code)]
const FCR_ENABLE_FIFO: u8 = 0x01;
#[allow(dead_code)]
const FCR_CLEAR_RX: u8 = 0x02;
#[allow(dead_code)]
const FCR_CLEAR_TX: u8 = 0x04;
#[allow(dead_code)]
const FCR_TRIGGER_14B: u8 = 0xC0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UartChipType {
    None,
    Uart8250,
    Uart16450,
    Uart16550,
    Uart16550A,
}

pub struct SerialPort {
    pub port: u16,
    pub irq: u8,
    pub chip_type: UartChipType,
    pub rx_buffer: VecDeque<u8>,
    pub is_initialized: bool,
}

impl SerialPort {
    pub const fn new(port: u16, irq: u8) -> Self {
        Self {
            port,
            irq,
            chip_type: UartChipType::None,
            rx_buffer: VecDeque::new(),
            is_initialized: false,
        }
    }

    /// Initialize the 16550A UART controller with hardware self-test, FIFO, and interrupts.
    pub fn init(&mut self) -> bool {
        unsafe {
            // 1. Disable all interrupts while configuring
            outb(self.port + REG_IER, 0x00);

            // 2. Hardware Scratch Register test (checks if chip is present)
            outb(self.port + REG_SCRATCH, 0x55);
            if inb(self.port + REG_SCRATCH) != 0x55 {
                return false;
            }
            outb(self.port + REG_SCRATCH, 0xAA);
            if inb(self.port + REG_SCRATCH) != 0xAA {
                return false;
            }

            // 3. Set baud rate: 115200 baud (divisor = 1: 115200 / 115200)
            self.set_baud_rate(115200);

            // 4. Configure line format: 8 bits, 1 stop bit, no parity (8-N-1)
            outb(self.port + REG_LCR, LCR_8_BITS | LCR_1_STOP_BIT | LCR_NO_PARITY);

            // 5. Enable and clear 16-byte FIFO with 14-byte trigger threshold
            outb(self.port + REG_IIR_FCR, FCR_ENABLE_FIFO | FCR_CLEAR_RX | FCR_CLEAR_TX | FCR_TRIGGER_14B);

            // 6. Identify chip type from IIR register
            let iir = inb(self.port + REG_IIR_FCR);
            self.chip_type = match (iir >> 6) & 0x03 {
                0x00 => UartChipType::Uart16450,
                0x01 => UartChipType::Uart16550,
                0x03 => UartChipType::Uart16550A,
                _ => UartChipType::Uart8250,
            };

            // 7. Perform Loopback Mode Self-Test
            outb(self.port + REG_MCR, MCR_LOOPBACK | MCR_OUT1 | MCR_OUT2 | MCR_RTS | MCR_DTR);
            outb(self.port + REG_DATA, 0xAE);
            let mut timeout = 1000;
            while !self.is_data_ready() && timeout > 0 {
                core::hint::spin_loop();
                timeout -= 1;
            }
            let loopback_result = inb(self.port + REG_DATA);
            if loopback_result != 0xAE {
                return false; // Hardware loopback failed
            }

            // 8. Exit loopback mode and enable normal operational mode with OUT2
            // NOTE: OUT2 is required on PC motherboards to gate the interrupt line to the PIC
            outb(self.port + REG_MCR, MCR_DTR | MCR_RTS | MCR_OUT1 | MCR_OUT2);

            // 9. Enable Received Data Available interrupt (ERBFI) and line status interrupt
            outb(self.port + REG_IER, IER_RX_AVAILABLE | IER_LINE_STATUS);

            self.is_initialized = true;
        }

        true
    }

    /// Set baud rate divisor. Default crystal frequency is 1.8432 MHz / 16 = 115200 Hz base.
    pub fn set_baud_rate(&self, baud: u32) {
        let divisor: u16 = (115200 / baud).max(1) as u16;
        unsafe {
            let lcr = inb(self.port + REG_LCR);
            outb(self.port + REG_LCR, lcr | LCR_DLAB); // Enable DLAB
            outb(self.port + REG_DATA, (divisor & 0xFF) as u8); // Divisor low byte
            outb(self.port + REG_IER, ((divisor >> 8) & 0xFF) as u8); // Divisor high byte
            outb(self.port + REG_LCR, lcr & !LCR_DLAB); // Disable DLAB
        }
    }

    pub fn is_transmit_empty(&self) -> bool {
        unsafe { (inb(self.port + REG_LSR) & LSR_THRE) != 0 }
    }

    pub fn write_byte(&self, byte: u8) {
        let mut timeout = 100_000;
        while !self.is_transmit_empty() && timeout > 0 {
            core::hint::spin_loop();
            timeout -= 1;
        }
        unsafe {
            outb(self.port + REG_DATA, byte);
        }
    }

    pub fn is_data_ready(&self) -> bool {
        unsafe { (inb(self.port + REG_LSR) & LSR_DATA_READY) != 0 }
    }

    pub fn try_read_byte(&mut self) -> Option<u8> {
        // First check internal interrupt-driven RX ring buffer
        if let Some(byte) = self.rx_buffer.pop_front() {
            return Some(byte);
        }

        // Otherwise check physical hardware FIFO directly
        if self.is_data_ready() {
            Some(unsafe { inb(self.port + REG_DATA) })
        } else {
            None
        }
    }

    pub fn read_byte(&mut self) -> u8 {
        loop {
            if let Some(b) = self.try_read_byte() {
                return b;
            }
            core::hint::spin_loop();
        }
    }

    /// Handle interrupt from UART controller: drains pending interrupt reasons and RX FIFO.
    pub fn handle_interrupt(&mut self) {
        unsafe {
            loop {
                let iir = inb(self.port + REG_IIR_FCR);
                if (iir & 1) != 0 {
                    // Bit 0 = 1 means no interrupt pending
                    break;
                }
                let int_id = (iir >> 1) & 0x07;
                match int_id {
                    // Receiver Line Status (0b011 = 3)
                    3 => {
                        let _ = inb(self.port + REG_LSR);
                    }
                    // Received Data Available (0b010 = 2) or Character Timeout (0b110 = 6)
                    2 | 6 => {
                        while (inb(self.port + REG_LSR) & LSR_DATA_READY) != 0 {
                            let byte = inb(self.port + REG_DATA);
                            if self.rx_buffer.len() < 4096 {
                                self.rx_buffer.push_back(byte);
                            }
                        }
                    }
                    // Transmitter Holding Register Empty (0b001 = 1)
                    1 => {
                        // Polled output used, nothing needed
                    }
                    // Modem Status (0b000 = 0)
                    0 => {
                        let _ = inb(self.port + REG_MSR);
                    }
                    _ => {
                        let _ = inb(self.port + REG_LSR);
                        while (inb(self.port + REG_LSR) & LSR_DATA_READY) != 0 {
                            let byte = inb(self.port + REG_DATA);
                            if self.rx_buffer.len() < 4096 {
                                self.rx_buffer.push_back(byte);
                            }
                        }
                    }
                }
            }
        }
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

// Global UART Controllers
pub static COM1: IrqSafeSpinlock<SerialPort> = IrqSafeSpinlock::new(SerialPort::new(COM1_PORT, 4));
pub static COM2: IrqSafeSpinlock<SerialPort> = IrqSafeSpinlock::new(SerialPort::new(COM2_PORT, 3));

// Alias for backwards compatibility with existing codebase
pub use COM1 as SERIAL1;

/// COM1 / COM3 Interrupt Service Routine (IRQ 4)
fn uart_irq4_handler(_ctx: &mut InterruptContext) {
    COM1.lock().handle_interrupt();
}

/// COM2 / COM4 Interrupt Service Routine (IRQ 3)
fn uart_irq3_handler(_ctx: &mut InterruptContext) {
    COM2.lock().handle_interrupt();
}

/// Early hardware initialization of COM1 before IDT/PIC are available.
pub fn init_early() {
    COM1.lock().init();
}

/// Full initialization of the UART subsystem: configures COM1 & COM2 and installs IRQ handlers.
pub fn init() {
    // 1. Initialize COM1
    let com1_ok = COM1.lock().is_initialized || COM1.lock().init();
    if com1_ok {
        let chip = COM1.lock().chip_type;
        kinfo!("UART COM1 online at 0x{:03X} (115200 8-N-1, {:?}, IRQ 4)", COM1_PORT, chip);
        register_irq_handler(4, uart_irq4_handler);
        unmask(4);
    }

    // 2. Initialize COM2
    let com2_ok = COM2.lock().init();
    if com2_ok {
        let chip = COM2.lock().chip_type;
        kinfo!("UART COM2 online at 0x{:03X} (115200 8-N-1, {:?}, IRQ 3)", COM2_PORT, chip);
        register_irq_handler(3, uart_irq3_handler);
        unmask(3);
    }
}

/// Non-blocking read of next received byte on COM1.
pub fn read_rx_char() -> Option<u8> {
    COM1.lock().try_read_byte()
}

#[doc(hidden)]
pub fn _serial_print(args: fmt::Arguments) {
    use core::fmt::Write;
    let mut serial = COM1.lock();
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

// ==========================================
// DevFS /dev/ttyS0 and /dev/ttyS1 Inodes
// ==========================================
pub struct UartDev(pub u8);

impl InodeOperations for UartDev {
    fn read(&self, _offset: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
        if buf.is_empty() {
            return Ok(0);
        }

        let mut count = 0;
        let mut port = if self.0 == 0 { COM1.lock() } else { COM2.lock() };

        while count < buf.len() {
            if let Some(byte) = port.try_read_byte() {
                buf[count] = byte;
                count += 1;
            } else {
                break;
            }
        }

        Ok(count)
    }

    fn write(&mut self, _offset: usize, buf: &[u8]) -> Result<usize, &'static str> {
        let port = if self.0 == 0 { COM1.lock() } else { COM2.lock() };
        for &byte in buf {
            if byte == b'\n' {
                port.write_byte(b'\r');
            }
            port.write_byte(byte);
        }
        Ok(buf.len())
    }

    fn stat(&self) -> Stat {
        Stat {
            inode: 120 + self.0 as u64,
            size: 0,
            file_type: FileType::CharDevice,
            mode: 0o666,
        }
    }
}
