// 3D flight scene.
//
// COORDINATES: world is Z-up, pad at the origin, exactly as in telemetry. three.js is made
// Z-up globally in src/three-setup.ts (Object3D.DEFAULT_UP = +Z), so positions and the
// [x,y,z,w] body-to-world quaternion are applied without any axis remapping. Body +Z is the
// nose. The lander mesh origin is at its feet, so a reported z = 0 sits on the pad.
//
// Nothing here goes through React state per packet: useFrame reads store/telemetry.ts.

import { useEffect, useMemo, useRef, useState } from "react";
import { Canvas, useFrame, useThree } from "@react-three/fiber";
import { Html, OrbitControls } from "@react-three/drei";
import * as THREE from "three";
import type { OrbitControls as OrbitControlsImpl } from "three-stdlib";
import { tele } from "../../store/telemetry";
import { useUi } from "../../store/ui";
import { thrustDirBody } from "../../lib/math";
import { THRUST_MAX_N } from "../../protocol";

type CamMode = "follow" | "pad" | "top";

interface Palette {
  bg: string; grid: string; gridStrong: string; body: string; accent: string;
  ref: string; trail: string; truth: string; plume: string; pad: string;
}

const cssVar = (n: string) => getComputedStyle(document.documentElement).getPropertyValue(n).trim();
function readPalette(): Palette {
  return {
    bg: cssVar("--scene-bg"), grid: cssVar("--scene-grid"), gridStrong: cssVar("--scene-grid-strong"),
    body: cssVar("--scene-body"), accent: cssVar("--s2"), ref: cssVar("--s-ref"), trail: cssVar("--s1"),
    truth: cssVar("--s-truth"), plume: cssVar("--scene-plume"), pad: cssVar("--text-2"),
  };
}

const Z = new THREE.Vector3(0, 0, 1);
const PIVOT_Z = 0.55;
const X90 = Math.PI / 2;

/** Thin strut between two body-frame points. */
function Strut({ a, b, r, children }: { a: [number, number, number]; b: [number, number, number]; r: number; children: React.ReactNode }) {
  const { pos, quat, len } = useMemo(() => {
    const va = new THREE.Vector3(...a);
    const vb = new THREE.Vector3(...b);
    const d = vb.clone().sub(va);
    return {
      pos: va.clone().add(vb).multiplyScalar(0.5),
      quat: new THREE.Quaternion().setFromUnitVectors(new THREE.Vector3(0, 1, 0), d.clone().normalize()),
      len: d.length(),
    };
  }, [a, b]);
  return (
    <mesh position={pos} quaternion={quat}>
      <cylinderGeometry args={[r, r, len, 6]} />
      {children}
    </mesh>
  );
}

const LEGS: [number, number][] = [[1, 1], [-1, 1], [-1, -1], [1, -1]];

/** Lander: body cylinder, nose, four legs, gimballed engine bell. Feet at z = 0, nose at about 2.1 m. */
function LanderMesh({ color, accent, ghost, gimbalRef, children }: { color: string; accent: string; ghost?: boolean; gimbalRef?: React.Ref<THREE.Group>; children?: React.ReactNode }) {
  const mat = ghost
    ? <meshBasicMaterial color={color} wireframe transparent opacity={0.55} />
    : <meshStandardMaterial color={color} roughness={0.55} metalness={0.25} />;
  const k = Math.SQRT1_2;
  return (
    <group>
      <mesh position={[0, 0, 1.15]} rotation={[X90, 0, 0]}>
        <cylinderGeometry args={[0.24, 0.24, 1.1, ghost ? 10 : 24]} />
        {mat}
      </mesh>
      <mesh position={[0, 0, 1.9]} rotation={[X90, 0, 0]}>
        <coneGeometry args={[0.24, 0.4, ghost ? 10 : 24]} />
        {mat}
      </mesh>
      {!ghost && (
        <>
          {/* +X marker stripe so roll is readable */}
          <mesh position={[0.245, 0, 1.25]}>
            <boxGeometry args={[0.02, 0.12, 0.7]} />
            <meshBasicMaterial color={accent} />
          </mesh>
          <mesh position={[0, 0, 1.0]} rotation={[X90, 0, 0]}>
            <cylinderGeometry args={[0.25, 0.25, 0.05, 24]} />
            <meshStandardMaterial color="#3a424d" roughness={0.6} />
          </mesh>
        </>
      )}
      {LEGS.map(([sx, sy], i) => (
        <group key={i}>
          <Strut a={[0.2 * k * sx, 0.2 * k * sy, 1.0]} b={[0.85 * k * sx, 0.85 * k * sy, 0.03]} r={0.025}>{mat}</Strut>
          <Strut a={[0.2 * k * sx, 0.2 * k * sy, 0.62]} b={[0.85 * k * sx, 0.85 * k * sy, 0.03]} r={0.018}>{mat}</Strut>
          {!ghost && (
            <mesh position={[0.85 * k * sx, 0.85 * k * sy, 0.015]} rotation={[X90, 0, 0]}>
              <cylinderGeometry args={[0.08, 0.08, 0.03, 10]} />
              {mat}
            </mesh>
          )}
        </group>
      ))}
      <group ref={gimbalRef} position={[0, 0, PIVOT_Z]}>
        <mesh position={[0, 0, -0.15]} rotation={[X90, 0, 0]}>
          <cylinderGeometry args={[0.07, 0.17, 0.3, ghost ? 8 : 20, 1, true]} />
          {ghost ? mat : <meshStandardMaterial color="#59626e" roughness={0.4} metalness={0.6} side={THREE.DoubleSide} />}
        </mesh>
        {children}
      </group>
    </group>
  );
}

