ENTRY(dll_entry)
SECTIONS
{
    /* Single LOAD segment: export table first, then code */
    . = 0x1e000000;
    __dll_start = .;

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

    __dll_end = .;
}
