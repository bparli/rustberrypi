// Borrowed from https://github.com/sslab-gatech/cs3210-rustos-public/blob/lab5/kern/src/net.rs
pub mod uspi;

use alloc::boxed::Box;

pub const USPI_FRAME_BUFFER_SIZE: u32 = 1600;

pub const IP_ADDR: [u8; 4] = [169, 254, 32, 10];
pub const USPI_TIMER_HZ: usize = 10;

use alloc::collections::btree_map::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;
use core::convert::TryInto;
use core::time::Duration;

use smoltcp::iface::{EthernetInterfaceBuilder, Neighbor, NeighborCache};
use smoltcp::phy::{self, Device, DeviceCapabilities};
use smoltcp::socket::{SocketHandle, SocketRef, TcpSocketBuffer};
use smoltcp::time::Instant;
use smoltcp::wire::{IpAddress, IpCidr};

use crate::{bsp, cpu, info, warn};
use spin::Mutex;

pub type SocketSet = smoltcp::socket::SocketSet<'static>;
pub type TcpSocket = smoltcp::socket::TcpSocket<'static>;
pub type EthernetInterface<T> = smoltcp::iface::EthernetInterface<'static, T>;

pub static USB: uspi::Usb = uspi::Usb::uninitialized();
pub static ETH: GlobalEthernetDriver = GlobalEthernetDriver::uninitialized();

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
pub fn create_interface() -> EthernetInterface<UsbEthernet> {
    info!("CREATE interface for smoltcp");
    let device = UsbEthernet;
    let hw_addr = USB.get_eth_addr();

    let private_cidr = IpCidr::new(
        IpAddress::v4(IP_ADDR[0], IP_ADDR[1], IP_ADDR[2], IP_ADDR[3]),
        16,
    );
    let local_cidr = IpCidr::new(IpAddress::v4(127, 0, 0, 1), 8);
    EthernetInterfaceBuilder::new(device)
        .ethernet_addr(hw_addr)
        .neighbor_cache(NeighborCache::new(BTreeMap::new()))
        .ip_addrs([private_cidr, local_cidr])
        .finalize()
}

const PORT_MAP_SIZE: usize = 65536 / 64;

pub struct EthernetDriver {
    /// A set of sockets
    socket_set: SocketSet,
    /// Bitmap to track the port usage
    port_map: [u64; PORT_MAP_SIZE],
    /// Internal ethernet interface
    ethernet: EthernetInterface<UsbEthernet>,
}

impl EthernetDriver {
    /// Creates a fresh ethernet driver.
    fn new() -> EthernetDriver {
        EthernetDriver {
            socket_set: SocketSet::new(Vec::new()),
            port_map: [0; PORT_MAP_SIZE],
            ethernet: create_interface(),
        }
    }

