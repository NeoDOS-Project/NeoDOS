ENTRY(nxl_entry)
SECTIONS
{
    /* Single LOAD segment: export table first, then code */
    . = 0x1e080000;
    __nxl_start = .;

    .export_table : {
        KEEP(*(.export_table))
    }

    .text : {
        . = ALIGN(16);
        *(.text.entry)
        *(.text*)
        *(.rodata*)
    }

    .data : {
        *(.data*)
    }

    .bss : {
        *(.bss*)
        *(COMMON)
    }

    __nxl_end = .;
}
