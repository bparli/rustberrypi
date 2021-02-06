use super::{InterruptController, LocalIRQ, PendingIRQs};
use crate::{bsp::device_driver::common::MMIODerefWrapper, cpu, exception};
use register::{mmio::*, register_structs};

// BCM2837 Local Peripheral Registers (QA7: Chapter 4)
register_structs! {
    #[allow(non_snake_case)]
    Registers {
        (0x00 => control: ReadWrite<u32>),
        (0x04 => _reserved),
        (0x08 => core_timer_prescaler: ReadWrite<u32>),
        (0x0C => gpu_interrupts_routing: ReadWrite<u32>),
        (0x10 => pm_interrupts_routing_set: ReadWrite<u32>),
        (0x14 => pm_interrupts_routing_clear: ReadWrite<u32>),
        (0x18 => _reserved1),
        (0x1C => core_timer_access_low: ReadWrite<u32>),
        (0x20 => core_timer_access_high: ReadWrite<u32>),
        (0x24 => local_interrupt_routing: ReadWrite<u32>),
        (0x28 =>  _reserved2),
        (0x2C => axi_outstanding_counters: ReadWrite<u32>),
        (0x30 => axi_outstanding_irq: ReadWrite<u32>),
        (0x34 => local_timer_control_status: ReadWrite<u32>),
        (0x38 => local_timer_clear_reload: ReadWrite<u32>),
        (0x3C => _reserved3),
        (0x40 => core_timer_interrupt_control: [ReadWrite<u32>; 4]),
        (0x50 => core_mailboxes_interrupt_control: [ReadWrite<u32>; 4]),
        (0x60 => core_irq_source: [ReadOnly<u32>; 4]),
        (0x70 => core_fiq_source: [ReadWrite<u32>; 4]),
        (0x80 => @END),
    }
}

type Regs = MMIODerefWrapper<Registers>;

type HandlerTable =
    [Option<exception::asynchronous::IRQDescriptor>; InterruptController::NUM_LOCAL_IRQS];

/// Representation of the peripheral interrupt regsler.
pub struct LocalIC {
    registers: Regs,

    // Stores registered IRQ handlers. Writable only during kernel init. RO afterwards.
    handler_tables: spin::RwLock<[HandlerTable; 4]>,
}

impl LocalIC {
    /// Returns a new handle to the interrupt controller.
    pub const unsafe fn new(base_addr: usize) -> Self {
        Self {
            registers: Regs::new(base_addr),
            handler_tables: spin::RwLock::new([[None; InterruptController::NUM_LOCAL_IRQS]; 4]),
        }
    }

    /// Query the list of pending IRQs.
    fn get_pending(&self) -> PendingIRQs {
        let pending_mask: u64 =
            u64::from(self.registers.core_irq_source[cpu::core_id::<usize>()].get());
        PendingIRQs::new(pending_mask)
    }
}

impl exception::asynchronous::interface::IRQManager for LocalIC {
    type IRQNumberType = LocalIRQ;

    fn register_handler(
        &self,
        irq: Self::IRQNumberType,
        descriptor: exception::asynchronous::IRQDescriptor,
    ) -> Result<(), &'static str> {
        let irq_number = irq.get();
        let mut handler_tables = self.handler_tables.write();

        if handler_tables[0][irq_number].is_some() {
            return Err("IRQ handler already registered");
        }
        handler_tables[cpu::core_id::<usize>()][irq_number] = Some(descriptor);

        Ok(())
    }

    fn enable(&self, _irq: Self::IRQNumberType) {
        // only local timer for now
        let enable_bit: u32 = 1 << 1;
        self.registers.core_timer_interrupt_control[cpu::core_id::<usize>()].set(enable_bit);
    }

    fn handle_pending_irqs<'irq_context>(
        &'irq_context self,
        _ic: &exception::asynchronous::IRQContext<'irq_context>,
        e: &mut exception::ExceptionContext,
    ) {
        let handler_tables = self.handler_tables.read();
        for irq_number in self.get_pending() {
            let core_handler_table = handler_tables[cpu::core_id::<usize>()];
            match core_handler_table[irq_number] {
                None => {
                    // check if from gpu first
                    if irq_number == 8 {
                        crate::info!("Local Interrupt Controller: IRQ 8 fired");
                    } else {
                        panic!(
                            "Local Interrupt Controller: No handler registered for IRQ {}",
                            irq_number
                        )
                    }
                }
                Some(descriptor) => {
                    // Call the IRQ handler. Panics on failure.
                    descriptor.handler.handle(e).expect("Error handling IRQ");
                }
            }
        }
    }

    fn print_handler(&self) {
        use crate::info;

        info!("      Local handler:");
        let core_handler_table = self.handler_tables.read()[cpu::core_id::<usize>()];
        for (i, opt) in core_handler_table.iter().enumerate() {
            if let Some(handler) = opt {
                info!("            {: >3}. {}", i, handler.name);
            }
        }
    }
}
