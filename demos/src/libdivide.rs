// libdivide.h - Optimized integer division
// https://libdivide.com
//
// Copyright (C) 2010 - 2021 ridiculous_fish, <libdivide@ridiculousfish.com>
// Copyright (C) 2016 - 2021 Kim Walisch, <kim.walisch@gmail.com>
//
// libdivide is dual-licensed under the Boost or zlib licenses.
// You may use libdivide under the terms of either of these.
// See LICENSE.txt for more details.

// Port from 4.0.0

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

// Explanation of the "more" field:
//
// * Bits 0-5 is the shift value (for shift path or mult path).
// * Bit 6 is the add indicator for mult path.
// * Bit 7 is set if the divisor is negative. We use bit 7 as the negative
//   divisor indicator so that we can efficiently use sign extension to
//   create a bitmask with all bits set to 1 (if the divisor is negative)
//   or 0 (if the divisor is positive).
//
// u32: [0-4] shift value
//      [5] ignored
//      [6] add indicator
//      magic number of 0 indicates shift path
//
// s32: [0-4] shift value
//      [5] ignored
//      [6] add indicator
//      [7] indicates negative divisor
//      magic number of 0 indicates shift path
//
// u64: [0-5] shift value
//      [6] add indicator
//      magic number of 0 indicates shift path
//
// s64: [0-5] shift value
//      [6] add indicator
//      [7] indicates negative divisor
//      magic number of 0 indicates shift path
//
// In s32 and s64 branchfree modes, the magic number is negated according to
// whether the divisor is negated. In branchfree strategy, it is not negated.

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
    mullhi(denom.magic, numer) >> denom.more
}
