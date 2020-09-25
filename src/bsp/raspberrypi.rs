pub mod atags;
pub mod driver;
pub mod exception;

use crate::memory;
use crate::{bsp::device_driver, console};
use core::fmt;

pub static GPIO: device_driver::GPIO =
    unsafe { device_driver::GPIO::new(memory::map::mmio::GPIO_BASE) };

static PL011_UART: device_driver::PL011Uart = unsafe {
    device_driver::PL011Uart::new(
        memory::map::mmio::PL011_UART_BASE,
        exception::asynchronous::irq_map::PL011_UART,
    )
};

pub static SYSTEM_TIMER: device_driver::SystemTimer = unsafe {
    device_driver::SystemTimer::new(
        memory::map::mmio::SYS_TIMER_BASE,
        exception::asynchronous::irq_map::SYSTEM_TIMER,
    )
};

// pub static LOCAL_TIMER: device_driver::LocalTimer =
//     unsafe { device_driver::LocalTimer::new(exception::asynchronous::irq_map::LOCAL_TIMER) };

pub static INTERRUPT_CONTROLLER: device_driver::InterruptController = unsafe {
    device_driver::InterruptController::new(
        memory::map::mmio::LOCAL_INTERRUPT_CONTROLLER_BASE,
        memory::map::mmio::PERIPHERAL_INTERRUPT_CONTROLLER_BASE,
    )
};

/// Board identification.
pub fn board_name() -> &'static str {
    "Raspberry Pi 3"
}

/// In case of a panic, the panic handler uses this function to take a last shot at printing
/// something before the system is halted.
/// - Use only for printing during a panic.
pub unsafe fn panic_console_out() -> impl fmt::Write {
    let mut uart = device_driver::PanicUart::new(memory::map::mmio::PL011_UART_BASE);
    uart.init();
    uart
}

/// Return a reference to the console.
pub fn console() -> &'static impl console::interface::All {
    &PL011_UART
}

//--------------------------------------------------------------------------------------------------
// Testing
//--------------------------------------------------------------------------------------------------

/// Minimal code needed to bring up the console in QEMU (for testing only). This is often less steps
/// than on real hardware due to QEMU's abstractions.
///
/// For the RPi, nothing needs to be done.
pub fn qemu_bring_up_console() {}
