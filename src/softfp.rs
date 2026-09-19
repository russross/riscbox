//! Integer implementation of the IEEE-754 operations used by the RISC-V F and D extensions.

pub const FLAG_INEXACT: u32 = 1;
pub const FLAG_UNDERFLOW: u32 = 2;
pub const FLAG_OVERFLOW: u32 = 4;
pub const FLAG_DIVIDE_ZERO: u32 = 8;
pub const FLAG_INVALID: u32 = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RoundingMode {
    NearEven,
    Zero,
    Down,
    Up,
    NearMaxMag,
}

#[derive(Clone, Copy)]
struct Format {
    bits: u32,
    frac: u32,
    exp_bits: u32,
    bias: i32,
}
const F32: Format = Format {
    bits: 32,
    frac: 23,
    exp_bits: 8,
    bias: 127,
};
const F64: Format = Format {
    bits: 64,
    frac: 52,
    exp_bits: 11,
    bias: 1023,
};

#[derive(Clone, Copy)]
enum Value {
    Zero(bool),
    Finite(bool, u128, i32),
    Inf(bool),
    Nan(bool),
}

fn decode(a: u64, f: Format) -> Value {
    let sign = a >> (f.bits - 1) != 0;
    let em = (1_u64 << f.exp_bits) - 1;
    let frac = a & ((1_u64 << f.frac) - 1);
    let exp = (a >> f.frac) & em;
    if exp == em {
        return if frac == 0 {
            Value::Inf(sign)
        } else {
            Value::Nan(frac & (1 << (f.frac - 1)) == 0)
        };
    }
    if exp == 0 {
        return if frac == 0 {
            Value::Zero(sign)
        } else {
            Value::Finite(sign, u128::from(frac), 1 - f.bias - f.frac.cast_signed())
        };
    }
    Value::Finite(
        sign,
        u128::from(frac | (1 << f.frac)),
        i32::try_from(exp).expect("exponent field fits i32") - f.bias - f.frac.cast_signed(),
    )
}

fn qnan(f: Format) -> u64 {
    (((1_u64 << f.exp_bits) - 1) << f.frac) | (1 << (f.frac - 1))
}
fn sign_bit(sign: bool, f: Format) -> u64 {
    u64::from(sign) << (f.bits - 1)
}
fn jam(a: u128, n: u32) -> u128 {
    if n == 0 {
        a
    } else if n >= 128 {
        u128::from(a != 0)
    } else {
        (a >> n) | u128::from(a & ((1_u128 << n) - 1) != 0)
    }
}
fn bit_len(a: u128) -> u32 {
    128 - a.leading_zeros()
}

