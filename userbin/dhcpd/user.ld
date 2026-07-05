ENTRY(_start)
SECTIONS
{
    . = 0;
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
