pub mod local_ic;
mod peripheral_ic;

use crate::{cpu, driver, exception};

/// Wrapper struct for a bitmask indicating pending IRQ numbers.
struct PendingIRQs {
    bitmask: u64,
}

pub type LocalIRQ =
    exception::asynchronous::IRQNumber<{ InterruptController::MAX_LOCAL_IRQ_NUMBER }>;
pub type PeripheralIRQ =
    exception::asynchronous::IRQNumber<{ InterruptController::MAX_PERIPHERAL_IRQ_NUMBER }>;

/// Used for the associated type of trait  [`exception::asynchronous::interface::IRQManager`].
#[derive(Copy, Clone)]
pub enum IRQNumber {
    Local(LocalIRQ),
    Peripheral(PeripheralIRQ),
}

/// Representation of the Interrupt Controller.
pub struct InterruptController {
    periph: peripheral_ic::PeripheralIC,
    local: local_ic::LocalIC,
}

impl PendingIRQs {
    pub fn new(bitmask: u64) -> Self {
        Self { bitmask }
    }
}

impl Iterator for PendingIRQs {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        use core::intrinsics::cttz;

        let next = cttz(self.bitmask);
        if next == 64 {
            return None;
        }

        self.bitmask &= !(1 << next);

        Some(next as usize)
    }
}

impl InterruptController {
    const MAX_LOCAL_IRQ_NUMBER: usize = 11;
    const MAX_PERIPHERAL_IRQ_NUMBER: usize = 63;
    const NUM_PERIPHERAL_IRQS: usize = Self::MAX_PERIPHERAL_IRQ_NUMBER + 1;
    const NUM_LOCAL_IRQS: usize = Self::MAX_LOCAL_IRQ_NUMBER + 1;

    /// Create an instance.
    ///
    /// # Safety
    ///
    /// - The user must ensure to provide the correct `base_addr`.
    pub const unsafe fn new(local_base_addr: usize, periph_base_addr: usize) -> Self {
        Self {
            periph: peripheral_ic::PeripheralIC::new(periph_base_addr),
            local: local_ic::LocalIC::new(local_base_addr),
        }
    }
}

//------------------------------------------------------------------------------
// OS Interface Code
//------------------------------------------------------------------------------

impl driver::interface::DeviceDriver for InterruptController {
    fn compatible(&self) -> &str {
        "BCM Interrupt Controller"
    }
}

impl exception::asynchronous::interface::IRQManager for InterruptController {
    type IRQNumberType = IRQNumber;

    fn register_handler(
        &self,
        irq: Self::IRQNumberType,
        descriptor: exception::asynchronous::IRQDescriptor,
    ) -> Result<(), &'static str> {
        match irq {
            IRQNumber::Local(lirq) => self.local.register_handler(lirq, descriptor),
            IRQNumber::Peripheral(pirq) => self.periph.register_handler(pirq, descriptor),
        }
    }

    fn enable(&self, irq: Self::IRQNumberType) {
        match irq {
            IRQNumber::Peripheral(pirq) => self.periph.enable(pirq),
            IRQNumber::Local(lirq) => self.local.enable(lirq),
        }
    }

    fn handle_pending_irqs<'irq_context>(
        &'irq_context self,
        ic: &exception::asynchronous::IRQContext<'irq_context>,
        e: &mut exception::ExceptionContext,
    ) {
        if cpu::core_id::<usize>() == 0 {
            self.periph.handle_pending_irqs(ic, e);
        }
        self.local.handle_pending_irqs(ic, e);
    }

    fn print_handler(&self) {
        self.periph.print_handler();
        self.local.print_handler();
    }
}
