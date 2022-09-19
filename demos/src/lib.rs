#![no_std]

#[cfg_attr(feature="real", link_section = ".fb")]
static mut FB: [u8; FB_W*FB_H] = [0; FB_W*FB_H];

#[inline(always)]
pub fn fb() -> &'static mut [u8; FB_W*FB_H] {
    unsafe { &mut FB }
}

#[cfg_attr(feature="real", link_section = ".priority")]
static mut INVERSES2: [f32; 32*256] = [0f32; 32*256];

#[inline(always)]
pub fn inverses2() -> &'static mut [f32; 32*256] {
    unsafe { &mut INVERSES2 }
}

pub trait Context {
    fn wait_for_line(&mut self, pixel_y: usize);
    fn set_lut(&mut self, i: u8, r: u8, g: u8, b: u8);
    fn stats_count_adds(&mut self, n: usize);
    fn stats_count_cmps(&mut self, n: usize);
    fn stats_count_shrs(&mut self, n: usize);
    fn stats_count_muls(&mut self, n: usize);
    fn stats_count_mems(&mut self, n: usize);
    fn stats_count_divs(&mut self, n: usize);
    fn stats_count_fcvts(&mut self, n: usize);
    fn stats_count_fmuls(&mut self, n: usize);
}

pub trait Demo {
    /// Called as soon as possible once the vertical blanking period before
    /// frame F starts getting read out and frame F+1 starts getting computed by
    /// render. Use to set up the colour LUT for frame F.
    fn pre_render(&mut self, context: &mut dyn Context);

    /// Called as soon as possible once the active area starts (and hence frame
    /// F is getting read out). Use to render frame F+1.
    fn render(&mut self, context: &mut dyn Context);
}

pub const FB_W: usize = 480;
pub const FB_H: usize = 272;

const Q: i32 = 10;
const FRAME_MAX: u32 = 1000;

fn sin_internal(offset: i32) -> i32 {
    assert!(offset >= 0 && offset <= (1<<Q));
    match Q {
        8 => offset * ((3<<16) - offset*offset) >> 17,
        9 => offset * ((3<<18) - offset*offset) >> 19,
        10 => (offset>>1) * ((3<<18) - (offset>>1)*(offset>>1)) >> 18,
        11 => (offset>>2) * ((3<<18) - (offset>>2)*(offset>>2)) >> 17,
        12 => (offset>>3) * ((3<<18) - (offset>>3)*(offset>>3)) >> 16,
        13 => (offset>>4) * ((3<<18) - (offset>>4)*(offset>>4)) >> 15,
        _ => unreachable!()
    }
}

fn cos_sin(theta: i32) -> (i32, i32) {
    assert!(theta >= 0 && theta <= (4<<Q));
    if theta <= 1<<Q {
        (sin_internal((1<<Q) - theta), sin_internal(theta))
    } else if theta <= 2<<Q {
        (-sin_internal(theta - (1<<Q)), sin_internal((2<<Q) - theta))
    } else if theta <= 3<<Q {
        (-sin_internal((3<<Q) - theta), -sin_internal(theta - (2<<Q)))
    } else {
        (sin_internal(theta - (3<<Q)), -sin_internal((4<<Q) - theta))
    }
}

pub struct Julia {
    frame: u32,
}

impl Julia {
    pub fn new() -> Self {
        for x in 0..32*256 {
            let inv = (1 << (Q-2)) as f32 / (x as f32);
            inverses2()[x] = inv * inv * (1. / (1 << (Q-1)) as f32);
        }
        Self { frame: 0 }
    }

    #[inline(always)]
    fn compute_value_hot(&self, context: &mut dyn Context, pixel_x: usize, pixel_y: usize, c_a: i32, c_b: i32) -> u8 {
        let mut a = (((pixel_x as i32)<<1) - (FB_W as i32)) << (Q-8);
        let mut b = (((pixel_y as i32)<<1) - (FB_H as i32)) << (Q-8);
        const ITER_MAX: i32 = 12;
        let mut prev_distqq = -120<<(2*Q);

        const MAX_DIST_SQR: i32 = 32;

        let mut iter = 0;

        macro_rules! iteration {
            () => {
                context.stats_count_muls(1);
                let a2qq = a*a;

                context.stats_count_muls(1);
                let b2qq = b*b;

                context.stats_count_adds(1);
                let this_distqq = a2qq+b2qq;

                context.stats_count_cmps(1);
                if this_distqq >= (MAX_DIST_SQR<<(2*Q)) {

                    context.stats_count_adds(2);
                    context.stats_count_shrs(2);
                    context.stats_count_divs(1);
                    let lerp = ((this_distqq - (MAX_DIST_SQR<<(2*Q))) >> (Q-8)) / ((this_distqq - prev_distqq) >> (2*Q-8));

                    context.stats_count_adds(1);
                    context.stats_count_shrs(1);
                    let final_iter = (iter << Q) - lerp;
                    return ((final_iter * 255) / (ITER_MAX << Q)) as u8
                }

                context.stats_count_shrs(1);
                context.stats_count_cmps(1);
                if this_distqq >> (2*Q-5) == 0 {
                    return 255;
                }

                let div_dist2 = |context: &mut dyn Context, x| {
                    context.stats_count_fcvts(2);
                    context.stats_count_fmuls(1);
                    context.stats_count_mems(1);
                    context.stats_count_shrs(1);
                    let index = (this_distqq>>(Q+2)) as usize;
                    if index >= 32*256 {
                        unsafe {
                            core::hint::unreachable_unchecked();
                        }
                    }
                    ((x as f32) * inverses2()[index]) as i32
                };

                context.stats_count_muls(1);
                let two_aibi = div_dist2(context, a*b);

                context.stats_count_adds(2);
                context.stats_count_shrs(1);
                a = ((div_dist2(context, a2qq - b2qq)) >> 1) + c_a;

                context.stats_count_adds(1);
                b = c_b - two_aibi;

                prev_distqq = this_distqq;
                iter += 1;
            }
        }

        iteration!();
        iteration!();
        iteration!();
        iteration!();
        iteration!();
        iteration!();
        iteration!();
        iteration!();
        iteration!();
        iteration!();
        iteration!();
        iteration!();
        assert!(iter == ITER_MAX);

        255
    }

