use crate::bsp::generic_timer;
use crate::info;
use crate::memory::map::mmio::BASE;
use crate::memory::ALLOCATOR;
use core::alloc::Layout;
use core::time::Duration;

/// MBox
/// https://github.com/raspberrypi/firmware/wiki/Mailbox-property-interfaces
pub const VIDEOCORE_MBOX: usize = BASE + 0x0000B880;

pub const MBOX_READ: *mut u32 = (VIDEOCORE_MBOX + 0x0) as *mut u32;
pub const MBOX_POLL: *mut u32 = (VIDEOCORE_MBOX + 0x10) as *mut u32;
pub const MBOX_SENDER: *mut u32 = (VIDEOCORE_MBOX + 0x14) as *mut u32;
pub const MBOX_STATUS: *mut u32 = (VIDEOCORE_MBOX + 0x18) as *mut u32;
pub const MBOX_CONFIG: *mut u32 = (VIDEOCORE_MBOX + 0x1C) as *mut u32;
pub const MBOX_WRITE: *mut u32 = (VIDEOCORE_MBOX + 0x20) as *mut u32;

pub const MBOX_RESPONSE: u32 = 0x80000000;
pub const MBOX_FULL: u32 = 0x80000000;
pub const MBOX_EMPTY: u32 = 0x40000000;

pub const MBOX_REQUEST: u32 = 0;

/// MBox channels
pub const MBOX_CH_POWER: u8 = 0;
pub const MBOX_CH_FB: u8 = 1;
pub const MBOX_CH_VUART: u8 = 2;
pub const MBOX_CH_VCHIQ: u8 = 3;
pub const MBOX_CH_LEDS: u8 = 4;
pub const MBOX_CH_BTNS: u8 = 5;
pub const MBOX_CH_TOUCH: u8 = 6;
pub const MBOX_CH_COUNT: u8 = 7;
pub const MBOX_CH_PROP: u32 = 8;

/// MBox tags
pub const MBOX_TAG_GETREVISION: u32 = 0x10002;
pub const MBOX_TAG_GETMAC: u32 = 0x10003;
pub const MBOX_TAG_GETSERIAL: u32 = 0x10004;
pub const MBOX_TAG_TEMPERATURE: u32 = 0x30006;
pub const MBOX_TAG_SET_POWER: u32 = 0x28001;
pub const MBOX_TAG_LAST: u32 = 0;

/// Power Management
pub const PM_RSTC: *mut u32 = (BASE + 0x0010001c) as *mut u32;
pub const PM_RSTS: *mut u32 = (BASE + 0x00100020) as *mut u32;
pub const PM_WDOG: *mut u32 = (BASE + 0x00100024) as *mut u32;
pub const PM_WDOG_MAGIC: u32 = 0x5a000000;
pub const PM_RSTC_FULLRST: u32 = 0x00000020;

use core::ptr::NonNull;

// Public interface to the mailbox
pub struct MBox {
    buffer: NonNull<[u32]>,
}

impl MBox {
    pub unsafe fn new() -> Result<MBox, ()> {
        let lay;
        match Layout::from_size_align(32 as usize * core::mem::size_of::<u32>(), 16) {
            Ok(layout) => lay = layout,

            Err(_) => {
                info!("[e] Layout Error!");
                return Err(());
            }
        }

        let ptr = (&ALLOCATOR)
            .lock()
            .allocate_first_fit(lay)
            .expect("Out of Memory I guess");

        let buffer = ptr.cast::<[u32; 32]>();

        return Ok(MBox { buffer });
    }