fn pack(
    sign: bool,
    mut sig: u128,
    mut exp: i32,
    rm: RoundingMode,
    flags: &mut u32,
    sticky: bool,
    f: Format,
) -> u64 {
    if sig == 0 {
        return sign_bit(sign, f);
    }
    sig = (sig << 1) | u128::from(sticky);
    exp -= 1;
    let precision = f.frac + 1;
    let normal_min = 1 - f.bias;
    let mut unbiased = exp + bit_len(sig).cast_signed() - 1;
    let target_exp = if unbiased < normal_min {
        normal_min - (precision.cast_signed() - 1)
    } else {
        unbiased - (precision.cast_signed() - 1)
    };
    let shift = target_exp - exp;
    let mut lost = 0_u128;
    let mut value = sig;
    if shift > 0 {
        let n = shift.cast_unsigned();
        if n >= 128 {
            lost = value;
            value = 0;
        } else {
            lost = value & ((1_u128 << n) - 1);
            value >>= n;
        }
    } else if shift < 0 {
        value <<= (-shift).cast_unsigned();
    }
    let inexact = lost != 0;
    if inexact {
        *flags |= FLAG_INEXACT;
        let n = shift.cast_unsigned();
        let half = n > 0 && n <= 128 && lost == (1_u128 << (n - 1));
        let above = (n < 128 && n > 0 && lost > (1_u128 << (n - 1)))
            || (n == 128 && lost > (1_u128 << 127));
        let inc = match rm {
            RoundingMode::NearEven => above || (half && value & 1 != 0),
            RoundingMode::NearMaxMag => above || half,
            RoundingMode::Zero => false,
            RoundingMode::Down => sign,
            RoundingMode::Up => !sign,
        };
        value += u128::from(inc);
    }
    if value == 0 {
        if inexact {
            *flags |= FLAG_UNDERFLOW;
        }
        return sign_bit(sign, f);
    }
    unbiased = target_exp + bit_len(value).cast_signed() - 1;
    let max_exp = ((1_i32 << f.exp_bits) - 2) - f.bias;
    if unbiased > max_exp {
        *flags |= FLAG_OVERFLOW | FLAG_INEXACT;
        let infinity = matches!(rm, RoundingMode::NearEven | RoundingMode::NearMaxMag)
            || matches!(rm, RoundingMode::Up) && !sign
            || matches!(rm, RoundingMode::Down) && sign;
        return sign_bit(sign, f)
            | if infinity {
                ((1_u64 << f.exp_bits) - 1) << f.frac
            } else {
                (((1_u64 << f.exp_bits) - 2) << f.frac) | ((1_u64 << f.frac) - 1)
            };
    }
    if unbiased < normal_min {
        if inexact {
            *flags |= FLAG_UNDERFLOW;
        }
        return sign_bit(sign, f) | u64::try_from(value).expect("subnormal significand fits u64");
    }
    let excess = bit_len(value) - precision;
    value >>= excess;
    let field = u64::try_from(unbiased + f.bias).expect("biased exponent is nonnegative");
    sign_bit(sign, f)
        | (field << f.frac)
        | (u64::try_from(value).expect("rounded significand fits u64") & ((1_u64 << f.frac) - 1))
}

fn invalid(flags: &mut u32, f: Format) -> u64 {
    *flags |= FLAG_INVALID;
    qnan(f)
}
fn nan2(a: Value, b: Value, flags: &mut u32, f: Format) -> Option<u64> {
    match (a, b) {
        (Value::Nan(sa), _) | (_, Value::Nan(sa)) => {
            if sa || matches!(a, Value::Nan(true)) || matches!(b, Value::Nan(true)) {
                *flags |= FLAG_INVALID;
            }
            Some(qnan(f))
        }
        _ => None,
    }
}

