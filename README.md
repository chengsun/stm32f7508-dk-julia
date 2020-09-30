# Julia set rendering on the STM32F7508-DK Discovery kit

## Requirements

* [https://www.st.com/en/evaluation-tools/stm32f7508-dk.html](STM32F7508-DK Discovery kit)
* Rust compiler (2018 edition) installed via `rustup`
* OpenOCD 0.10.0-dev compiled from git ([more recent than 2020-01-03](http://openocd.zylin.com/#/c/4926/))

## Build

1.  Follow the [https://rust-embedded.github.io/book/intro/install.html](steps
    in the Embedded Rust Book) to set up a working toolchain.

    The target architecture is `thumbv7em-none-eabihf`.

2.  Plug in the STM32F7508-DK in the mini-USB (ST-LINK) port.

3.  Start OpenOCD in the background, using the cfg file in this repo:

    ```bash
    openocd -f ./openocd.cfg
    ```

3.  Build and flash the binary:

    ```bash
    cargo run --release
    ```
