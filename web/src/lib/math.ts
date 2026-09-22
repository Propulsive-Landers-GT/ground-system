import type { Quat, Vec3 } from "../protocol";

const DEG = 180 / Math.PI;

/** ZYX (yaw-pitch-roll) Euler angles in degrees from a body-to-world [x,y,z,w] quaternion. */
export function quatToEulerDeg(q: Quat): Vec3 {
  const [x, y, z, w] = q;
  const roll = Math.atan2(2 * (w * x + y * z), 1 - 2 * (x * x + y * y));
  const s = Math.max(-1, Math.min(1, 2 * (w * y - z * x)));
  const pitch = Math.asin(s);
  const yaw = Math.atan2(2 * (w * z + x * y), 1 - 2 * (y * y + z * z));
  return [roll * DEG, pitch * DEG, yaw * DEG];
}

/** Thrust direction in the body frame for gimbal angles (rad). The plume points the opposite way. */
export function thrustDirBody(theta: number, phi: number): Vec3 {
  return [Math.sin(theta) * Math.cos(phi), Math.sin(phi), Math.cos(theta) * Math.cos(phi)];
}

export const norm3 = (v: Vec3) => Math.hypot(v[0], v[1], v[2]);
export const clamp = (v: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, v));
export const rad2deg = (r: number) => r * DEG;
export const deg2rad = (d: number) => d / DEG;