fn add_parts(
    (sa, mut a, ea): (bool, u128, i32),
    (sb, mut b, eb): (bool, u128, i32),
    rm: RoundingMode,
    flags: &mut u32,
    f: Format,
) -> u64 {
    const GUARD_BITS: u32 = 16;
    let e = ea.max(eb);
    a = jam(a << GUARD_BITS, (e - ea).cast_unsigned());
    b = jam(b << GUARD_BITS, (e - eb).cast_unsigned());
    if sa == sb {
        pack(sa, a + b, e - GUARD_BITS.cast_signed(), rm, flags, false, f)
    } else if a > b {
        pack(sa, a - b, e - GUARD_BITS.cast_signed(), rm, flags, false, f)
    } else if b > a {
        pack(sb, b - a, e - GUARD_BITS.cast_signed(), rm, flags, false, f)
    } else {
        sign_bit(matches!(rm, RoundingMode::Down), f)
    }
}
fn add(a: u64, b: u64, rm: RoundingMode, flags: &mut u32, f: Format) -> u64 {
    let (av, bv) = (decode(a, f), decode(b, f));
    if let Some(n) = nan2(av, bv, flags, f) {
        return n;
    }
    match (av, bv) {
        (Value::Inf(sa), Value::Inf(sb)) if sa != sb => invalid(flags, f),
        (Value::Inf(s), _) | (_, Value::Inf(s)) => {
            sign_bit(s, f) | (((1_u64 << f.exp_bits) - 1) << f.frac)
        }
        (Value::Zero(sa), Value::Zero(sb)) => sign_bit(
            if sa == sb {
                sa
            } else {
                matches!(rm, RoundingMode::Down)
            },
            f,
        ),
        (Value::Zero(_), _) => b,
        (_, Value::Zero(_)) => a,
        (Value::Finite(sa, ma, ea), Value::Finite(sb, mb, eb)) => {
            add_parts((sa, ma, ea), (sb, mb, eb), rm, flags, f)
        }
        _ => unreachable!(),
    }
}
fn mul(a: u64, b: u64, rm: RoundingMode, flags: &mut u32, f: Format) -> u64 {
    let (av, bv) = (decode(a, f), decode(b, f));
    if let Some(n) = nan2(av, bv, flags, f) {
        return n;
    }
    let sign = ((a ^ b) >> (f.bits - 1)) != 0;
    match (av, bv) {
        (Value::Inf(_), Value::Zero(_)) | (Value::Zero(_), Value::Inf(_)) => invalid(flags, f),
        (Value::Inf(_), _) | (_, Value::Inf(_)) => {
            sign_bit(sign, f) | (((1_u64 << f.exp_bits) - 1) << f.frac)
        }
        (Value::Zero(_), _) | (_, Value::Zero(_)) => sign_bit(sign, f),
        (Value::Finite(_, ma, ea), Value::Finite(_, mb, eb)) => {
            pack(sign, ma * mb, ea + eb, rm, flags, false, f)
        }
        _ => unreachable!(),
    }
}
fn div(a: u64, b: u64, rm: RoundingMode, flags: &mut u32, f: Format) -> u64 {
    let (av, bv) = (decode(a, f), decode(b, f));
    if let Some(n) = nan2(av, bv, flags, f) {
        return n;
    }
    let sign = ((a ^ b) >> (f.bits - 1)) != 0;
    match (av, bv) {
        (Value::Inf(_), Value::Inf(_)) | (Value::Zero(_), Value::Zero(_)) => invalid(flags, f),
        (Value::Inf(_), _) => sign_bit(sign, f) | (((1_u64 << f.exp_bits) - 1) << f.frac),
        (_, Value::Inf(_)) | (Value::Zero(_), _) => sign_bit(sign, f),
        (Value::Finite(_, _, _), Value::Zero(_)) => {
            *flags |= FLAG_DIVIDE_ZERO;
            sign_bit(sign, f) | (((1_u64 << f.exp_bits) - 1) << f.frac)
        }
        (Value::Finite(_, ma, ea), Value::Finite(_, mb, eb)) => {
            let n = f.frac + 10;
            let num = ma << n;
            pack(
                sign,
                num / mb,
                ea - eb - n.cast_signed(),
                rm,
                flags,
                !num.is_multiple_of(mb),
                f,
            )
        }
        _ => unreachable!(),
    }
}

fn isqrt(n: u128) -> (u128, u128) {
    let mut root = 0;
    let mut rem = 0;
    for i in (0..64).rev() {
        rem = (rem << 2) | ((n >> (i * 2)) & 3);
        let trial = (root << 2) | 1;
        if rem >= trial {
            rem -= trial;
            root = (root << 1) | 1;
        } else {
            root <<= 1;
        }
    }
    (root, rem)
}
fn sqrt(bits: u64, rm: RoundingMode, flags: &mut u32, format: Format) -> u64 {
    match decode(bits, format) {
        Value::Nan(s) => {
            if s {
                *flags |= FLAG_INVALID;
            }
            qnan(format)
        }
        Value::Inf(false) | Value::Zero(_) => bits,
        Value::Inf(true) | Value::Finite(true, _, _) => invalid(flags, format),
        Value::Finite(false, mut significand, mut exponent) => {
            if exponent & 1 != 0 {
                significand <<= 1;
                exponent -= 1;
            }
            let precision = format.frac + 1;
            let shift = (2 * (precision + 4) - bit_len(significand)) & !1;
            let (root, remainder) = isqrt(significand << shift);
            pack(
                false,
                root,
                exponent / 2 - (shift / 2).cast_signed(),
                rm,
                flags,
                remainder != 0,
                format,
            )
        }
    }
}

