use crate::memory;
use core::convert;
use core::{fmt, ops::RangeInclusive};
use cortex_a::{barrier, regs::*};
use register::register_bitfields;

/// Memory Management interfaces.
pub mod interface {

    /// MMU functions.
    pub trait MMU {
        /// Called by the kernel during early init. Supposed to take the translation tables from the
        /// `BSP`-supplied `virt_mem_layout()` and install/activate them for the respective MMU.
        ///
        /// # Safety
        ///
        /// - Changes the HW's global state.
        unsafe fn init(&self) -> Result<(), &'static str>;
    }
}

/// Architecture agnostic translation types.
#[allow(missing_docs)]
#[derive(Copy, Clone)]
pub enum Translation {
    Identity,
    Offset(usize),
}

/// Architecture agnostic memory attributes.
#[allow(missing_docs)]
#[derive(Copy, Clone)]
pub enum MemAttributes {
    CacheableDRAM,
    Device,
}

/// Architecture agnostic access permissions.
#[allow(missing_docs)]
#[derive(Copy, Clone)]
pub enum AccessPermissions {
    ReadOnly,
    ReadWrite,
}

/// Collection of memory attributes.
#[allow(missing_docs)]
#[derive(Copy, Clone)]
pub struct AttributeFields {
    pub mem_attributes: MemAttributes,
    pub acc_perms: AccessPermissions,
    pub execute_never: bool,
}

/// Architecture agnostic descriptor for a memory range.
#[allow(missing_docs)]
pub struct RangeDescriptor {
    pub name: &'static str,
    pub virtual_range: fn() -> RangeInclusive<usize>,
    pub translation: Translation,
    pub attribute_fields: AttributeFields,
}

/// Type for expressing the kernel's virtual memory layout.
pub struct KernelVirtualLayout<const NUM_SPECIAL_RANGES: usize> {
    /// The last (inclusive) address of the address space.
    max_virt_addr_inclusive: usize,

    /// Array of descriptors for non-standard (normal cacheable DRAM) memory regions.
    inner: [RangeDescriptor; NUM_SPECIAL_RANGES],
}

//--------------------------------------------------------------------------------------------------
// Public Code
//--------------------------------------------------------------------------------------------------

impl Default for AttributeFields {
    fn default() -> AttributeFields {
        AttributeFields {
            mem_attributes: MemAttributes::CacheableDRAM,
            acc_perms: AccessPermissions::ReadWrite,
            execute_never: true,
        }
    }
}

/// Human-readable output of a RangeDescriptor.
impl fmt::Display for RangeDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // Call the function to which self.range points, and dereference the result, which causes
        // Rust to copy the value.
        let start = *(self.virtual_range)().start();
        let end = *(self.virtual_range)().end();
        let size = end - start + 1;

        // log2(1024).
        const KIB_RSHIFT: u32 = 10;

        // log2(1024 * 1024).
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
            "      {:#010x} - {:#010x} | {: >3} {} | {: <3} {} {: <3} | {}",
            start, end, size, unit, attr, acc_p, xn, self.name
        )
    }
}

impl<const NUM_SPECIAL_RANGES: usize> KernelVirtualLayout<{ NUM_SPECIAL_RANGES }> {
    /// Create a new instance.
    pub const fn new(max: usize, layout: [RangeDescriptor; NUM_SPECIAL_RANGES]) -> Self {
        Self {
            max_virt_addr_inclusive: max,
            inner: layout,
        }
    }

    /// For a virtual address, find and return the output address and corresponding attributes.
    ///
    /// If the address is not found in `inner`, return an identity mapped default with normal
    /// cacheable DRAM attributes.
    pub fn virt_addr_properties(
        &self,
        virt_addr: usize,
    ) -> Result<(usize, AttributeFields), &'static str> {
        if virt_addr > self.max_virt_addr_inclusive {
            return Err("Address out of range");
        }

