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

#[derive(Copy, Clone)]
enum LTDCState {
    Uninitialised,
    Initialised,
}

static LTDC_STATE: Mutex<RefCell<LTDCState>> = Mutex::new(RefCell::new(LTDCState::Uninitialised));

const BORDER: usize = 0;
const FB_W: usize = LTDC_INFO.aw as usize - 2*BORDER;
const FB_H: usize = LTDC_INFO.ah as usize - 2*BORDER;

#[entry]
fn main() -> ! {
    let _cp = cortex_m::Peripherals::take().unwrap();
    let dp = stm32f7::stm32f750::Peripherals::take().unwrap();

    cortex_m::interrupt::free(move |cs| {

        //////////////////////////////////////////////////////////////////////////
        // increase the Flash latency wait states to be safe

        let flash = dp.FLASH;
        {
            let latency = 7;
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
        let plln = 216;
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
        let pllsain = 54;
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

        // Step 2. Enable and setup the LTDC peripheral.

        let ltdc = dp.LTDC;

        rcc.apb2enr.modify(|_, w| { w.ltdcen().bit(true) });

        ltdc.sscr.write(|w| { w.hsw().bits(LTDC_INFO.hsync - 1).vsh().bits(LTDC_INFO.vsync - 1) });
        ltdc.bpcr.write(|w| { w.ahbp().bits(LTDC_INFO.hsync + LTDC_INFO.hbp - 1).avbp().bits(LTDC_INFO.vsync + LTDC_INFO.vbp - 1) });
        ltdc.awcr.write(|w| { w.aaw().bits(LTDC_INFO.hsync + LTDC_INFO.hbp + LTDC_INFO.aw - 1).aah().bits(LTDC_INFO.vsync + LTDC_INFO.vbp + LTDC_INFO.ah - 1) });
        ltdc.twcr.write(|w| { w.totalw().bits(LTDC_INFO.hsync + LTDC_INFO.hbp + LTDC_INFO.aw + LTDC_INFO.hfp - 1).totalh().bits(LTDC_INFO.vsync + LTDC_INFO.vbp + LTDC_INFO.ah + LTDC_INFO.vfp - 1) });

        ltdc.gcr.write(|w| { w.hspol().active_low().vspol().active_low().depol().active_low().pcpol().rising_edge() });

        // enable line interrupt
        ltdc.lipcr.write(|w| { w.lipos().bits(LTDC_INFO.vsync + LTDC_INFO.vbp + BORDER as u16) });
        ltdc.ier.write(|w| { w.lie().enabled() });

        // enable the LTDC peripheral
        ltdc.gcr.modify(|_, w| { w.ltdcen().enabled() });

        // enable the screen and turn on the backlight
        gpioi.bsrr.write(|w| { w.bs12().bit(true) });
        gpiok.bsrr.write(|w| { w.bs3().bit(true) });

        *GLTDC.borrow(cs).borrow_mut() = Some(ltdc);
        *GSTATE.borrow(cs).borrow_mut() = Some(demos::Julia::new());
        unsafe { NVIC::unmask(Interrupt::LTDC); }
    });

    loop {
        cortex_m::asm::wfi();
    }
}

/*
fn cie_lch_to_rgb(l: i32, c: i32, h: i32) -> (i32, i32, i32) {
    let (cos_h, sin_h) = cos_sin(h);
    let u = (c * cos_h) >> Q;
    let v = (c * sin_h) >> Q;

    const U_N = (0.2009 * ((1 << Q) as f32)) as i32;
    const V_N = (0.4610 * ((1 << Q) as f32)) as i32;

    const X_N = (95.047 * ((1 << Q) as f32)) as i32;
    const Y_N = (100.000 * ((1 << Q) as f32)) as i32;
    const Z_N = (108.883 * ((1 << Q) as f32)) as i32;

    let u_prime = u / (13 * l) + U_N;
    let v_prime = v / (13 * l) + V_N;
    let f32_to_q = |f| { f * ((1 << Q) as f32) as i32 };
    let cube = |x| { x*x*x };
    let y =
        if l <= 8<<13 {
            let f = 3./29.;
            let f_cube = (f*f*f) * ((1 << Q) as f32) as i32;
            (Y_N * l * f_cube) >> (2*Q)
        } else {
            let f = (l + 16<<Q)/116;
            let f_cube = (f*f*f) >> (2*Q);
            (Y_N * f_cube) >> Q
        };
    let x = y * (9 * u_prime)/(4 * v_prime);
    let z = y * ((12<<Q) - 3*u_prime - 20*v_prime) / (4 * v_prime);

    let mat_xyz = |c_x, c_y, c_z| {
        (f32_to_q(c_x) * x + f32_to_q(c_y) * y + f32_to_q(c_z) * z) >> Q
    };

    let r_linear = mat_xyz(+3.24096994, -1.53738318, -0.49861076);
    let g_linear = mat_xyz(-0.96924364, +1.87596750, +0.04155506);
    let r_linear = mat_xyz(+0.05563008, -0.20397696, +1.05697151);

    let gamma = |linear| {
        if linear <= f32_to_q(0.0031308) {
            (f32_to_q(12.92) * linear) >> Q
        } else {
            (((f32_to_q(1.055) * q_pow(u, f32_to_q(1./2.4))) >> Q) - f32_to_q(0.055))
        }
    };

    (x, y, z)
}
*/

struct ContextS<'a> {
    fb: &'a mut [u8],
    ltdc: &'a mut LTDC,
    fb_w: usize,
    fb_h: usize,
}

