ENTRY(kernel_entry)

PHDRS {
    text PT_LOAD FILEHDR PHDRS FLAGS(5);
    data PT_LOAD FLAGS(6);
}

SECTIONS {
    . = NT61_IMAGE_BASE;
    PROVIDE(__image_start = .);
    . += 0x1000;

    .text : ALIGN(0x1000) {
        *(.text.startup .text.startup.*)
        *(.text .text.* .rodata .rodata.* .srodata .srodata.*)
        *(.eh_frame .eh_frame.*)
    } :text

    .data : ALIGN(0x1000) {
        *(.data .data.* .sdata .sdata.*)
        *(.got .got.*)
    } :data

    .bss (NOLOAD) : ALIGN(0x1000) {
        *(.bss .bss.* .sbss .sbss.* COMMON)
    } :data

    PROVIDE(__image_end = .);

    /DISCARD/ : {
        *(.note.GNU-stack)
        *(.note.gnu*)
        *(.dynsym .dynstr .dynamic)
    }
}
