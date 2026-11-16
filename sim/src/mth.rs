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
}
