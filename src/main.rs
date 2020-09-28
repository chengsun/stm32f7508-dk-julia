#![no_std]
#![no_main]

// pick a panicking behavior
#[cfg(debug_assertions)]
use panic_semihosting as _; // logs messages to the host stderr; requires a debugger
// use panic_halt as _; // you can put a breakpoint on `rust_begin_unwind` to catch panics

#[cfg(not(debug_assertions))]
use panic_semihosting as _; // logs messages to the host stderr; requires a debugger
//use panic_halt as _;
// use panic_abort as _; // requires nightly

use core::cell::RefCell;
use core::cmp::{max, min};
use core::convert::TryInto;
use core::sync::atomic::{AtomicU32, Ordering};

use cortex_m::interrupt::Mutex;
use cortex_m::peripheral::{SYST, syst::SystClkSource};
use cortex_m_rt::{entry, exception, ExceptionFrame};
use cortex_m_semihosting::hprintln;
use stm32f7::stm32f750::{interrupt, Interrupt, LTDC, NVIC, TIM7};

fn sin_internal(offset: i32) -> i32 {
    assert!(offset >= 0 && offset <= (1<<13));
    (offset>>4) * ((3<<18) - (offset>>4) * (offset>>4)) >> 15
}

fn cos_sin(theta: i32) -> (i32, i32) {
    assert!(theta >= 0 && theta <= (4<<13));
    if theta <= 1<<13 {
        (sin_internal((1<<13) - theta), sin_internal(theta))
    } else if theta <= 2<<13 {
        (-sin_internal(theta - (1<<13)), sin_internal((2<<13) - theta))
    } else if theta <= 3<<13 {
        (-sin_internal((3<<13) - theta), -sin_internal(theta - (2<<13)))
    } else {
        (sin_internal(theta - (3<<13)), -sin_internal((4<<13) - theta))
    }
}

struct CPUUtilisation {
}

impl CPUUtilisation {
    pub fn new() -> CPUUtilisation {
        CPUUtilisation {
        }
    }
    pub fn mark_idle_start(&mut self) {
    }
    pub fn mark_busy_start(&mut self) {
    }
}

static GCPU_UTILISATION: Mutex<RefCell<Option<CPUUtilisation>>> = Mutex::new(RefCell::new(None));

static GWAKEUP_CTR: AtomicU32 = AtomicU32::new(0);
static GLTDC_CTR: AtomicU32 = AtomicU32::new(0);
static GLTDC_ER_CTR: AtomicU32 = AtomicU32::new(0);
static GLTDC: Mutex<RefCell<Option<LTDC>>> = Mutex::new(RefCell::new(None));
static GTIM7: Mutex<RefCell<Option<TIM7>>> = Mutex::new(RefCell::new(None));

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

