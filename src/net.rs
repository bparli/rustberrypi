// Borrowed from https://github.com/sslab-gatech/cs3210-rustos-public/blob/lab5/kern/src/net.rs
pub mod uspi;

use alloc::boxed::Box;

pub const USPI_FRAME_BUFFER_SIZE: u32 = 1600;

pub const IP_ADDR: [u8; 4] = [169, 254, 32, 10];
pub const USPI_TIMER_HZ: usize = 10;

use alloc::vec;
use alloc::vec::Vec;
use core::convert::TryInto;
use core::time::Duration;

use smoltcp::iface::{EthernetInterfaceBuilder, Neighbor, NeighborCache};
use smoltcp::phy::{self, Device, DeviceCapabilities};
use smoltcp::time::Instant;
use smoltcp::wire::{IpAddress, IpCidr};

use crate::{bsp, cpu, info, warn};
use spin::Mutex;

pub type SocketSet = smoltcp::socket::SocketSet<'static>;
pub type TcpSocket = smoltcp::socket::TcpSocket<'static>;
pub type EthernetInterface<T> = smoltcp::iface::EthernetInterface<'static, T>;

pub static USB: uspi::Usb = uspi::Usb::uninitialized();
//pub static ETH: GlobalEthernetDriver = GlobalEthernetDriver::uninitialized();

/// 8-byte aligned `u8` slice.
#[repr(align(8))]
struct FrameBuf([u8; USPI_FRAME_BUFFER_SIZE as usize]);

/// A fixed size buffer with length tracking functionality.
pub struct Frame {
    buf: Box<FrameBuf>,
    len: u32,
}

impl Frame {
    pub fn new() -> Self {
        Frame {
            buf: Box::new(FrameBuf([0; USPI_FRAME_BUFFER_SIZE as usize])),
            len: USPI_FRAME_BUFFER_SIZE,
        }
    }

    pub fn as_ptr(&self) -> *const u8 {
        self.buf.0.as_ptr()
    }

    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.buf.0.as_mut_ptr()
    }

    pub fn len(&self) -> u32 {
        self.len
    }

    pub fn set_len(&mut self, len: u32) {
        assert!(len <= USPI_FRAME_BUFFER_SIZE as u32);
        self.len = len;
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.buf.0[..self.len as usize]
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.buf.0[..self.len as usize]
    }
}

#[derive(Debug)]
pub struct UsbEthernet;

impl<'a> Device<'a> for UsbEthernet {
    type RxToken = RxToken;
    type TxToken = TxToken;

    fn capabilities(&self) -> DeviceCapabilities {
        let mut capability = DeviceCapabilities::default();
        capability.max_transmission_unit = USPI_FRAME_BUFFER_SIZE as usize;
        capability.max_burst_size = Some(1);
        capability
    }

    fn receive(&'a mut self) -> Option<(Self::RxToken, Self::TxToken)> {
        info!("UsbEthernet receive");
        let mut frame = Frame::new();
        match USB.recv_frame(&mut frame) {
            Some(_) => {
                let rx = RxToken { frame };
                let tx = TxToken {};
                Some((rx, tx))
            }
            _ => None,
        }
    }

    fn transmit(&'a mut self) -> Option<Self::TxToken> {
        info!("UsbEthernet TRANSMIT");
        Some(TxToken)
    }
}

pub struct RxToken {
    frame: Frame,
}

impl phy::RxToken for RxToken {
    fn consume<R, F>(mut self, _timestamp: Instant, f: F) -> smoltcp::Result<R>
    where
        F: FnOnce(&mut [u8]) -> smoltcp::Result<R>,
    {
        f(self.frame.as_mut_slice())
    }
}

pub struct TxToken;

impl phy::TxToken for TxToken {
    fn consume<R, F>(self, _timestamp: Instant, len: usize, f: F) -> smoltcp::Result<R>
    where
        F: FnOnce(&mut [u8]) -> smoltcp::Result<R>,
    {
        info!("phy::TxToken for TxToken consume");
        let mut frame = Frame::new();
        frame.set_len(len.try_into().unwrap());
        let result = f(frame.as_mut_slice());
        USB.send_frame(&frame);
        result
    }
}

