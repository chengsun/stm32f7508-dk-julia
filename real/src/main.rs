#![no_std]
#![no_main]

use demos::Context;
use panic_halt as _;

use core::cell::RefCell;
use core::convert::TryInto;

use cortex_m::interrupt::Mutex;
use cortex_m_rt::entry;
use stm32f7::stm32f750::{interrupt, Interrupt, LTDC, NVIC};

static GLTDC: Mutex<RefCell<Option<LTDC>>> = Mutex::new(RefCell::new(None));
static GSTATE: Mutex<RefCell<Option<demos::Julia>>> = Mutex::new(RefCell::new(None));

struct LTDCInfo {
    hsync: u16,
    hbp: u16,
    aw: u16,
    hfp: u16,
    vsync: u16,
    vbp: u16,
    ah: u16,
    vfp: u16,
}

const LTDC_INFO: LTDCInfo = LTDCInfo {
    hsync: 1,
    hbp: 39,
    aw: 480,
    hfp: 5,
    vsync: 1,
    vbp: 7,
    ah: 272,
    vfp: 8,
};

const FB_W: usize = LTDC_INFO.aw as usize;
const FB_H: usize = LTDC_INFO.ah as usize;

const _: () = assert!(FB_W == demos::FB_W);
const _: () = assert!(FB_H == demos::FB_H);

#[derive(Copy, Clone)]
enum LTDCState {
    Uninitialised,
    Initialised,
}

static LTDC_STATE: Mutex<RefCell<LTDCState>> = Mutex::new(RefCell::new(LTDCState::Uninitialised));