fn fma(a: u64, b: u64, c: u64, rm: RoundingMode, flags: &mut u32, f: Format) -> u64 {
    let (av, bv, cv) = (decode(a, f), decode(b, f), decode(c, f));
    let invalid_product = matches!(av, Value::Inf(_)) && matches!(bv, Value::Zero(_))
        || matches!(av, Value::Zero(_)) && matches!(bv, Value::Inf(_));
    if invalid_product {
        *flags |= FLAG_INVALID;
    }
    if matches!(av, Value::Nan(_)) || matches!(bv, Value::Nan(_)) || matches!(cv, Value::Nan(_)) {
        if matches!(av, Value::Nan(true))
            || matches!(bv, Value::Nan(true))
            || matches!(cv, Value::Nan(true))
        {
            *flags |= FLAG_INVALID;
        }
        return qnan(f);
    }
    let ps = ((a ^ b) >> (f.bits - 1)) != 0;
    match (av, bv, cv) {
        (Value::Inf(_), Value::Zero(_), _) | (Value::Zero(_), Value::Inf(_), _) => qnan(f),
        (Value::Inf(_), _, Value::Inf(sc)) | (_, Value::Inf(_), Value::Inf(sc)) if ps != sc => {
            invalid(flags, f)
        }
        (Value::Inf(_), _, _) | (_, Value::Inf(_), _) => {
            sign_bit(ps, f) | (((1_u64 << f.exp_bits) - 1) << f.frac)
        }
        (_, _, Value::Inf(_)) => c,
        (Value::Zero(_), _, _) | (_, Value::Zero(_), _) => add(sign_bit(ps, f), c, rm, flags, f),
        (Value::Finite(_, ma, ea), Value::Finite(_, mb, eb), Value::Finite(sc, mc, ec)) => {
            add_parts((ps, ma * mb, ea + eb), (sc, mc, ec), rm, flags, f)
        }
        (Value::Finite(_, ma, ea), Value::Finite(_, mb, eb), Value::Zero(sc)) => {
            add_parts((ps, ma * mb, ea + eb), (sc, 0, 0), rm, flags, f)
        }
        _ => unreachable!(),
    }
}

fn low_u32(value: u64) -> u32 {
    let bytes = value.to_le_bytes();
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}
macro_rules! api32 {
    ($add:ident,$sub:ident,$mul:ident,$div:ident,$sqrt:ident,$fma:ident) => {
        pub fn $add(a: u32, b: u32, r: RoundingMode, x: &mut u32) -> u32 {
            low_u32(add(a.into(), b.into(), r, x, F32))
        }
        pub fn $sub(a: u32, b: u32, r: RoundingMode, x: &mut u32) -> u32 {
            low_u32(add(
                u64::from(a),
                u64::from(b) ^ sign_bit(true, F32),
                r,
                x,
                F32,
            ))
        }
        pub fn $mul(a: u32, b: u32, r: RoundingMode, x: &mut u32) -> u32 {
            low_u32(mul(a.into(), b.into(), r, x, F32))
        }
        pub fn $div(a: u32, b: u32, r: RoundingMode, x: &mut u32) -> u32 {
            low_u32(div(a.into(), b.into(), r, x, F32))
        }
        pub fn $sqrt(a: u32, r: RoundingMode, x: &mut u32) -> u32 {
            low_u32(sqrt(a.into(), r, x, F32))
        }
        pub fn $fma(a: u32, b: u32, c: u32, r: RoundingMode, x: &mut u32) -> u32 {
            low_u32(fma(a.into(), b.into(), c.into(), r, x, F32))
        }
    };
}
api32!(add_f32, sub_f32, mul_f32, div_f32, sqrt_f32, fma_f32);
pub fn add_f64(a: u64, b: u64, r: RoundingMode, x: &mut u32) -> u64 {
    add(a, b, r, x, F64)
}
pub fn sub_f64(a: u64, b: u64, r: RoundingMode, x: &mut u32) -> u64 {
    add(a, b ^ sign_bit(true, F64), r, x, F64)
}
pub fn mul_f64(a: u64, b: u64, r: RoundingMode, x: &mut u32) -> u64 {
    mul(a, b, r, x, F64)
}
pub fn div_f64(a: u64, b: u64, r: RoundingMode, x: &mut u32) -> u64 {
    div(a, b, r, x, F64)
}
pub fn sqrt_f64(a: u64, r: RoundingMode, x: &mut u32) -> u64 {
    sqrt(a, r, x, F64)
}
pub fn fma_f64(
    first: u64,
    second: u64,
    addend: u64,
    rounding: RoundingMode,
    flags: &mut u32,
) -> u64 {
    fma(first, second, addend, rounding, flags, F64)
}

