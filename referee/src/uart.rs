// referee/src/uart.rs
// Eä UART Driver v5.0 — Robust serial logging

use core::fmt::{self, Write};

#[derive(Debug)]
pub enum UartError {
    NotInitialized,
    Timeout,
}

pub struct Uart {
    initialized: bool,
    base_port: u16,
}

impl Uart {
    pub const fn new() -> Self {
        Self {
            initialized: false,
            base_port: 0x3F8, // Standard COM1
        }
    }

    /// Initialize 16550 UART
    pub fn init(&mut self) -> Result<(), UartError> {
        unsafe {
            // Disable interrupts
            self.outb(self.base_port + 1, 0x00);
            
            // Enable DLAB for baud rate setting
            self.outb(self.base_port + 3, 0x80);
            
            // Set baud rate divisor (38400 baud)
            self.outb(self.base_port + 0, 0x03);
            self.outb(self.base_port + 1, 0x00);
            
            // 8 bits, no parity, one stop bit
            self.outb(self.base_port + 3, 0x03);
            
            // Enable FIFO, clear them, with 14-byte threshold
            self.outb(self.base_port + 2, 0xC7);
            
            // Enable modem control (RTS/DSR)
            self.outb(self.base_port + 4, 0x0B);
            
            // Enable interrupts
            self.outb(self.base_port + 1, 0x01);
        }

        self.initialized = true;
        Ok(())
    }

    /// Write a single byte with timeout
    fn write_byte(&mut self, byte: u8) -> Result<(), UartError> {
        if !self.initialized {
            return Err(UartError::NotInitialized);
        }

        // Wait for transmit buffer empty with timeout
        let mut timeout = 100_000;
        while timeout > 0 {
            if self.inb(self.base_port + 5) & 0x20 != 0 {
                break;
            }
            timeout -= 1;
        }

        if timeout == 0 {
            return Err(UartError::Timeout);
        }

        unsafe {
            self.outb(self.base_port, byte);
        }

        Ok(())
    }

    /// Write string to UART
    pub fn write_str(&mut self, s: &str) -> Result<(), UartError> {
        for &byte in s.as_bytes() {
            self.write_byte(byte)?;
        }
        Ok(())
    }

    /// Low-level port output
    unsafe fn outb(&self, port: u16, value: u8) {
        core::arch::asm!(
            "out dx, al",
            in("dx") port,
            in("al") value,
            options(nostack, preserves_flags)
        );
    }

    /// Low-level port input
    fn inb(&self, port: u16) -> u8 {
        let value: u8;
        unsafe {
            core::arch::asm!(
                "in al, dx",
                out("al") value,
                in("dx") port,
                options(nostack, preserves_flags)
            );
        }
        value
    }
}

impl Write for Uart {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write_str(s).map_err(|_| fmt::Error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uart_creation() {
        let uart = Uart::new();
        assert!(!uart.initialized);
        assert_eq!(uart.base_port, 0x3F8);
    }
}