    /// Polls the ethernet interface.
    /// See also `smoltcp::iface::EthernetInterface::poll()`.
    fn poll(&mut self, timestamp: Instant) {
        info!("EthernetDriver::poll() timestamp: {:?}", timestamp);
        match self.ethernet.poll(&mut self.socket_set, timestamp) {
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
    fn poll_delay(&mut self, timestamp: Instant) -> Duration {
        match self.ethernet.poll_delay(&self.socket_set, timestamp) {
            Some(delay) => delay.into(),
            None => Duration::from_millis(0),
        }
    }

    /// Marks a port as used. Returns `Some(port)` on success, `None` on failure.
    pub fn mark_port(&mut self, port: u16) -> Option<u16> {
        let port_map_index = port as usize / 64;
        let entry_bit_shift = port as u64 % 64;
        if (self.port_map[port_map_index] & (1 << entry_bit_shift)) >> entry_bit_shift == 0 {
            self.port_map[port_map_index] |= 1 << entry_bit_shift;
            info!("EthernetDriver::mark_port(): port {} marked", port);
            Some(port)
        } else {
            info!(
                "! EthernetDriver::mark_port(): port {} already marked",
                port
            );
            None
        }
    }

    /// Clears used bit of a port. Returns `Some(port)` on success, `None` on failure.
    pub fn erase_port(&mut self, port: u16) -> Option<u16> {
        let port_map_index = port as usize / 64;
        let entry_bit_shift = port as u64 % 64;
        if (self.port_map[port_map_index] & (1 << entry_bit_shift)) >> entry_bit_shift == 1 {
            self.port_map[port_map_index] &= !(1 << entry_bit_shift);
            info!("EthernetDriver::erase_port(): port {} freed", port);
            Some(port)
        } else {
            info!("! EthernetDriver::erase_port(): port {} already free", port);
            None
        }
    }

    /// Returns the first open port between the ephemeral port range 49152 ~ 65535.
    /// Note that this function does not mark the returned port.
    pub fn get_ephemeral_port(&mut self) -> Option<u16> {
        for port_map_index in 768..1024 {
            let first_unset_bit = (!self.port_map[port_map_index]).trailing_zeros();
            if first_unset_bit < 64 {
                let port = (port_map_index as u16 * 64) + first_unset_bit as u16;
                info!(
                    "EthernetDriver::get_ephemeral_port(): port {} available",
                    port
                );
                return Some(port);
            }
        }
        info!("! EthernetDriver::get_ephemeral_port(): no ephemeral ports free");
        None
    }

    /// Finds a socket with a `SocketHandle`.
    pub fn get_socket(&mut self, handle: SocketHandle) -> SocketRef<'_, TcpSocket> {
        self.socket_set.get::<TcpSocket>(handle)
    }

    /// This function creates a new TCP socket, adds it to the internal socket
    /// set, and returns the `SocketHandle` of the new socket.
    pub fn add_socket(&mut self) -> SocketHandle {
        let rx_buffer = TcpSocketBuffer::new(vec![0; 16384]);
        let tx_buffer = TcpSocketBuffer::new(vec![0; 16384]);
        let tcp_socket = TcpSocket::new(rx_buffer, tx_buffer);
        self.socket_set.add(tcp_socket)
    }

    /// Releases a socket from the internal socket set.
    pub fn release(&mut self, handle: SocketHandle) {
        self.socket_set.release(handle);
    }

    /// Prunes the internal socket set.
    pub fn prune(&mut self) {
        self.socket_set.prune();
    }
}

/// A thread-safe wrapper for `EthernetDriver`.
pub struct GlobalEthernetDriver(Mutex<Option<EthernetDriver>>);

impl GlobalEthernetDriver {
    pub const fn uninitialized() -> GlobalEthernetDriver {
        GlobalEthernetDriver(Mutex::new(None))
    }

    pub fn initialize(&self) {
        let mut lock = self.0.lock();
        *lock = Some(EthernetDriver::new());
    }

    pub fn poll(&self, timestamp: Instant) {
        if cpu::core_id::<usize>() == 0 {
            self.0
                .lock()
                .as_mut()
                .expect("Uninitialized EthernetDriver")
                .poll(timestamp)
        }
    }

    pub fn poll_delay(&self, timestamp: Instant) -> Duration {
        self.0
            .lock()
            .as_mut()
            .expect("Uninitialized EthernetDriver")
            .poll_delay(timestamp)
    }

    pub fn mark_port(&self, port: u16) -> Option<u16> {
        self.0
            .lock()
            .as_mut()
            .expect("Uninitialized EthernetDriver")
            .mark_port(port)
    }

    pub fn get_ephemeral_port(&self) -> Option<u16> {
        self.0
            .lock()
            .as_mut()
            .expect("Uninitialized EthernetDriver")
            .get_ephemeral_port()
    }

    pub fn add_socket(&self) -> SocketHandle {
        self.0
            .lock()
            .as_mut()
            .expect("Uninitialized EthernetDriver")
            .add_socket()
    }

    /// Enters a critical region and execute the provided closure with a mutable
    /// reference to the socket.
    pub fn with_socket<F, R>(&self, handle: SocketHandle, f: F) -> R
    where
        F: FnOnce(&mut SocketRef<'_, TcpSocket>) -> R,
    {
        let mut guard = self.0.lock();
        let mut socket = guard
            .as_mut()
            .expect("Uninitialized EthernetDriver")
            .get_socket(handle);

        f(&mut socket)
    }

    /// Enters a critical region and execute the provided closure with a mutable
    /// reference to the inner ethernet driver.
    pub fn critical<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut EthernetDriver) -> R,
    {
        let mut guard = self.0.lock();
        let mut ethernet = guard.as_mut().expect("Uninitialized EthernetDriver");

        f(&mut ethernet)
    }
}

pub extern "C" fn poll_ethernet(_: uspi::TKernelTimerHandle, _: *mut u8, _: *mut u8) {
    ETH.poll(Instant::from_millis(
        bsp::generic_timer().current_time().as_millis() as i64,
    ));
    let delay = ETH.poll_delay(Instant::from_millis(
        bsp::generic_timer().current_time().as_millis() as i64,
    ));
    USB.start_kernel_timer(delay, Some(poll_ethernet));
}