#[entry]
fn main() -> ! {
    let _cp = cortex_m::Peripherals::take().unwrap();
    let dp = stm32f7::stm32f750::Peripherals::take().unwrap();

    cortex_m::interrupt::free(move |cs| {

        //////////////////////////////////////////////////////////////////////////
        // increase the Flash latency wait states to be safe

        let flash = dp.FLASH;
        {
            let latency = 6;
            flash.acr.write(|w| {
                w.latency().bits(latency).arten().set_bit().prften().set_bit()
            });

            while flash.acr.read().latency().bits() != latency { }
        }

        //////////////////////////////////////////////////////////////////////////
        // configure RCC
        // - enable the PLL and set it to the system clock source
        // - enable the PLLSAI and set it to the LTDC clock source

        let rcc = dp.RCC;

        // PLL input = HSI = 16MHz
        // PLL output = 16MHz * PLLN / PLLM / PLLP = 16MHz * 64 / 8 / 8
        // 2 <= PLLM <= 63
        // 1 <= HSI / PLLM <= 2
        // 50 <= PLLN <= 432
        // 100 <= HSI * PLLN / PLLM / PLLP <= 216
        let pllm = 8;
        let plln = 200;
        let pllp = 2;
        rcc.pllcfgr.write(|w| unsafe {
            let w = w.pllsrc().bit(false).pllm().bits(pllm).plln().bits(plln);
            match pllp {
                2 => w.pllp().div2(),
                4 => w.pllp().div4(),
                6 => w.pllp().div6(),
                8 => w.pllp().div8(),
                _ => unreachable!()
            }
        });
        // PLLSAI input = HSI = 16MHz
        // PLLSAI output = 16MHz * PLLSAIN / PLLM / PLLSAIR / PLLSAIDIVR = 16MHz * 54 / 8 / 3 / 4
        // 50 <= PLLSAIN <= 432
        // 2 <= PLLSAIR <= 7
        let pllsain = 66;
        let pllsair = 5;
        let pllsaidivr = 4;
        rcc.pllsaicfgr.write(|w| unsafe {
            w.pllsain().bits(pllsain).pllsair().bits(pllsair)
        });
        rcc.dckcfgr1.write(|w| {
            match pllsaidivr {
                2 => w.pllsaidivr().div2(),
                4 => w.pllsaidivr().div4(),
                8 => w.pllsaidivr().div8(),
                16 => w.pllsaidivr().div16(),
                _ => unreachable!()
            }
        });

        rcc.cr.write(|w| {
            w.pllon().bit(true).pllsaion().bit(true)
        });

        // Wait for PLL and PLLSAI locks
        loop {
            let cr = rcc.cr.read();
            if cr.pllrdy().bit() && cr.pllsairdy().bit() { break; }
        }

        // Switch system clock
        rcc.cfgr.write(|w| { w.sw().pll().ppre1().div4().ppre2().div2() });
        while !rcc.cfgr.read().sws().is_pll() { }

        let gpioc = dp.GPIOC;
        let gpiod = dp.GPIOD;
        let gpioe = dp.GPIOE;
        let gpiof = dp.GPIOF;
        let gpiog = dp.GPIOG;
        let gpioh = dp.GPIOH;
        let gpioi = dp.GPIOI;
        let gpioj = dp.GPIOJ;
        let gpiok = dp.GPIOK;

        // enable clock for these GPIO ports
        rcc.ahb1enr.modify(|_, w| { w
                                    .gpiocen().bit(true)
                                    .gpioden().bit(true)
                                    .gpioeen().bit(true)
                                    .gpiofen().bit(true)
                                    .gpiogen().bit(true)
                                    .gpiohen().bit(true)
                                    .gpioien().bit(true)
                                    .gpiojen().bit(true)
                                    .gpioken().bit(true) });

        //////////////////////////////////////////////////////////////////////////
        // configure the SDRAM
        //
        // function     pin  af#
        // --- controlled by FMC ---
        // FMC_SDCLK    PG8  12
        // FMC_SDCKE0   PC3  12
        // FMC_SDNCAS   PG15 12
        // FMC_SDNRAS   PF11 12
        // FMC_SDNWE    PH5  12
        // FMC_SDNE0    PH3  12
        // FMC_NBL0     PE0  12
        // FMC_NBL1     PE1  12
        // FMC_A0       PF0  12
        // FMC_A1       PF1  12
        // FMC_A2       PF2  12
        // FMC_A3       PF3  12
        // FMC_A4       PF4  12
        // FMC_A5       PF5  12
        // FMC_A6       PF12 12
        // FMC_A7       PF13 12
        // FMC_A8       PF14 12
        // FMC_A9       PF15 12
        // FMC_A10      PG0  12
        // FMC_A11      PG1  12
        // FMC_A14_BA0  PG4  12
        // FMC_A15_BA1  PG5  12
        // FMC_D0_DA0   PD14 12
        // FMC_D1_DA1   PD15 12
        // FMC_D2_DA2   PD0  12
        // FMC_D3_DA3   PD1  12
        // FMC_D4_DA4   PE7  12
        // FMC_D5_DA5   PE8  12
        // FMC_D6_DA6   PE9  12
        // FMC_D7_DA7   PE10 12
        // FMC_D8_DA8   PE11 12
        // FMC_D9_DA9   PE12 12
        // FMC_D10_DA10 PE13 12
        // FMC_D11_DA11 PE14 12
        // FMC_D12_DA12 PE15 12
        // FMC_D13_DA13 PD8  12
        // FMC_D14_DA14 PD9  12
        // FMC_D15_DA15 PD10 12

        // Step 1: First, enable the FMC controlled pins.
        //
        // for FMC pins, OSPEEDR = very_high, because high_speed caps out at
        // ~50MHz

        // PC3
        gpioc.afrl   .modify(|_, w| { w.   afrl3().af12() });
        gpioc.otyper .modify(|_, w| { w.     ot3().push_pull() });
        gpioc.pupdr  .modify(|_, w| { w.  pupdr3().floating() });
        gpioc.ospeedr.modify(|_, w| { w.ospeedr3().very_high_speed() });
        gpioc.moder  .modify(|_, w| { w.  moder3().alternate() });

        // PD{0,1,8..10,14,15}
        gpiod.afrl   .modify(|_, w| { w
                                      .   afrl0 ().af12()
                                      .   afrl1 ().af12() });
        gpiod.afrh   .modify(|_, w| { w
                                      .   afrh8 ().af12()
                                      .   afrh9 ().af12()
                                      .   afrh10().af12()
                                      .   afrh14().af12()
                                      .   afrh15().af12() });
        gpiod.otyper .modify(|_, w| { w
                                      .     ot0 ().push_pull()
                                      .     ot1 ().push_pull()
                                      .     ot8 ().push_pull()
                                      .     ot9 ().push_pull()
                                      .     ot10().push_pull()
                                      .     ot14().push_pull()
                                      .     ot15().push_pull() });
        gpiod.pupdr  .modify(|_, w| { w
                                      .  pupdr0 ().floating()
                                      .  pupdr1 ().floating()
                                      .  pupdr8 ().floating()
                                      .  pupdr9 ().floating()
                                      .  pupdr10().floating()
                                      .  pupdr14().floating()
                                      .  pupdr15().floating() });
        gpiod.ospeedr.modify(|_, w| { w
                                      .ospeedr0 ().very_high_speed()
                                      .ospeedr1 ().very_high_speed()
                                      .ospeedr8 ().very_high_speed()
                                      .ospeedr9 ().very_high_speed()
                                      .ospeedr10().very_high_speed()
                                      .ospeedr14().very_high_speed()
                                      .ospeedr15().very_high_speed() });
        gpiod.moder  .modify(|_, w| { w
                                      .  moder0 ().alternate()
                                      .  moder1 ().alternate()
                                      .  moder8 ().alternate()
                                      .  moder9 ().alternate()
                                      .  moder10().alternate()
                                      .  moder14().alternate()
                                      .  moder15().alternate() });

        // PE{0,1,7..15}
        gpioe.afrl   .modify(|_, w| { w
                                      .   afrl0 ().af12()
                                      .   afrl1 ().af12()
                                      .   afrl7 ().af12() });
        gpioe.afrh   .modify(|_, w| { w
                                      .   afrh8 ().af12()
                                      .   afrh9 ().af12()
                                      .   afrh10().af12()
                                      .   afrh11().af12()
                                      .   afrh12().af12()
                                      .   afrh13().af12()
                                      .   afrh14().af12()
                                      .   afrh15().af12() });
        gpioe.otyper .modify(|_, w| { w
                                      .     ot0 ().push_pull()
                                      .     ot1 ().push_pull()
                                      .     ot7 ().push_pull()
                                      .     ot8 ().push_pull()
                                      .     ot9 ().push_pull()
                                      .     ot10().push_pull()
                                      .     ot11().push_pull()
                                      .     ot12().push_pull()
                                      .     ot13().push_pull()
                                      .     ot14().push_pull()
                                      .     ot15().push_pull() });
        gpioe.pupdr  .modify(|_, w| { w
                                      .  pupdr0 ().floating()
                                      .  pupdr1 ().floating()
                                      .  pupdr7 ().floating()
                                      .  pupdr8 ().floating()
                                      .  pupdr9 ().floating()
                                      .  pupdr10().floating()
                                      .  pupdr11().floating()
                                      .  pupdr12().floating()
                                      .  pupdr13().floating()
                                      .  pupdr14().floating()
                                      .  pupdr15().floating() });
        gpioe.ospeedr.modify(|_, w| { w
                                      .ospeedr0 ().very_high_speed()
                                      .ospeedr1 ().very_high_speed()
                                      .ospeedr7 ().very_high_speed()
                                      .ospeedr8 ().very_high_speed()
                                      .ospeedr9 ().very_high_speed()
                                      .ospeedr10().very_high_speed()
                                      .ospeedr11().very_high_speed()
                                      .ospeedr12().very_high_speed()
                                      .ospeedr13().very_high_speed()
                                      .ospeedr14().very_high_speed()
                                      .ospeedr15().very_high_speed() });
        gpioe.moder  .modify(|_, w| { w
                                      .  moder0 ().alternate()
                                      .  moder1 ().alternate()
                                      .  moder7 ().alternate()
                                      .  moder8 ().alternate()
                                      .  moder9 ().alternate()
                                      .  moder10().alternate()
                                      .  moder11().alternate()
                                      .  moder12().alternate()
                                      .  moder13().alternate()
                                      .  moder14().alternate()
                                      .  moder15().alternate() });

        // PF{0..5,11..15}
        gpiof.afrl   .modify(|_, w| { w
                                      .   afrl0 ().af12()
                                      .   afrl1 ().af12()
                                      .   afrl2 ().af12()
                                      .   afrl3 ().af12()
                                      .   afrl4 ().af12()
                                      .   afrl5 ().af12() });
        gpiof.afrh   .modify(|_, w| { w
                                      .   afrh11().af12()
                                      .   afrh12().af12()
                                      .   afrh13().af12()
                                      .   afrh14().af12()
                                      .   afrh15().af12() });
        gpiof.otyper .modify(|_, w| { w
                                      .     ot0 ().push_pull()
                                      .     ot1 ().push_pull()
                                      .     ot2 ().push_pull()
                                      .     ot3 ().push_pull()
                                      .     ot4 ().push_pull()
                                      .     ot5 ().push_pull()
                                      .     ot11().push_pull()
                                      .     ot12().push_pull()
                                      .     ot13().push_pull()
                                      .     ot14().push_pull()
                                      .     ot15().push_pull() });
        gpiof.pupdr  .modify(|_, w| { w
                                      .  pupdr0 ().floating()
                                      .  pupdr1 ().floating()
                                      .  pupdr2 ().floating()
                                      .  pupdr3 ().floating()
                                      .  pupdr4 ().floating()
                                      .  pupdr5 ().floating()
                                      .  pupdr11().floating()
                                      .  pupdr12().floating()
                                      .  pupdr13().floating()
                                      .  pupdr14().floating()
                                      .  pupdr15().floating() });
        gpiof.ospeedr.modify(|_, w| { w
                                      .ospeedr0 ().very_high_speed()
                                      .ospeedr1 ().very_high_speed()
                                      .ospeedr2 ().very_high_speed()
                                      .ospeedr3 ().very_high_speed()
                                      .ospeedr4 ().very_high_speed()
                                      .ospeedr5 ().very_high_speed()
                                      .ospeedr11().very_high_speed()
                                      .ospeedr12().very_high_speed()
                                      .ospeedr13().very_high_speed()
                                      .ospeedr14().very_high_speed()
                                      .ospeedr15().very_high_speed() });
        gpiof.moder  .modify(|_, w| { w
                                      .  moder0 ().alternate()
                                      .  moder1 ().alternate()
                                      .  moder2 ().alternate()
                                      .  moder3 ().alternate()
                                      .  moder4 ().alternate()
                                      .  moder5 ().alternate()
                                      .  moder11().alternate()
                                      .  moder12().alternate()
                                      .  moder13().alternate()
                                      .  moder14().alternate()
                                      .  moder15().alternate() });

        // PG{0,1,4,5,8,15}
        gpiog.afrl   .modify(|_, w| { w
                                      .   afrl0 ().af12()
                                      .   afrl1 ().af12()
                                      .   afrl4 ().af12()
                                      .   afrl5 ().af12() });
        gpiog.afrh   .modify(|_, w| { w
                                      .   afrh8 ().af12()
                                      .   afrh15().af12() });
        gpiog.otyper .modify(|_, w| { w
                                      .     ot0 ().push_pull()
                                      .     ot1 ().push_pull()
                                      .     ot4 ().push_pull()
                                      .     ot5 ().push_pull()
                                      .     ot8 ().push_pull()
                                      .     ot15().push_pull() });
        gpiog.pupdr  .modify(|_, w| { w
                                      .  pupdr0 ().floating()
                                      .  pupdr1 ().floating()
                                      .  pupdr4 ().floating()
                                      .  pupdr5 ().floating()
                                      .  pupdr8 ().floating()
                                      .  pupdr15().floating() });
        gpiog.ospeedr.modify(|_, w| { w
                                      .ospeedr0 ().very_high_speed()
                                      .ospeedr1 ().very_high_speed()
                                      .ospeedr4 ().very_high_speed()
                                      .ospeedr5 ().very_high_speed()
                                      .ospeedr8 ().very_high_speed()
                                      .ospeedr15().very_high_speed() });
        gpiog.moder  .modify(|_, w| { w
                                      .  moder0 ().alternate()
                                      .  moder1 ().alternate()
                                      .  moder4 ().alternate()
                                      .  moder5 ().alternate()
                                      .  moder8 ().alternate()
                                      .  moder15().alternate() });

        // PH{3,5}
        gpioh.afrl   .modify(|_, w| { w
                                      .   afrl3 ().af12()
                                      .   afrl5 ().af12() });
        gpioh.otyper .modify(|_, w| { w
                                      .     ot3 ().push_pull()
                                      .     ot5 ().push_pull() });
        gpioh.pupdr  .modify(|_, w| { w
                                      .  pupdr3 ().floating()
                                      .  pupdr5 ().floating() });
        gpioh.ospeedr.modify(|_, w| { w
                                      .ospeedr3 ().very_high_speed()
                                      .ospeedr5 ().very_high_speed() });
        gpioh.moder  .modify(|_, w| { w
                                      .  moder3 ().alternate()
                                      .  moder5 ().alternate() });

        // Step 2. Enable and setup the FMC peripheral.

        let fmc = dp.FMC;

        rcc.ahb3enr.modify(|_, w| { w.fmcen().enabled() });

        fmc.sdcr1.write(|w| { w
                              .rpipe().no_delay()
                              .rburst().disabled()
                              .sdclk().div2()
                              .wp().disabled()
                              .cas().clocks2()
                              .nb().nb4()
                              .mwid().bits16()
                              .nr().bits12()
                              .nc().bits8() });
        fmc.sdtr1.write(|w| { w
                              .trcd().bits(1)
                              .trp().bits(1)
                              .twr().bits(1)
                              .trc().bits(6)
                              .tras().bits(3) // TODO: this is actually min 42ns in the SDRAM spec, but 40ns here. If this needs slowing to 50ns then TWR also needs slowing to 30ns
                              .txsr().bits(6)
                              .tmrd().bits(1) });
        // start delivering clock -- SDCKE driven high
        fmc.sdcmr.write(|w| { w
                              .mode().clock_configuration_enable()
                              .ctb1().issued() });
        while fmc.sdsr.read().busy().is_busy() {}
        // wait 100us
        cortex_m::asm::delay(20000);
        // issue "precharge all"
        fmc.sdcmr.write(|w| { w
                              .mode().pall()
                              .ctb1().issued() });
        while fmc.sdsr.read().busy().is_busy() {}
        // issue "auto-refresh"
        fmc.sdcmr.write(|w| { w
                              .mode().auto_refresh_command()
                              .ctb1().issued()
                              .nrfs().bits(1) });
        while fmc.sdsr.read().busy().is_busy() {}
        // issue "load mode register"
        fmc.sdcmr.write(|w| { w
                              .mode().load_mode_register()
                              .ctb1().issued()
                              .mrd().bits(0b000_1_00_010_0_000) });
        while fmc.sdsr.read().busy().is_busy() {}
        // program refresh rate
        fmc.sdrtr.write(|w| { w.count().bits(1542) });


        //////////////////////////////////////////////////////////////////////////
        // Initialize the demo

        *GSTATE.borrow(cs).borrow_mut() = Some(demos::Julia::new());

        //////////////////////////////////////////////////////////////////////////
        // configure the LCD GPIO pins
        //
        // function   pin  af#
        // --- controlled by LTDC ---
        // LCD_HSYNC  PI10  14
        // LCD_VSYNC  PI9   14
        // LCD_DE     PK7   14
        // LCD_CLK    PI14  14
        // LCD_R7     PJ6   14
        // LCD_R6     PJ5   14
        // LCD_R5     PJ4   14
        // LCD_R4     PJ3   14
        // LCD_R3     PJ2   14
        // LCD_R2     PJ1   14
        // LCD_R1     PJ0   14
        // LCD_R0     PI15  14
        // LCD_G7     PK2   14
        // LCD_G6     PK1   14
        // LCD_G5     PK0   14
        // LCD_G4     PJ11  14
        // LCD_G3     PJ10  14
        // LCD_G2     PJ9   14
        // LCD_G1     PJ8   14
        // LCD_G0     PJ7   14
        // LCD_B7     PK6   14
        // LCD_B6     PK5   14
        // LCD_B5     PK4   14
        // LCD_B4     PG12  9
        // LCD_B3     PJ15  14
        // LCD_B2     PJ14  14
        // LCD_B1     PJ13  14
        // LCD_B0     PE4   14
        // --- controlled by user ---
        // LCD_DISP   PI12
        // LCD_INT    PI13        (not hooked up yet)
        // LCD_SCL    PH7        (not hooked up yet)
        // LCD_SDA    PH8        (not hooked up yet)
        // LCD_BLCTRL PK3

        // Step 1: First, enable the LTDC controlled pins.
        //
        // for LCD data pins, OSPEEDR = medium, because:
        //   480 * 272 * 60Hz ~= 8MHz
        //   low_speed caps out at ~4MHz
        //   medium_speed caps out at ~25MHz

        // E4
        gpioe.afrl   .modify(|_, w| { w.   afrl4().af14() });
        gpioe.otyper .modify(|_, w| { w.     ot4().push_pull() });
        gpioe.pupdr  .modify(|_, w| { w.  pupdr4().floating() });
        gpioe.ospeedr.modify(|_, w| { w.ospeedr4().medium_speed() });
        gpioe.moder  .modify(|_, w| { w.  moder4().alternate() });

        // G12
        gpiog.afrh   .modify(|_, w| { w.   afrh12().af9() });
        gpiog.otyper .modify(|_, w| { w.     ot12().push_pull() });
        gpiog.pupdr  .modify(|_, w| { w.  pupdr12().floating() });
        gpiog.ospeedr.modify(|_, w| { w.ospeedr12().medium_speed() });
        gpiog.moder  .modify(|_, w| { w.  moder12().alternate() });

        // I{9,10,14,15}
        gpioi.afrh   .modify(|_, w| { w
                                      .   afrh9 ().af14()
                                      .   afrh10().af14()
                                      .   afrh14().af14()
                                      .   afrh15().af14() });
        gpioi.otyper .modify(|_, w| { w
                                      .     ot9 ().push_pull()
                                      .     ot10().push_pull()
                                      .     ot14().push_pull()
                                      .     ot15().push_pull() });
        gpioi.pupdr  .modify(|_, w| { w
                                      .  pupdr9 ().floating()
                                      .  pupdr10().floating()
                                      .  pupdr14().floating()
                                      .  pupdr15().floating() });
        gpioi.ospeedr.modify(|_, w| { w
                                      .ospeedr9 ().medium_speed()
                                      .ospeedr10().medium_speed()
                                      .ospeedr14().medium_speed()
                                      .ospeedr15().medium_speed() });
        gpioi.moder  .modify(|_, w| { w
                                      .  moder9 ().alternate()
                                      .  moder10().alternate()
                                      .  moder14().alternate()
                                      .  moder15().alternate() });

        // J{0..11,13..15}
        gpioj.afrl   .modify(|_, w| { w
                                      .   afrl0 ().af14()
                                      .   afrl1 ().af14()
                                      .   afrl2 ().af14()
                                      .   afrl3 ().af14()
                                      .   afrl4 ().af14()
                                      .   afrl5 ().af14()
                                      .   afrl6 ().af14()
                                      .   afrl7 ().af14() });
        gpioj.afrh   .modify(|_, w| { w
                                      .   afrh8 ().af14()
                                      .   afrh9 ().af14()
                                      .   afrh10().af14()
                                      .   afrh11().af14()
                                      .   afrh13().af14()
                                      .   afrh14().af14()
                                      .   afrh15().af14() });
        gpioj.otyper .modify(|_, w| { w
                                      .     ot0 ().push_pull()
                                      .     ot1 ().push_pull()
                                      .     ot2 ().push_pull()
                                      .     ot3 ().push_pull()
                                      .     ot4 ().push_pull()
                                      .     ot5 ().push_pull()
                                      .     ot6 ().push_pull()
                                      .     ot7 ().push_pull()
                                      .     ot8 ().push_pull()
                                      .     ot9 ().push_pull()
                                      .     ot10().push_pull()
                                      .     ot11().push_pull()
                                      .     ot13().push_pull()
                                      .     ot14().push_pull()
                                      .     ot15().push_pull() });
        gpioj.pupdr  .modify(|_, w| { w
                                      .  pupdr0 ().floating()
                                      .  pupdr1 ().floating()
                                      .  pupdr2 ().floating()
                                      .  pupdr3 ().floating()
                                      .  pupdr4 ().floating()
                                      .  pupdr5 ().floating()
                                      .  pupdr6 ().floating()
                                      .  pupdr7 ().floating()
                                      .  pupdr8 ().floating()
                                      .  pupdr9 ().floating()
                                      .  pupdr10().floating()
                                      .  pupdr11().floating()
                                      .  pupdr13().floating()
                                      .  pupdr14().floating()
                                      .  pupdr15().floating() });
        gpioj.ospeedr.modify(|_, w| { w
                                      .ospeedr0 ().medium_speed()
                                      .ospeedr1 ().medium_speed()
                                      .ospeedr2 ().medium_speed()
                                      .ospeedr3 ().medium_speed()
                                      .ospeedr4 ().medium_speed()
                                      .ospeedr5 ().medium_speed()
                                      .ospeedr6 ().medium_speed()
                                      .ospeedr7 ().medium_speed()
                                      .ospeedr8 ().medium_speed()
                                      .ospeedr9 ().medium_speed()
                                      .ospeedr10().medium_speed()
                                      .ospeedr11().medium_speed()
                                      .ospeedr13().medium_speed()
                                      .ospeedr14().medium_speed()
                                      .ospeedr15().medium_speed() });
        gpioj.moder  .modify(|_, w| { w
                                      .  moder0 ().alternate()
                                      .  moder1 ().alternate()
                                      .  moder2 ().alternate()
                                      .  moder3 ().alternate()
                                      .  moder4 ().alternate()
                                      .  moder5 ().alternate()
                                      .  moder6 ().alternate()
                                      .  moder7 ().alternate()
                                      .  moder8 ().alternate()
                                      .  moder9 ().alternate()
                                      .  moder10().alternate()
                                      .  moder11().alternate()
                                      .  moder13().alternate()
                                      .  moder14().alternate()
                                      .  moder15().alternate() });

        // K{0..2,4..7}
        gpiok.afrl   .modify(|_, w| { w
                                      .   afrl0 ().af14()
                                      .   afrl1 ().af14()
                                      .   afrl2 ().af14()
                                      .   afrl4 ().af14()
                                      .   afrl5 ().af14()
                                      .   afrl6 ().af14()
                                      .   afrl7 ().af14() });
        gpiok.otyper .modify(|_, w| { w
                                      .     ot0 ().push_pull()
                                      .     ot1 ().push_pull()
                                      .     ot2 ().push_pull()
                                      .     ot4 ().push_pull()
                                      .     ot5 ().push_pull()
                                      .     ot6 ().push_pull()
                                      .     ot7 ().push_pull() });
        gpiok.pupdr  .modify(|_, w| { w
                                      .  pupdr0 ().floating()
                                      .  pupdr1 ().floating()
                                      .  pupdr2 ().floating()
                                      .  pupdr4 ().floating()
                                      .  pupdr5 ().floating()
                                      .  pupdr6 ().floating()
                                      .  pupdr7 ().floating() });
        gpiok.ospeedr.modify(|_, w| { w
                                      .ospeedr0 ().medium_speed()
                                      .ospeedr1 ().medium_speed()
                                      .ospeedr2 ().medium_speed()
                                      .ospeedr4 ().medium_speed()
                                      .ospeedr5 ().medium_speed()
                                      .ospeedr6 ().medium_speed()
                                      .ospeedr7 ().medium_speed() });
        gpiok.moder  .modify(|_, w| { w
                                      .  moder0 ().alternate()
                                      .  moder1 ().alternate()
                                      .  moder2 ().alternate()
                                      .  moder4 ().alternate()
                                      .  moder5 ().alternate()
                                      .  moder6 ().alternate()
                                      .  moder7 ().alternate() });

        // Now, enable the user-controlled pins.

        // I12
        gpioi.otyper .modify(|_, w| { w.     ot12().push_pull() });
        gpioi.pupdr  .modify(|_, w| { w.  pupdr12().floating() });
        gpioi.ospeedr.modify(|_, w| { w.ospeedr12().low_speed() });
        gpioi.moder  .modify(|_, w| { w.  moder12().output() });

        // K3
        gpiok.otyper .modify(|_, w| { w.     ot3().push_pull() });
        gpiok.pupdr  .modify(|_, w| { w.  pupdr3().floating() });
        gpiok.ospeedr.modify(|_, w| { w.ospeedr3().low_speed() });
        gpiok.moder  .modify(|_, w| { w.  moder3().output() });

        // Step 2. Enable and setup the LTDC peripheral.

        let ltdc = dp.LTDC;

        rcc.apb2enr.modify(|_, w| { w.ltdcen().bit(true) });

        ltdc.sscr.write(|w| { w.hsw().bits(LTDC_INFO.hsync - 1).vsh().bits(LTDC_INFO.vsync - 1) });
        ltdc.bpcr.write(|w| { w.ahbp().bits(LTDC_INFO.hsync + LTDC_INFO.hbp - 1).avbp().bits(LTDC_INFO.vsync + LTDC_INFO.vbp - 1) });
        ltdc.awcr.write(|w| { w.aaw().bits(LTDC_INFO.hsync + LTDC_INFO.hbp + LTDC_INFO.aw - 1).aah().bits(LTDC_INFO.vsync + LTDC_INFO.vbp + LTDC_INFO.ah - 1) });
        ltdc.twcr.write(|w| { w.totalw().bits(LTDC_INFO.hsync + LTDC_INFO.hbp + LTDC_INFO.aw + LTDC_INFO.hfp - 1).totalh().bits(LTDC_INFO.vsync + LTDC_INFO.vbp + LTDC_INFO.ah + LTDC_INFO.vfp - 1) });

        ltdc.gcr.write(|w| { w.hspol().active_low().vspol().active_low().depol().active_low().pcpol().rising_edge() });

        // enable line interrupt
        ltdc.lipcr.write(|w| { w.lipos().bits(LTDC_INFO.vsync + LTDC_INFO.vbp) });
        ltdc.ier.write(|w| { w.lie().enabled() });

        // enable the LTDC peripheral
        ltdc.gcr.modify(|_, w| { w.ltdcen().enabled() });

        // enable the screen and turn on the backlight
        gpioi.bsrr.write(|w| { w.bs12().bit(true) });
        gpiok.bsrr.write(|w| { w.bs3().bit(true) });

        *GLTDC.borrow(cs).borrow_mut() = Some(ltdc);
        unsafe { NVIC::unmask(Interrupt::LTDC); }
    });

    loop {
        cortex_m::asm::wfi();
    }
}

