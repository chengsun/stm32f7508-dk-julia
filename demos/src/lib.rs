#![cfg_attr(feature="real", no_std)]

include!(concat!(env!("OUT_DIR"), "/lookup_table.rs"));

mod libdivide;

#[cfg_attr(feature="real", link_section = ".fb")]
static mut FB: [u16; FB_W*FB_H] = [0; FB_W*FB_H];

#[inline(always)]
pub fn fb() -> &'static mut [u16; FB_W*FB_H] {
    unsafe { &mut FB }
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

const MAX_DIST_SQR: i32 = 25;
const INDEX_MQ: i32 = Q+2;
const INDEX_LEN: usize = (MAX_DIST_SQR<<(2*Q - INDEX_MQ)) as usize;

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
        Self { rotate_frame: 0, translate_frame: 0 }
    }

    fn compute_value(&self, context: &mut dyn Context, ray_direction_x: i32, ray_direction_y: i32, ray_direction_z: i32, translate_z: i32) -> u16 {
        const ITER_MAX: i32 = 17;

        let mut ray_len = 0u32;
        let mut frag_color = 0u32;

        for _ in 0..ITER_MAX {
            let mut p_x = (ray_direction_x * ray_len as i32) >> Q;
            let mut p_y = (ray_direction_y * ray_len as i32) >> Q;
            let mut p_z = (ray_direction_z * ray_len as i32) >> Q;
            p_z += translate_z;
            p_x = (p_x + (1<<Q)) & ((2<<Q) - 1);
            p_y = (p_y + (1<<Q)) & ((2<<Q) - 1);
            p_z = (p_z + (1<<Q)) & ((2<<Q) - 1);
            let index = ((p_x >> (Q+1-7)) * 128 * 128 + (p_y >> (Q+1-7)) * 128 + (p_z >> (Q+1-7))) as usize;
            let lookup_result = LOOKUP_TABLE[index];
            let distance = ((lookup_result >> 24) as u32) << (Q-8);
            ray_len += distance >> 3;
            frag_color += lookup_result & 0xFFFFFF;
        }

        let r = (frag_color >> 16) & 0xFF;
        let g = (frag_color >> 8) & 0xFF;
        let b = (frag_color >> 0) & 0xFF;
        (((r >> 3) << 11) | ((g >> 2) << 5) | ((b >> 3) << 0)) as u16
    }
}

impl Demo for Julia {
    fn pre_render(&mut self, context: &mut dyn Context) {
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

        let translate_z = ((self.translate_frame as i32) * (2 << Q) / TRANSLATE_FRAME_MAX as i32);

        let (rotate_cos, rotate_sin) = cos_sin(((4 * self.rotate_frame as i32) << Q) / ROTATE_FRAME_MAX as i32);
        for pixel_y in 0..FB_H {
            context.wait_for_line(pixel_y);
            for pixel_x in 0..FB_W {
                let mut ray_direction_x = ((((pixel_x as i32)<<1) - (FB_W as i32)) << Q) / (FB_H as i32);
                let mut ray_direction_y = ((((pixel_y as i32)<<1) - (FB_H as i32)) << Q) / (FB_H as i32);
                let mut ray_direction_z = 1 << Q;

                (ray_direction_x, ray_direction_z) = rotate_2d(ray_direction_x, ray_direction_z, rotate_cos, rotate_sin);
                (ray_direction_y, ray_direction_z) = rotate_2d(ray_direction_y, ray_direction_z, rotate_cos, rotate_sin);

                let value = self.compute_value(context, ray_direction_x, ray_direction_y, ray_direction_z, translate_z);
                fb()[pixel_y * FB_W + pixel_x] = value;
            }
        }
    }
}
