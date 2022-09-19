// Derived from the Rust port at https://github.com/youknowone/rust-divide
// which was in turn ported from libdivide 4.0.0
//
// This is an extremely cut down version of the division algorithm, with many
// corners cut for performance reasons -- as a result the result of the division
// may be inaccurate (but visually acceptable). Do not use for any other
// purpose.
//
// -----------------------------------------------------------------------------
//
// libdivide.h - Optimized integer division
// https://libdivide.com
//
// Copyright (C) 2010 - 2021 ridiculous_fish, <libdivide@ridiculousfish.com>
// Copyright (C) 2016 - 2021 Kim Walisch, <kim.walisch@gmail.com>
//
//
// This software is provided 'as-is', without any express or implied
// warranty. In no event will the authors be held liable for any damages
// arising from the use of this software.
// 
// Permission is granted to anyone to use this software for any purpose,
// including commercial applications, and to alter it and redistribute it
// freely, subject to the following restrictions:
// 
// 1. The origin of this software must not be misrepresented; you must not
// claim that you wrote the original software. If you use this software
// in a product, an acknowledgment in the product documentation would be
// appreciated but is not required.
// 2. Altered source versions must be plainly marked as such, and must not be
// misrepresented as being the original software.
// 3. This notice may not be removed or altered from any source distribution.
//
// -----------------------------------------------------------------------------
//

use core::convert::TryInto;

#[repr(packed)]
pub struct Divider {
    magic: i32,
    more: u8,
}

pub const DIVIDER_NULL: Divider = Divider {
    magic: 0,
    more: 0,
};

#[inline]
fn mullhi(x: i32, y: i32) -> i32 {
    let x = i64::from(x);
    let y = i64::from(y);
    let r = x * y;
    (r >> 32 as usize)
        .try_into()
        .unwrap_or_else(|_| unsafe { core::hint::unreachable_unchecked() })
}

pub fn gen(d: i32) -> Divider {
    assert!(d > 0);
    let abs_d = d as u32;
    let floor_log_2_d = 31 - abs_d.leading_zeros();
    if (abs_d & (abs_d - 1)) == 0 {
        // This codepath was modified from upstream libdivide. It makes it so
        // that we never have to special-case in `div` for zero magic (`smmul`
        // is fast on this ARM chip). The approximation is close enough for our
        // purposes!
        Divider {
            magic: i32::MAX,
            more: (floor_log_2_d - 1) as u8,
        }
    } else {
        let magic = {
            let q = 1u64 << (floor_log_2_d + 31);
            let r = abs_d as u64;
            (q / r + 1) as i32
        };
        let more = (floor_log_2_d - 1) as u8;
        Divider { magic, more }
    }
}

pub fn div(numer: i32, denom: &Divider) -> i32 {
    if denom.more >= 32 { unsafe { core::hint::unreachable_unchecked(); } }
    mullhi(denom.magic, numer) >> denom.more
}