struct ContextS<'a> {
    ltdc: &'a mut LTDC,
}

impl<'a> ContextS<'a> {
    #[cold]
    fn wait_for_line_cold(&mut self, pixel_y: usize) {
        loop {
            if self.ltdc.cpsr.read().cypos().bits() > LTDC_INFO.vsync + LTDC_INFO.vbp + pixel_y as u16 {
                break;
            }
            #[cfg(feature = "strict_frame_timing")]
            if self.ltdc.isr.read().lif().is_reached() {
                for pixel_x in 0..FB_W {
                    demos::fb()[pixel_y * FB_W + pixel_x] = 222;
                }
                panic!("Timed out on line {}", pixel_y);
            }
        }
    }
}

impl<'a> demos::Context for ContextS<'a> {
    #[inline(always)]
    fn wait_for_line(&mut self, pixel_y: usize) {
        #[cfg(feature = "strict_frame_timing")]
        if self.ltdc.cpsr.read().cypos().bits() <= LTDC_INFO.vsync + LTDC_INFO.vbp + pixel_y as u16 {
            self.wait_for_line_cold(pixel_y);
        }
    }
    fn stats_count_adds(&mut self, _: usize) {}
    fn stats_count_cmps(&mut self, _: usize) {}
    fn stats_count_shrs(&mut self, _: usize) {}
    fn stats_count_muls(&mut self, _: usize) {}
    fn stats_count_mems(&mut self, _: usize) {}
    fn stats_count_divs(&mut self, _: usize) {}
    fn stats_count_fcvts(&mut self, _: usize) {}
    fn stats_count_fmuls(&mut self, _: usize) {}
}

