use crate::bsp::atags::{Atag, Atags};
use crate::memory::mmu::*;
use core::ops::Range;
use core::ops::RangeInclusive;
use linked_list_allocator::LockedHeap;

pub mod mmu;

#[global_allocator]
pub static ALLOCATOR: LockedHeap = LockedHeap::empty();

/// Zero out a memory region.
///
/// # Safety
///
/// - `range.start` and `range.end` must be valid.
/// - `range.start` and `range.end` must be `T` aligned.
pub unsafe fn zero_volatile<T>(range: Range<*mut T>)
where
    T: From<u8>,
{
    let mut ptr = range.start;

    while ptr < range.end {
        core::ptr::write_volatile(ptr, T::from(0));
        ptr = ptr.offset(1);
    }
}

/// System memory map.
#[rustfmt::skip]
pub mod map {
    pub const END_INCLUSIVE:                            usize =        0xFFFF_FFFF;

    pub const GPIO_OFFSET:                              usize =        0x0020_0000;
    pub const UART_OFFSET:                              usize =        0x0020_1000;
    pub const SYS_TIMER_OFFSET:                         usize =        0x0000_3000;

    /// Physical devices.
    pub mod mmio {
        use super::*;

        pub const BASE:                                 usize =        0x3F00_0000;
        pub const PERIPHERAL_INTERRUPT_CONTROLLER_BASE: usize = BASE + 0x0000_B200;
        pub const GPIO_BASE:                            usize = BASE + GPIO_OFFSET;
        pub const PL011_UART_BASE:                      usize = BASE + UART_OFFSET;
        pub const SYS_TIMER_BASE:                       usize = BASE + SYS_TIMER_OFFSET;
        pub const LOCAL_INTERRUPT_CONTROLLER_BASE:      usize =        0x4000_0000;
        pub const END_INCLUSIVE:                        usize =        0x4000_FFFF;
    }
}

/// Types used for compiling the virtual memory layout of the kernel using
/// address ranges.
pub mod kernel_mem_range {
    use core::ops::RangeInclusive;

    #[derive(Copy, Clone)]
    pub enum MemAttributes {
        CacheableDRAM,
        NonCacheableDRAM,
        Device,
    }

    #[derive(Copy, Clone)]
    pub enum AccessPermissions {
        ReadOnly,
        ReadWrite,
    }

    #[allow(dead_code)]
    #[derive(Copy, Clone)]
    pub enum Translation {
        Identity,
        Offset(usize),
    }

    #[derive(Copy, Clone)]
    pub struct AttributeFields {
        pub mem_attributes: MemAttributes,
        pub acc_perms: AccessPermissions,
        pub execute_never: bool,
    }

    impl Default for AttributeFields {
        fn default() -> AttributeFields {
            AttributeFields {
                mem_attributes: MemAttributes::CacheableDRAM,
                acc_perms: AccessPermissions::ReadWrite,
                execute_never: true,
            }
        }
    }

    pub struct Descriptor {
        pub name: &'static str,
        pub virtual_range: fn() -> RangeInclusive<usize>,
        pub translation: Translation,
        pub attribute_fields: AttributeFields,
    }
}

//--------------------------------------------------------------------------------------------------
// Public Definitions
//--------------------------------------------------------------------------------------------------

const NUM_MEM_RANGES: usize = 2;

/// The virtual memory layout.
///
/// The layout must contain only special ranges, aka anything that is _not_ normal cacheable DRAM.
/// It is agnostic of the paging granularity that the architecture's MMU will use.
pub static LAYOUT: KernelVirtualLayout<{ NUM_MEM_RANGES }> = KernelVirtualLayout::new(
    map::END_INCLUSIVE,
    [
        RangeDescriptor {
            name: "Kernel code and RO data",
            virtual_range: || {
                // Using the linker script, we ensure that the RO area is consecutive and 64 KiB
                // aligned, and we export the boundaries via symbols:
                //
                // [__ro_start, __ro_end)
                extern "C" {
                    // The inclusive start of the read-only area, aka the address of the first
                    // byte of the area.
                    static __ro_start: usize;

                    // The exclusive end of the read-only area, aka the address of the first
                    // byte _after_ the RO area.
                    static __ro_end: usize;
                }

                unsafe {
                    // Notice the subtraction to turn the exclusive end into an inclusive end.
                    #[allow(clippy::range_minus_one)]
                    RangeInclusive::new(
                        &__ro_start as *const _ as usize,
                        &__ro_end as *const _ as usize - 1,
                    )
                }
            },
            translation: Translation::Identity,
            attribute_fields: AttributeFields {
                mem_attributes: MemAttributes::CacheableDRAM,
                acc_perms: AccessPermissions::ReadOnly,
                execute_never: false,
            },
        },
        RangeDescriptor {
            name: "Device MMIO",
            virtual_range: || RangeInclusive::new(map::mmio::BASE, map::mmio::END_INCLUSIVE),
            translation: Translation::Identity,
            attribute_fields: AttributeFields {
                mem_attributes: MemAttributes::Device,
                acc_perms: AccessPermissions::ReadWrite,
                execute_never: true,
            },
        },
    ],
);

//--------------------------------------------------------------------------------------------------
// Public Code
//--------------------------------------------------------------------------------------------------

/// Return the address space size in bytes.
pub const fn addr_space_size() -> usize {
    map::END_INCLUSIVE + 1
}

/// Return a reference to the virtual memory layout.
pub fn virt_mem_layout() -> &'static KernelVirtualLayout<{ NUM_MEM_RANGES }> {
    &LAYOUT
}

// taken from https://github.com/sslab-gatech/cs3210-rustos-public/tree/lab5/lib/pi/src/atags
// Returns the (start address, end address) of the available memory on this
// system if it can be determined. If it cannot, `None` is returned.
//
// This function is expected to return `Some` under all normal cirumstances.
pub fn heap_map() -> Option<(usize, usize)> {
    extern "C" {
        static __text_end: usize;
    }
    let binary_end = unsafe { &__text_end as *const _ as usize };

    let atags = Atags::get();
    for atag in atags {
        let (mem_start, mem_size) = match atag {
            Atag::Mem(mem) => (mem.start, mem.size),
            _ => continue,
        };
        return Some((binary_end, (mem_start + mem_size) as usize));
    }
    None
}

//--------------------------------------------------------------------------------------------------
// Testing
//--------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use test_macros::kernel_test;

    /// Check 64 KiB alignment of the kernel's virtual memory layout sections.
    #[kernel_test]
    fn virt_mem_layout_sections_are_64KiB_aligned() {
        const SIXTYFOUR_KIB: usize = 65536;

        for i in LAYOUT.inner().iter() {
            let start: usize = *(i.virtual_range)().start();
            let end: usize = *(i.virtual_range)().end() + 1;

            assert_eq!(start % SIXTYFOUR_KIB, 0);
            assert_eq!(end % SIXTYFOUR_KIB, 0);
            assert!(end >= start);
        }
    }

    /// Check `zero_volatile()`.
    #[kernel_test]
    fn zero_volatile_works() {
        let mut x: [usize; 3] = [10, 11, 12];
        let x_range = x.as_mut_ptr_range();

        unsafe { zero_volatile(x_range) };

        assert_eq!(x, [0, 0, 0]);
    }
}