#[entry]
fn main() -> ! {
    hprintln!("Starting up").unwrap();

    let cp = cortex_m::Peripherals::take().unwrap();
    let dp = stm32f7::stm32f750::Peripherals::take().unwrap();

    cortex_m::interrupt::free(move |cs| {

        //////////////////////////////////////////////////////////////////////////
        // increase the Flash latency wait states to be safe

        let flash = dp.FLASH;
        {
            let latency = 6;
            flash.acr.write(|w| {
                w.latency().bits(latency).arten().bit(true)
            });
            hprintln!("Requested Flash latency wait states increase").unwrap();

            let mut i: u32 = 0;
            while flash.acr.read().latency().bits() != latency { i += 1; }
            hprintln!("Flash latency wait states increased after {} iters", i).unwrap();
        }

        //////////////////////////////////////////////////////////////////////////
        // configure RCC
        // - enable the PLL and set it to the system clock source
        // - enable the PLLSAI and set it to the LTDC clock source

        let rcc = dp.RCC;

        // PLL input = HSI = 16MHz
        // PLL output = 16MHz * PLLN / PLLM / PLLP = 16MHz * 64 / 8 / 8
        #[cfg(feature = "clock_debug")] let hsi = 16.;
        let pllm = 8;
        let plln = 210;
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
        #[cfg(feature = "clock_debug")] {
            let pll_vco_input = hsi / (pllm as f32);
            let pll_vco_output = pll_vco_input * (plln as f32);
            let sysclk = pll_vco_output / (pllp as f32);
            hprintln!("PLL VCO input  = {} MHz (1-2 MHz)", pll_vco_input).unwrap();
            hprintln!("PLL VCO output = {} MHz (100-432 MHz)", pll_vco_output).unwrap();
            hprintln!("sysclk         = {} MHz (<216 MHz)", sysclk).unwrap();
        }
        // PLLSAI input = HSI = 16MHz
        // PLLSAI output = 16MHz * PLLSAIN / PLLM / PLLSAIR / PLLSAIDIVR = 16MHz * 54 / 8 / 3 / 4
        let pllsain = 54;
        let pllsair = 3;
        let pllsaidivr = 4;
        rcc.pllsaicfgr.write(|w| unsafe {
            w.pllsain().bits(pllsain).pllsair().bits(pllsair)
        });
        rcc.dckcfgr1.write(|w| {
            match pllsaidivr {
                2 => w.pllsaidivr().div2(),
                4 => w.pllsaidivr().div4(),
                8 => w.pllsaidivr().div8(),
                _ => unreachable!()
            }
        });
        #[cfg(feature = "clock_debug")] {
            let pllsai_vco_input = hsi / (pllm as f32);
            let pllsai_vco_output = pllsai_vco_input * (pllsain as f32);
            let pllsai_r_output = pllsai_vco_output / (pllsair as f32);
            let ltdc_clk = pllsai_r_output / (pllsaidivr as f32);
            hprintln!("PLLSAI VCO input  = {} MHz (1-2 MHz)", pllsai_vco_input).unwrap();
            hprintln!("PLLSAI VCO output = {} MHz (100-432 MHz)", pllsai_vco_output).unwrap();
            hprintln!("PLLSAI_R output   = {} MHz", pllsai_r_output).unwrap();
            hprintln!("LTDC clk          = {} MHz", ltdc_clk).unwrap();
        }

        rcc.cr.write(|w| {
            w.pllon().bit(true).pllsaion().bit(true)
        });
        hprintln!("PLL and PLLSAI started").unwrap();

        {
            let mut i: u32 = 0;
            loop {
                let cr = rcc.cr.read();
                if cr.pllrdy().bit() && cr.pllsairdy().bit() { break; }
                i += 1;
            }
            hprintln!("PLL and PLLSAI locked after {} iters", i).unwrap();
        }

        rcc.cfgr.write(|w| unsafe {
            w.sw().bits(0b10)
        });
        hprintln!("Requested system clock switch to PLL").unwrap();

        {
            let mut i: u32 = 0;
            while rcc.cfgr.read().sws().bits() != 0b10 { i += 1; }
            hprintln!("System clock switched to PLL after {} iters", i).unwrap();
        }

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

        let gpioe = dp.GPIOE;
        let gpiog = dp.GPIOG;
        let gpioi = dp.GPIOI;
        let gpioj = dp.GPIOJ;
        let gpiok = dp.GPIOK;

        // enable clock for these GPIO ports
        rcc.ahb1enr.modify(|_, w| { w
                                    .gpioeen().bit(true)
                                    .gpiogen().bit(true)
                                    .gpioien().bit(true)
                                    .gpiojen().bit(true)
                                    .gpioken().bit(true) });

        // E4
        gpioe.afrl   .write(|w| { w.   afrl4().af14() });
        gpioe.otyper .write(|w| { w.     ot4().push_pull() });
        gpioe.pupdr  .write(|w| { w.  pupdr4().floating() });
        gpioe.ospeedr.write(|w| { w.ospeedr4().medium_speed() });
        gpioe.moder  .write(|w| { w.  moder4().alternate() });

        // G12
        gpiog.afrh   .write(|w| { w.   afrh12().af9() });
        gpiog.otyper .write(|w| { w.     ot12().push_pull() });
        gpiog.pupdr  .write(|w| { w.  pupdr12().floating() });
        gpiog.ospeedr.write(|w| { w.ospeedr12().medium_speed() });
        gpiog.moder  .write(|w| { w.  moder12().alternate() });

        // I{9,10,14,15}
        gpioi.afrh   .write(|w| { w
                                  .   afrh9 ().af14()
                                  .   afrh10().af14()
                                  .   afrh14().af14()
                                  .   afrh15().af14() });
        gpioi.otyper .write(|w| { w
                                  .     ot9 ().push_pull()
                                  .     ot10().push_pull()
                                  .     ot14().push_pull()
                                  .     ot15().push_pull() });
        gpioi.pupdr  .write(|w| { w
                                  .  pupdr9 ().floating()
                                  .  pupdr10().floating()
                                  .  pupdr14().floating()
                                  .  pupdr15().floating() });
        gpioi.ospeedr.write(|w| { w
                                  .ospeedr9 ().medium_speed()
                                  .ospeedr10().medium_speed()
                                  .ospeedr14().medium_speed()
                                  .ospeedr15().medium_speed() });
        gpioi.moder  .write(|w| { w
                                  .  moder9 ().alternate()
                                  .  moder10().alternate()
                                  .  moder14().alternate()
                                  .  moder15().alternate() });

        // J{0..11,13..15}
        gpioj.afrl   .write(|w| { w
                                  .   afrl0 ().af14()
                                  .   afrl1 ().af14()
                                  .   afrl2 ().af14()
                                  .   afrl3 ().af14()
                                  .   afrl4 ().af14()
                                  .   afrl5 ().af14()
                                  .   afrl6 ().af14()
                                  .   afrl7 ().af14() });
        gpioj.afrh   .write(|w| { w
                                  .   afrh8 ().af14()
                                  .   afrh9 ().af14()
                                  .   afrh10().af14()
                                  .   afrh11().af14()
                                  .   afrh13().af14()
                                  .   afrh14().af14()
                                  .   afrh15().af14() });
        gpioj.otyper .write(|w| { w
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
        gpioj.pupdr  .write(|w| { w
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
        gpioj.ospeedr.write(|w| { w
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
        gpioj.moder  .write(|w| { w
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
        gpiok.afrl   .write(|w| { w
                                  .   afrl0 ().af14()
                                  .   afrl1 ().af14()
                                  .   afrl2 ().af14()
                                  .   afrl4 ().af14()
                                  .   afrl5 ().af14()
                                  .   afrl6 ().af14()
                                  .   afrl7 ().af14() });
        gpiok.otyper .write(|w| { w
                                  .     ot0 ().push_pull()
                                  .     ot1 ().push_pull()
                                  .     ot2 ().push_pull()
                                  .     ot4 ().push_pull()
                                  .     ot5 ().push_pull()
                                  .     ot6 ().push_pull()
                                  .     ot7 ().push_pull() });
        gpiok.pupdr  .write(|w| { w
                                  .  pupdr0 ().floating()
                                  .  pupdr1 ().floating()
                                  .  pupdr2 ().floating()
                                  .  pupdr4 ().floating()
                                  .  pupdr5 ().floating()
                                  .  pupdr6 ().floating()
                                  .  pupdr7 ().floating() });
        gpiok.ospeedr.write(|w| { w
                                  .ospeedr0 ().medium_speed()
                                  .ospeedr1 ().medium_speed()
                                  .ospeedr2 ().medium_speed()
                                  .ospeedr4 ().medium_speed()
                                  .ospeedr5 ().medium_speed()
                                  .ospeedr6 ().medium_speed()
                                  .ospeedr7 ().medium_speed() });
        gpiok.moder  .write(|w| { w
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

        hprintln!("Configured GPIOs for LCD").unwrap();

        let ltdc = dp.LTDC;

        rcc.apb2enr.modify(|_, w| { w.ltdcen().bit(true) });

        ltdc.sscr.write(|w| unsafe { w.hsw().bits(LTDC_INFO.hsync - 1).vsh().bits(LTDC_INFO.vsync - 1) });
        ltdc.bpcr.write(|w| unsafe { w.ahbp().bits(LTDC_INFO.hsync + LTDC_INFO.hbp - 1).avbp().bits(LTDC_INFO.vsync + LTDC_INFO.vbp - 1) });
        ltdc.awcr.write(|w| unsafe { w.aaw().bits(LTDC_INFO.hsync + LTDC_INFO.hbp + LTDC_INFO.aw - 1).aah().bits(LTDC_INFO.vsync + LTDC_INFO.vbp + LTDC_INFO.ah - 1) });
        ltdc.twcr.write(|w| unsafe { w.totalw().bits(LTDC_INFO.hsync + LTDC_INFO.hbp + LTDC_INFO.aw + LTDC_INFO.hfp - 1).totalh().bits(LTDC_INFO.vsync + LTDC_INFO.vbp + LTDC_INFO.ah + LTDC_INFO.vfp - 1) });

        ltdc.gcr.write(|w| { w.hspol().bit(false).vspol().bit(false).depol().bit(false).pcpol().bit(false) });

        // set background colour
        ltdc.bccr.write(|w| unsafe { w.bcred().bits(0xff).bcgreen().bits(0x80).bcblue().bits(0x00) });

        // enable line interrupt
        ltdc.lipcr.write(|w| unsafe { w.lipos().bits(LTDC_INFO.vsync + LTDC_INFO.vbp + LTDC_INFO.ah) });
        ltdc.ier.write(|w| { w.lie().bit(true).fuie().bit(true).terrie().bit(true) });

        // enable the LTDC peripheral
        ltdc.gcr.modify(|_, w| { w.ltdcen().bit(true) });

        // enable the screen and turn on the backlight
        gpioi.bsrr.write(|w| { w.bs12().bit(true) });
        gpiok.bsrr.write(|w| { w.bs3().bit(true) });

        *GLTDC.borrow(cs).borrow_mut() = Some(ltdc);
        unsafe { NVIC::unmask(Interrupt::LTDC); }
        unsafe { NVIC::unmask(Interrupt::LTDC_ER); }

        //////////////////////////////////////////////////////////////////////////
        // message timer loop

        let tim7 = dp.TIM7;

        rcc.apb1enr.modify(|_, w| { w.tim7en().bit(true) });
        rcc.apb1lpenr.modify(|_, w| { w.tim7lpen().bit(true) });
        tim7.dier.write(|w| { w.uie().bit(true) });
        tim7.psc.write(|w| { w.psc().bits(32_000 - 1) });
        tim7.arr.write(|w| { w.arr().bits(20_000) });
        tim7.cr1.write(|w| { w.cen().bit(true) });
        *GTIM7.borrow(cs).borrow_mut() = Some(tim7);
        unsafe { NVIC::unmask(Interrupt::TIM7); }

        // configure the freerunning system timer
        let mut syst = cp.SYST;
        syst.set_clock_source(SystClkSource::Core);
        syst.set_reload(0xFFFFFFFF);
        syst.clear_current();
        syst.enable_counter();

        *GCPU_UTILISATION.borrow(cs).borrow_mut() = Some(CPUUtilisation::new());
    });

    loop {
        cortex_m::interrupt::free(move |cs| {
            let mut x = GCPU_UTILISATION.borrow(cs).borrow_mut();
            let cpu_utilisation = x.as_mut().unwrap();
            cpu_utilisation.mark_idle_start();
            cortex_m::asm::wfi();
            cpu_utilisation.mark_busy_start();
        });
        // the interrupt runs between these two calls to [free].
        cortex_m::interrupt::free(move |cs| {
            let mut x = GCPU_UTILISATION.borrow(cs).borrow_mut();
            let cpu_utilisation = x.as_mut().unwrap();
            cpu_utilisation.mark_idle_start();
            GWAKEUP_CTR.fetch_add(1, Ordering::SeqCst);
        });
    }
}

#[derive(Copy, Clone)]
enum LTDCState {
    Uninitialised,
    Initialised,
}

static LTDC_STATE: Mutex<RefCell<LTDCState>> = Mutex::new(RefCell::new(LTDCState::Uninitialised));

const BORDER: usize = 10;
const FB_W: usize = LTDC_INFO.aw as usize - 2*BORDER;
const FB_H: usize = LTDC_INFO.ah as usize - 2*BORDER;
const FRAME_MAX: u32 = 120;
#[interrupt]
fn LTDC() {
    static mut LTDC: Option<LTDC> = None;
    static mut FB: [u8; FB_W*FB_H] = [0; FB_W*FB_H];
    static mut FRAME: u32 = 0;

    let ltdc = LTDC.get_or_insert_with(|| {
        cortex_m::interrupt::free(|cs| GLTDC.borrow(cs).replace(None).unwrap())
    });
    if !ltdc.isr.read().lif().bit() { return; }
    ltdc.icr.write(|w| { w.clif().bit(true) });

    match cortex_m::interrupt::free(|cs| *(LTDC_STATE.borrow(cs).borrow())) {
        LTDCState::Uninitialised => {
            ////////////////////////////////////////////////////////////////////////
            // configure layers

            // x, y
            ltdc.layer1.whpcr.write(|w| unsafe { w.whstpos().bits(LTDC_INFO.hsync + LTDC_INFO.hbp + 10).whsppos().bits(LTDC_INFO.hsync + LTDC_INFO.hbp + LTDC_INFO.aw - 11) });
            ltdc.layer1.wvpcr.write(|w| unsafe { w.wvstpos().bits(LTDC_INFO.vsync + LTDC_INFO.vbp + 10).wvsppos().bits(LTDC_INFO.vsync + LTDC_INFO.vbp + LTDC_INFO.ah - 11) });
            // format
            // TODO: make enumerated values
            ltdc.layer1.pfcr.write(|w| unsafe { w.pf().bits(0b101) });
            // framebuffer
            ltdc.layer1.cfbar.write(|w| unsafe { w.cfbadd().bits(&*FB as *const u8 as u32) });
            // line length, pitch
            ltdc.layer1.cfblr.write(|w| unsafe { w.cfbll().bits((FB_W + 3).try_into().unwrap()).cfbp().bits(FB_W.try_into().unwrap()) });
            // number of lines
            ltdc.layer1.cfblnr.write(|w| unsafe { w.cfblnbr().bits(FB_H.try_into().unwrap()) });
            // blending mode
            ltdc.layer1.bfcr.write(|w| unsafe { w.bf1().bits(0b110).bf2().bits(0b111) });
            ltdc.layer1.cr.write(|w| { w.len().bit(true) });

            ltdc.layer2.bfcr.write(|w| unsafe { w.bf1().bits(0b110).bf2().bits(0b111) });
            ltdc.layer2.cr.write(|w| { w.len().bit(false) });

            // reload shadow registers immediately
            ltdc.srcr.write(|w| { w.imr().bit(true) });
            hprintln!("Requested LCD register reload immediately").unwrap();
            {
                let mut i: u32 = 0;
                while ltdc.srcr.read().imr().bit() { i += 1; }
                hprintln!("LCD register reloaded after {} iters", i).unwrap();
            }

            cortex_m::interrupt::free(|cs| *(LTDC_STATE.borrow(cs).borrow_mut()) = LTDCState::Initialised);
        },
        LTDCState::Initialised => {
            let coeff = (0.7885 * (1<<13) as f32) as i32;
            let (cos, sin) = cos_sin(((4 * *FRAME as i32) << 13) / FRAME_MAX as i32);
            let c_a = (coeff * cos) >> 13;
            let c_b = (coeff * sin) >> 13;
            let compute_value = |pixel_x, pixel_y| {
                let mut a = (((pixel_x * 2) as i32 - FB_W as i32) << 13) / min(FB_W, FB_H) as i32;
                let mut b = (((pixel_y * 2) as i32 - FB_H as i32) << 13) / min(FB_W, FB_H) as i32;
                let mut final_iter = -1;
                const ITER_MAX: i32 = 9;
                let mut prev_dist_m_4 = -1;

                for iter in 0..ITER_MAX {
                    let a2 = a*a >> 13;
                    let b2 = b*b >> 13;
                    let this_dist_m_4 = a2+b2 - (4<<13);
                    if this_dist_m_4 >= 0 {
                        if final_iter < 0 {
                            let lerp = (this_dist_m_4 << 13) / (this_dist_m_4 - prev_dist_m_4);
                            final_iter = (iter << 13) - lerp;
                        }
                        a = 2<<13;
                        b = 0;
                    } else {
                        let ab = a*b >> 13;
                        a = a2 - b2 + c_a;
                        b = ab + ab + c_b;
                    }
                    prev_dist_m_4 = this_dist_m_4;
                }
                (1 + (final_iter * 254) / (ITER_MAX << 13)) as u8
            };
            let populate_value = |fb: &mut [u8], pixel_x, pixel_y, value| {
                fb[pixel_y * FB_W + pixel_x] = value;
                fb[(FB_H-pixel_y-1) * FB_W + (FB_W-pixel_x-1)] = value;
            };
            for pixel_x in 0..FB_W {
                let value = compute_value(pixel_x, 0);
                populate_value(&mut *FB, pixel_x, 0, value);
            }
            for pixel_y in 1..FB_H/2 {
                for pixel_x in ((pixel_y & 1)..FB_W).step_by(2) {
                    let value = compute_value(pixel_x, pixel_y);
                    populate_value(&mut *FB, pixel_x, pixel_y, value);
                }
                if pixel_y >= 2 {
                    for pixel_x in ((pixel_y & 1)..FB_W).step_by(2) {
                        let value =
                            (((*FB)[(pixel_y-2) * FB_W + pixel_x] as u32
                            + (*FB)[(pixel_y+0) * FB_W + pixel_x] as u32
                            + (*FB)[(pixel_y-1) * FB_W + pixel_x-1] as u32
                            + (*FB)[(pixel_y-1) * FB_W + pixel_x+1] as u32) / 4) as u8;
                        populate_value(&mut *FB, pixel_x, pixel_y-1, value);
                    }
                }
            }
            for pixel_x in 0..FB_W {
                let value = compute_value(pixel_x, FB_H/2);
                populate_value(&mut *FB, pixel_x, FB_H/2, value);
            }
            *FRAME += 1;
            if *FRAME >= FRAME_MAX {
                *FRAME = 0;
            }
            GLTDC_CTR.fetch_add(1, Ordering::SeqCst);
        },
    }
}

#[interrupt]
fn LTDC_ER() {
    match cortex_m::interrupt::free(|cs| *(LTDC_STATE.borrow(cs).borrow())) {
        LTDCState::Uninitialised => {
        },
        LTDCState::Initialised => {
            GLTDC_ER_CTR.fetch_add(1, Ordering::SeqCst);
        }
    }
}

#[interrupt]
fn TIM7() {
    cortex_m::interrupt::free(|cs| GTIM7.borrow(cs).borrow_mut().as_mut().unwrap().sr.modify(|_, w| { w.uif().bit(false) }));

    let ltdc_ctr = GLTDC_CTR.swap(0, Ordering::SeqCst);
    let ltdc_er_ctr = GLTDC_ER_CTR.swap(0, Ordering::SeqCst);
    let wakeup_ctr = GWAKEUP_CTR.swap(0, Ordering::SeqCst);
    //hprintln!("ltdc: {}, ltdc_er: {}, wakeup: {}", ltdc_ctr, ltdc_er_ctr, wakeup_ctr).unwrap();
}

#[exception]
fn HardFault(ef: &ExceptionFrame) -> ! {
    panic!("{:#?}", ef);
}

#[exception]
fn DefaultHandler(irqn: i16) {
    panic!("Unhandled exception (IRQn = {})", irqn);
}