#[interrupt]
fn LTDC() {
    cortex_m::interrupt::free(|cs| {
        let mut ltdc_ = GLTDC.borrow(cs).borrow_mut();
        let ltdc = ltdc_.as_mut().unwrap();
        ltdc.icr.write(|w| { w.clif().clear() });

        let mut state_ = GSTATE.borrow(cs).borrow_mut();
        let state = state_.as_mut().unwrap();

        match cortex_m::interrupt::free(|cs| *(LTDC_STATE.borrow(cs).borrow())) {
            LTDCState::Uninitialised => {
                ////////////////////////////////////////////////////////////////////////
                // configure layers

                // set background colour
                ltdc.bccr.write(|w| { w.bcred().bits(0xff).bcgreen().bits(0x80).bcblue().bits(0x00) });

                // x, y
                ltdc.layer1.whpcr.write(|w| { w.whstpos().bits(LTDC_INFO.hsync + LTDC_INFO.hbp).whsppos().bits(LTDC_INFO.hsync + LTDC_INFO.hbp + LTDC_INFO.aw - 1) });
                ltdc.layer1.wvpcr.write(|w| { w.wvstpos().bits(LTDC_INFO.vsync + LTDC_INFO.vbp).wvsppos().bits(LTDC_INFO.vsync + LTDC_INFO.vbp + LTDC_INFO.ah - 1) });
                // format
                ltdc.layer1.pfcr.write(|w| { w.pf().rgb565() });
                // framebuffer
                ltdc.layer1.cfbar.write(|w| { w.cfbadd().bits(&*demos::fb() as *const u16 as u32) });
                // line length, pitch
                ltdc.layer1.cfblr.write(|w| { w.cfbll().bits((2*FB_W + 3).try_into().unwrap()).cfbp().bits((2*FB_W).try_into().unwrap()) });
                // number of lines
                ltdc.layer1.cfblnr.write(|w| { w.cfblnbr().bits(FB_H.try_into().unwrap()) });
                // blending mode
                ltdc.layer1.bfcr.write(|w| { w.bf1().constant().bf2().constant() });
                ltdc.layer1.cr.write(|w| { w.len().enabled() });

                // reload shadow registers immediately
                ltdc.srcr.write(|w| { w.imr().reload() });
                while ltdc.srcr.read().imr().is_reload() { }

                *(LTDC_STATE.borrow(cs).borrow_mut()) = LTDCState::Initialised;

                {
                    let mut context = ContextS { ltdc };
                    use demos::Demo;
                    state.pre_render(&mut context);
                }
            },
            LTDCState::Initialised => {
                let mut context = ContextS { ltdc };
                use demos::Demo;
                state.render(&mut context);
                context.wait_for_line(FB_H-1);
                state.pre_render(&mut context);
            },
        }
        #[cfg(feature = "strict_frame_timing")]
        assert!(!ltdc.isr.read().lif().bit());
    });
}
