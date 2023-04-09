#![cfg_attr(feature="real", no_std)]

#[cfg_attr(feature="real", link_section = ".fb")]
static mut FB: [u16; FB_W*FB_H] = [0; FB_W*FB_H];

#[inline(always)]
pub fn fb() -> &'static mut [u16; FB_W*FB_H] {
    unsafe { &mut FB }
}

#[cfg_attr(feature="real", link_section = ".fb")]
static mut LOOKUP_TABLE: [u32; 128*128*128] = [0; 128*128*128];

#[inline(always)]
pub fn lookup_table() -> &'static mut [u32; 128*128*128] {
    unsafe { &mut LOOKUP_TABLE }
}

pub trait Context {
    fn wait_for_line(&mut self, pixel_y: usize);
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
    /// render.
    fn pre_render(&mut self, context: &mut dyn Context);

    /// Called as soon as possible once the active area starts (and hence frame
    /// F is getting read out). Use to render frame F+1.
    fn render(&mut self, context: &mut dyn Context);
}

pub const FB_W: usize = 480;
pub const FB_H: usize = 272;

const ROTATE_FRAME_MAX: u32 = 942;
const TRANSLATE_FRAME_MAX: u32 = 188;

fn sin_internal_q10(offset: i32) -> i32 {
    (offset>>1) * ((3<<18) - (offset>>1)*(offset>>1)) >> 18
}

fn cos_sin_q10(theta: i32) -> (i32, i32) {
    assert!(theta >= 0 && theta <= (4<<10));
    if theta <= 1<<10 {
        (sin_internal_q10((1<<10) - theta), sin_internal_q10(theta))
    } else if theta <= 2<<10 {
        (-sin_internal_q10(theta - (1<<10)), sin_internal_q10((2<<10) - theta))
    } else if theta <= 3<<10 {
        (-sin_internal_q10((3<<10) - theta), -sin_internal_q10(theta - (2<<10)))
    } else {
        (sin_internal_q10(theta - (3<<10)), -sin_internal_q10((4<<10) - theta))
    }
}

fn sin_internal_q13(offset: i32) -> i32 {
    (offset>>4) * ((3<<18) - (offset>>4)*(offset>>4)) >> 15
}

fn cos_sin_q13(theta: i32) -> (i32, i32) {
    assert!(theta >= 0 && theta <= (4<<13));
    if theta <= 1<<13 {
        (sin_internal_q13((1<<13) - theta), sin_internal_q13(theta))
    } else if theta <= 2<<13 {
        (-sin_internal_q13(theta - (1<<13)), sin_internal_q13((2<<13) - theta))
    } else if theta <= 3<<13 {
        (-sin_internal_q13((3<<13) - theta), -sin_internal_q13(theta - (2<<13)))
    } else {
        (sin_internal_q13(theta - (3<<13)), -sin_internal_q13((4<<13) - theta))
    }
}

fn isqrt(n: i32) -> i32 {
    assert!(n >= 0);
    if n == 0 {
        return 0;
    }
    let mut x = n;
    loop {
        assert!(x > 0);
        let y = (x + (n / x)) >> 1;
        if y >= x {
            return x;
        }
        x = y;
    }
}

const FQ13: f64 = (1<<13) as f64;

// https://www.quinapalus.com/efunc.html
fn qexp_q13(mut x: i32) -> i32 {
    assert!(x >= 0);

    let mut y = 1 << 13;
    let mut t;

    fn saturating_shl(n: i32, s: i32) -> i32 {
        if n >= 1 << (31 - s) {
            i32::MAX
        } else {
            n << s
        }
    }

    t = x - (11.09035489*FQ13) as i32;
    if t*2 >= x { return i32::MAX; }
    if t > 0 { x = t; y = saturating_shl(y, 16); }

    t = x - (5.545177444*FQ13) as i32;
    if t > 0 { x = t; y = saturating_shl(y, 8); }

    t = x - (2.772588722*FQ13) as i32;
    if t > 0 { x = t; y = saturating_shl(y, 4); }

    t = x - (1.386294361*FQ13) as i32;
    if t > 0 { x = t; y = saturating_shl(y, 2); }

    t = x - (0.6931471806*FQ13) as i32;
    if t > 0 { x = t; y = saturating_shl(y, 1); }

    t = x - (0.4054651081*FQ13) as i32;
    if t > 0 { x = t; y = y.saturating_add(y>>1); }

    t = x - (0.2231435513*FQ13) as i32;
    if t > 0 { x = t; y = y.saturating_add(y>>2); }

    t = x - (0.1177830357*FQ13) as i32;
    if t > 0 { x = t; y = y.saturating_add(y>>3); }

    t = x - (0.06062462182*FQ13) as i32;
    if t > 0 { x = t; y = y.saturating_add(y>>4); }

    t = x - (0.03077165867*FQ13) as i32;
    if t > 0 { x = t; y = y.saturating_add(y>>5); }

    t = x - (0.01550418654*FQ13) as i32;
    if t > 0 { x = t; y = y.saturating_add(y>>6); }

    y.saturating_add((y >> 13).saturating_mul(x))
}