fn class(a: u64, f: Format) -> u32 {
    match decode(a, f) {
        Value::Inf(true) => 1,
        Value::Finite(true, m, _) => {
            if bit_len(m) <= f.frac {
                1 << 2
            } else {
                1 << 1
            }
        }
        Value::Zero(true) => 1 << 3,
        Value::Zero(false) => 1 << 4,
        Value::Finite(false, m, _) => {
            if bit_len(m) <= f.frac {
                1 << 5
            } else {
                1 << 6
            }
        }
        Value::Inf(false) => 1 << 7,
        Value::Nan(true) => 1 << 8,
        Value::Nan(false) => 1 << 9,
    }
}
fn compare(a: u64, b: u64, flags: &mut u32, f: Format, quiet: bool) -> Option<core::cmp::Ordering> {
    let (av, bv) = (decode(a, f), decode(b, f));
    if matches!(av, Value::Nan(_)) || matches!(bv, Value::Nan(_)) {
        if !quiet || matches!(av, Value::Nan(true)) || matches!(bv, Value::Nan(true)) {
            *flags |= FLAG_INVALID;
        }
        return None;
    }
    let mask = (1_u64 << (f.bits - 1)) - 1;
    if a & mask == 0 && b & mask == 0 {
        return Some(core::cmp::Ordering::Equal);
    }
    let sa = a >> (f.bits - 1) != 0;
    let sb = b >> (f.bits - 1) != 0;
    Some(if sa != sb {
        if sa {
            core::cmp::Ordering::Less
        } else {
            core::cmp::Ordering::Greater
        }
    } else if sa {
        (b & mask).cmp(&(a & mask))
    } else {
        (a & mask).cmp(&(b & mask))
    })
}
fn minmax(a: u64, b: u64, flags: &mut u32, f: Format, max: bool) -> u64 {
    let (av, bv) = (decode(a, f), decode(b, f));
    if matches!(av, Value::Nan(true)) || matches!(bv, Value::Nan(true)) {
        *flags |= FLAG_INVALID;
    }
    if matches!(av, Value::Nan(_)) {
        return if matches!(bv, Value::Nan(_)) {
            qnan(f)
        } else {
            b
        };
    }
    if matches!(bv, Value::Nan(_)) {
        return a;
    }
    let ord = compare(a, b, flags, f, true).unwrap();
    if ord == core::cmp::Ordering::Equal {
        if max { a & b } else { a | b }
    } else if (ord == core::cmp::Ordering::Greater) == max {
        a
    } else {
        b
    }
}

pub fn min_f32(a: u32, b: u32, x: &mut u32) -> u32 {
    low_u32(minmax(a.into(), b.into(), x, F32, false))
}
pub fn max_f32(a: u32, b: u32, x: &mut u32) -> u32 {
    low_u32(minmax(a.into(), b.into(), x, F32, true))
}
pub fn eq_f32(a: u32, b: u32, x: &mut u32) -> bool {
    compare(a.into(), b.into(), x, F32, true) == Some(core::cmp::Ordering::Equal)
}
pub fn lt_f32(a: u32, b: u32, x: &mut u32) -> bool {
    compare(a.into(), b.into(), x, F32, false) == Some(core::cmp::Ordering::Less)
}
pub fn le_f32(a: u32, b: u32, x: &mut u32) -> bool {
    matches!(
        compare(a.into(), b.into(), x, F32, false),
        Some(core::cmp::Ordering::Less | core::cmp::Ordering::Equal)
    )
}
#[must_use]
pub fn class_f32(a: u32) -> u32 {
    class(a.into(), F32)
}
pub fn min_f64(a: u64, b: u64, x: &mut u32) -> u64 {
    minmax(a, b, x, F64, false)
}
pub fn max_f64(a: u64, b: u64, x: &mut u32) -> u64 {
    minmax(a, b, x, F64, true)
}
pub fn eq_f64(a: u64, b: u64, x: &mut u32) -> bool {
    compare(a, b, x, F64, true) == Some(core::cmp::Ordering::Equal)
}
pub fn lt_f64(a: u64, b: u64, x: &mut u32) -> bool {
    compare(a, b, x, F64, false) == Some(core::cmp::Ordering::Less)
}
pub fn le_f64(a: u64, b: u64, x: &mut u32) -> bool {
    matches!(
        compare(a, b, x, F64, false),
        Some(core::cmp::Ordering::Less | core::cmp::Ordering::Equal)
    )
}
#[must_use]
pub fn class_f64(a: u64) -> u32 {
    class(a, F64)
}

