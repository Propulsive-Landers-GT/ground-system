//! Kinematics for the mock vehicle. This is a test fixture, not a simulator: the body
//! follows a reference path with a PD law, and attitude, gimbal and thrust are derived
//! from the resulting acceleration so the numbers look coherent on the UI.

use gs_protocol::TrajectoryMsg;

pub type Vec3 = [f64; 3];

pub const G: f64 = 9.81;
pub const WET_MASS_KG: f64 = 78.0;
pub const DRY_MASS_KG: f64 = 58.0;
pub const MAX_THRUST_N: f64 = 1200.0;
pub const MAX_GIMBAL_RAD: f64 = 15.0 * std::f64::consts::PI / 180.0;

/// Effective exhaust velocity of a monopropellant engine (Isp ~160 s).
const EXHAUST_VELOCITY: f64 = 160.0 * G;
/// PD tracking gains: natural frequency 2 rad/s, critically damped.
const KP: f64 = 4.0;
const KD: f64 = 4.0;
const MAX_LATERAL_ACCEL: f64 = 3.0;
const MAX_VERTICAL_ACCEL: f64 = 4.0;
/// Gimbal deflection per g of lateral acceleration.
const GIMBAL_PER_G: f64 = 0.6;
const RCS_YAW_ACCEL: f64 = 0.15;
const RCS_DEADBAND: f64 = 0.03;
const TRAJECTORY_NODES: usize = 32;

/// Straight-line minimum-jerk path: starts and ends at rest.
#[derive(Debug, Clone)]
pub struct Plan {
    start: Vec3,
    end: Vec3,
    start_time_s: f64,
    time_of_flight_s: f64,
}

pub struct Reference {
    pub position: Vec3,
    pub velocity: Vec3,
    pub acceleration: Vec3,
}

impl Plan {
    pub fn new(start: Vec3, end: Vec3, start_time_s: f64, time_of_flight_s: f64) -> Self {
        Self {
            start,
            end,
            start_time_s,
            time_of_flight_s: time_of_flight_s.max(0.1),
        }
    }

    /// Picks a time of flight that keeps peak acceleration around 1.5 m/s².
    pub fn between(start: Vec3, end: Vec3, start_time_s: f64) -> Self {
        let distance = norm(sub(end, start));
        Self::new(start, end, start_time_s, (4.0 * distance).sqrt().max(4.0))
    }

    /// Holds a point for `duration_s`.
    pub fn hold(point: Vec3, start_time_s: f64, duration_s: f64) -> Self {
        Self::new(point, point, start_time_s, duration_s)
    }

    pub fn is_finished(&self, time_s: f64) -> bool {
        time_s >= self.start_time_s + self.time_of_flight_s
    }

    pub fn reference(&self, time_s: f64) -> Reference {
        let tof = self.time_of_flight_s;
        let tau = ((time_s - self.start_time_s) / tof).clamp(0.0, 1.0);
        let (t2, t3) = (tau * tau, tau * tau * tau);
        let s = 10.0 * t3 - 15.0 * t3 * tau + 6.0 * t3 * t2;
        let ds = (30.0 * t2 - 60.0 * t3 + 30.0 * t2 * t2) / tof;
        let dds = (60.0 * tau - 180.0 * t2 + 120.0 * t3) / (tof * tof);
        let delta = sub(self.end, self.start);
        Reference {
            position: add(self.start, scale(delta, s)),
            velocity: scale(delta, ds),
            acceleration: scale(delta, dds),
        }
    }

    /// The part of the path still ahead, which is what guidance would re-send.
    pub fn to_msg(&self, time_s: f64) -> TrajectoryMsg {
        let end_time_s = self.start_time_s + self.time_of_flight_s;
        let remaining_s = (end_time_s - time_s).max(0.0);
        let positions: Vec<Vec3> = (0..TRAJECTORY_NODES)
            .map(|i| {
                let fraction = i as f64 / (TRAJECTORY_NODES - 1) as f64;
                self.reference(time_s + fraction * remaining_s).position
            })
            .collect();
        TrajectoryMsg::from_positions(time_s, remaining_s, &positions, self.end)
    }
}

