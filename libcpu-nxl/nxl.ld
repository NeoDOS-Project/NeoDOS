ENTRY(nxl_entry)
SECTIONS
{
    /* Slot 2: 0x1E080000 (slot 0 = libneodos, slot 1 = libmath) */
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