function makeLine(capacity: number, color: string, opacity = 1) {
  const geo = new THREE.BufferGeometry();
  geo.setAttribute("position", new THREE.BufferAttribute(new Float32Array(capacity * 3), 3));
  geo.setDrawRange(0, 0);
  const line = new THREE.Line(geo, new THREE.LineBasicMaterial({ color, transparent: opacity < 1, opacity }));
  line.frustumCulled = false;
  return line;
}

function World({ pal, mode, modeRev }: { pal: Palette; mode: CamMode; modeRev: number }) {
  const vehicle = useRef<THREE.Group>(null);
  const ghost = useRef<THREE.Group>(null);
  const gimbal = useRef<THREE.Group>(null);
  const plume = useRef<THREE.Group>(null);
  const shadow = useRef<THREE.Mesh>(null);
  const target = useRef<THREE.Group>(null);
  const tag = useRef<HTMLDivElement>(null);
  const controls = useRef<OrbitControlsImpl>(null);
  const { camera } = useThree();
  const hoverAlt = useUi((s) => s.params?.flight.hover_altitude_m ?? 50);

  const trail = useMemo(() => makeLine(tele.trail.length / 3, pal.trail), [pal.trail]);
  const traj = useMemo(() => makeLine(64, pal.ref, 0.9), [pal.ref]);
  const drop = useMemo(() => makeLine(2, pal.gridStrong), [pal.gridStrong]);
  const pole = useMemo(() => {
    const l = makeLine(2, pal.gridStrong);
    const p = l.geometry.getAttribute("position") as THREE.BufferAttribute;
    p.setXYZ(0, 0, 0, 0);
    p.setXYZ(1, 0, 0, hoverAlt);
    l.geometry.setDrawRange(0, 2);
    return l;
  }, [pal.gridStrong, hoverAlt]);
  useEffect(() => () => { for (const l of [trail, traj, drop, pole]) { l.geometry.dispose(); (l.material as THREE.Material).dispose(); } }, [trail, traj, drop, pole]);

  const seen = useRef({ trail: -1, traj: -1 });
  useEffect(() => { seen.current = { trail: -1, traj: -1 }; }, [trail, traj]);

  const tmp = useMemo(() => ({ v: new THREE.Vector3(), d: new THREE.Vector3(), q: new THREE.Quaternion() }), []);

  // Camera presets. Follow and top-down keep tracking the vehicle; the operator can still orbit.
  useEffect(() => {
    const c = controls.current;
    if (!c) return;
    const p = tele.flight?.position ?? [0, 0, 0];
    if (mode === "follow") {
      c.target.set(p[0], p[1], p[2] + 1);
      camera.position.set(p[0] + 5.5, p[1] - 8, p[2] + 3.2);
    } else if (mode === "top") {
      c.target.set(p[0], p[1], p[2]);
      camera.position.set(p[0], p[1] - 0.02, p[2] + 28);
    } else {
      c.target.set(0, 0, hoverAlt * 0.55);
      camera.position.set(hoverAlt * 1.2, -hoverAlt * 1.6, hoverAlt * 0.5);
    }
    c.update();
  }, [mode, modeRev, camera, hoverAlt]);

  useFrame((_s, dt) => {
    const f = tele.flight;
    const c = controls.current;
    if (vehicle.current) vehicle.current.visible = !!f;
    if (!f) return;

    const [x, y, z] = f.position;
    vehicle.current?.position.set(x, y, z);
    vehicle.current?.quaternion.set(f.attitude[0], f.attitude[1], f.attitude[2], f.attitude[3]).normalize();

    // Thrust direction in the body frame; bell and plume hang off the pivot pointing the other way.
    const t = thrustDirBody(f.gimbal_theta, f.gimbal_phi);
    tmp.d.set(t[0], t[1], t[2]);
    gimbal.current?.quaternion.setFromUnitVectors(Z, tmp.d);
    if (plume.current) {
      const len = Math.max(0, f.thrust / THRUST_MAX_N) * 3.2;
      plume.current.visible = len > 0.02;
      plume.current.scale.set(1, 1, Math.max(0.001, len));
    }

    if (ghost.current) {
      ghost.current.visible = !!f.truth;
      if (f.truth) {
        ghost.current.position.set(...f.truth.position);
        ghost.current.quaternion.set(...f.truth.attitude).normalize();
      }
    }

    shadow.current?.position.set(x, y, 0.02);
    const dp = drop.geometry.getAttribute("position") as THREE.BufferAttribute;
    dp.setXYZ(0, x, y, 0);
    dp.setXYZ(1, x, y, z);
    dp.needsUpdate = true;
    drop.geometry.setDrawRange(0, 2);

    if (seen.current.trail !== tele.trailRev) {
      seen.current.trail = tele.trailRev;
      const a = trail.geometry.getAttribute("position") as THREE.BufferAttribute;
      (a.array as Float32Array).set(tele.trail);
      a.needsUpdate = true;
      trail.geometry.setDrawRange(0, tele.trailLen);
    }
    if (seen.current.traj !== tele.trajectoryRev) {
      seen.current.traj = tele.trajectoryRev;
      const tr = tele.trajectory;
      const a = traj.geometry.getAttribute("position") as THREE.BufferAttribute;
      const n = tr ? Math.min(64, tr.positions.length) : 0;
      for (let i = 0; i < n; i++) a.setXYZ(i, ...tr!.positions[i]);
      a.needsUpdate = true;
      traj.geometry.setDrawRange(0, n);
      if (target.current) {
        target.current.visible = !!tr;
        if (tr) target.current.position.set(...tr.target);
      }
    }

    if (c && mode !== "pad") {
      const k = 1 - Math.exp(-dt * 10);
      tmp.v.set(x, y, z + (mode === "follow" ? 1 : 0)).sub(c.target).multiplyScalar(k);
      c.target.add(tmp.v);
      camera.position.add(tmp.v);
    }
    c?.update();

    if (tag.current) {
      const far = camera.position.distanceTo(vehicle.current!.position) > 28;
      tag.current.style.display = far ? "block" : "none";
      if (far) tag.current.firstElementChild!.textContent = `${z.toFixed(1)} m`;
    }
  });

  const ticks = useMemo(() => {
    const out: number[] = [];
    for (let h = 10; h < hoverAlt - 1; h += 10) out.push(h);
    return out;
  }, [hoverAlt]);

  return (
    <>
      <color attach="background" args={[pal.bg]} />
      <ambientLight intensity={1.1} />
      <directionalLight position={[30, -40, 80]} intensity={2.2} />
      <directionalLight position={[-20, 30, 10]} intensity={0.6} />

      {/* GridHelper lies in XZ; rotate it into the XY ground plane of the Z-up world. */}
      <gridHelper args={[400, 40, pal.gridStrong, pal.grid]} rotation={[X90, 0, 0]} />
      <gridHelper args={[40, 40, pal.grid, pal.grid]} rotation={[X90, 0, 0]} position={[0, 0, 0.005]} />

      {/* Pad marker at the origin */}
      <mesh position={[0, 0, 0.01]}>
        <ringGeometry args={[1.35, 1.5, 48]} />
        <meshBasicMaterial color={pal.pad} />
      </mesh>
      <mesh position={[0, 0, 0.01]}>
        <planeGeometry args={[2.0, 0.08]} />
        <meshBasicMaterial color={pal.pad} />
      </mesh>
      <mesh position={[0, 0, 0.01]}>
        <planeGeometry args={[0.08, 2.0]} />
        <meshBasicMaterial color={pal.pad} />
      </mesh>

      {/* Altitude reference: pole with 10 m ticks and a ring at the hover altitude */}
      <primitive object={pole} />
      {ticks.map((h) => (
        <group key={h} position={[0, 0, h]}>
          <mesh>
            <ringGeometry args={[0.5, 0.56, 24]} />
            <meshBasicMaterial color={pal.gridStrong} side={THREE.DoubleSide} />
          </mesh>
          <Html className="scene-label" position={[0.8, 0, 0]} zIndexRange={[5, 0]}>{h} m</Html>
        </group>
      ))}
      <group position={[0, 0, hoverAlt]}>
        <mesh>
          <ringGeometry args={[2.96, 3.0, 96]} />
          <meshBasicMaterial color={pal.ref} side={THREE.DoubleSide} />
        </mesh>
        <Html className="scene-label strong" position={[3.3, 0, 0]} zIndexRange={[5, 0]}>{hoverAlt.toFixed(0)} m hover</Html>
      </group>

      <primitive object={traj} />
      <primitive object={trail} />
      <primitive object={drop} />
      <group ref={target} visible={false}>
        <mesh>
          <octahedronGeometry args={[0.35]} />
          <meshBasicMaterial color={pal.ref} wireframe />
        </mesh>
        <Html className="scene-label" position={[0.6, 0, 0.3]} zIndexRange={[5, 0]}>target</Html>
      </group>

      <mesh ref={shadow}>
        <ringGeometry args={[0.55, 0.62, 32]} />
        <meshBasicMaterial color={pal.trail} transparent opacity={0.7} />
      </mesh>

      <group ref={vehicle} visible={false}>
        <LanderMesh color={pal.body} accent={pal.accent} gimbalRef={gimbal}>
          <Plume plumeRef={plume} color={pal.plume} />
        </LanderMesh>
        <Html position={[0, 0, 1]} center zIndexRange={[6, 0]} style={{ pointerEvents: "none" }}>
          <div className="scene-tag" ref={tag} style={{ display: "none" }}><span /></div>
        </Html>
      </group>

      <group ref={ghost} visible={false}>
        <LanderMesh color={pal.truth} accent={pal.truth} ghost />
      </group>

      <OrbitControls ref={controls} makeDefault enableDamping={false} minDistance={2} maxDistance={400} maxPolarAngle={Math.PI * 0.499} />
    </>
  );
}