fn rotate_2d_q10(x: i32, y: i32, cos: i32, sin: i32) -> (i32, i32) {
    ((cos * x + sin * y) >> 10, (cos * y - sin * x) >> 10)
}

pub struct Julia {
    rotate_frame: u32,
    translate_frame: u32,
}

impl Julia {
    pub fn new() -> Self {
        for x in 0..(FB_W * FB_H) {
            fb()[x] = 0;
        }

        fn lookup_q13(mut x: i32, mut y: i32, mut z: i32) -> (i32, i32, i32, i32) {
            let mut min_distance = i32::MAX;
            let mut accum = 1i32 << 13;

            for iter in 0..20 {
                x = (x & ((1<<(13+1)) - 1)) - (1<<13);
                y = (y & ((1<<(13+1)) - 1)) - (1<<13);
                z = (z & ((1<<(13+1)) - 1)) - (1<<13);

                const Q2_SQRT_2: i32 = (1.414213562 * FQ13) as i32;
                (y, z) = ((Q2_SQRT_2 * (y + z)) >> (13+1), (Q2_SQRT_2 * (z - y)) >> (13+1));

                let sqr_distance = (x*x + y*y + z*z);
                let this_distance = isqrt(sqr_distance);
                let this_accum = sqr_distance >> (13+1);

                min_distance = min_distance.min(this_distance);

                accum = (accum * this_accum) >> 13;
                let this_accum = this_accum.max(1);
                x = (x << 13) / this_accum - (1 << 13);
                y = (y << 13) / this_accum - (1 << 13);
                z = (z << 13) / this_accum - (1 << 13);
            }

            fn qsin_q13(theta: i32) -> i32 {
                let theta = theta & ((1<<(13+2))-1);
                cos_sin_q13(theta).1
            }

            let base_color_r = (1.75*FQ13) as i32 + qsin_q13(((3.*0.57*FQ13) as i32 * min_distance as i32) >> 13);
            let base_color_g = (1.75*FQ13) as i32 + qsin_q13(((4.*0.57*FQ13) as i32 * min_distance as i32) >> 13);
            let base_color_b = (1.75*FQ13) as i32 + qsin_q13(((6.*0.57*FQ13) as i32 * min_distance as i32) >> 13);

            let exp_accum = qexp_q13(accum as i32 * 32);
            let accum_color_r = (0.0227*FQ13) as i32 * base_color_r / exp_accum;
            let accum_color_g = (0.0227*FQ13) as i32 * base_color_g / exp_accum;
            let accum_color_b = (0.0227*FQ13) as i32 * base_color_b / exp_accum;

            accum = accum.min(1<<13);

            return (accum_color_r, accum_color_g, accum_color_b, accum);
        }

        let mut flag = false;
        for x in 0..128 {
            let x_q = x << (13-6);
            for y in 0..128 {
                let y_q = y << (13-6);
                for z in 0..128 {
                    let z_q = z << (13-6);

                    let (accum_color_r, accum_color_g, accum_color_b, accum) = lookup_q13(x_q, y_q, z_q);

                    lookup_table()[(x*128*128 + y*128 + z) as usize] =
                        (((accum >> (13-6)) as u32) << 24) |
                        (((accum_color_r >> (13-8)) as u32) << 16) |
                        (((accum_color_g >> (13-8)) as u32) << 8) |
                        (((accum_color_b >> (13-8)) as u32) << 0);
                }
            }
        }

        Self { rotate_frame: 0, translate_frame: 0 }
    }

