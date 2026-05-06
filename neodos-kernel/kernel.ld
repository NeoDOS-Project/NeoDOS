ENTRY(_start)
SECTIONS
{
    . = 0x200000;
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

    __kernel_end = .;
}
