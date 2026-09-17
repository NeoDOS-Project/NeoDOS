ENTRY(_start)
SECTIONS
{
    . = 0x4000000;
    __kernel_start = .;
    
    .text : {
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

    .dbg : {
        *(.dbg*)
        *(.bss.dbg*)
        *(.data.dbg*)
    }

    __kernel_end = .;
}
