#![allow(non_snake_case)]

use alloc::boxed::Box;
use alloc::string::String;
use core::alloc::{GlobalAlloc, Layout};
use core::ffi::c_void;
use core::slice;
use core::str::{from_utf8, Utf8Error};
use smoltcp::wire::EthernetAddress;

use crate::exception::asynchronous::{interface::IRQHandler, interface::IRQManager, IRQDescriptor};
use crate::memory::ALLOCATOR;
use crate::net::Frame;
use crate::{info, syscall};
use spin;

pub type TKernelTimerHandle = u64;
pub type TKernelTimerHandler = Option<
    unsafe extern "C" fn(hTimer: TKernelTimerHandle, pParam: *mut c_void, pContext: *mut c_void),
>;
pub type TInterruptHandler = Option<unsafe extern "C" fn(pParam: *mut c_void)>;

static USB_DRIVER: USBHandler = USBHandler::uninitialized();
static TIMER3_DRIVER: USBHandler = USBHandler::uninitialized();
struct Param(*mut c_void);

mod inner {
    use crate::{cpu, info, warn};
    use core::convert::TryInto;
    use core::ptr;
    use core::time::Duration;

    use super::{TKernelTimerHandle, TKernelTimerHandler};
    use crate::net::Frame;
    use crate::net::USPI_TIMER_HZ;

    #[allow(non_camel_case_types)]
    type c_uint = usize;

    pub struct USPi(());

    extern "C" {
        /// Returns 0 on failure
        fn USPiInitialize() -> i32;
        /// Check if the ethernet controller is available.
        /// Returns != 0 if available
        fn USPiEthernetAvailable() -> i32;
        fn USPiGetMACAddress(Buffer: &mut [u8; 6]);
        /// Returns != 0 if link is up
        fn USPiEthernetIsLinkUp() -> i32;
        /// Returns 0 on failure
        fn USPiSendFrame(pBuffer: *const u8, nLength: u32) -> i32;
        /// pBuffer must have size USPI_FRAME_BUFFER_SIZE
        /// Returns 0 if no frame is available or on failure
        fn USPiReceiveFrame(pBuffer: *mut u8, pResultLength: *mut u32) -> i32;
        /// Returns a timer handle (0 on failure)
        fn TimerStartKernelTimer(
            pThis: TKernelTimerHandle,
            nDelay: c_uint, // in HZ units
            pHandler: TKernelTimerHandler,
            pParam: *mut core::ffi::c_void,
            pContext: *mut core::ffi::c_void,
        ) -> c_uint;
        fn TimerGet() -> TKernelTimerHandle;
    }

    impl !Sync for USPi {}

    impl USPi {
        /// The caller should assure that this function is called only once
        /// during the lifetime of the kernel.
        pub unsafe fn initialize() -> Option<Self> {
            if USPiInitialize() != 0 {
                return Some(USPi(()));
            }
            None
        }

        /// Returns whether ethernet is available on RPi
        pub fn is_eth_available(&mut self) -> bool {
            unsafe { USPiEthernetAvailable() != 0 }
        }

        /// Returns MAC address of RPi
        pub fn get_mac_address(&mut self, buf: &mut [u8; 6]) {
            unsafe { USPiGetMACAddress(buf) }
        }

        /// Checks whether RPi ethernet link is up or not
        pub fn is_eth_link_up(&mut self) -> bool {
            unsafe { USPiEthernetIsLinkUp() != 0 }
        }

        /// Sends an ethernet frame using USPiSendFrame
        pub fn send_frame(&mut self, frame: &Frame) -> Option<i32> {
            //info!("Send frame {:?}", frame);
            let result = unsafe { USPiSendFrame(frame.as_ptr(), frame.len()) };
            match result {
                0 => None,
                n => Some(n),
            }
        }

        /// Receives an ethernet frame using USPiRecvFrame
        pub fn recv_frame<'a>(&mut self, frame: &mut Frame) -> Option<i32> {
            let mut result_len = 0;
            //info!("Recv frame {:?}", frame);
            let result = unsafe { USPiReceiveFrame(frame.as_mut_ptr(), &mut result_len) };
            frame.set_len(result_len);
            match result {
                0 => None,
                n => Some(n),
            }
        }