/** Lives inside the gimbal group, so it swings with the bell. Scaled along Z by thrust. */
function Plume({ plumeRef, color }: { plumeRef: React.Ref<THREE.Group>; color: string }) {
  return (
    <group position={[0, 0, -0.3]}>
      <group ref={plumeRef} visible={false}>
        {/* Cone is built along +Y with its apex up; rotate so the apex points down -Z, base at the nozzle exit. */}
        <mesh position={[0, 0, -0.5]} rotation={[-X90, 0, 0]}>
          <coneGeometry args={[0.15, 1, 16, 1, true]} />
          <meshBasicMaterial color={color} transparent opacity={0.8} side={THREE.DoubleSide} depthWrite={false} />
        </mesh>
        <mesh position={[0, 0, -0.3]} rotation={[-X90, 0, 0]}>
          <coneGeometry args={[0.08, 0.6, 12, 1, true]} />
          <meshBasicMaterial color="#ffffff" transparent opacity={0.85} depthWrite={false} />
        </mesh>
      </group>
    </group>
  );
}

// Constant identity: R3F re-applies the camera prop when it changes, which would yank the view.
const CAMERA = { position: [5.5, -8, 4.2] as [number, number, number], fov: 45, near: 0.1, far: 2000, up: [0, 0, 1] as [number, number, number] };
const GL = { antialias: true, powerPreference: "high-performance" as const };
const DPR: [number, number] = [1, 2];