/// What the flight computer is commanding the hardware to do.
#[derive(Debug, Clone, Copy, Default)]
pub struct Actuation {
    pub gimbal_theta: f64,
    pub gimbal_phi: f64,
    pub thrust: f64,
    pub rcs: i8,
}

#[derive(Debug, Clone)]
pub struct Body {
    pub position: Vec3,
    pub velocity: Vec3,
    /// Body-to-world rotation vector (axis * angle). Tilt stays small, so x/y are
    /// effectively roll/pitch and z is yaw.
    pub rotation: Vec3,
    pub angular_velocity: Vec3,
    pub mass: f64,
}

impl Body {
    pub fn at_rest(position: Vec3) -> Self {
        Self {
            position,
            velocity: [0.0; 3],
            rotation: [0.0; 3],
            angular_velocity: [0.0; 3],
            mass: WET_MASS_KG,
        }
    }

    pub fn on_ground(&self) -> bool {
        self.position[2] <= 0.0
    }

    pub fn tilt_deg(&self) -> f64 {
        self.rotation[0].hypot(self.rotation[1]).to_degrees()
    }

    pub fn attitude(&self) -> [f64; 4] {
        quaternion_from_rotation_vector(self.rotation)
    }

    pub fn propellant_used_fraction(&self) -> f64 {
        ((WET_MASS_KG - self.mass) / (WET_MASS_KG - DRY_MASS_KG)).clamp(0.0, 1.0)
    }

    /// One step of powered flight along `reference`. Returns the actuation that
    /// "produced" the motion.
    pub fn fly(&mut self, reference: &Reference, time_s: f64, dt: f64) -> Actuation {
        let gust = gust_acceleration(time_s);
        let limits = [MAX_LATERAL_ACCEL, MAX_LATERAL_ACCEL, MAX_VERTICAL_ACCEL];
        let mut accel = [0.0; 3];
        for i in 0..3 {
            let pd = KP * (reference.position[i] - self.position[i])
                + KD * (reference.velocity[i] - self.velocity[i]);
            accel[i] = (reference.acceleration[i] + pd).clamp(-limits[i], limits[i]);
        }

        for i in 0..3 {
            self.velocity[i] += (accel[i] + gust[i]) * dt;
            self.position[i] += self.velocity[i] * dt;
        }
        self.stop_at_ground();

        // The thrust axis points along the specific force; add a little structural wobble.
        let specific_force = [accel[0], accel[1], accel[2] + G];
        let wobble = 0.005 * (4.4 * time_s).sin();
        let rcs = self.rcs_command();
        let yaw_accel = 0.02 * (0.3 * time_s).sin() - RCS_YAW_ACCEL * f64::from(rcs);
        let yaw_rate = self.angular_velocity[2] + yaw_accel * dt;
        let rotation = [
            -specific_force[1] / specific_force[2] + wobble,
            specific_force[0] / specific_force[2] + 0.7 * wobble,
            self.rotation[2] + yaw_rate * dt,
        ];
        self.angular_velocity = [
            (rotation[0] - self.rotation[0]) / dt,
            (rotation[1] - self.rotation[1]) / dt,
            yaw_rate,
        ];
        self.rotation = rotation;

        let thrust = (self.mass * norm(specific_force)).clamp(0.0, MAX_THRUST_N);
        self.burn(thrust, dt);

        Actuation {
            gimbal_theta: (GIMBAL_PER_G * accel[0] / G + 0.4 * wobble)
                .clamp(-MAX_GIMBAL_RAD, MAX_GIMBAL_RAD),
            gimbal_phi: (GIMBAL_PER_G * accel[1] / G - 0.4 * wobble)
                .clamp(-MAX_GIMBAL_RAD, MAX_GIMBAL_RAD),
            thrust,
            rcs,
        }
    }

    /// Unpowered: ballistic until the ground, then at rest.
    pub fn fall(&mut self, dt: f64) {
        if self.on_ground() {
            self.velocity = [0.0; 3];
        } else {
            self.velocity[2] -= G * dt;
            for i in 0..3 {
                self.position[i] += self.velocity[i] * dt;
            }
            self.stop_at_ground();
        }
        self.angular_velocity = [0.0; 3];
    }

