// Shared three.js scene constants + helpers for turning an STL into a cover PNG.
// The constants mirror the live viewer (`components/StlViewer.tsx`) so the WYSIWYG
// "capture current view" path and the offscreen "auto-generate" path look alike.

import * as THREE from "three";
import { STLLoader } from "three/examples/jsm/loaders/STLLoader.js";

/** Camera direction (also the default viewer position) and field of view. */
export const VIEWER_CAMERA_DIR: [number, number, number] = [80, 80, 120];
export const VIEWER_CAMERA_FOV = 45;

/** Material used for the rendered mesh (matches the viewer's purple). */
export const VIEWER_MATERIAL = { color: "#a78bfa", metalness: 0.1, roughness: 0.6 };

/** Margin around the model when framing, matching the viewer's `<Bounds margin>`. */
const FIT_MARGIN = 1.4;

/** Read a canvas as PNG bytes, shaped as `number[]` for the `Vec<u8>` IPC param. */
export function canvasToPngBytes(canvas: HTMLCanvasElement): number[] {
  const dataUrl = canvas.toDataURL("image/png");
  const b64 = dataUrl.slice(dataUrl.indexOf(",") + 1);
  const bin = atob(b64);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return Array.from(bytes);
}

/**
 * Render STL bytes to a clean, fixed-angle PNG off-screen (no grid/gizmo). Used by
 * the one-click "set as cover" action. Background is transparent. Disposes all GPU
 * resources before returning.
 */
export function renderStlToPng(bytes: number[], size = 512): number[] {
  const geom = new STLLoader().parse(new Uint8Array(bytes).buffer);
  geom.computeBoundingBox();
  geom.computeVertexNormals();

  const scene = new THREE.Scene();
  scene.add(new THREE.AmbientLight(0xffffff, 0.55));
  const key = new THREE.DirectionalLight(0xffffff, 0.9);
  key.position.set(100, 100, 100);
  scene.add(key);
  const fill = new THREE.DirectionalLight(0xffffff, 0.4);
  fill.position.set(-100, -50, -100);
  scene.add(fill);

  const material = new THREE.MeshStandardMaterial({
    color: VIEWER_MATERIAL.color,
    metalness: VIEWER_MATERIAL.metalness,
    roughness: VIEWER_MATERIAL.roughness,
  });
  const mesh = new THREE.Mesh(geom, material);
  scene.add(mesh);

  // Frame the bounding sphere from the shared camera direction.
  const sphere = new THREE.Sphere();
  geom.boundingBox!.getBoundingSphere(sphere);
  const radius = sphere.radius || 1;
  const camera = new THREE.PerspectiveCamera(VIEWER_CAMERA_FOV, 1, radius * 0.01, radius * 100);
  const dir = new THREE.Vector3(...VIEWER_CAMERA_DIR).normalize();
  const dist = (radius * FIT_MARGIN) / Math.sin((VIEWER_CAMERA_FOV * Math.PI) / 360);
  camera.position.copy(sphere.center.clone().add(dir.multiplyScalar(dist)));
  camera.lookAt(sphere.center);

  const renderer = new THREE.WebGLRenderer({
    preserveDrawingBuffer: true,
    alpha: true,
    antialias: true,
  });
  try {
    renderer.setSize(size, size);
    renderer.setClearColor(0x000000, 0); // transparent background
    renderer.render(scene, camera);
    return canvasToPngBytes(renderer.domElement);
  } finally {
    geom.dispose();
    material.dispose();
    renderer.dispose();
    renderer.forceContextLoss();
  }
}
