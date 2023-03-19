Development boards for hobbyists

Remember that our end goal is to get some pretty pictures to show up on the LCD
screen.  Right now, we have a board with a microcontroller which can run some
code, hooked up to an LCD. This should be easy, right?

Unfortunately, there are lots of hoops to jump through first. Embedded
development requires a bottom-up approach. None of the things we do will make
much sense otherwise.

# part 1: what's a microcontroller?

## CPU, memory, GPIO

<complete>

If this diagram looks familiar to you, then feel free to skip this section.

<empty>

Let's describe piece-by-piece what's inside a microcontroller, and motivate why
each piece needs to exist.

<cpu>

Let's start with the CPU. This is going to be a low-power, single-core CPU.

<flash>

How does it know what instructions to execute? Let's add flash.

<ram>

Also we're going to need some memory. This will typically be SRAM.
Not a lot of it

<bus>

The CPU needs to be able to communicate with each other. Let's add a bus.

talk about address spaces, memory maps.
Let's assume the simple case of a single 32-bit linear space.

<gpio>

talking to the outside world, leaving the physical package

talk about how this is also on the bus (and hence gets its own address space)

also talk about how this is configured

This is a peripheral. We'll see some other peripherals (memory-mapped configs).

<clock>

the CPU needs this

again, this is configurable.

* internal oscillator (not very accurate)
* crystal
* PLL

## Bitbanging

Here's the voltages we need to send to the LCD panel.

Great! We're done, right?

Well, kind of. It's not great. How do we get the timing to work?

counting CPU cycles

* pretty accurate, but not perfect, but a lot of work to calculate
* hard to interleave other work

## interrupts

a primitive form of hardware concurrency

this is all internal to the CPU

but the triggers can be from peripherals
(TODO: do these external triggers go on the bus?)

<timer>

timer interrupt

## Dedicated peripherals

... And now we can see why this is called a development board. The usual way in
industry is:

1) evaluate the microcontroller's peripherals
2) buy just the microcontroller
3) solder it into your own board with your own peripherals

## part 1

* back of the envelope of capability (no double-buffering; x cycles per pixel). aiming for 30fps
* setting up the hardware; whirlwind intro to microcontrollers and peripherals -- which datasheets to read; setting up PLL; GPIO; LTDC. contribution to stm32-rs
* racing the beam; lcd panel timing
* floating vs fixed point precision
* cos/sin taylor approximation
* rotational symmetry
* scaling up
* misc maths optimisations
* artism: palette; HSV
Julia!
future optimisations: some connectedness property s.t. if a given rectangle perimeter is all in the set, the entire thing is in the set 

## part 2

* playing around in shadertoy, writing an emulator. discovering things via generalisations.
* optimising complex power to the -2
* being flexible about fixed-point: q vs qq
* fail: crappy op-count accounting and optimisation (leading to xpp optimisation which turned out not to work out)
* fast division: int division -> float multiplication with precomp (flexible fixed-point) -> int mul-and-shift with precomp
* fast vs slow sram; linker scripts
* reading assembly: avoiding automatic bounds checks, shift modulo, unchecked_unreachable
* diffing rust remarks: reducing register pressure; what is licm? a2qq+b2qq vs this_distqq - 2*b2qq
* manual loop unrolling
* embracing visual artifacts: borders from averaging 255 and 0; fuzziness from inaccuracy; the hole in the cells!