    /// Propellant consumed by firing at `thrust` for `dt` (also used for jog firings).
    pub fn burn(&mut self, thrust: f64, dt: f64) {
        self.mass = (self.mass - thrust / EXHAUST_VELOCITY * dt).max(DRY_MASS_KG);
    }

    /// Bang-bang roll control with a deadband. +1 fires the CW thruster (negative yaw).
    fn rcs_command(&self) -> i8 {
        let error = self.rotation[2] + 1.5 * self.angular_velocity[2];
        if error > RCS_DEADBAND {
            1
        } else if error < -RCS_DEADBAND {
            -1
        } else {
            0
        }
    }

    fn stop_at_ground(&mut self) {
        if self.position[2] < 0.0 {
            self.position[2] = 0.0;
            self.velocity = [0.0; 3];
        }
    }
}

/// Slow pseudo-wind so the vehicle wanders a little around its reference.
fn gust_acceleration(time_s: f64) -> Vec3 {
    [
        0.25 * (0.50 * time_s).sin() + 0.10 * (1.7 * time_s).sin(),
        0.25 * (0.37 * time_s + 1.0).sin() + 0.10 * (2.1 * time_s).sin(),
        0.10 * (0.80 * time_s + 2.0).sin(),
    ]
}

/// `[x, y, z, w]`, body-to-world.
pub fn quaternion_from_rotation_vector(rotation: Vec3) -> [f64; 4] {
    let angle = norm(rotation);
    if angle < 1e-9 {
        return [0.0, 0.0, 0.0, 1.0];
    }
    let k = (angle / 2.0).sin() / angle;
    [
        rotation[0] * k,
        rotation[1] * k,
        rotation[2] * k,
        (angle / 2.0).cos(),
    ]
}

/// Small deterministic noise source (xorshift), so the mock needs no RNG dependency.
pub struct Noise(u64);

impl Noise {
    pub fn new() -> Self {
        Self(0x9E37_79B9_7F4A_7C15)
    }

    /// Uniform in `[-amplitude, amplitude]`.
    pub fn sample(&mut self, amplitude: f64) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        let unit = (self.0 >> 11) as f64 / (1u64 << 53) as f64;
        (2.0 * unit - 1.0) * amplitude
    }

    pub fn vec3(&mut self, amplitude: f64) -> Vec3 {
        [
            self.sample(amplitude),
            self.sample(amplitude),
            self.sample(amplitude),
        ]
    }
}

pub fn add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub fn scale(a: Vec3, k: f64) -> Vec3 {
    [a[0] * k, a[1] * k, a[2] * k]
}

pub fn norm(a: Vec3) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_starts_and_ends_at_rest() {
        let plan = Plan::between([4.0, -3.0, 0.0], [4.0, -3.0, 30.0], 10.0);
        let start = plan.reference(10.0);
        assert_eq!(start.position, [4.0, -3.0, 0.0]);
        assert_eq!(start.velocity, [0.0; 3]);

        let end = plan.reference(1000.0);
        assert!(norm(sub(end.position, [4.0, -3.0, 30.0])) < 1e-9);
        assert!(norm(end.velocity) < 1e-9);
        assert!(plan.is_finished(1000.0));
    }

    #[test]
    fn body_tracks_a_plan_to_its_end() {
        let plan = Plan::between([0.0; 3], [0.0, 0.0, 20.0], 0.0);
        let mut body = Body::at_rest([0.0; 3]);
        let dt = 0.02;
        let mut t = 0.0;
        let mut peak_thrust: f64 = 0.0;
        while t < 20.0 {
            let actuation = body.fly(&plan.reference(t), t, dt);
            peak_thrust = peak_thrust.max(actuation.thrust);
            t += dt;
        }
        assert!((body.position[2] - 20.0).abs() < 0.3, "{:?}", body.position);
        assert!(body.tilt_deg() < 5.0);
        assert!(body.mass < WET_MASS_KG);
        assert!(peak_thrust > WET_MASS_KG * G * 0.9 && peak_thrust <= MAX_THRUST_N);
    }
}
