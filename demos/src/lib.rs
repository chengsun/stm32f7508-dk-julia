#![no_std]

#[cfg_attr(feature="real", link_section = ".fb")]
static mut FB: [u8; FB_W*FB_H] = [0; FB_W*FB_H];

#[inline(always)]
pub fn fb() -> &'static mut [u8; FB_W*FB_H] {
    unsafe { &mut FB }
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
const FRAME_MAX: u32 = 300;

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
        Self { frame: 0 }
    }

    #[inline(always)]
    fn compute_value_hot(&self, context: &mut dyn Context, pixel_x: usize, pixel_y: usize, c_a: i32, c_b: i32) -> u8 {
        let fb_size = core::cmp::min(FB_W, FB_H) as i32;
        let mut a = (((pixel_x as i32) << Q) - ((FB_W as i32 - 1) << (Q-1))) * 2 / fb_size;
        let mut b = (((pixel_y as i32) << Q) - ((FB_H as i32 - 1) << (Q-1))) * 2 / fb_size;
        const ITER_MAX: i32 = 36;
        let mut final_iter = ITER_MAX<<Q;
        let mut prev_dist = -40<<Q;

        for iter in 0..ITER_MAX {
            context.stats_count_muls(1);
            context.stats_count_shrs(1);
            let a2 = a*a >> Q;

            context.stats_count_muls(1);
            context.stats_count_shrs(1);
            let b2 = b*b >> Q;

            context.stats_count_adds(1);
            let this_dist = a2+b2;

            context.stats_count_cmps(1);
            if this_dist >= (4<<Q) {

                context.stats_count_adds(2);
                context.stats_count_shrs(2);
                context.stats_count_divs(1);
                let lerp = ((this_dist - (4<<Q)) << 8) / ((this_dist - prev_dist) >> (Q-8));

                context.stats_count_adds(1);
                context.stats_count_shrs(1);
                final_iter = (iter << Q) - lerp;
                break;
            }

            context.stats_count_muls(1);
            context.stats_count_shrs(1);
            let two_ab = a*b >> (Q-1);

            context.stats_count_adds(2);
            a = a2 - b2 + c_a;

            context.stats_count_adds(1);
            b = two_ab + c_b;

            prev_dist = this_dist;
        }
        ((final_iter * 255) / (ITER_MAX << Q)) as u8
    }

    #[inline(never)]
    fn compute_value_cold(&self, context: &mut dyn Context, pixel_x: usize, pixel_y: usize, c_a: i32, c_b: i32) -> u8 {
        self.compute_value_hot(context, pixel_x, pixel_y, c_a, c_b)
    }
}

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
    fn render(&mut self, context: &mut dyn Context) {
        self.frame += 1;
        if self.frame >= FRAME_MAX {
            self.frame = 0;
        }

        let coeff = (0.7885 * (1<<Q) as f32) as i32;
        let (cos, sin) = cos_sin(((4 * self.frame as i32) << Q) / FRAME_MAX as i32);
        let c_a = (coeff * cos) >> Q;
        let c_b = (coeff * sin) >> Q;
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
