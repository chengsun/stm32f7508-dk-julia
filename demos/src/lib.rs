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

const Q: i32 = 10;
const ROTATE_FRAME_MAX: u32 = 942;
const TRANSLATE_FRAME_MAX: u32 = 188;

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

fn rotate_2d(x: i32, y: i32, cos: i32, sin: i32) -> (i32, i32) {
    ((cos * x + sin * y) >> Q, (cos * y - sin * x) >> Q)
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

        fn rotate_2d(p: (f64, f64), angle: f64) -> (f64, f64) {
            let (sin, cos) = angle.sin_cos();
            (cos * p.0 + sin * p.1, cos * p.1 - sin * p.0)
        }

        fn floor_mod(x: f64, y: f64) -> f64 {
            x - y * (x / y).floor()
        }

        fn lookup(mut x: f64, mut y: f64, mut z: f64) -> (f64, f64, f64, f64) {
            let mut min_distance : f64 = 100.;
            let mut accum = 1.;

            for _ in 0..20 {
                x = floor_mod(x, 2.) - 1.;
                y = floor_mod(y, 2.) - 1.;
                z = floor_mod(z, 2.) - 1.;

                (y, z) = rotate_2d((y, z), core::f64::consts::PI / 4.);

                let sqr_distance = x*x + y*y + z*z;
                let this_distance = sqr_distance.sqrt();
                let this_accum = sqr_distance / 2.;

                min_distance = min_distance.min(this_distance);

                accum *= this_accum;
                x = x / this_accum - 1.;
                y = y / this_accum - 1.;
                z = z / this_accum - 1.;
            }

            let base_color_r = 1.75 + (3. * min_distance * 0.9).sin();
            let base_color_g = 1.75 + (4. * min_distance * 0.9).sin();
            let base_color_b = 1.75 + (6. * min_distance * 0.9).sin();

            let accum_color_r = 0.0227 * base_color_r / (accum * 32.).exp();
            let accum_color_g = 0.0227 * base_color_g / (accum * 32.).exp();
            let accum_color_b = 0.0227 * base_color_b / (accum * 32.).exp();

            accum = accum.min(1.);

            return (accum_color_r, accum_color_g, accum_color_b, accum);
        }

        for x in 0..128 {
            let fx = (x as f64) / 64.;
            for y in 0..128 {
                let fy = (y as f64) / 64.;
                for z in 0..128 {
                    let fz = (z as f64) / 64.;

                    let (accum_color_r, accum_color_g, accum_color_b, accum) = lookup(fx, fy, fz);

                    lookup_table()[x*128*128 + y*128 + z] =
                        (((accum * 63.9999) as u32) << 24) |
                        (((accum_color_r * 255.9999) as u32) << 16) |
                        (((accum_color_g * 255.9999) as u32) << 8) |
                        (((accum_color_b * 255.9999) as u32) << 0);
                }
            }
        }

        Self { rotate_frame: 0, translate_frame: 0 }
    }

    fn compute_value(&self, context: &mut dyn Context, ray_direction_x: i32, ray_direction_y: i32, ray_direction_z: i32, translate_z: i32) -> u16 {
        const ITER_MAX: i32 = 16;

        let mut frag_color = 0u32;

        context.stats_count_cmps(1);
        let ray_direction_x: u32 = ray_direction_x.abs() as u32;
        context.stats_count_cmps(1);
        let ray_direction_y: u32 = ray_direction_y.abs() as u32;
        let ray_direction_z: u32 = ray_direction_z as u32;

        assert!(Q == 10);

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

        let (rotate_cos, rotate_sin) = cos_sin(((4 * self.rotate_frame as i32) << Q) / ROTATE_FRAME_MAX as i32);
        let translate_z = (self.translate_frame as i32) * (2 << Q) / TRANSLATE_FRAME_MAX as i32;

        for pixel_y in 0..FB_H {
            context.wait_for_line(pixel_y);
            let mut pixel_x = (self.rotate_frame as usize + pixel_y as usize) & 1;
            while pixel_x < FB_W {
                let mut ray_direction_x = ((((pixel_x as i32)<<1) - (FB_W as i32)) << Q) / (FB_H as i32);
                let mut ray_direction_y = ((((pixel_y as i32)<<1) - (FB_H as i32)) << Q) / (FB_H as i32);
                let mut ray_direction_z = 1 << Q;

                (ray_direction_x, ray_direction_z) = rotate_2d(ray_direction_x, ray_direction_z, rotate_cos, rotate_sin);
                (ray_direction_y, ray_direction_z) = rotate_2d(ray_direction_y, ray_direction_z, rotate_cos, rotate_sin);

                let value = self.compute_value(context, ray_direction_x, ray_direction_y, ray_direction_z, translate_z);
                fb()[pixel_y * FB_W + pixel_x] = value;

                pixel_x += 2;
            }
        }
    }
}
