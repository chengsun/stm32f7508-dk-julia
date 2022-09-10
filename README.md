# Julia set rendering on the STM32F7508-DK Discovery kit

# Targeting the hardware

### Requirements

*   [STM32F7508-DK Discovery kit](https://www.st.com/en/evaluation-tools/stm32f7508-dk.html)
*   Rust compiler (2021 edition) installed via `rustup`
*   OpenOCD >=0.11.0 (or a 0.10.0-dev build [more recent than
    2020-01-03](http://openocd.zylin.com/#/c/4926/))

### Instructions

1.  Follow the [steps in the Embedded Rust
    Book](https://rust-embedded.github.io/book/intro/install.html) to set up a
    working toolchain.

    The target architecture is `thumbv7em-none-eabihf`.

2.  Plug in the STM32F7508-DK in the mini-USB (ST-LINK) port.

3.  Start OpenOCD in the background, using the cfg file in this repo:

    ```bash
    cd real
    openocd -f ./openocd.cfg
    ```

3.  Build and flash the binary:

    ```bash
    cd real
    cargo run --release
    ```

## Emulating locally

### Instructions

1.  Build and run the binary:

    ```bash
    cd emulated
    cargo run --release
    ```