        for i in self.inner.iter() {
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

    /// Print the memory layout.
    pub fn print_layout(&self) {
        use crate::info;

        for i in self.inner.iter() {
            info!("{}", i);
        }
    }

    #[cfg(test)]
    pub fn inner(&self) -> &[RangeDescriptor; NUM_SPECIAL_RANGES] {
        &self.inner
    }
}

//--------------------------------------------------------------------------------------------------
// Private Definitions
//--------------------------------------------------------------------------------------------------

// A table descriptor, as per ARMv8-A Architecture Reference Manual Figure D5-15.
register_bitfields! {u64,
    STAGE1_TABLE_DESCRIPTOR [
        /// Physical address of the next descriptor.
        NEXT_LEVEL_TABLE_ADDR_64KiB OFFSET(16) NUMBITS(32) [], // [47:16]

        TYPE  OFFSET(1) NUMBITS(1) [
            Block = 0,
            Table = 1
        ],

        VALID OFFSET(0) NUMBITS(1) [
            False = 0,
            True = 1
        ]
    ]
}

// A level 3 page descriptor, as per ARMv8-A Architecture Reference Manual Figure D5-17.
register_bitfields! {u64,
    STAGE1_PAGE_DESCRIPTOR [
        /// Privileged execute-never.
        PXN      OFFSET(53) NUMBITS(1) [
            False = 0,
            True = 1
        ],

        /// Physical address of the next table descriptor (lvl2) or the page descriptor (lvl3).
        OUTPUT_ADDR_64KiB OFFSET(16) NUMBITS(32) [], // [47:16]

        /// Access flag.
        AF       OFFSET(10) NUMBITS(1) [
            False = 0,
            True = 1
        ],

        /// Shareability field.
        SH       OFFSET(8) NUMBITS(2) [
            OuterShareable = 0b10,
            InnerShareable = 0b11
        ],

        /// Access Permissions.
        AP       OFFSET(6) NUMBITS(2) [
            RW_EL1 = 0b00,
            RW_EL1_EL0 = 0b01,
            RO_EL1 = 0b10,
            RO_EL1_EL0 = 0b11
        ],

        /// Memory attributes index into the MAIR_EL1 register.
        AttrIndx OFFSET(2) NUMBITS(3) [],

        TYPE     OFFSET(1) NUMBITS(1) [
            Block = 0,
            Table = 1
        ],

        VALID    OFFSET(0) NUMBITS(1) [
            False = 0,
            True = 1
        ]
    ]
}

const SIXTYFOUR_KIB_SHIFT: usize = 16; //  log2(64 * 1024)
const FIVETWELVE_MIB_SHIFT: usize = 29; // log2(512 * 1024 * 1024)

/// A table descriptor for 64 KiB aperture.
///
/// The output points to the next table.
#[derive(Copy, Clone)]
#[repr(transparent)]
struct TableDescriptor(u64);

/// A page descriptor with 64 KiB aperture.
///
/// The output points to physical memory.
#[derive(Copy, Clone)]
#[repr(transparent)]
struct PageDescriptor(u64);

/// Big monolithic struct for storing the translation tables. Individual levels must be 64 KiB
/// aligned, hence the "reverse" order of appearance.
#[repr(C)]
#[repr(align(65536))]
struct TranslationTables<const N: usize> {
    /// Page descriptors, covering 64 KiB windows per entry.
    lvl3: [[PageDescriptor; 8192]; N],