        /// A wrapper function to `TimerStartKernelHandler`.
        pub fn start_kernel_timer(&mut self, delay: Duration, handler: TKernelTimerHandler) {
            info!(
                "Core {}, delay {:?}, handler {:?}",
                cpu::core_id::<usize>(),
                &delay,
                handler.map(|v| v as usize as *mut u8)
            );

            let divisor = (1000 / USPI_TIMER_HZ) as u128;
            let delay_as_hz = (delay.as_millis() + divisor - 1) / divisor;

            if let Ok(c_delay) = delay_as_hz.try_into() {
                unsafe {
                    TimerStartKernelTimer(
                        TimerGet(),
                        c_delay,
                        handler,
                        ptr::null_mut(),
                        ptr::null_mut(),
                    );
                }
            }
        }
    }
}

pub use inner::USPi;

unsafe fn layout(size: usize) -> Layout {
    Layout::from_size_align_unchecked(size + core::mem::size_of::<usize>(), 16)
}

#[no_mangle]
fn malloc(size: u32) -> *mut c_void {
    let layout = unsafe { layout(size as usize) };
    let kernel_ptr = unsafe { ALLOCATOR.alloc(layout) };
    let ptr = unsafe { kernel_ptr.offset(core::mem::size_of::<usize>() as isize) };
    let ptr_size_ptr = kernel_ptr as *mut usize;
    unsafe { *ptr_size_ptr = size as usize };
    ptr as *mut c_void
}

#[no_mangle]
fn free(ptr: *mut c_void) {
    let kernel_ptr = unsafe { ptr.offset(0 - (core::mem::size_of::<usize>() as isize)) };
    let ptr_size_ptr = kernel_ptr as *mut usize;
    let layout = unsafe { layout(*ptr_size_ptr) };
    unsafe { ALLOCATOR.dealloc(kernel_ptr as *mut u8, layout) };
}

#[no_mangle]
pub fn TimerSimpleMsDelay(nMilliSeconds: u32) {
    syscall::sleep(nMilliSeconds as u64)
}

#[no_mangle]
pub fn TimerSimpleusDelay(nMicroSeconds: u32) {
    syscall::sleep(1000 * nMicroSeconds as u64)
}

#[no_mangle]
pub fn MsDelay(nMilliSeconds: u32) {
    TimerSimpleMsDelay(nMilliSeconds);
}

#[no_mangle]
pub fn usDelay(nMicroSeconds: u32) {
    TimerSimpleusDelay(nMicroSeconds);
}

/// Registers `pHandler` to the kernel's IRQ handler registry.
/// When the next time the kernel receives `nIRQ` signal, `pHandler` handler
/// function should be invoked with `pParam`.
///
/// If `nIRQ == Interrupt::Usb`, register the handler to FIQ interrupt handler
/// registry. Otherwise, register the handler to the global IRQ interrupt handler.
#[no_mangle]
pub unsafe fn ConnectInterrupt(nIRQ: u32, pHandler: TInterruptHandler, pParam: *mut c_void) {
    use crate::bsp::device_driver::{IRQNumber, PeripheralIRQ};
    use crate::bsp::exception::asynchronous::irq_manager;

    let handler = pHandler.unwrap();
    unsafe impl Send for Param {};
    unsafe impl Sync for Param {};
    let param = Param(pParam);

    match nIRQ as usize {
        2 => {
            USB_DRIVER.initialize(handler, param);
            pub const usb: IRQNumber = IRQNumber::Peripheral(PeripheralIRQ::new(2));
            let descriptor = IRQDescriptor {
                name: "USB",
                handler: &USB_DRIVER,
            };
            irq_manager().register_handler(usb, descriptor).unwrap();
            irq_manager().enable(usb);
        }
        1 => {
            TIMER3_DRIVER.initialize(handler, param);
            pub const timer: IRQNumber = IRQNumber::Peripheral(PeripheralIRQ::new(1));
            let descriptor = IRQDescriptor {
                name: "Timer3",
                handler: &TIMER3_DRIVER,
            };
            irq_manager().register_handler(timer, descriptor).unwrap();
            irq_manager().enable(timer);
        }
        int => panic!("FIQ is {:?}, only Timer3 and Usb supported", int),
    }
}

