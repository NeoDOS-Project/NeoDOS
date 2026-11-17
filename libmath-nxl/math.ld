ENTRY(nxl_entry)
SECTIONS
{
    /* PIE: link at virtual 0. The loader relocates the library to any free
     * slot (R_X86_64_RELATIVE); the export table stays at offset 0. */
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