    pub unsafe fn call(&mut self, ch: u32) -> bool {
        while (MBOX_STATUS.read_volatile() & MBOX_FULL) != 0 {}

        /* write the address of our message to the mailbox with channel identifier */
        let buf = self.buffer.as_ptr() as *const u32;
        MBOX_WRITE.write_volatile((buf as u32 & !0xF) | (ch & 0xF));

        generic_timer().spin_sleep(Duration::from_millis(100));

        /* now wait for the response */
        loop {
            /* is there a response? */
            while (MBOX_STATUS.read_volatile() & MBOX_EMPTY) != 0 {}
            let resp: u32 = MBOX_READ.read_volatile();

            /* is it a response to our message? */
            if ((resp & 0xF) == ch) && ((resp & !0xF) == buf as u32) {
                llvm_asm!("dsb SY" ::: "memory" : "volatile");
                /* is it a valid successful response? */
                return self.buffer.as_ref()[1] == MBOX_RESPONSE;
            }
        }
    }

    pub fn serial_number(&mut self) -> Option<u64> {
        let buf = unsafe { self.buffer.as_mut() };
        buf[0] = 8 * 4;
        buf[1] = MBOX_REQUEST;
        buf[2] = MBOX_TAG_GETSERIAL;
        buf[3] = 8;
        buf[4] = 8;
        buf[5] = 0;
        buf[6] = 0;
        buf[7] = MBOX_TAG_LAST;

        unsafe {
            if self.call(MBOX_CH_PROP) {
                let ser: u64 =
                    (self.buffer.as_ref()[5] as u64) | ((self.buffer.as_ref()[6] as u64) << 32);
                Some(ser)
            } else {
                None
            }
        }
    }

    pub fn mac_address(&mut self) -> Option<u64> {
        let buf = unsafe { self.buffer.as_mut() };
        buf[0] = 8 * 4;
        buf[1] = MBOX_REQUEST;
        buf[2] = MBOX_TAG_GETMAC;
        buf[3] = 8;
        buf[4] = 8;
        buf[5] = 0;
        buf[6] = 0;
        buf[7] = MBOX_TAG_LAST;

        unsafe {
            if self.call(MBOX_CH_PROP) {
                let ser: u64 =
                    (self.buffer.as_ref()[5] as u64) | ((self.buffer.as_ref()[6] as u64) << 32);
                Some(ser)
            } else {
                None
            }
        }
    }

    pub fn board_revision(&mut self) -> Option<u32> {
        let buf = unsafe { self.buffer.as_mut() };
        buf[0] = 7 * 4;
        buf[1] = MBOX_REQUEST;
        buf[2] = MBOX_TAG_GETREVISION;
        buf[3] = 4;
        buf[4] = 8;
        buf[5] = 0;
        buf[6] = MBOX_TAG_LAST;

        if unsafe { self.call(MBOX_CH_PROP) } {
            Some(unsafe { self.buffer.as_ref()[5] })
        } else {
            None
        }
    }

    pub fn core_temperature(&mut self) -> Option<u32> {
        let buf = unsafe { self.buffer.as_mut() };
        buf[0] = 8 * 4;
        buf[1] = MBOX_REQUEST;
        buf[2] = MBOX_TAG_TEMPERATURE;
        buf[3] = 8;
        buf[4] = 8;
        buf[5] = 0;
        buf[6] = 0;
        buf[7] = MBOX_TAG_LAST;

        if unsafe { self.call(MBOX_CH_PROP) } {
            Some(unsafe { self.buffer.as_ref()[6] })
        } else {
            None
        }
    }

    pub fn set_power_state(&mut self, device_id: u32, enable: bool) -> Option<bool> {
        let mut state = 0u32;
        state |= 1 << 1; // wait for power change
        if enable {
            state |= 1;
        }

        let buf = unsafe { self.buffer.as_mut() };
        buf[0] = 8 * 4;
        buf[1] = MBOX_REQUEST;
        buf[2] = MBOX_TAG_SET_POWER;
        buf[3] = 8;
        buf[4] = 8;
        buf[5] = device_id;
        buf[6] = state;
        buf[7] = MBOX_TAG_LAST;

        if unsafe { self.call(MBOX_CH_PROP) } {
            Some((unsafe { self.buffer.as_ref()[6] & 0b10 }) == 0)
        } else {
            None
        }
    }
}
