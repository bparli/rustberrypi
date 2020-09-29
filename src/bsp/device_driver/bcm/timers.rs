use crate::{bsp, driver, exception};
use core::ops;
use core::time::Duration;
use cortex_a::regs::*;
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
    fn handle(&self, _e: &mut exception::ExceptionContext) -> Result<(), &'static str> {
        let mut data = self.inner.lock();
        data.handle();

        //crate::sched::SCHEDULER.timer_tick(_e);

        Ok(())
    }
}

pub struct GenericSystemTimer {
    base_addr: usize,
}

impl GenericSystemTimer {
    pub const unsafe fn new(base_addr: usize) -> Self {
        Self { base_addr }
    }

    fn ptr(&self) -> *const RegisterBlock {
        self.base_addr as *const _
    }

    pub fn current_time(&self) -> Duration {
        let low = self.CLO.get();
        let high = self.CHI.get();
        Duration::from_micros(((high as u64) << 32) | low as u64)
    }
}

impl ops::Deref for GenericSystemTimer {
    type Target = RegisterBlock;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.ptr() }
    }
}

pub struct LocalTimer {
    interval: u64,
    irq_number: bsp::device_driver::IRQNumber,
}

impl LocalTimer {
    pub const unsafe fn new(irq_number: bsp::device_driver::IRQNumber) -> Self {
        Self {
            interval: 200, // in milliseconds
            irq_number: irq_number,
        }
    }

    pub fn resolution(&self) -> Duration {
        Duration::from_nanos(1_000_000_000 / (CNTFRQ_EL0.get() as u64))
    }

    pub fn init(&self) {
        self.tick();
    }

    pub fn register_and_enable_irq_handler(&'static self) -> Result<(), &'static str> {
        use bsp::exception::asynchronous::irq_manager;
        use exception::asynchronous::{interface::IRQManager, IRQDescriptor};

        // setup irq handler for local timer
        let descriptor = IRQDescriptor {
            name: "Local Timer",
            handler: self,
        };

        irq_manager().register_handler(self.irq_number, descriptor)?;
        irq_manager().enable(self.irq_number);
        self.tick();
        Ok(())
    }

    fn tick(&self) {
        use core::convert::TryInto;
        let timer_frequency = CNTFRQ_EL0.get() as u64;
        let interval = Duration::from_millis(self.interval);
        let ticks = (timer_frequency * interval.as_nanos() as u64) / 1_000_000_000;
        CNTP_TVAL_EL0.set(ticks.try_into().unwrap());
        CNTP_CTL_EL0.modify(CNTP_CTL_EL0::ENABLE::SET + CNTP_CTL_EL0::IMASK::CLEAR);
    }
}

impl exception::asynchronous::interface::IRQHandler for LocalTimer {
    fn handle(&self, e: &mut exception::ExceptionContext) -> Result<(), &'static str> {
        use crate::sched::SCHEDULER;
        SCHEDULER.timer_tick(e);
        self.tick();

        Ok(())
    }
}
