use std::env;
use std::fs;
use std::io::{BufWriter, Write};
use std::path::Path;

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

    let accum_color_r = 0.0234 * base_color_r / (accum * 32.).exp();
    let accum_color_g = 0.0234 * base_color_g / (accum * 32.).exp();
    let accum_color_b = 0.0234 * base_color_b / (accum * 32.).exp();

    accum = accum.min(1.);

    return (accum_color_r, accum_color_g, accum_color_b, accum);
}

fn main() {
    let out_dir = env::var_os("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("lookup_table.rs");

    let mut file = BufWriter::new(fs::File::create(&dest_path).unwrap());

    write!(file,
        "static LOOKUP_TABLE: [u32; 128*128*128] = [\n"
    ).unwrap();

    let mut i = 0;

    for x in -64..64 {
        let x = (x as f64) / 64.;
        for y in -64..64 {
            let y = (y as f64) / 64.;
            for z in -64..64 {
                let z = (z as f64) / 64.;

                let (accum_color_r, accum_color_g, accum_color_b, accum) = lookup(x, y, z);

                write!(file,
                       "  0x{:08x},\n",
                    (((accum * 255.9999) as u32) << 24) |
                    (((accum_color_r * 255.9999) as u32) << 16) |
                    (((accum_color_g * 255.9999) as u32) << 8) |
                    (((accum_color_b * 255.9999) as u32) << 0)
                ).unwrap();

                i += 1;
            }
        }
    }

    write!(file, "];\n").unwrap();

    println!("cargo:rerun-if-changed=build.rs");
}
