SECTIONS
{
    /* Set current address to the value from which the RPi starts execution */
    . = 0x80000;

    __ro_start = .;
    .text :
    {
        *(.text._start) *(.text*)
    }

    .rodata :
    {
        *(.rodata*)
    }
    . = ALIGN(4096); /* Fill up to 64 KiB */
    __ro_end = .;

    .data :
    {
        *(.data*)
    }
    . = ALIGN(4096);

    /* Section is zeroed in u64 chunks, align start and end to 8 bytes */
    .bss :
    {
        __bss_start = .;
        *(.bss*);
        __bss_end = .;
    }
    . = ALIGN(4096); 

    /DISCARD/ : { *(.comment*) }
}