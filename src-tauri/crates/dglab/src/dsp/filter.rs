
//TODO : dealta time duration time = time
pub struct OneEuroFilter {
    min_cutoff: f32,
    beta: f32,
    d_cutoff: f32,

    prev_x: f32,
    prev_dx: f32,
    prev_filtered: f32,
    prev_time: f32,
}

impl OneEuroFilter {
    pub fn new(min_cutoff: f32, beta: f32, d_cutoff: f32) -> Self {
        Self {
            min_cutoff,
            beta,
            d_cutoff,
            prev_x: 0.0,
            prev_dx: 0.0,
            prev_filtered: 0.0,
            prev_time: 0.0,
        }
    }

    fn alpha(dt: f32, cutoff: f32) -> f32 {
        let tau = 1.0 / (2.0 * std::f32::consts::PI * cutoff);
        1.0 / (1.0 + tau / dt)
    }

    fn lowpass(prev: f32, value: f32, alpha: f32) -> f32 {
        prev + alpha * (value - prev)
    }

    pub fn update(&mut self, time: f32, x: f32) -> f32 {
        if self.prev_time == 0.0 {
            self.prev_time = time;
            self.prev_x = x;
            self.prev_filtered = x;
            return x;
        }

        let dt = time - self.prev_time;
        self.prev_time = time;

        if dt <= 0.0 {
            return self.prev_filtered;
        }

        let dx = (x - self.prev_x) / dt;
        self.prev_x = x;

        let alpha_d = Self::alpha(dt, self.d_cutoff);
        let edx = Self::lowpass(self.prev_dx, dx, alpha_d);
        self.prev_dx = edx;

        let cutoff = self.min_cutoff + self.beta * edx.abs();

        let alpha = Self::alpha(dt, cutoff);
        let filtered = Self::lowpass(self.prev_filtered, x, alpha);

        self.prev_filtered = filtered;

        filtered
    }
}

pub struct MotionEstimator {
    filter: OneEuroFilter,

    prev_distance: f32,
    prev_velocity: f32,
    prev_acceleration: f32,
    prev_time: f32,
}

impl MotionEstimator {
    pub fn new(filter: OneEuroFilter) -> Self {
        Self {
            filter,
            prev_distance: 0.0,
            prev_velocity: 0.0,
            prev_acceleration: 0.0,
            prev_time: 0.0,
        }
    }

    pub fn update(&mut self, time: f32, distance: f32) -> (f32, f32, f32, f32) {
        let filtered = self.filter.update(time, distance);

        if self.prev_time == 0.0 {
            self.prev_time = time;
            self.prev_distance = filtered;
            return (filtered, 0.0, 0.0, 0.0);
        }

        let dt = time - self.prev_time;
        self.prev_time = time;

        if dt <= 0.0 {
            return (filtered, 0.0, 0.0, 0.0);
        }

        let velocity = (filtered - self.prev_distance) / dt;
        let acceleration = (velocity - self.prev_velocity) / dt;
        let jerk = (acceleration - self.prev_acceleration) / dt;

        self.prev_distance = filtered;
        self.prev_velocity = velocity;
        self.prev_acceleration = acceleration;

        (filtered, velocity, acceleration, jerk)
    }
}
