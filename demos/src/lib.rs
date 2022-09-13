#![no_std]

pub trait Context {
    fn fb_h(&self) -> usize;
    fn fb_w(&self) -> usize;
    fn fb(&mut self) -> &mut [u8];
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

        let fb_w = context.fb_w();
        let fb_h = context.fb_h();
        let coeff = (0.7885 * (1<<Q) as f32) as i32;
        let (cos, sin) = cos_sin(((4 * self.frame as i32) << Q) / FRAME_MAX as i32);
        let c_a = (coeff * cos) >> Q;
        let c_b = (coeff * sin) >> Q;
        let compute_value = |context: &mut dyn Context, pixel_x, pixel_y| {
            let fb_size = core::cmp::min(fb_w, fb_h) as i32;
            let mut a = (((pixel_x as i32) << Q) - ((fb_w as i32 - 1) << (Q-1))) * 2 / fb_size;
            let mut b = (((pixel_y as i32) << Q) - ((fb_h as i32 - 1) << (Q-1))) * 2 / fb_size;
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
        };
        let average_value = |fb: &[u8], pixel_x, pixel_y| {
            ((fb[(pixel_y-1) * fb_w + pixel_x] as u32
              + fb[(pixel_y+1) * fb_w + pixel_x] as u32
              + fb[(pixel_y+0) * fb_w + pixel_x-1] as u32
              + fb[(pixel_y+0) * fb_w + pixel_x+1] as u32)
             / 4) as u8
        };
        {
            let pixel_y = 0;
            context.wait_for_line(pixel_y);
            for pixel_x in 0..fb_w {
                let value = compute_value(context, pixel_x, pixel_y);
                context.fb()[pixel_y * fb_w + pixel_x] = value;
            }
        }
        for pixel_y in 1..fb_h/2+1 {
            context.wait_for_line(pixel_y);
            if pixel_y < fb_h/2 {
                let mut pixel_x = pixel_y & 1;
                while pixel_x < fb_w {
                    let value = compute_value(context, pixel_x, pixel_y);
                    context.fb()[pixel_y * fb_w + pixel_x] = value;
                    pixel_x += 2;
                }
            }
            if pixel_y >= 2 {
                let pixel_y = pixel_y - 1;
                let mut pixel_x = (pixel_y & 1) ^ 1;
                while pixel_x < fb_w {
                    let value = average_value(&context.fb(), pixel_x, pixel_y);
                    context.fb()[pixel_y * fb_w + pixel_x] = value;
                    pixel_x += 2;
                }
            }
        }
        {
            let pixel_y = fb_h/2;
            context.wait_for_line(pixel_y);
            let mut pixel_x = pixel_y & 1;
            while pixel_x < fb_w {
                context.fb()[pixel_y * fb_w + pixel_x] = context.fb()[(fb_h - pixel_y - 1) * fb_w + fb_w - pixel_x - 1];
                pixel_x += 2;
            }
        }
        {
            let pixel_y = fb_h/2-1;
            let mut pixel_x = (pixel_y & 1) ^ 1;
            while pixel_x < fb_w {
                let value = average_value(&context.fb(), pixel_x, pixel_y);
                context.fb()[pixel_y * fb_w + pixel_x] = value;
                context.fb()[(fb_h - pixel_y - 1) * fb_w + fb_w - pixel_x - 1] = value;
                pixel_x += 2;
            }
        }
        for pixel_y in fb_h/2+1..fb_h {
            context.wait_for_line(pixel_y);
            for pixel_x in 0..fb_w {
                context.fb()[pixel_y * fb_w + pixel_x] = context.fb()[(fb_h - pixel_y - 1) * fb_w + fb_w - pixel_x - 1];
            }
        }
    }
}