fn convert_format(a: u64, src: Format, dst: Format, rm: RoundingMode, flags: &mut u32) -> u64 {
    match decode(a, src) {
        Value::Zero(s) => sign_bit(s, dst),
        Value::Inf(s) => sign_bit(s, dst) | (((1_u64 << dst.exp_bits) - 1) << dst.frac),
        Value::Nan(s) => {
            if s {
                *flags |= FLAG_INVALID;
            }
            qnan(dst)
        }
        Value::Finite(s, m, e) => pack(s, m, e, rm, flags, false, dst),
    }
}
pub fn f32_to_f64(a: u32, flags: &mut u32) -> u64 {
    convert_format(a.into(), F32, F64, RoundingMode::NearEven, flags)
}
pub fn f64_to_f32(a: u64, rm: RoundingMode, flags: &mut u32) -> u32 {
    low_u32(convert_format(a, F64, F32, rm, flags))
}

fn float_to_int(
    source: u64,
    format: Format,
    rounding: RoundingMode,
    flags: &mut u32,
    signed: bool,
    bits: u32,
) -> u128 {
    let (sign, magnitude, exponent) = match decode(source, format) {
        Value::Zero(_) => return 0,
        Value::Finite(value_sign, value_magnitude, value_exponent) => {
            (value_sign, value_magnitude, value_exponent)
        }
        Value::Inf(value_sign) => {
            *flags |= FLAG_INVALID;
            return if signed {
                if value_sign {
                    1_u128 << (bits - 1)
                } else {
                    (1_u128 << (bits - 1)) - 1
                }
            } else if value_sign {
                0
            } else {
                (1_u128 << bits) - 1
            };
        }
        Value::Nan(_) => {
            *flags |= FLAG_INVALID;
            return if signed {
                (1_u128 << (bits - 1)) - 1
            } else {
                (1_u128 << bits) - 1
            };
        }
    };
    let mut result = magnitude;
    let mut lost = 0;
    if exponent >= 0 {
        if exponent >= 128 || bit_len(result) + exponent.cast_unsigned() > 128 {
            *flags |= FLAG_INVALID;
            return if signed {
                if sign {
                    1_u128 << (bits - 1)
                } else {
                    (1_u128 << (bits - 1)) - 1
                }
            } else if sign {
                0
            } else {
                (1_u128 << bits) - 1
            };
        }
        result <<= exponent.cast_unsigned();
    } else {
        let shift = (-exponent).cast_unsigned();
        if shift >= 128 {
            lost = result;
            result = 0;
        } else {
            lost = result & ((1_u128 << shift) - 1);
            result >>= shift;
        }
        if lost != 0 {
            let half = shift > 0 && shift <= 128 && lost == (1_u128 << (shift - 1));
            let above = (shift < 128 && shift > 0 && lost > (1_u128 << (shift - 1)))
                || (shift == 128 && lost > (1_u128 << 127));
            let inc = match rounding {
                RoundingMode::NearEven => above || (half && result & 1 != 0),
                RoundingMode::NearMaxMag => above || half,
                RoundingMode::Zero => false,
                RoundingMode::Down => sign,
                RoundingMode::Up => !sign,
            };
            result += u128::from(inc);
        }
    }
    let overflow = if signed {
        if sign {
            result > (1_u128 << (bits - 1))
        } else {
            result > ((1_u128 << (bits - 1)) - 1)
        }
    } else {
        sign && result != 0 || (!sign && result > ((1_u128 << bits) - 1))
    };
    if overflow {
        *flags |= FLAG_INVALID;
        return if signed {
            if sign {
                1_u128 << (bits - 1)
            } else {
                (1_u128 << (bits - 1)) - 1
            }
        } else if sign {
            0
        } else {
            (1_u128 << bits) - 1
        };
    }
    if lost != 0 {
        *flags |= FLAG_INEXACT;
    }
    if sign {
        (!result).wrapping_add(1) & ((1_u128 << bits) - 1)
    } else {
        result
    }
}
fn result_u32(value: u128) -> u32 {
    u32::try_from(value).expect("32-bit conversion result fits u32")
}
fn result_u64(value: u128) -> u64 {
    u64::try_from(value).expect("64-bit conversion result fits u64")
}
pub fn f32_to_i32(a: u32, r: RoundingMode, x: &mut u32) -> i32 {
    result_u32(float_to_int(a.into(), F32, r, x, true, 32)).cast_signed()
}
pub fn f32_to_u32(a: u32, r: RoundingMode, x: &mut u32) -> u32 {
    result_u32(float_to_int(a.into(), F32, r, x, false, 32))
}
pub fn f32_to_i64(a: u32, r: RoundingMode, x: &mut u32) -> i64 {
    result_u64(float_to_int(a.into(), F32, r, x, true, 64)).cast_signed()
}
pub fn f32_to_u64(a: u32, r: RoundingMode, x: &mut u32) -> u64 {
    result_u64(float_to_int(a.into(), F32, r, x, false, 64))
}
pub fn f64_to_i32(a: u64, r: RoundingMode, x: &mut u32) -> i32 {
    result_u32(float_to_int(a, F64, r, x, true, 32)).cast_signed()
}
pub fn f64_to_u32(a: u64, r: RoundingMode, x: &mut u32) -> u32 {
    result_u32(float_to_int(a, F64, r, x, false, 32))
}
pub fn f64_to_i64(a: u64, r: RoundingMode, x: &mut u32) -> i64 {
    result_u64(float_to_int(a, F64, r, x, true, 64)).cast_signed()
}
pub fn f64_to_u64(a: u64, r: RoundingMode, x: &mut u32) -> u64 {
    result_u64(float_to_int(a, F64, r, x, false, 64))
}

