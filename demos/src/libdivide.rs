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

const ADD_MARKER: u8 = 0x40;
const NEGATIVE_DIVISOR: u8 = 0x80;
const SHIFT_MASK: u8 = 0x1F;

#[inline]
fn mullhi(x: i32, y: i32) -> i32 {
    let x = i64::from(x);
    let y = i64::from(y);
    let r = x * y;
    (r >> 32 as usize)
        .try_into()
        .unwrap_or_else(|_| unsafe { core::hint::unreachable_unchecked() })
}

pub fn gen(sself: i32) -> Divider {
    internal_gen(sself, false)
}

fn internal_gen(d: i32, branchfree: bool) -> Divider {
    assert!(d > 0);

    // If d is a power of 2, or negative a power of 2, we have to use a shift.
    // This is especially important because the magic algorithm fails for -1.
    // To check if d is a power of 2 or its inverse, it suffices to check
    // whether its absolute value has exactly one bit set. This works even for
    // INT_MIN, because abs(INT_MIN) == INT_MIN, and INT_MIN has one bit set
    // and is a power of 2.
    let abs_d = d as u32;
    let floor_log_2_d = 31 - abs_d.leading_zeros();
    // check if exactly one bit is set,
    // don't care if abs_d is 0 since that's divide by zero
    if (abs_d & (abs_d - 1)) == 0 {
        // Branchfree and normal paths are exactly the same
        Divider {
            magic: 0,
            more: floor_log_2_d as u8 | if d < 0 { NEGATIVE_DIVISOR } else { 0 },
        }
    } else {
        assert!(floor_log_2_d >= 1);

        // the dividend here is 2**(floor_log_2_d + 31), so the low 32 bit word
        // is 0 and the high word is floor_log_2_d - 1
        let (proposed_m, rem) = {
            let q = 1u64 << (floor_log_2_d + 31);
            let r = abs_d as u64;
            (q / r, q % r)
        };
        let mut proposed_m = proposed_m as u32;
        let rem = rem as u32;
        let e = abs_d - rem;

        // We are going to start with a power of floor_log_2_d - 1.
        // This works if works if e < 2**floor_log_2_d.
        let more = if !branchfree && e < (1 << floor_log_2_d) {
            // This power works
            (floor_log_2_d - 1) as u8
        } else {
            // We need to go one higher. This should not make proposed_m
            // overflow, but it will make it negative when interpreted as an
            // int32_t.
            proposed_m = proposed_m.wrapping_add(proposed_m);
            let twice_rem = rem.wrapping_add(rem);
            if twice_rem >= abs_d || twice_rem < rem {
                proposed_m += 1;
            }
            floor_log_2_d as u8 | ADD_MARKER
        };

        proposed_m += 1;
        let magic = proposed_m as i32;

        Divider { magic, more }
    }
}

pub fn div(numer: i32, denom: &Divider) -> i32 {
    let more = denom.more;
    let shift = more & SHIFT_MASK;

    if 0 == denom.magic {
        numer >> shift
    } else {
        let mut uq = mullhi(denom.magic, numer) as u32;
        if 0 != (more & ADD_MARKER) {
            // q += (more < 0 ? -numer : numer)
            // cast required to avoid UB
            uq = uq.wrapping_add(numer as u32);
        }
        (uq as i32) >> shift
    }
}
