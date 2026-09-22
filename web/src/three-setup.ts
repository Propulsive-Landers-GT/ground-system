// COORDINATES: the vehicle world frame is Z-up (pad at origin). three.js defaults to Y-up.
// We make three.js Z-up globally instead of remapping data: DEFAULT_UP is set before any
// camera or object is created, so world coordinates from telemetry are used verbatim
// (x, y, z) everywhere in the scene. Geometries that three builds along +Y (cylinders,
// cones) are rotated +90° about X once, inside the mesh, to lie along +Z.
import { Object3D } from "three";

Object3D.DEFAULT_UP.set(0, 0, 1);
