use super::{InterruptController, PendingIRQs, PeripheralIRQ};
use crate::{bsp::device_driver::common::MMIODerefWrapper, exception};
use register::{mmio::*, register_structs};

register_structs! {
    #[allow(non_snake_case)]
    WORegisterBlock {
        (0x00 => _reserved1),
        (0x10 => ENABLE_1: WriteOnly<u32>),
        (0x14 => ENABLE_2: WriteOnly<u32>),
        (0x24 => @END),
    }
}

register_structs! {
    #[allow(non_snake_case)]
    RORegisterBlock {
        (0x00 => _reserved1),
        (0x04 => PENDING_1: ReadOnly<u32>),
        (0x08 => PENDING_2: ReadOnly<u32>),
        (0x0c => @END),
    }
}

/// Abstraction for the WriteOnly parts of the associated MMIO registers.
type WriteOnlyRegs = MMIODerefWrapper<WORegisterBlock>;

/// Abstraction for the ReadOnly parts of the associated MMIO registers.
type ReadOnlyRegs = MMIODerefWrapper<RORegisterBlock>;

type HandlerTable =
    [Option<exception::asynchronous::IRQDescriptor>; InterruptController::NUM_PERIPHERAL_IRQS];

//--------------------------------------------------------------------------------------------------
// Public Definitions
//--------------------------------------------------------------------------------------------------

/// Representation of the peripheral interrupt regsler.
pub struct PeripheralIC {
    /// Access to write registers is guarded with a lock.
    wo_regs: spin::Mutex<WriteOnlyRegs>,

    /// Register read access is unguarded.
    ro_regs: ReadOnlyRegs,

    /// Stores registered IRQ handlers. Writable only during kernel init. RO afterwards.
    handler_table: spin::RwLock<HandlerTable>,
}

//--------------------------------------------------------------------------------------------------
// Public Code
//--------------------------------------------------------------------------------------------------

impl PeripheralIC {
    /// Create an instance.
    ///
    /// # Safety
    ///
    /// - The user must ensure to provide the correct `base_addr`.
    pub const unsafe fn new(base_addr: usize) -> Self {
        Self {
            wo_regs: spin::Mutex::new(WriteOnlyRegs::new(base_addr)),
            ro_regs: ReadOnlyRegs::new(base_addr),
            handler_table: spin::RwLock::new([None; InterruptController::NUM_PERIPHERAL_IRQS]),
        }
    }

    /// Query the list of pending IRQs.
    fn get_pending(&self) -> PendingIRQs {
        let pending_mask: u64 = (u64::from(self.ro_regs.PENDING_2.get()) << 32)
            | u64::from(self.ro_regs.PENDING_1.get());

        PendingIRQs::new(pending_mask)
    }
}

//------------------------------------------------------------------------------
// OS Interface Code
//------------------------------------------------------------------------------

impl exception::asynchronous::interface::IRQManager for PeripheralIC {
    type IRQNumberType = PeripheralIRQ;

    fn register_handler(
        &self,
        irq: Self::IRQNumberType,
        descriptor: exception::asynchronous::IRQDescriptor,
    ) -> Result<(), &'static str> {
        let mut table = self.handler_table.write();
        let irq_number = irq.get();
        if table[irq_number].is_some() {
            return Err("IRQ handler already registered");
        }
        table[irq_number] = Some(descriptor);

        Ok(())
    }

    fn enable(&self, irq: Self::IRQNumberType) {
        let regs = &self.wo_regs.lock();
        let enable_reg = if irq.get() <= 31 {
            &regs.ENABLE_1
        } else {
            &regs.ENABLE_2
        };

        let enable_bit: u32 = 1 << (irq.get() % 32);

        // Writing a 1 to a bit will set the corresponding IRQ enable bit. All other IRQ enable
        // bits are unaffected. So we don't need read and OR'ing here.
        enable_reg.set(enable_bit);
    }

    fn handle_pending_irqs<'irq_context>(
        &'irq_context self,
        _ic: &exception::asynchronous::IRQContext<'irq_context>,
        e: &mut exception::ExceptionContext,
    ) {
        let table = &self.handler_table.read();
        for irq_number in self.get_pending() {
            match table[irq_number] {
                None => panic!("No handler registered for IRQ {}", irq_number),
                Some(descriptor) => {
                    // Call the IRQ handler. Panics on failure.
                    descriptor.handler.handle(e).expect("Error handling IRQ");
                }
            }
        }
    }

    fn print_handler(&self) {
        use crate::info;

        info!("      Peripheral handler:");

        let table = &self.handler_table.read();
        for (i, opt) in table.iter().enumerate() {
            if let Some(handler) = opt {
                info!("            {: >3}. {}", i, handler.name);
            }
        }
    }
}
