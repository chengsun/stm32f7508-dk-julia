use sdl2::rect::Point;
use sdl2::pixels::Color;
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use std::time::Duration;

const FB_W: usize = 480;
const FB_H: usize = 272;

struct ContextS<'a> {
    fb: &'a mut [u8],
    lut: &'a mut [(u8, u8, u8)],
    adds: usize,
    cmps: usize,
    shrs: usize,
    muls: usize,
    mems: usize,
    divs: usize,
    fcvts: usize,
    fmuls: usize,
}

impl<'a> demos::Context for ContextS<'a> {
    fn fb_h(&self) -> usize {
        FB_H
    }
    fn fb_w(&self) -> usize {
        FB_W
    }
    fn fb(&mut self) -> &mut [u8] {
        self.fb
    }
    fn wait_for_line(&mut self, _pixel_y: usize) {
    }
    fn set_lut(&mut self, i: u8, r: u8, g: u8, b: u8) {
        self.lut[i as usize] = (r, g, b);
    }
    fn stats_count_adds(&mut self, n: usize) { self.adds += n; }
    fn stats_count_cmps(&mut self, n: usize) { self.cmps += n; }
    fn stats_count_shrs(&mut self, n: usize) { self.shrs += n; }
    fn stats_count_muls(&mut self, n: usize) { self.muls += n; }
    fn stats_count_mems(&mut self, n: usize) { self.mems += n; }
    fn stats_count_divs(&mut self, n: usize) { self.divs += n; }
    fn stats_count_fcvts(&mut self, n: usize) { self.fcvts += n; }
    fn stats_count_fmuls(&mut self, n: usize) { self.fmuls += n; }
}

pub fn main() {
    let sdl_context = sdl2::init().unwrap();
    let video_subsystem = sdl_context.video().unwrap();

    let window = video_subsystem
        .window("stm32f7508-dk", FB_W.try_into().unwrap(), FB_H.try_into().unwrap())
        .position_centered()
        .build()
        .unwrap();

    let mut canvas = window.into_canvas().build().unwrap();

    let mut fb = [0u8; FB_W * FB_H];
    let mut lut = [(0u8, 0u8, 0u8); 256];

    let mut state = demos::Julia::new();

    let mut event_pump = sdl_context.event_pump().unwrap();
    'running: loop {
        for event in event_pump.poll_iter() {
            match event {
                Event::Quit {..} |
                Event::KeyDown { keycode: Some(Keycode::Escape), .. } => {
                    break 'running
                },
                _ => {}
            }
        }

        {
            let mut context = ContextS {
                fb: &mut fb,
                lut: &mut lut,
                adds: 0,
                cmps: 0,
                shrs: 0,
                muls: 0,
                mems: 0,
                divs: 0,
                fcvts: 0,
                fmuls: 0,
            };
            use demos::Demo;
            state.render(&mut context);
            let scale = 100000;
            println!("{:4} +{:4} >{:4} >>{:4} *{:4} []{:4} /{:4} fi{:4} f*{:4}",
                     (1*context.adds +
                     1*context.cmps +
                     1*context.shrs +
                     1*context.muls +
                     2*context.mems +
                     10*context.divs +
                     1*context.fcvts +
                     3*context.fmuls) / scale,

                     (1*context.adds)/scale,
                     (1*context.cmps)/scale,
                     (1*context.shrs)/scale,
                     (1*context.muls)/scale,
                     (2*context.mems)/scale,
                     (10*context.divs)/scale,
                     (1*context.fcvts)/scale,
                     (3*context.fmuls)/scale);
        }

        for y in 0..FB_H {
            for x in 0..FB_W {
                let (r, g, b) = lut[fb[y * FB_W + x] as usize];
                canvas.set_draw_color(Color::RGB(r, g, b));
                canvas.draw_point(Point::new(x as i32, y as i32)).unwrap();
            }
        }

        canvas.present();
        ::std::thread::sleep(Duration::new(0, 1_000_000_000u32 / 60));
    }
}
