use crate::bsp::device_driver::GPIO;
use crate::{bsp, console, driver};
pub use asm::nop;
use core::{fmt, ops};
use cortex_a::asm;
use register::{mmio::*, register_bitfields};
use spin;

// Auxilary mini UART registers
//
// Descriptions taken from
// https://github.com/raspberrypi/documentation/files/1888662/BCM2837-ARM-Peripherals.-.Revised.-.V2-1.pdf
register_bitfields! {
    u32,

    /// Auxiliary enables
    AUX_ENABLES [
        /// If set the mini UART is enabled. The UART will immediately
        /// start receiving data, especially if the UART1_RX line is
        /// low.
        /// If clear the mini UART is disabled. That also disables any
        /// mini UART register access
        MINI_UART_ENABLE OFFSET(0) NUMBITS(1) []
    ],

    /// Mini Uart Interrupt Identify
    AUX_MU_IIR [
        /// Writing with bit 1 set will clear the receive FIFO
        /// Writing with bit 2 set will clear the transmit FIFO
        FIFO_CLEAR OFFSET(1) NUMBITS(2) [
            Rx = 0b01,
            Tx = 0b10,
            All = 0b11
        ]
    ],

    /// Mini Uart Line Control
    AUX_MU_LCR [
        /// Mode the UART works in
        DATA_SIZE OFFSET(0) NUMBITS(2) [
            SevenBit = 0b00,
            EightBit = 0b11
        ]
    ],

    /// Mini Uart Line Status
    AUX_MU_LSR [
        /// This bit is set if the transmit FIFO is empty and the transmitter is
        /// idle. (Finished shifting out the last bit).
        TX_IDLE    OFFSET(6) NUMBITS(1) [],

        /// This bit is set if the transmit FIFO can accept at least
        /// one byte.
        TX_EMPTY   OFFSET(5) NUMBITS(1) [],

        /// This bit is set if the receive FIFO holds at least 1
        /// symbol.
        DATA_READY OFFSET(0) NUMBITS(1) []
    ],

    /// Mini Uart Extra Control
    AUX_MU_CNTL [
        /// If this bit is set the mini UART transmitter is enabled.
        /// If this bit is clear the mini UART transmitter is disabled.
        TX_EN OFFSET(1) NUMBITS(1) [
            Disabled = 0,
            Enabled = 1
        ],

        /// If this bit is set the mini UART receiver is enabled.
        /// If this bit is clear the mini UART receiver is disabled.
        RX_EN OFFSET(0) NUMBITS(1) [
            Disabled = 0,
            Enabled = 1
        ]
    ],

    /// Mini Uart Baudrate
    AUX_MU_BAUD [
        /// Mini UART baudrate counter
        RATE OFFSET(0) NUMBITS(16) []
    ]
}

#[allow(non_snake_case)]
#[repr(C)]
pub struct RegisterBlock {
    __reserved_0: u32,                                  // 0x00
    AUX_ENABLES: ReadWrite<u32, AUX_ENABLES::Register>, // 0x04
    __reserved_1: [u32; 14],                            // 0x08
    AUX_MU_IO: ReadWrite<u32>,                          // 0x40 - Mini Uart I/O Data
    AUX_MU_IER: WriteOnly<u32>,                         // 0x44 - Mini Uart Interrupt Enable
    AUX_MU_IIR: WriteOnly<u32, AUX_MU_IIR::Register>,   // 0x48
    AUX_MU_LCR: WriteOnly<u32, AUX_MU_LCR::Register>,   // 0x4C
    AUX_MU_MCR: WriteOnly<u32>,                         // 0x50
    AUX_MU_LSR: ReadOnly<u32, AUX_MU_LSR::Register>,    // 0x54
    __reserved_2: [u32; 2],                             // 0x58
    AUX_MU_CNTL: WriteOnly<u32, AUX_MU_CNTL::Register>, // 0x60
    __reserved_3: u32,                                  // 0x64
    AUX_MU_BAUD: WriteOnly<u32, AUX_MU_BAUD::Register>, // 0x68
}

pub struct MiniUart {
    inner: spin::Mutex<MiniUartInner>,
}

impl MiniUart {
    /// # Safety
    ///
    /// - The user must ensure to provide the correct `base_addr`.
    pub const unsafe fn new(base_addr: usize) -> Self {
        Self {
            inner: spin::Mutex::new(MiniUartInner::new(base_addr)),
        }
    }

    pub fn init(&self, gpio: &GPIO) {
        let data = self.inner.lock();
        data.init(gpio);
    }
}

pub struct MiniUartInner {
    base_addr: usize,
}

