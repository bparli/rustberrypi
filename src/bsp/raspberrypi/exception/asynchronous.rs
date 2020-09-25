use crate::{bsp, exception};

pub mod irq_map {
    use super::bsp::device_driver::{IRQNumber, LocalIRQ, PeripheralIRQ};

    pub const PL011_UART: IRQNumber = IRQNumber::Peripheral(PeripheralIRQ::new(57));
    pub const SYSTEM_TIMER: IRQNumber = IRQNumber::Peripheral(PeripheralIRQ::new(1));
    pub const LOCAL_TIMER: IRQNumber = IRQNumber::Local(LocalIRQ::new(1));
}

/// Return a reference to the IRQ manager.
pub fn irq_manager() -> &'static impl exception::asynchronous::interface::IRQManager<
    IRQNumberType = bsp::device_driver::IRQNumber,
> {
    &super::super::INTERRUPT_CONTROLLER
}
