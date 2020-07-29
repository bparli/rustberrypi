use crate::{bsp, driver, exception, sched};
use core::ops;
use register::{mmio::*, register_bitfields, register_structs};
use spin;

register_bitfields! {
    u32,

    /// Control/Status Register
    CS [
        M3  OFFSET(3) NUMBITS(1) [
            NoMatch = 0,
            Match = 1
        ],
        M2  OFFSET(2) NUMBITS(1) [
            NoMatch = 0,
            Match = 1
        ],
        M1  OFFSET(1) NUMBITS(1) [
            NoMatch = 0,
            Match = 1
        ],
        M0  OFFSET(0) NUMBITS(1) [
            NoMatch = 0,
            Match = 1
        ]
    ]
}

register_structs! {
    #[allow(non_snake_case)]
    pub RegisterBlock {
        (0x00 => CS: ReadWrite<u32, CS::Register>),
        (0x04 => CLO: ReadOnly<u32>),
        (0x08 => CHI: ReadOnly<u32>),
        (0x0c => C0: ReadWrite<u32>),
        (0x10 => C1: ReadWrite<u32>),
        (0x14 => C2: ReadWrite<u32>),
        (0x18 => C3: ReadWrite<u32>),
        (0x22 => @END),
    }
}

pub struct SystemTimer {
    inner: spin::Mutex<SystemTimerInner>,
    irq_number: bsp::device_driver::IRQNumber,
}

pub struct SystemTimerInner {
    base_addr: usize,
    interval: u32,
    cur_val: u32,
}

impl ops::Deref for SystemTimerInner {
    type Target = RegisterBlock;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.ptr() }
    }
}

impl SystemTimerInner {
    pub const unsafe fn new(base_addr: usize) -> Self {
        Self {
            base_addr,
            interval: 200000,
            cur_val: 0,
        }
    }

    pub fn init(&mut self) {
        self.cur_val = self.CLO.get();
        self.cur_val += self.interval;
        self.C1.set(self.cur_val);
    }

    fn ptr(&self) -> *const RegisterBlock {
        self.base_addr as *const _
    }

    fn handle(&mut self) {
        self.cur_val += self.interval;
        self.C1.set(self.cur_val);
        self.CS.write(CS::M1::Match);
    }
}

impl SystemTimer {
    pub const unsafe fn new(base_addr: usize, irq_number: bsp::device_driver::IRQNumber) -> Self {
        Self {
            inner: spin::Mutex::new(SystemTimerInner::new(base_addr)),
            irq_number: irq_number,
        }
    }
}

impl driver::interface::DeviceDriver for SystemTimer {
    fn compatible(&self) -> &str {
        "System Timer"
    }

    fn init(&self) -> Result<(), ()> {
        let mut data = self.inner.lock();
        data.init();

        Ok(())
    }

    fn register_and_enable_irq_handler(&'static self) -> Result<(), &'static str> {
        use bsp::exception::asynchronous::irq_manager;
        use exception::asynchronous::{interface::IRQManager, IRQDescriptor};

        let descriptor = IRQDescriptor {
            name: "System Timer",
            handler: self,
        };

        irq_manager().register_handler(self.irq_number, descriptor)?;
        irq_manager().enable(self.irq_number);

        Ok(())
    }
}

impl exception::asynchronous::interface::IRQHandler for SystemTimer {
    fn handle(&self, e: &mut exception::ExceptionContext) -> Result<(), &'static str> {
        use sched::SCHEDULER;

        let mut data = self.inner.lock();
        data.handle();
        SCHEDULER.timer_tick(e);

        Ok(())
    }
}