/// Deref to RegisterBlock
///
/// Allows writing
/// ```
/// self.MU_IER.read()
/// ```
/// instead of something along the lines of
/// ```
/// unsafe { (*MiniUart::ptr()).MU_IER.read() }
/// ```
impl ops::Deref for MiniUartInner {
    type Target = RegisterBlock;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.ptr() }
    }
}

impl MiniUartInner {
    pub const fn new(base_addr: usize) -> MiniUartInner {
        MiniUartInner { base_addr }
    }

    /// Returns a pointer to the register block
    fn ptr(&self) -> *const RegisterBlock {
        self.base_addr as *const _
    }

    ///Set baud rate and characteristics (115200 8N1) and map to GPIO
    pub fn init(&self, gpio: &GPIO) {
        // initialize UART
        self.AUX_ENABLES.modify(AUX_ENABLES::MINI_UART_ENABLE::SET);
        self.AUX_MU_IER.set(0);
        self.AUX_MU_CNTL.set(0);
        self.AUX_MU_LCR.write(AUX_MU_LCR::DATA_SIZE::EightBit);
        self.AUX_MU_MCR.set(0);
        self.AUX_MU_IIR.write(AUX_MU_IIR::FIFO_CLEAR::All);
        self.AUX_MU_BAUD.write(AUX_MU_BAUD::RATE.val(270)); // 115200 baud

        gpio.map_mini_uart();

        self.AUX_MU_CNTL
            .write(AUX_MU_CNTL::RX_EN::Enabled + AUX_MU_CNTL::TX_EN::Enabled);

        // Clear FIFOs before using the device
        self.AUX_MU_IIR.write(AUX_MU_IIR::FIFO_CLEAR::All);
    }

    pub fn wait_tx_fifo_empty(&self) {
        loop {
            if self.AUX_MU_LSR.is_set(AUX_MU_LSR::TX_IDLE) {
                break;
            }

            nop();
        }
    }

    fn read_char(&self) -> char {
        // wait until something is in the buffer
        loop {
            if self.AUX_MU_LSR.is_set(AUX_MU_LSR::DATA_READY) {
                break;
            }

            nop();
        }

        // read it and return
        let mut ret = self.AUX_MU_IO.get() as u8 as char;

        // convert carrige return to newline
        if ret == '\r' {
            ret = '\n'
        }

        ret
    }

    fn clear(&self) {
        loop {
            if self.AUX_MU_LSR.is_set(AUX_MU_LSR::DATA_READY) {
                self.AUX_MU_IO.get();
            } else {
                break;
            }
        }
    }

    fn write_char(&self, c: char) {
        // wait until we can send
        loop {
            if self.AUX_MU_LSR.is_set(AUX_MU_LSR::TX_EMPTY) {
                break;
            }

            nop();
        }

        // write the character to the buffer
        self.AUX_MU_IO.set(c as u32);
    }
}

impl Drop for MiniUartInner {
    fn drop(&mut self) {
        self.AUX_ENABLES
            .modify(AUX_ENABLES::MINI_UART_ENABLE::CLEAR);
    }
}

impl fmt::Write for MiniUartInner {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.chars() {
            self.write_char(c);
        }
        Ok(())
    }
}

impl console::interface::Write for MiniUart {
    /// Passthrough of `args` to the `core::fmt::Write` implementation, but guarded by a Mutex to
    /// serialize access.
    fn write_char(&self, c: char) {
        let data = self.inner.lock();
        data.write_char(c);
    }

    fn write_fmt(&self, args: core::fmt::Arguments) -> fmt::Result {
        // Fully qualified syntax for the call to `core::fmt::Write::write:fmt()` to increase
        // readability.
        let mut data = self.inner.lock();
        fmt::Write::write_fmt(&mut *data, args)
    }

    fn flush(&self) {
        let data = self.inner.lock();
        data.wait_tx_fifo_empty();
    }
}

impl console::interface::Read for MiniUart {
    fn read_char(&self) -> char {
        let data = self.inner.lock();
        data.read_char()
    }

    fn clear(&self) {
        let data = self.inner.lock();
        data.clear()
    }
}

impl console::interface::Statistics for MiniUart {
    fn chars_written(&self) -> usize {
        0
    }

    fn chars_read(&self) -> usize {
        0
    }
}

impl driver::interface::DeviceDriver for MiniUart {
    fn compatible(&self) -> &str {
        "Mini UART"
    }

    fn init(&self) -> Result<(), ()> {
        let data = self.inner.lock();
        data.init(&bsp::GPIO);

        Ok(())
    }
}