    /// Table descriptors, covering 512 MiB windows.
    lvl2: [TableDescriptor; N],
}

/// Usually evaluates to 1 GiB for RPi3 and 4 GiB for RPi 4.
const ENTRIES_512_MIB: usize = memory::addr_space_size() >> FIVETWELVE_MIB_SHIFT;

/// The translation tables.
///
/// # Safety
///
/// - Supposed to land in `.bss`. Therefore, ensure that they boil down to all "0" entries.
static mut TABLES: TranslationTables<{ ENTRIES_512_MIB }> = TranslationTables {
    lvl3: [[PageDescriptor(0); 8192]; ENTRIES_512_MIB],
    lvl2: [TableDescriptor(0); ENTRIES_512_MIB],
};

trait BaseAddr {
    fn base_addr_u64(&self) -> u64;
    fn base_addr_usize(&self) -> usize;
}

/// Constants for indexing the MAIR_EL1.
#[allow(dead_code)]
mod mair {
    pub const DEVICE: u64 = 0;
    pub const NORMAL: u64 = 1;
}

//--------------------------------------------------------------------------------------------------
// Public Definitions
//--------------------------------------------------------------------------------------------------

/// Memory Management Unit type.
pub struct MemoryManagementUnit;

//--------------------------------------------------------------------------------------------------
// Global instances
//--------------------------------------------------------------------------------------------------

static MMU: MemoryManagementUnit = MemoryManagementUnit;

//--------------------------------------------------------------------------------------------------
// Private Code
//--------------------------------------------------------------------------------------------------

impl<T, const N: usize> BaseAddr for [T; N] {
    fn base_addr_u64(&self) -> u64 {
        self as *const T as u64
    }

    fn base_addr_usize(&self) -> usize {
        self as *const T as usize
    }
}

impl convert::From<usize> for TableDescriptor {
    fn from(next_lvl_table_addr: usize) -> Self {
        let shifted = next_lvl_table_addr >> SIXTYFOUR_KIB_SHIFT;
        let val = (STAGE1_TABLE_DESCRIPTOR::VALID::True
            + STAGE1_TABLE_DESCRIPTOR::TYPE::Table
            + STAGE1_TABLE_DESCRIPTOR::NEXT_LEVEL_TABLE_ADDR_64KiB.val(shifted as u64))
        .value;

        TableDescriptor(val)
    }
}

/// Convert the kernel's generic memory range attributes to HW-specific attributes of the MMU.
impl convert::From<AttributeFields>
    for register::FieldValue<u64, STAGE1_PAGE_DESCRIPTOR::Register>
{
    fn from(attribute_fields: AttributeFields) -> Self {
        // Memory attributes.
        let mut desc = match attribute_fields.mem_attributes {
            MemAttributes::CacheableDRAM => {
                STAGE1_PAGE_DESCRIPTOR::SH::InnerShareable
                    + STAGE1_PAGE_DESCRIPTOR::AttrIndx.val(mair::NORMAL)
            }
            MemAttributes::Device => {
                STAGE1_PAGE_DESCRIPTOR::SH::OuterShareable
                    + STAGE1_PAGE_DESCRIPTOR::AttrIndx.val(mair::DEVICE)
            }
        };

        // Access Permissions.
        desc += match attribute_fields.acc_perms {
            AccessPermissions::ReadOnly => STAGE1_PAGE_DESCRIPTOR::AP::RO_EL1,
            AccessPermissions::ReadWrite => STAGE1_PAGE_DESCRIPTOR::AP::RW_EL1,
        };

        // Execute Never.
        desc += if attribute_fields.execute_never {
            STAGE1_PAGE_DESCRIPTOR::PXN::True
        } else {
            STAGE1_PAGE_DESCRIPTOR::PXN::False
        };

        desc
    }
}

impl PageDescriptor {
    fn new(output_addr: usize, attribute_fields: AttributeFields) -> Self {
        let shifted = output_addr >> SIXTYFOUR_KIB_SHIFT;
        let val = (STAGE1_PAGE_DESCRIPTOR::VALID::True
            + STAGE1_PAGE_DESCRIPTOR::AF::True
            + attribute_fields.into()
            + STAGE1_PAGE_DESCRIPTOR::TYPE::Table
            + STAGE1_PAGE_DESCRIPTOR::OUTPUT_ADDR_64KiB.val(shifted as u64))
        .value;

        Self(val)
    }
}

/// Setup function for the MAIR_EL1 register.
fn set_up_mair() {
    // Define the memory types being mapped.
    MAIR_EL1.write(
        // Attribute 1 - Cacheable normal DRAM.
        MAIR_EL1::Attr1_Normal_Outer::WriteBack_NonTransient_ReadWriteAlloc +
        MAIR_EL1::Attr1_Normal_Inner::WriteBack_NonTransient_ReadWriteAlloc +

        // Attribute 0 - Device.
        MAIR_EL1::Attr0_Device::nonGathering_nonReordering_EarlyWriteAck,
    );
}

/// Iterates over all static translation table entries and fills them at once.
///
/// # Safety
///
/// - Modifies a `static mut`. Ensure it only happens from here.
unsafe fn populate_tt_entries() -> Result<(), &'static str> {
    for (l2_nr, l2_entry) in TABLES.lvl2.iter_mut().enumerate() {
        *l2_entry = TABLES.lvl3[l2_nr].base_addr_usize().into();

        for (l3_nr, l3_entry) in TABLES.lvl3[l2_nr].iter_mut().enumerate() {
            let virt_addr = (l2_nr << FIVETWELVE_MIB_SHIFT) + (l3_nr << SIXTYFOUR_KIB_SHIFT);

            let (output_addr, attribute_fields) =
                memory::virt_mem_layout().virt_addr_properties(virt_addr)?;

            *l3_entry = PageDescriptor::new(output_addr, attribute_fields);
        }
    }