struct USBHandler {
    handler: Option<Box<unsafe extern "C" fn(*mut c_void)>>,
    param: Option<Box<Param>>,
}

impl USBHandler {
    pub const fn uninitialized() -> Self {
        Self {
            handler: None,
            param: None,
        }
    }
    pub fn initialize(&self, handler: unsafe extern "C" fn(*mut c_void), param: Param) -> Self {
        Self {
            handler: Some(Box::new(handler)),
            param: Some(Box::new(param)),
        }
    }
}

impl IRQHandler for USBHandler {
    fn handle(&self, _e: &mut crate::exception::ExceptionContext) -> Result<(), &'static str> {
        match &self.handler {
            Some(handler) => match &self.param {
                Some(param) => unsafe { (handler)(param.0) },
                None => {}
            },
            None => {}
        }
        Ok(())
    }
}

#[no_mangle]
pub unsafe fn DoLogWrite(_pSource: *const u8, _Severity: u32, pMessage: *const u8) {
    let message = match cstring(pMessage) {
        Ok(message_string) => message_string,
        Err(_) => String::from("pMessage sent to DoLogWrite() is not valid UTF-8"),
    };
    info!("[USPi Log] {}", message);
}

#[no_mangle]
pub fn DebugHexdump(_pBuffer: *const c_void, _nBufLen: u32, _pSource: *const u8) {
    unimplemented!("You don't have to implement this")
}

#[no_mangle]
pub unsafe fn uspi_assertion_failed(pExpr: *const u8, pFile: *const u8, nLine: u32) {
    let expr = match cstring(pExpr) {
        Ok(expr_string) => expr_string,
        Err(_) => String::from("pExpr sent to uspi_assertion_failed() is not valid UTF-8"),
    };
    let file = match cstring(pFile) {
        Ok(file_string) => file_string,
        Err(_) => String::from("pFile sent to uspi_assertion_failed() is not valid UTF-8"),
    };
    info!(
        "USPi Assertion Failed: Expression: {}, File: {}, Line: {}",
        expr, file, nLine
    );
}

unsafe fn cstr_len(cstr_ptr: *const u8) -> usize {
    let mut index = 0;
    while *cstr_ptr.offset(index) != 0 {
        index += 1;
    }
    index as usize
}

unsafe fn cstring(cstr_ptr: *const u8) -> Result<String, Utf8Error> {
    let len = cstr_len(cstr_ptr);
    let slice = slice::from_raw_parts(cstr_ptr, len);
    Ok(String::from(from_utf8(slice)?))
}

pub struct Usb(pub spin::Mutex<Option<USPi>>);

impl Usb {
    pub const fn uninitialized() -> Usb {
        Usb(spin::Mutex::new(None))
    }

    pub fn initialize(&self) -> bool {
        let mut inner = self.0.lock();
        unsafe {
            *inner = USPi::initialize();
            !inner.is_none()
        }
    }

    pub fn is_eth_available(&self) -> bool {
        self.0
            .lock()
            .as_mut()
            .expect("USB not initialized")
            .is_eth_available()
    }

    pub fn get_eth_addr(&self) -> EthernetAddress {
        let mut buf = [0; 6];
        self.0
            .lock()
            .as_mut()
            .expect("USB not initialized")
            .get_mac_address(&mut buf);
        return EthernetAddress::from_bytes(&buf);
    }

    pub fn is_eth_link_up(&self) -> bool {
        self.0
            .lock()
            .as_mut()
            .expect("USB not initialized")
            .is_eth_link_up()
    }

    pub fn send_frame(&self, frame: &Frame) -> Option<i32> {
        self.0
            .lock()
            .as_mut()
            .expect("USB not initialized")
            .send_frame(frame)
    }

    pub fn recv_frame(&self, frame: &mut Frame) -> Option<i32> {
        self.0
            .lock()
            .as_mut()
            .expect("USB not initialized")
            .recv_frame(frame)
    }
}
