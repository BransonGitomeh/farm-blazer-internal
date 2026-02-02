
pub struct WorldNoise {
    seed: u32,
}

impl WorldNoise {
    pub fn new(seed: u32) -> Self {
        Self { seed }
    }

    #[inline(always)]
    fn hash(&self, x: i32, y: i32) -> f32 {
        let mut h = (x as u32).wrapping_mul(374761393).wrapping_add(self.seed);
        h = h.wrapping_add((y as u32).wrapping_mul(668265263));
        h = (h ^ (h >> 13)).wrapping_mul(1274126177);
        h = h ^ (h >> 16);
        (h as f32) / (u32::MAX as f32)
    }

    /// Returns a smooth noise value between 0.0 and 1.0
    pub fn get_noise(&self, x: f32, y: f32) -> f32 {
        let x_floor = x.floor() as i32;
        let y_floor = y.floor() as i32;

        let u = x - x.floor();
        let v = y - y.floor();

        // Smoothstep
        let su = u * u * (3.0 - 2.0 * u);
        let sv = v * v * (3.0 - 2.0 * v);

        let bl = self.hash(x_floor, y_floor);
        let br = self.hash(x_floor + 1, y_floor);
        let tl = self.hash(x_floor, y_floor + 1);
        let tr = self.hash(x_floor + 1, y_floor + 1);

        let b = bl + su * (br - bl);
        let t = tl + su * (tr - tl);

        b + sv * (t - b)
    }

    /// Standard Fractal Brownian Motion
    /// Returns 0.0 - 1.0 (approx)
    pub fn fbm(&self, x: f32, y: f32, octaves: usize, persistence: f32, scale: f32) -> f32 {
        let mut total = 0.0;
        let mut frequency = scale;
        let mut amplitude = 1.0;
        let mut max_val = 0.0;

        for _ in 0..octaves {
            total += self.get_noise(x * frequency, y * frequency) * amplitude;
            max_val += amplitude;
            
            amplitude *= persistence;
            frequency *= 2.0;
        }

        total / max_val
    }
}