    Ok(())
}

/// Configure various settings of stage 1 of the EL1 translation regime.
fn configure_translation_control() {
    let ips = ID_AA64MMFR0_EL1.read(ID_AA64MMFR0_EL1::PARange);
    TCR_EL1.write(
        TCR_EL1::TBI0::Ignored
            + TCR_EL1::IPS.val(ips)
            + TCR_EL1::TG0::KiB_64
            + TCR_EL1::SH0::Inner
            + TCR_EL1::ORGN0::WriteBack_ReadAlloc_WriteAlloc_Cacheable
            + TCR_EL1::IRGN0::WriteBack_ReadAlloc_WriteAlloc_Cacheable
            + TCR_EL1::EPD0::EnableTTBR0Walks
            + TCR_EL1::T0SZ.val(32), // TTBR0 spans 4 GiB total.
    );
}

//--------------------------------------------------------------------------------------------------
// Public Code
//--------------------------------------------------------------------------------------------------

/// Return a reference to the MMU.
pub fn mmu() -> &'static impl memory::mmu::interface::MMU {
    &MMU
}

//------------------------------------------------------------------------------
// OS Interface Code
//------------------------------------------------------------------------------

impl memory::mmu::interface::MMU for MemoryManagementUnit {
    unsafe fn init(&self) -> Result<(), &'static str> {
        // Fail early if translation granule is not supported. Both RPis support it, though.
        if !ID_AA64MMFR0_EL1.matches_all(ID_AA64MMFR0_EL1::TGran64::Supported) {
            return Err("64 KiB translation granule not supported");
        }

        // Populate translation tables.
        populate_tt_entries()?;

        Ok(())
    }
}

pub unsafe fn core_setup() {
    // Prepare the memory attribute indirection register.
    set_up_mair();
    // Point to the LVL2 table base address in TTBR0.
    TTBR0_EL1.set_baddr(TABLES.lvl2.base_addr_u64());
    TTBR1_EL1.set_baddr(TABLES.lvl2.base_addr_u64());

    configure_translation_control();

    // Switch the MMU on.
    //
    // First, force all previous changes to be seen before the MMU is enabled.
    barrier::isb(barrier::SY);

    // Enable the MMU and turn on data and instruction caching.
    SCTLR_EL1.modify(SCTLR_EL1::M::Enable + SCTLR_EL1::C::Cacheable + SCTLR_EL1::I::Cacheable);

    // Force MMU init to complete before next instruction
    barrier::isb(barrier::SY);
}
