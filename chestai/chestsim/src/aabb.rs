//! Port of 1.8.9 `AxisAlignedBB` and the `Vec3` helpers used for ray picks.

pub type V3 = [f64; 3];

#[inline]
pub fn dist(a: V3, b: V3) -> f64 {
    dist_sq(a, b).sqrt()
}

#[inline]
pub fn dist_sq(a: V3, b: V3) -> f64 {
    let (x, y, z) = (b[0] - a[0], b[1] - a[1], b[2] - a[2]);
    x * x + y * y + z * z
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min: V3,
    pub max: V3,
}

/// Player width 0.6F and height 1.8F, widened to double the way the game does.
pub const HALF_WIDTH: f64 = 0.3f32 as f64;
pub const HEIGHT: f64 = 1.8f32 as f64;
pub const EYE_HEIGHT: f64 = 1.62f32 as f64;

impl Aabb {
    pub fn new(min: V3, max: V3) -> Self {
        Aabb { min, max }
    }

    /// `Entity.setPosition` for a player.
    #[inline]
    pub fn player(p: V3) -> Self {
        Aabb {
            min: [p[0] - HALF_WIDTH, p[1], p[2] - HALF_WIDTH],
            max: [p[0] + HALF_WIDTH, p[1] + HEIGHT, p[2] + HALF_WIDTH],
        }
    }

    #[inline]
    pub fn offset(&self, x: f64, y: f64, z: f64) -> Self {
        Aabb {
            min: [self.min[0] + x, self.min[1] + y, self.min[2] + z],
            max: [self.max[0] + x, self.max[1] + y, self.max[2] + z],
        }
    }

    #[inline]
    pub fn expand(&self, x: f64, y: f64, z: f64) -> Self {
        Aabb {
            min: [self.min[0] - x, self.min[1] - y, self.min[2] - z],
            max: [self.max[0] + x, self.max[1] + y, self.max[2] + z],
        }
    }

    /// `contract(x, y, z)`.
    #[inline]
    pub fn contract(&self, x: f64, y: f64, z: f64) -> Self {
        Aabb {
            min: [self.min[0] + x, self.min[1] + y, self.min[2] + z],
            max: [self.max[0] - x, self.max[1] - y, self.max[2] - z],
        }
    }

    #[inline]
    pub fn add_coord(&self, x: f64, y: f64, z: f64) -> Self {
        let mut r = *self;
        if x < 0.0 { r.min[0] += x } else if x > 0.0 { r.max[0] += x }
        if y < 0.0 { r.min[1] += y } else if y > 0.0 { r.max[1] += y }
        if z < 0.0 { r.min[2] += z } else if z > 0.0 { r.max[2] += z }
        r
    }

    #[inline]
    pub fn intersects(&self, o: &Aabb) -> bool {
        o.max[0] > self.min[0] && o.min[0] < self.max[0]
            && o.max[1] > self.min[1] && o.min[1] < self.max[1]
            && o.max[2] > self.min[2] && o.min[2] < self.max[2]
    }

    /// `this.calculateXOffset(other, offset)`: `self` is the obstacle.
    #[inline]
    pub fn calc_x_offset(&self, o: &Aabb, mut off: f64) -> f64 {
        if o.max[1] > self.min[1] && o.min[1] < self.max[1] && o.max[2] > self.min[2] && o.min[2] < self.max[2] {
            if off > 0.0 && o.max[0] <= self.min[0] {
                let d = self.min[0] - o.max[0];
                if d < off { off = d }
            } else if off < 0.0 && o.min[0] >= self.max[0] {
                let d = self.max[0] - o.min[0];
                if d > off { off = d }
            }
        }
        off
    }

    #[inline]
    pub fn calc_y_offset(&self, o: &Aabb, mut off: f64) -> f64 {
        if o.max[0] > self.min[0] && o.min[0] < self.max[0] && o.max[2] > self.min[2] && o.min[2] < self.max[2] {
            if off > 0.0 && o.max[1] <= self.min[1] {
                let d = self.min[1] - o.max[1];
                if d < off { off = d }
            } else if off < 0.0 && o.min[1] >= self.max[1] {
                let d = self.max[1] - o.min[1];
                if d > off { off = d }
            }
        }
        off
    }

    #[inline]
    pub fn calc_z_offset(&self, o: &Aabb, mut off: f64) -> f64 {
        if o.max[0] > self.min[0] && o.min[0] < self.max[0] && o.max[1] > self.min[1] && o.min[1] < self.max[1] {
            if off > 0.0 && o.max[2] <= self.min[2] {
                let d = self.min[2] - o.max[2];
                if d < off { off = d }
            } else if off < 0.0 && o.min[2] >= self.max[2] {
                let d = self.max[2] - o.min[2];
                if d > off { off = d }
            }
        }
        off
    }

    #[inline]
    pub fn is_vec_inside(&self, v: V3) -> bool {
        v[0] > self.min[0] && v[0] < self.max[0]
            && v[1] > self.min[1] && v[1] < self.max[1]
            && v[2] > self.min[2] && v[2] < self.max[2]
    }

    /// `Vec3.getIntermediateWith{X,Y,Z}Value` for the given axis.
    #[inline]
    fn intermediate(a: V3, b: V3, axis: usize, value: f64) -> Option<V3> {
        let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        if d[axis] * d[axis] < 1.0000000116860974E-7 {
            return None;
        }
        let t = (value - a[axis]) / d[axis];
        if t >= 0.0 && t <= 1.0 {
            Some([a[0] + d[0] * t, a[1] + d[1] * t, a[2] + d[2] * t])
        } else {
            None
        }
    }

    #[inline]
    fn in_face(&self, v: V3, axis: usize) -> bool {
        let (u, w) = match axis { 0 => (1, 2), 1 => (0, 2), _ => (0, 1) };
        v[u] >= self.min[u] && v[u] <= self.max[u] && v[w] >= self.min[w] && v[w] <= self.max[w]
    }

    /// `calculateIntercept(a, b)`: closest point where segment a-b enters a face.
    pub fn calculate_intercept(&self, a: V3, b: V3) -> Option<V3> {
        let mut best: Option<V3> = None;
        let mut best_d = f64::INFINITY;
        for axis in 0..3 {
            for value in [self.min[axis], self.max[axis]] {
                if let Some(p) = Self::intermediate(a, b, axis, value) {
                    if self.in_face(p, axis) {
                        let d = dist_sq(a, p);
                        if best.is_none() || d < best_d {
                            best = Some(p);
                            best_d = d;
                        }
                    }
                }
            }
        }
        best
    }

    /// Closest point of the box to `p` (used for observations only).
    #[inline]
    pub fn closest_point(&self, p: V3) -> V3 {
        [
            p[0].clamp(self.min[0], self.max[0]),
            p[1].clamp(self.min[1], self.max[1]),
            p[2].clamp(self.min[2], self.max[2]),
        ]
    }

    #[inline]
    pub fn center(&self) -> V3 {
        [
            (self.min[0] + self.max[0]) * 0.5,
            (self.min[1] + self.max[1]) * 0.5,
            (self.min[2] + self.max[2]) * 0.5,
        ]
    }
}
