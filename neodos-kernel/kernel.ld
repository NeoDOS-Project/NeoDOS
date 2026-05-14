ENTRY(_start)
SECTIONS
{
    . = 0x2000000;
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
