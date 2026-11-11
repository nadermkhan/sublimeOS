ENTRY(_start)

SECTIONS
{
    . = 1M;
    __boot_start = .;

    .boot.text : ALIGN(4K)
    {
        KEEP(*(.boot.header))
        *(.boot.text)
    }

    .boot.data : ALIGN(4K)
    {
        *(.boot.data)
        *(.boot.bss)
    }

    __boot_end = .;

    /* Switch to higher half virtual address space */
    . = ALIGN(2M) + 0xFFFFFFFF80000000;

    .text : AT(ADDR(.text) - 0xFFFFFFFF80000000)
    {
        __text_start = .;
        *(.text .text.*)
        __text_end = .;
    }

    .rodata : AT(ADDR(.rodata) - 0xFFFFFFFF80000000)
    {
        __rodata_start = .;
        *(.rodata .rodata.*)
        __rodata_end = .;
    }

    .data : AT(ADDR(.data) - 0xFFFFFFFF80000000)
    {
        __data_start = .;
        *(.data .data.*)
        *(.got .got.*)
        __data_end = .;
    }

    .bss : AT(ADDR(.bss) - 0xFFFFFFFF80000000)
    {
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        __bss_end = .;
    }

    . = ALIGN(4K);
    __kernel_end = .;
    __kernel_phys_end = . - 0xFFFFFFFF80000000;

    /DISCARD/ :
    {
        *(.comment)
        *(.eh_frame)
        *(.note.gnu.build-id)
    }
}