const MODES: { id: CamMode; label: string }[] = [
  { id: "follow", label: "Follow" },
  { id: "pad", label: "Pad" },
  { id: "top", label: "Top-down" },
];

export function Scene3D() {
  const theme = useUi((s) => s.theme);
  const hasFlight = useUi((s) => s.hasFlight);
  const [mode, setMode] = useState<CamMode>("follow");
  const [rev, setRev] = useState(0);
  const pal = useMemo(readPalette, [theme]);
  const hasTruth = useUi((s) => s.source === "Sim");
  const stale = useUi((s) => s.stale);

  // Snap the follow camera onto the vehicle when the first packet arrives.
  useEffect(() => { if (hasFlight) setRev((r) => r + 1); }, [hasFlight]);

  return (
    <section className="panel scene" aria-label="3D flight view">
      <Canvas
        camera={CAMERA}
        dpr={DPR}
        gl={GL}
      >
        <World pal={pal} mode={mode} modeRev={rev} />
      </Canvas>
      <div className="scene-ui">
        <div className="seg" role="group" aria-label="Camera">
          {MODES.map((m) => (
            <button key={m.id} className="seg-btn" aria-pressed={mode === m.id} onClick={() => { setMode(m.id); setRev((r) => r + 1); }}>
              {m.label}
            </button>
          ))}
        </div>
      </div>
      <ul className="scene-legend" aria-label="Scene legend">
        <li><span className="swatch" style={{ color: "var(--s1)" }} />flown path</li>
        <li><span className="swatch" style={{ color: "var(--s-ref)" }} />reference</li>
        {hasTruth && <li><span className="swatch" style={{ color: "var(--s-truth)" }} />sim truth (wire)</li>}
        <li className="axes-note">Z up, pad at origin</li>
      </ul>
      {!hasFlight && <div className="scene-wait">Waiting for telemetry</div>}
      {stale && <div className="scene-stale">STALE: showing last received state</div>}
    </section>
  );
}
