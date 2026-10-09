ENTRY(nxl_entry)
SECTIONS
{
    /* PIE: link at virtual 0; the loader relocates it to a free slot. */
    . = 0;
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