    #[inline(never)]
    fn compute_value_cold(&self, context: &mut dyn Context, pixel_x: usize, pixel_y: usize, c_a: i32, c_b: i32) -> u8 {
        self.compute_value_hot(context, pixel_x, pixel_y, c_a, c_b)
    }
}

/* complex exponentiation by -2:
    float bmag2 = dot(b, b);
    vec2 binv = b/vec2(bmag2);
    return vec2((binv.x*binv.x - binv.y*binv.y), -2.*binv.x*binv.y);
*/

impl Demo for Julia {
    fn pre_render(&mut self, context: &mut dyn Context) {
        for i in 0x00u32..=0xFFu32 {
            let h = (((self.frame * 360))/FRAME_MAX + i) % 360;
            let s = if i < 0xFF { 256-i } else { i };
            let (_, sin) = cos_sin((2*i << Q) as i32 / 256);
            let v = ((sin * 256) >> Q) as u32;

            let h_sector = h / 60;
            let h_frac = h % 60;

            let p = v * ( 256 - s ) / 256;
            let q = v * ( 256*60 - s * h_frac ) / (256*60);
            let t = v * ( 256*60 - s * ( 60 - h_frac ) ) / (256*60);

            let (r, g, b) = match h_sector {
                0 => (v, t, p),
                1 => (q, v, p),
                2 => (p, v, t),
                3 => (p, q, v),
                4 => (t, p, v),
                5 => (v, p, q),
                _ => unreachable!()
            };
            let clamp = |x: u32| { if x > 255 { 255 } else { x } };
            let r = clamp(r);
            let g = clamp(g);
            let b = clamp(b);
            context.set_lut(i as u8, r as u8, g as u8, b as u8);
        }
    }

    #[inline(always)]
    fn render(&mut self, context: &mut dyn Context) {
        self.frame += 1;
        if self.frame == 5 { self.frame = 220; }
        if self.frame >= FRAME_MAX {
            self.frame = 0;
        }

        let coeff = (0.7885 * (1<<Q) as f32) as i32;
        let (cos, sin) = cos_sin(((4 * self.frame as i32) << Q) / FRAME_MAX as i32);
        let c_a = (coeff * cos * cos.abs()) >> (2*Q);
        let c_b = (coeff * sin * sin.abs()) >> (2*Q);
        let average_value = |fb: &[u8; FB_W*FB_H], pixel_x, pixel_y| {
            ((fb[(pixel_y-1) * FB_W + pixel_x] as u32
              + fb[(pixel_y+1) * FB_W + pixel_x] as u32
              + fb[(pixel_y+0) * FB_W + pixel_x-1] as u32
              + fb[(pixel_y+0) * FB_W + pixel_x+1] as u32)
             / 4) as u8
        };
        {
            let pixel_y = 0;
            context.wait_for_line(pixel_y);
            for pixel_x in 0..FB_W {
                let value = self.compute_value_cold(context, pixel_x, pixel_y, c_a, c_b);
                fb()[pixel_y * FB_W + pixel_x] = value;
            }
        }
        for pixel_y in 1..FB_H/2+1 {
            context.wait_for_line(pixel_y);
            if pixel_y < FB_H/2 {
                let mut pixel_x = pixel_y & 1;
                while pixel_x < FB_W {
                    let value = self.compute_value_hot(context, pixel_x, pixel_y, c_a, c_b);
                    fb()[pixel_y * FB_W + pixel_x] = value;
                    pixel_x += 2;
                }
            }
            if pixel_y >= 2 {
                let pixel_y = pixel_y - 1;
                let mut pixel_x = (pixel_y & 1) ^ 1;
                while pixel_x < FB_W {
                    let value = average_value(&fb(), pixel_x, pixel_y);
                    fb()[pixel_y * FB_W + pixel_x] = value;
                    pixel_x += 2;
                }
            }
        }
        {
            let pixel_y = FB_H/2;
            context.wait_for_line(pixel_y);
            let mut pixel_x = pixel_y & 1;
            while pixel_x < FB_W {
                fb()[pixel_y * FB_W + pixel_x] = fb()[(FB_H - pixel_y - 1) * FB_W + FB_W - pixel_x - 1];
                pixel_x += 2;
            }
        }
        {
            let pixel_y = FB_H/2-1;
            let mut pixel_x = (pixel_y & 1) ^ 1;
            while pixel_x < FB_W {
                let value = average_value(&fb(), pixel_x, pixel_y);
                fb()[pixel_y * FB_W + pixel_x] = value;
                fb()[(FB_H - pixel_y - 1) * FB_W + FB_W - pixel_x - 1] = value;
                pixel_x += 2;
            }
        }
        for pixel_y in FB_H/2+1..FB_H {
            context.wait_for_line(pixel_y);
            for pixel_x in 0..FB_W {
                fb()[pixel_y * FB_W + pixel_x] = fb()[(FB_H - pixel_y - 1) * FB_W + FB_W - pixel_x - 1];
            }
        }
    }
}
