ENTRY(_start)
SECTIONS
{
    . = 0x400000;
    __user_start = .;

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

    __user_end = .;
}