impl<'a> demos::Context for ContextS<'a> {
    fn fb_h(&self) -> usize {
        self.fb_h
    }
    fn fb_w(&self) -> usize {
        self.fb_w
    }
    fn fb(&mut self) -> &mut [u8] {
        self.fb
    }
    fn wait_for_line(&mut self, pixel_y: usize) {
        #[cfg(not(debug_assertions))]
        loop {
            if self.ltdc.cpsr.read().cypos().bits() > LTDC_INFO.vsync + LTDC_INFO.vbp + BORDER as u16 + pixel_y as u16 {
                break;
            }
            if self.ltdc.isr.read().lif().is_reached() {
                for pixel_x in 0..FB_W {
                    self.fb[pixel_y * FB_W + pixel_x] = 222;
                }
                panic!("Timed out on line {}", pixel_y);
            }
            // It is a mystery why this helps. But it helps significantly.
            cortex_m::asm::nop();
        }
    }
    fn set_lut(&mut self, i: u8, r: u8, g: u8, b: u8) {
        self.ltdc.layer1.clutwr.write(|w| { w.clutadd().bits(i as u8).red().bits(r as u8).green().bits(g as u8).blue().bits(b as u8) });
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
    static mut FB: [u8; FB_W*FB_H] = [0; FB_W*FB_H];

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
                ltdc.layer1.whpcr.write(|w| { w.whstpos().bits(LTDC_INFO.hsync + LTDC_INFO.hbp + BORDER as u16).whsppos().bits(LTDC_INFO.hsync + LTDC_INFO.hbp + LTDC_INFO.aw - BORDER as u16 - 1) });
                ltdc.layer1.wvpcr.write(|w| { w.wvstpos().bits(LTDC_INFO.vsync + LTDC_INFO.vbp + BORDER as u16).wvsppos().bits(LTDC_INFO.vsync + LTDC_INFO.vbp + LTDC_INFO.ah - BORDER as u16 - 1) });
                // format
                // TODO: make enumerated values
                ltdc.layer1.pfcr.write(|w| { w.pf().l8() });
                // framebuffer
                ltdc.layer1.cfbar.write(|w| { w.cfbadd().bits(&*FB as *const u8 as u32) });
                // line length, pitch
                ltdc.layer1.cfblr.write(|w| { w.cfbll().bits((FB_W + 3).try_into().unwrap()).cfbp().bits(FB_W.try_into().unwrap()) });
                // number of lines
                ltdc.layer1.cfblnr.write(|w| { w.cfblnbr().bits(FB_H.try_into().unwrap()) });
                // blending mode
                ltdc.layer1.bfcr.write(|w| { w.bf1().constant().bf2().constant() });
                ltdc.layer1.cr.write(|w| { w.len().enabled().cluten().enabled() });

                // reload shadow registers immediately
                ltdc.srcr.write(|w| { w.imr().reload() });
                while ltdc.srcr.read().imr().is_reload() { }

                *(LTDC_STATE.borrow(cs).borrow_mut()) = LTDCState::Initialised;

                {
                    let mut context = ContextS { fb_w: FB_W, fb_h: FB_H, fb: &mut *FB, ltdc: ltdc };
                    use demos::Demo;
                    state.pre_render(&mut context);
                }
            },
            LTDCState::Initialised => {
                let mut context = ContextS { fb_w: FB_W, fb_h: FB_H, fb: &mut *FB, ltdc: ltdc };
                use demos::Demo;
                state.render(&mut context);
                context.wait_for_line(FB_H-1);
                state.pre_render(&mut context);
            },
        }
        #[cfg(not(debug_assertions))]
        assert!(!ltdc.isr.read().lif().bit());
    });
}
