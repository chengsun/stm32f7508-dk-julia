MEMORY
{
  /* NOTE 1 K = 1 KiBi = 1024 bytes */
  FLASH : ORIGIN = 0x00200000, LENGTH = 64K
  STACK : ORIGIN = 0x20000000, LENGTH = 1K
  PRIORITY : ORIGIN = 0x20000000 + LENGTH(STACK), LENGTH = 32*256*4
  FB : ORIGIN = 0x20000000 + LENGTH(STACK) + LENGTH(PRIORITY), LENGTH = 480*272
  RAM : ORIGIN = 0x20000000 + LENGTH(STACK) + LENGTH(PRIORITY) + LENGTH(FB), LENGTH = 320K - LENGTH(STACK) - LENGTH(PRIORITY) - LENGTH(FB)
}

/* This is where the call stack will be allocated. */
/* The stack is of the full descending type. */
/* You may want to use this variable to locate the call stack and static
   variables in different memory regions. Below is shown the default value */
_stack_start = ORIGIN(STACK) + LENGTH(STACK);

/* You can use this symbol to customize the location of the .text section */
/* If omitted the .text section will be placed right after the .vector_table
   section */
/* This is required only on microcontrollers that store some configuration right
   after the vector table */
/* _stext = ORIGIN(FLASH) + 0x400; */

/* Example of putting non-initialized variables into custom RAM locations. */
/* This assumes you have defined a region RAM2 above, and in the Rust
   sources added the attribute `#[link_section = ".ram2bss"]` to the data
   you want to place there. */
/* Note that the section will not be zero-initialized by the runtime! */
SECTIONS {
     .priority (NOLOAD) : ALIGN(4) {
       *(.priority);
       . = ALIGN(16);
     } > PRIORITY

     .fb (NOLOAD) : ALIGN(4) {
       *(.fb);
       . = ALIGN(16);
     } > FB
   } INSERT AFTER .bss;
