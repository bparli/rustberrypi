use core::fmt;
use core::ops::Range;
use core::ops::RangeInclusive;
use kernel_mem_range::*;
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
    pub const START:                                    usize =        0x0000_0000;
    pub const END:                                      usize =        0x3FFF_FFFF;
    
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

    pub mod virt {
        pub const KERN_STACK_START:    usize =             super::START;
        pub const KERN_STACK_END:      usize =             0x0007_FFFF;

        // The second 2 MiB block.
        pub const HEAP_START:          usize =             0x0020_0000;
        pub const HEAP_END:            usize =             0x005F_FFFF;
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

/// A virtual memory layout that is agnostic of the paging granularity that the
/// hardware MMU will use.
///
/// Contains only special ranges, aka anything that is _not_ normal cacheable
/// DRAM.
static KERNEL_VIRTUAL_LAYOUT: [Descriptor; 5] = [
    Descriptor {
        name: "Kernel stack",
        virtual_range: || {
            RangeInclusive::new(map::virt::KERN_STACK_START, map::virt::KERN_STACK_END)
        },
        translation: Translation::Identity,
        attribute_fields: AttributeFields {
            mem_attributes: MemAttributes::CacheableDRAM,
            acc_perms: AccessPermissions::ReadWrite,
            execute_never: true,
        },
    },
    Descriptor {
        name: "Kernel code and RO data",
        virtual_range: || {
            // Using the linker script, we ensure that the RO area is consecutive and 4
            // KiB aligned, and we export the boundaries via symbols:
            //
            // [__ro_start, __ro_end)
            extern "C" {
                // The inclusive start of the read-only area, aka the address of the
                // first byte of the area.
                static __ro_start: u64;

                // The exclusive end of the read-only area, aka the address of
                // the first byte _after_ the RO area.
                static __ro_end: u64;
            }

            unsafe {
                // Notice the subtraction to turn the exclusive end into an
                // inclusive end
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
    Descriptor {
        name: "Kernel data and BSS",
        virtual_range: || {
            extern "C" {
                static __ro_end: u64;
                static __bss_end: u64;
            }

            unsafe {
                RangeInclusive::new(
                    &__ro_end as *const _ as usize,
                    &__bss_end as *const _ as usize - 1,
                )
            }
        },
        translation: Translation::Identity,
        attribute_fields: AttributeFields {
            mem_attributes: MemAttributes::CacheableDRAM,
            acc_perms: AccessPermissions::ReadWrite,
            execute_never: true,
        },
    },
    Descriptor {
        name: "Heap",
        virtual_range: || RangeInclusive::new(map::virt::HEAP_START, map::virt::HEAP_END),
        translation: Translation::Identity,
        attribute_fields: AttributeFields {
            mem_attributes: MemAttributes::NonCacheableDRAM,
            acc_perms: AccessPermissions::ReadWrite,
            execute_never: true,
        },
    },
    Descriptor {
        name: "Device MMIO",
        virtual_range: || RangeInclusive::new(map::mmio::BASE, map::mmio::END_INCLUSIVE),
        translation: Translation::Identity,
        attribute_fields: AttributeFields {
            mem_attributes: MemAttributes::Device,
            acc_perms: AccessPermissions::ReadWrite,
            execute_never: true,
        },
    },
];

/// For a given virtual address, find and return the output address and
/// according attributes.
///
/// If the address is not covered in VIRTUAL_LAYOUT, return a default for normal
/// cacheable DRAM.
fn get_virt_addr_properties(virt_addr: usize) -> Result<(usize, AttributeFields), &'static str> {
    if virt_addr > map::END {
        return Err("Address out of range.");
    }

    for i in KERNEL_VIRTUAL_LAYOUT.iter() {
        if (i.virtual_range)().contains(&virt_addr) {
            let output_addr = match i.translation {
                Translation::Identity => virt_addr,
                Translation::Offset(a) => a + (virt_addr - (i.virtual_range)().start()),
            };

            return Ok((output_addr, i.attribute_fields));
        }
    }

    Ok((virt_addr, AttributeFields::default()))
}

/// Return the address space size in bytes.
pub const fn addr_space_size() -> usize {
    map::END + 1
}

pub fn heap_size() -> usize {
    map::virt::HEAP_END - map::virt::HEAP_START
}

/// Human-readable output of a Descriptor.
impl fmt::Display for Descriptor {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // Call the function to which self.range points, and dereference the
        // result, which causes Rust to copy the value.
        let start = *(self.virtual_range)().start();
        let end = *(self.virtual_range)().end();
        let size = end - start + 1;

        // log2(1024)
        const KIB_RSHIFT: u32 = 10;

        // log2(1024 * 1024)
        const MIB_RSHIFT: u32 = 20;

        let (size, unit) = if (size >> MIB_RSHIFT) > 0 {
            (size >> MIB_RSHIFT, "MiB")
        } else if (size >> KIB_RSHIFT) > 0 {
            (size >> KIB_RSHIFT, "KiB")
        } else {
            (size, "Byte")
        };

        let attr = match self.attribute_fields.mem_attributes {
            MemAttributes::CacheableDRAM => "C",
            MemAttributes::NonCacheableDRAM => "NC",
            MemAttributes::Device => "Dev",
        };

        let acc_p = match self.attribute_fields.acc_perms {
            AccessPermissions::ReadOnly => "RO",
            AccessPermissions::ReadWrite => "RW",
        };

        let xn = if self.attribute_fields.execute_never {
            "PXN"
        } else {
            "PX"
        };

        write!(
            f,
            "      {:#010X} - {:#010X} | {: >3} {} | {: <3} {} {: <3} | {}",
            start, end, size, unit, attr, acc_p, xn, self.name
        )
    }
}

/// Print the kernel memory layout.
pub fn print_layout() {
    use crate::info;

    info!("[i] Kernel memory layout:");

    for i in KERNEL_VIRTUAL_LAYOUT.iter() {
        info!("{}", i);
    }
}

/// Return a reference to the virtual memory layout.
pub fn virt_mem_layout() -> &'static [Descriptor; 5] {
    &KERNEL_VIRTUAL_LAYOUT
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_macros::kernel_test;

    /// Check 4 KiB alignment of the kernel's virtual memory layout sections.
    #[kernel_test]
    fn virt_mem_layout_sections_are_4KiB_aligned() {
        const FOUR_KIB: usize = 4096;

        for i in KERNEL_VIRTUAL_LAYOUT.iter() {
            if i.name != "Kernel data and BSS" {
                let start: usize = *(i.virtual_range)().start();
                let end: usize = *(i.virtual_range)().end() + 1;

                assert_eq!(start % FOUR_KIB, 0);
                assert_eq!(end % FOUR_KIB, 0);
                assert!(end >= start);
            }
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