    fn compute_value_q10(&self, context: &mut dyn Context, ray_direction_x: i32, ray_direction_y: i32, ray_direction_z: i32, translate_z: i32) -> u16 {
        const ITER_MAX: i32 = 16;

        let mut frag_color = 0u32;

        context.stats_count_cmps(1);
        let ray_direction_x: u32 = ray_direction_x.abs() as u32;
        context.stats_count_cmps(1);
        let ray_direction_y: u32 = ray_direction_y.abs() as u32;
        let ray_direction_z: u32 = ray_direction_z as u32;

        context.stats_count_shrs(1);
        let mut p_x: u32 = ray_direction_x>>3;
        context.stats_count_shrs(1);
        let p_y: u32 = ray_direction_y>>3;
        context.stats_count_shrs(1);
        context.stats_count_adds(1);
        let p_z: u32 = (ray_direction_z>>3) + translate_z as u32;

        // ray_direction: have 11 bits, require 7 bits

        context.stats_count_shrs(1);
        let ray_direction_x = ray_direction_x << 1;
        context.stats_count_shrs(3);
        context.stats_count_adds(3);
        let ray_direction_zy = (ray_direction_z >> 4) << 16 | (ray_direction_y >> 4);

        context.stats_count_shrs(1);
        p_x = p_x << 10;
        context.stats_count_shrs(2);
        context.stats_count_adds(2);
        let mut p_zy = p_z << 21 | p_y << 5;

        // 33222222222211111111110000000000
        // 10987654321098765432109876543210
        //               xxxxxxxx
        //         zzzzzzzz        yyyyyyyy
        //   dddddddddddddd  dddddddddddddd
        //                 YYYYYYYyyyyyyyyy
        // ZZZZZZZzzzzzzzzz
        //            xxxxxxx
        //                   yyyyyyy
        //                          zzzzzzz
        //                 0011111110000000

        for iters_left in (1..=ITER_MAX).rev() {
            // can shave one shift off using BFI
            context.stats_count_adds(4);
            context.stats_count_shrs(1);
            let index =
                ((p_x & 0x1FC000)) +
                ((p_zy >> 2) & 0x3F80) +
                ((p_zy >> 25));
            context.stats_count_mems(1);
            let lookup_result = lookup_table()[index as usize];

            // distance: have 6 bits, require 6 bits
            context.stats_count_shrs(1);
            let distance = (lookup_result >> 24) as u32;
            context.stats_count_cmps(1);
            if distance == 0 {
                context.stats_count_adds(1);
                context.stats_count_muls(1);
                frag_color += lookup_result * iters_left as u32;
                break;
            }

            context.stats_count_adds(1);
            context.stats_count_muls(1);
            p_x += ray_direction_x * distance;
            context.stats_count_adds(1);
            context.stats_count_muls(1);
            p_zy += ray_direction_zy * distance;
            context.stats_count_adds(1);
            frag_color += lookup_result;
        }

        // can use bfi here as well to save cycles
        context.stats_count_adds(3);
        context.stats_count_shrs(3);
        (((frag_color & 0xF80000) >> 8) | ((frag_color & 0xFC00) >> 5) | ((frag_color & 0xF8) >> 3)) as u16
    }
}

impl Demo for Julia {
    fn pre_render(&mut self, _context: &mut dyn Context) {
    }

    #[inline(always)]
    fn render(&mut self, context: &mut dyn Context) {
        self.rotate_frame += 1;
        if self.rotate_frame >= ROTATE_FRAME_MAX {
            self.rotate_frame = 0;
        }

        self.translate_frame += 1;
        if self.translate_frame >= TRANSLATE_FRAME_MAX {
            self.translate_frame = 0;
        }

        let (rotate_cos, rotate_sin) = cos_sin_q10(((4 * self.rotate_frame as i32) << 10) / ROTATE_FRAME_MAX as i32);
        let translate_z = (self.translate_frame as i32) * (2 << 10) / TRANSLATE_FRAME_MAX as i32;

        for pixel_y in 0..FB_H {
            context.wait_for_line(pixel_y);
            let mut pixel_x = (self.rotate_frame as usize + pixel_y as usize) & 1;
            while pixel_x < FB_W {
                let mut ray_direction_x = ((((pixel_x as i32)<<1) - (FB_W as i32)) << 10) / (FB_H as i32);
                let mut ray_direction_y = ((((pixel_y as i32)<<1) - (FB_H as i32)) << 10) / (FB_H as i32);
                let mut ray_direction_z = 1 << 10;

                (ray_direction_x, ray_direction_z) = rotate_2d_q10(ray_direction_x, ray_direction_z, rotate_cos, rotate_sin);
                (ray_direction_y, ray_direction_z) = rotate_2d_q10(ray_direction_y, ray_direction_z, rotate_cos, rotate_sin);

                let value = self.compute_value_q10(context, ray_direction_x, ray_direction_y, ray_direction_z, translate_z);
                fb()[pixel_y * FB_W + pixel_x] = value;

                pixel_x += 2;
            }
        }
    }
}
