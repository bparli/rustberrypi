use crate::{bsp::device_driver::common::MMIODerefWrapper, cpu, driver};
use register::{mmio::*, register_bitfields, register_structs};
use spin;
pub use cortex_a::asm::nop;

// GPIO registers.
//
// Descriptions taken from
// https://github.com/raspberrypi/documentation/files/1888662/BCM2837-ARM-Peripherals.-.Revised.-.V2-1.pdf
register_bitfields! {
    u32,

    /// GPIO Function Select 1
    GPFSEL1 [
        /// Pin 15
        FSEL15 OFFSET(15) NUMBITS(3) [
            Input = 0b000,
            Output = 0b001,
            AltFunc0 = 0b100,  // PL011 UART RX
            AltFunc1 = 0b010  // Mini UART - Alternate function 5

        ],

        /// Pin 14
        FSEL14 OFFSET(12) NUMBITS(3) [
            Input = 0b000,
            Output = 0b001,
            AltFunc0 = 0b100,  // PL011 UART TX
            AltFunc1 = 0b010  // Mini UART - Alternate function 5
        ]
    ],

    /// GPIO Pull-up/down Clock Register 0
    GPPUDCLK0 [
        /// Pin 15
        PUDCLK15 OFFSET(15) NUMBITS(1) [
            NoEffect = 0,
            AssertClock = 1
        ],

        /// Pin 14
        PUDCLK14 OFFSET(14) NUMBITS(1) [
            NoEffect = 0,
            AssertClock = 1
        ]
    ]
}

register_structs! {
    #[allow(non_snake_case)]
    RegisterBlock {
        (0x00 => GPFSEL0: ReadWrite<u32>),
        (0x04 => GPFSEL1: ReadWrite<u32, GPFSEL1::Register>),
        (0x08 => GPFSEL2: ReadWrite<u32>),
        (0x0C => GPFSEL3: ReadWrite<u32>),
        (0x10 => GPFSEL4: ReadWrite<u32>),
        (0x14 => GPFSEL5: ReadWrite<u32>),
        (0x18 => _reserved1),
        (0x94 => GPPUD: ReadWrite<u32>),
        (0x98 => GPPUDCLK0: ReadWrite<u32, GPPUDCLK0::Register>),
        (0x9C => GPPUDCLK1: ReadWrite<u32>),
        (0xA0 => @END),
    }
}

/// Abstraction for the associated MMIO registers.
type Regs = MMIODerefWrapper<RegisterBlock>;

/// Representation of the GPIO HW.
pub struct GPIO {
    inner: spin::Mutex<Regs>,
}

impl GPIO {
    /// Create an instance.
    ///
    /// # Safety
    ///
    /// - The user must ensure to provide the correct `base_addr`.
    pub const unsafe fn new(base_addr: usize) -> Self {
        Self {
            inner: spin::Mutex::new(Regs::new(base_addr)),
        }
    }

    /// Map PL011 UART as standard output.
    ///
    /// TX to pin 14
    /// RX to pin 15
    pub fn map_pl011_uart(&self) {
        let data = &self.inner.lock();

        // Map to pins.
        data.GPFSEL1
            .modify(GPFSEL1::FSEL14::AltFunc0 + GPFSEL1::FSEL15::AltFunc0);

        // Enable pins 14 and 15.
        data.GPPUD.set(0);
        cpu::spin_for_cycles(150);

        data.GPPUDCLK0
            .write(GPPUDCLK0::PUDCLK14::AssertClock + GPPUDCLK0::PUDCLK15::AssertClock);
        cpu::spin_for_cycles(150);

        data.GPPUDCLK0.set(0);
    }

    pub fn map_mini_uart(&self) {
        let data = &self.inner.lock();

        // map UART1 to GPIO pins
        data.GPFSEL1
            .modify(GPFSEL1::FSEL14::AltFunc1 + GPFSEL1::FSEL15::AltFunc1);

        data.GPPUD.set(0); // enable pins 14 and 15
        cpu::spin_for_cycles(150);

        data.GPPUDCLK0
            .write(GPPUDCLK0::PUDCLK14::AssertClock + GPPUDCLK0::PUDCLK15::AssertClock);
        cpu::spin_for_cycles(150);

        data.GPPUDCLK0.set(0);
    }
}

//------------------------------------------------------------------------------
// OS Interface Code
//------------------------------------------------------------------------------

impl driver::interface::DeviceDriver for GPIO {
    fn compatible(&self) -> &str {
        "BCM GPIO"
    }
}