/// Creates and returns a new ethernet interface using `UsbEthernet` struct.
fn create_interface() -> EthernetInterface<UsbEthernet> {
    info!("CREATE interface for smoltcp");
    let device = UsbEthernet;
    let hw_addr = USB.get_eth_addr();

    unsafe {
        ETH.private_cidr = Some(IpCidr::new(
            IpAddress::v4(IP_ADDR[0], IP_ADDR[1], IP_ADDR[2], IP_ADDR[3]),
            16,
        ));
        ETH.local_cidr = Some(IpCidr::new(IpAddress::v4(127, 0, 0, 1), 8));

        let neighbor_cache = NeighborCache::new(&mut ETH.neighbor_cache_storage.as_mut()[..]);

        EthernetInterfaceBuilder::new(device)
            .ethernet_addr(hw_addr)
            .neighbor_cache(neighbor_cache)
            .ip_addrs([ETH.private_cidr.unwrap(), ETH.local_cidr.unwrap()])
            .finalize()
    }
}

const PORT_MAP_SIZE: usize = 65536 / 64;

pub static mut ETH: EthernetDriver = EthernetDriver {
    socket_set: None,
    ethernet: None,
    neighbor_cache_storage: [None; 16],
    private_cidr: None,
    local_cidr: None,
};

pub struct EthernetDriver {
    /// A set of sockets
    socket_set: Option<SocketSet>,
    /// Internal ethernet interface
    ethernet: Option<Mutex<EthernetInterface<UsbEthernet>>>,

    neighbor_cache_storage: [Option<(IpAddress, Neighbor)>; 16],

    private_cidr: Option<IpCidr>,

    local_cidr: Option<IpCidr>,
}

impl EthernetDriver {
    /// Creates a fresh ethernet driver.
    pub fn initialize(&mut self) {
        self.ethernet = Some(Mutex::new(create_interface()));
        self.socket_set = Some(SocketSet::new(Vec::new()));
    }

    /// Polls the ethernet interface.
    /// See also `smoltcp::iface::EthernetInterface::poll()`.
    pub fn poll(&mut self, timestamp: Instant) {
        info!("EthernetDriver::poll() timestamp: {:?}", timestamp);
        let mut eth = self.ethernet.as_mut().unwrap().lock();
        info!("EthernetDriver::poll() timestamp: {:?}", timestamp);
        match eth.poll(&mut self.socket_set.as_mut().unwrap(), timestamp) {
            Ok(packets_processed) => {
                if packets_processed {
                    info!("EthernetDriver::poll() packets processed");
                } else {
                    info!("EthernetDriver::poll() no packets processed");
                }
            }
            Err(e) => match e {
                smoltcp::Error::Unrecognized => (),
                e => warn!("EthernetDriver::poll() error: {:?}", e),
            },
        }
    }

    /// Returns an advisory wait time to call `poll()` the next time.
    /// See also `smoltcp::iface::EthernetInterface::poll_delay()`.
    pub fn poll_delay(&mut self, timestamp: Instant) -> Duration {
        let eth = self.ethernet.as_ref().unwrap().lock();
        match eth.poll_delay(&self.socket_set.as_ref().unwrap(), timestamp) {
            Some(delay) => delay.into(),
            None => Duration::from_millis(0),
        }
    }
}

pub extern "C" fn poll_ethernet(_: uspi::TKernelTimerHandle, _: *mut u8, _: *mut u8) {
    unsafe {
        ETH.poll(Instant::from_millis(
            bsp::generic_timer().current_time().as_millis() as i64,
        ));
        let delay = ETH.poll_delay(Instant::from_millis(
            bsp::generic_timer().current_time().as_millis() as i64,
        ));
        USB.start_kernel_timer(delay, Some(poll_ethernet));
    }
}
