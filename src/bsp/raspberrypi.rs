// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Copyright (c) 2018-2020 Andre Richter <andre.o.richter@gmail.com>

//! Top-level BSP file for the Raspberry Pi 3 and 4.

pub mod console;
pub mod cpu;
pub mod driver;
pub mod exception;
pub mod memory;

//--------------------------------------------------------------------------------------------------
// Global instances
//--------------------------------------------------------------------------------------------------
use super::device_driver;

static GPIO: device_driver::GPIO =
    unsafe { device_driver::GPIO::new(memory::map::mmio::GPIO_BASE) };

static PL011_UART: device_driver::PL011Uart = unsafe {
    device_driver::PL011Uart::new(
        memory::map::mmio::PL011_UART_BASE,
        exception::asynchronous::irq_map::PL011_UART,
    )
};

static SYSTEM_TIMER: device_driver::SystemTimer =
    unsafe { device_driver::SystemTimer::new(
        memory::map::mmio::SYS_TIMER_BASE,
        exception::asynchronous::irq_map::SYSTEM_TIMER,
    ) 
};

static INTERRUPT_CONTROLLER: device_driver::InterruptController = unsafe {
    device_driver::InterruptController::new(
        memory::map::mmio::LOCAL_INTERRUPT_CONTROLLER_BASE,
        memory::map::mmio::PERIPHERAL_INTERRUPT_CONTROLLER_BASE,
    )
};

//--------------------------------------------------------------------------------------------------
// Public Code
//--------------------------------------------------------------------------------------------------

/// Board identification.
pub fn board_name() -> &'static str {
    "Raspberry Pi 3"
}
