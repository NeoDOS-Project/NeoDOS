ENTRY(nxl_entry)
SECTIONS
{
    . = 0x1e0c0000;
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