fn int_to_float(sign: bool, m: u128, f: Format, rm: RoundingMode, flags: &mut u32) -> u64 {
    if m == 0 {
        sign_bit(false, f)
    } else {
        pack(sign, m, 0, rm, flags, false, f)
    }
}
fn int32_float(sign: bool, magnitude: u128, f: Format, rm: RoundingMode, x: &mut u32) -> u32 {
    u32::try_from(int_to_float(sign, magnitude, f, rm, x)).expect("binary32 result fits u32")
}
pub fn i32_to_f32(a: i32, r: RoundingMode, x: &mut u32) -> u32 {
    int32_float(a < 0, a.unsigned_abs().into(), F32, r, x)
}
pub fn u32_to_f32(a: u32, r: RoundingMode, x: &mut u32) -> u32 {
    int32_float(false, a.into(), F32, r, x)
}
pub fn i64_to_f32(a: i64, r: RoundingMode, x: &mut u32) -> u32 {
    int32_float(a < 0, a.unsigned_abs().into(), F32, r, x)
}
pub fn u64_to_f32(a: u64, r: RoundingMode, x: &mut u32) -> u32 {
    int32_float(false, a.into(), F32, r, x)
}
pub fn i32_to_f64(a: i32, r: RoundingMode, x: &mut u32) -> u64 {
    int_to_float(a < 0, a.unsigned_abs().into(), F64, r, x)
}
pub fn u32_to_f64(a: u32, r: RoundingMode, x: &mut u32) -> u64 {
    int_to_float(false, a.into(), F64, r, x)
}
pub fn i64_to_f64(a: i64, r: RoundingMode, x: &mut u32) -> u64 {
    int_to_float(a < 0, a.unsigned_abs().into(), F64, r, x)
}
pub fn u64_to_f64(a: u64, r: RoundingMode, x: &mut u32) -> u64 {
    int_to_float(false, a.into(), F64, r, x)
}
