//! Port of the parts of 1.8.9 `MathHelper` the combat code depends on.
//! The sine lookup table is reproduced because movement and look vectors
//! use it, so a libm sine would drift from the real client.
use std::sync::OnceLock;

static SIN: OnceLock<Vec<f32>> = OnceLock::new();

#[inline]
fn table() -> &'static [f32] {
    SIN.get_or_init(|| {
        (0..65536)
            .map(|i| ((i as f64) * std::f64::consts::PI * 2.0 / 65536.0).sin() as f32)
            .collect()
    })
}

pub const PI_F: f32 = std::f64::consts::PI as f32;
pub const DEG2RAD_F: f32 = 0.017453292;

#[inline]
pub fn sin(v: f32) -> f32 {
    table()[(((v * 10430.378f32) as i32) & 65535) as usize]
}

#[inline]
pub fn cos(v: f32) -> f32 {
    table()[(((v * 10430.378f32 + 16384.0f32) as i32) & 65535) as usize]
}

#[inline]
pub fn sqrt_f(v: f32) -> f32 {
    (v as f64).sqrt() as f32
}

#[inline]
pub fn sqrt_d(v: f64) -> f32 {
    v.sqrt() as f32
}

#[inline]
pub fn floor_d(v: f64) -> i32 {
    let i = v as i32;
    if v < i as f64 { i - 1 } else { i }
}

#[inline]
pub fn floor_f(v: f32) -> i32 {
    let i = v as i32;
    if v < i as f32 { i - 1 } else { i }
}

/// `MathHelper.func_181161_i`: a fast inverse square root.
#[inline]
fn fast_inv_sqrt(v: f64) -> f64 {
    let d0 = 0.5 * v;
    let i = 6910469410427058090i64.wrapping_sub((v.to_bits() as i64) >> 1);
    let v = f64::from_bits(i as u64);
    v * (1.5 - d0 * v * v)
}

/// `MathHelper.atan2(y, x)` of 1.8.9: the table-driven approximation the
/// game uses for the rotation of thrown items.
pub fn atan2(mut a: f64, mut b: f64) -> f64 {
    let d0 = b * b + a * a;
    if d0.is_nan() {
        return f64::NAN;
    }
    let flag = a < 0.0;
    if flag {
        a = -a;
    }
    let flag1 = b < 0.0;
    if flag1 {
        b = -b;
    }
    let flag2 = a > b;
    if flag2 {
        std::mem::swap(&mut a, &mut b);
    }
    let d9 = fast_inv_sqrt(d0);
    b *= d9;
    a *= d9;
    let bias = f64::from_bits(4805340802404319232u64);
    let d2 = bias + a;
    // (int) of the raw bits: the low 32 bits, which hold the table index.
    let i = (d2.to_bits() as u32 as i32) as usize;
    let d3 = f64::from_bits(crate::atan_tab::ASIN[i.min(256)]);
    let d4 = f64::from_bits(crate::atan_tab::COS[i.min(256)]);
    let d5 = d2 - bias;
    let d6 = a * d4 - b * d5;
    let d7 = (6.0 + d6 * d6) * d6 * 0.16666666666666666;
    let mut d8 = d3 + d7;
    if flag2 {
        d8 = std::f64::consts::FRAC_PI_2 - d8;
    }
    if flag1 {
        d8 = std::f64::consts::PI - d8;
    }
    if flag {
        d8 = -d8;
    }
    d8
}

/// Wrap an angle in degrees to [-180, 180).
#[inline]
pub fn wrap_deg(a: f64) -> f64 {
    let mut a = a % 360.0;
    if a >= 180.0 { a -= 360.0; }
    if a < -180.0 { a += 360.0; }
    a
}

/// `Entity.getVectorForRotation(pitch, yaw)`.
#[inline]
pub fn look_vec(pitch: f32, yaw: f32) -> [f64; 3] {
    let f = cos(-yaw * DEG2RAD_F - PI_F);
    let f1 = sin(-yaw * DEG2RAD_F - PI_F);
    let f2 = -cos(-pitch * DEG2RAD_F);
    let f3 = sin(-pitch * DEG2RAD_F);
    [(f1 * f2) as f64, f3 as f64, (f * f2) as f64]
}

/// Small deterministic RNG (xorshift64*), one per match.
#[derive(Clone, Debug)]
pub struct Rng(pub u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E3779B97F4A7C15) | 1)
    }
    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }
    /// Uniform in [0, 1).
    #[inline]
    pub fn f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
    #[inline]
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.f64()
    }
    #[inline]
    pub fn chance(&mut self, p: f64) -> bool {
        self.f64() < p
    }
    /// A standard normal number (the polar method, as `Random.nextGaussian`
    /// does it, without keeping the second value).
    pub fn gaussian(&mut self) -> f64 {
        loop {
            let v1 = 2.0 * self.f64() - 1.0;
            let v2 = 2.0 * self.f64() - 1.0;
            let s = v1 * v1 + v2 * v2;
            if s < 1.0 && s != 0.0 {
                return v1 * (-2.0 * s.ln() / s).sqrt();
            }
        }
    }
}
