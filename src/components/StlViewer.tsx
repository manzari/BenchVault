import { Canvas, useThree } from "@react-three/fiber";
import { OrbitControls, GizmoHelper, GizmoViewport, Grid, Bounds } from "@react-three/drei";
import { Suspense, useEffect, useRef, useState } from "react";
import type { MutableRefObject } from "react";
import { notifications } from "@mantine/notifications";
import * as THREE from "three";
import { STLLoader } from "three/examples/jsm/loaders/STLLoader.js";
import {
  ActionIcon,
  Collapse,
  Group,
  Loader,
  Paper,
  Stack,
  Text,
  Tooltip,
  UnstyledButton,
} from "@mantine/core";
import {
  IconChevronDown,
  IconChevronUp,
  IconBoxMultiple,
  IconFocusCentered,
  IconMaximize,
  IconMinimize,
  IconPalette,
  IconBox,
  IconBoxAlignTopLeft,
  IconCamera,
} from "@tabler/icons-react";
import { api } from "../lib/api";
import { withErrorToast } from "../lib/errors";
import { canvasToPngBytes } from "../lib/stlRender";
import type { AttachedAsset } from "../lib/types";
import { useUiStore } from "../lib/stores/ui";

interface Props {
  parts: AttachedAsset[];
  objectId: number;
  /** Called after the cover is successfully updated from a captured view. */
  onCoverChanged?: () => void;
  /** Bump `nonce` to select `id` in the viewer (e.g. from the SCAD list). */
  previewRequest?: { id: number; nonce: number } | null;
}

/**
 * Lives inside the <Canvas> so it can reach the WebGL renderer. Publishes a capture
 * function (render + read PNG bytes) through `apiRef` for the toolbar button to call.
 */
function CaptureBridge({ apiRef }: { apiRef: MutableRefObject<(() => number[]) | null> }) {
  const { gl, scene, camera } = useThree();
  useEffect(() => {
    apiRef.current = () => {
      gl.render(scene, camera);
      return canvasToPngBytes(gl.domElement);
    };
    return () => {
      apiRef.current = null;
    };
  }, [gl, scene, camera, apiRef]);
  return null;
}

interface CachedPart {
  id: number;
  name: string;
  geometry: THREE.BufferGeometry;
}

function formatBytes(n: number): string {
  if (n >= 1024 * 1024 * 1024) return `${(n / (1024 * 1024 * 1024)).toFixed(1)} GB`;
  if (n >= 1024 * 1024) return `${Math.round(n / (1024 * 1024))} MB`;
  if (n >= 1024) return `${Math.round(n / 1024)} KB`;
  return `${n} B`;
}

export function StlViewer({ parts, objectId, onCoverChanged, previewRequest }: Props) {
  const [cache, setCache] = useState<Map<number, CachedPart>>(() => new Map());
  const [selectedId, setSelectedId] = useState<number | null>(parts[0]?.asset.id ?? null);
  const [loading, setLoading] = useState(false);
  // Per-part preview failures (e.g. an OpenSCAD render error) and post-render
  // oversize flags (a .scad's source size tells us nothing about the mesh size).
  const [errors, setErrors] = useState<Map<number, string>>(() => new Map());
  const [oversized, setOversized] = useState<Set<number>>(() => new Set());
  const [fitTrigger, setFitTrigger] = useState(0);
  const [partsPanelOpen, setPartsPanelOpen] = useState(false);
  const [isFullscreen, setIsFullscreen] = useState(false);
  const [capturing, setCapturing] = useState(false);
  const wrapperRef = useRef<HTMLDivElement>(null);
  const captureRef = useRef<(() => number[]) | null>(null);

  const wireframe = useUiStore((s) => s.viewerWireframe);
  const setWireframe = useUiStore((s) => s.setViewerWireframe);
  const background = useUiStore((s) => s.viewerBackground);
  const cycleBackground = useUiStore((s) => s.cycleViewerBackground);
  const maxPreviewBytes = useUiStore((s) => s.stlPreviewMaxBytes);

  const partsKey = parts.map((p) => p.asset.id).join(",");

  // Mirror the cache in a ref so the unmount-disposal effect (which runs with []
  // deps) sees the latest Map rather than the empty one from the first render.
  const cacheRef = useRef(cache);
  cacheRef.current = cache;

  // Keep selection valid when the parts list changes; drop cached geometries for parts that disappeared.
  useEffect(() => {
    const ids = new Set(parts.map((p) => p.asset.id));
    setCache((prev) => {
      let changed = false;
      const next = new Map(prev);
      for (const [id, p] of prev) {
        if (!ids.has(id)) {
          p.geometry.dispose();
          next.delete(id);
          changed = true;
        }
      }
      return changed ? next : prev;
    });
    if (parts.length === 0) {
      setSelectedId(null);
    } else if (selectedId == null || !ids.has(selectedId)) {
      setSelectedId(parts[0].asset.id);
    }
  }, [partsKey]);

  // Lazy-load the selected STL on demand. Cached geometries persist so switching back is instant.
  useEffect(() => {
    // Clear any stale spinner from a superseded load on every non-loading path,
    // otherwise switching to a cached/oversized part leaves it spinning forever.
    if (selectedId == null) { setLoading(false); return; }
    if (cache.has(selectedId)) { setLoading(false); return; }
    const target = parts.find((p) => p.asset.id === selectedId);
    if (!target) { setLoading(false); return; }
    const isScad = target.asset.kind === "scad";
    // Refuse oversized meshes up front — loading them into WebGL OOMs the webview.
    // A .scad's on-disk size is the tiny source, so we can't guard until after
    // the render; that check happens below on the returned bytes.
    if (!isScad && (target.asset.size_bytes ?? 0) > maxPreviewBytes) { setLoading(false); return; }
    let cancelled = false;
    setLoading(true);
    setErrors((prev) => {
      if (!prev.has(selectedId)) return prev;
      const next = new Map(prev);
      next.delete(selectedId);
      return next;
    });
    setOversized((prev) => {
      if (!prev.has(selectedId)) return prev;
      const next = new Set(prev);
      next.delete(selectedId);
      return next;
    });
    (async () => {
      try {
        const bytes = isScad
          ? await api.renderScadPreview(selectedId)
          : await api.readAssetBytes(selectedId);
        if (cancelled) return;
        // Post-render size guard (the .scad source size told us nothing).
        if (bytes.length > maxPreviewBytes) {
          setOversized((prev) => new Set(prev).add(selectedId));
          return;
        }
        const buf = new Uint8Array(bytes).buffer;
        const geom = new STLLoader().parse(buf);
        geom.computeBoundingBox();
        geom.computeVertexNormals();
        if (cancelled) {
          geom.dispose();
          return;
        }
        setCache((prev) => {
          if (prev.has(selectedId)) {
            geom.dispose();
            return prev;
          }
          const next = new Map(prev);
          next.set(selectedId, {
            id: selectedId,
            name: target.asset.original_filename,
            geometry: geom,
          });
          return next;
        });
      } catch (e) {
        if (!cancelled) {
          console.error("Preview load failed", e);
          const msg = (e as { message?: string })?.message ?? String(e);
          setErrors((prev) => new Map(prev).set(selectedId, msg));
        }
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [selectedId, partsKey, maxPreviewBytes]);

  // Dispose all cached geometries on unmount.
  useEffect(() => {
    return () => {
      cacheRef.current.forEach((p) => p.geometry.dispose());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Track fullscreen state changes
  useEffect(() => {
    const onFs = () => setIsFullscreen(document.fullscreenElement === wrapperRef.current);
    document.addEventListener("fullscreenchange", onFs);
    return () => document.removeEventListener("fullscreenchange", onFs);
  }, []);

  // External "preview this part" request (e.g. from the SCAD list): select it and
  // scroll the viewer into view. Keyed on the nonce so repeat clicks re-fire.
  useEffect(() => {
    if (previewRequest == null) return;
    if (!parts.some((p) => p.asset.id === previewRequest.id)) return;
    setSelectedId(previewRequest.id);
    wrapperRef.current?.scrollIntoView({ behavior: "smooth", block: "nearest" });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [previewRequest?.nonce]);

  const selected = selectedId != null ? cache.get(selectedId) ?? null : null;
  const selectedIndex = parts.findIndex((p) => p.asset.id === selectedId);
  const selectedMeta = selectedIndex >= 0 ? parts[selectedIndex] : null;
  const selectedSize = selectedMeta?.asset.size_bytes ?? 0;
  const isScadSelected = selectedMeta?.asset.kind === "scad";
  const selectedError = selectedId != null ? errors.get(selectedId) ?? null : null;
  const selectedOversized = selectedId != null && oversized.has(selectedId);
  const selectedTooLarge =
    selectedOversized || (selectedMeta != null && !isScadSelected && selectedSize > maxPreviewBytes);

  const toggleFullscreen = () => {
    if (!document.fullscreenElement) wrapperRef.current?.requestFullscreen?.();
    else document.exitFullscreen();
  };

  const onFit = () => setFitTrigger((n) => n + 1);

  // Capture the current framing as the object's cover. Hide the grid + gizmo first so
  // the image is a clean render of just the model, then read the canvas as a PNG.
  const captureCover = async () => {
    if (selectedId == null || capturing) return;
    setCapturing(true);
    let png: number[] | null = null;
    try {
      // Let the helpers unmount and a clean frame paint before reading the buffer.
      await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
      png = captureRef.current?.() ?? null;
    } finally {
      setCapturing(false);
    }
    if (!png) return;
    const name = `cover-${selectedMeta?.asset.original_filename ?? "render"}.png`;
    const res = await withErrorToast("Failed to set cover", () =>
      api.setCoverFromRender(objectId, png!, name),
    );
    if (res.ok) {
      notifications.show({ color: "green", title: "Cover updated", message: "Saved the current view as the cover." });
      onCoverChanged?.();
    }
  };

  const showGrid = background !== "solid";
  const isDarkBg = background !== "light-grid";

  // Re-fit Bounds whenever the selected part changes or the fit button is clicked.
  const boundsKey = `${fitTrigger}:${selectedId ?? "none"}`;

  const bgIcons: Record<typeof background, JSX.Element> = {
    "dark-grid": <IconPalette size={16} />,
    "light-grid": <IconBoxAlignTopLeft size={16} />,
    "solid": <IconBox size={16} />,
  };
  const bgLabel: Record<typeof background, string> = {
    "dark-grid": "Background: dark grid",
    "light-grid": "Background: light grid",
    "solid": "Background: solid",
  };

  return (
    <Stack gap="sm" style={{ width: "100%", minWidth: 0 }}>
      <div
        ref={wrapperRef}
        style={{
          width: "100%",
          minWidth: 0,
          background: isDarkBg
            ? "linear-gradient(180deg, var(--mantine-color-dark-7) 0%, var(--mantine-color-dark-8) 100%)"
            : "linear-gradient(180deg, var(--mantine-color-gray-1) 0%, var(--mantine-color-gray-3) 100%)",
          borderRadius: 8,
          minHeight: isFullscreen ? "100vh" : 480,
          height: isFullscreen ? "100vh" : 480,
          position: "relative",
          overflow: "hidden",
        }}
      >
        {loading && (
          <Stack
            gap={8}
            align="center"
            style={{ position: "absolute", top: "50%", left: "50%", transform: "translate(-50%, -50%)", zIndex: 2 }}
          >
            <Loader />
            {isScadSelected && (
              <Text size="xs" c="dimmed" ta="center">
                Rendering with OpenSCAD…
              </Text>
            )}
          </Stack>
        )}

        {selectedError && !loading && (
          <Stack
            gap={4}
            align="center"
            style={{
              position: "absolute",
              top: "50%",
              left: "50%",
              transform: "translate(-50%, -50%)",
              zIndex: 2,
              maxWidth: 420,
              textAlign: "center",
              padding: "0 16px",
            }}
          >
            <IconBox size={32} color={isDarkBg ? "#888" : "#999"} />
            <Text size="sm" fw={600} c={isDarkBg ? "gray.3" : "dark.5"}>
              {isScadSelected ? "Render failed" : "Preview failed"}
            </Text>
            <Text size="xs" c="dimmed" style={{ whiteSpace: "pre-wrap", wordBreak: "break-word" }}>
              {selectedError}
            </Text>
          </Stack>
        )}

        {selectedTooLarge && !loading && !selectedError && (
          <Stack
            gap={4}
            align="center"
            style={{
              position: "absolute",
              top: "50%",
              left: "50%",
              transform: "translate(-50%, -50%)",
              zIndex: 2,
              maxWidth: 360,
              textAlign: "center",
              padding: "0 16px",
            }}
          >
            <IconBox size={32} color={isDarkBg ? "#888" : "#999"} />
            <Text size="sm" fw={600} c={isDarkBg ? "gray.3" : "dark.5"}>
              Too large to preview
            </Text>
            <Text size="xs" c="dimmed">
              {selectedOversized ? (
                <>
                  The rendered mesh for {selectedMeta?.asset.original_filename} exceeds the{" "}
                  {formatBytes(maxPreviewBytes)} preview limit. Raise the limit in Settings to load it
                  anyway.
                </>
              ) : (
                <>
                  {selectedMeta?.asset.original_filename} is {formatBytes(selectedSize)}, over the{" "}
                  {formatBytes(maxPreviewBytes)} preview limit. Raise the limit in Settings to load it
                  anyway.
                </>
              )}
            </Text>
          </Stack>
        )}

        {/* Floating toolbar (top-right) */}
        <Paper
          className="gcf-floating"
          p={4}
          style={{ position: "absolute", top: 8, right: 8, zIndex: 3 }}
        >
          <Group gap={2}>
            <Tooltip label="Set this view as cover">
              <ActionIcon
                variant="subtle"
                onClick={captureCover}
                disabled={selectedId == null}
                loading={capturing}
                aria-label="Set this view as cover"
                c="white"
              >
                <IconCamera size={16} />
              </ActionIcon>
            </Tooltip>
            <Tooltip label="Fit view">
              <ActionIcon variant="subtle" onClick={onFit} aria-label="Fit view" c="white">
                <IconFocusCentered size={16} />
              </ActionIcon>
            </Tooltip>
            <Tooltip label={wireframe ? "Solid" : "Wireframe"}>
              <ActionIcon
                variant={wireframe ? "filled" : "subtle"}
                onClick={() => setWireframe(!wireframe)}
                aria-label="Toggle wireframe"
                c={wireframe ? undefined : "white"}
              >
                <IconBoxMultiple size={16} />
              </ActionIcon>
            </Tooltip>
            <Tooltip label={bgLabel[background]}>
              <ActionIcon variant="subtle" onClick={cycleBackground} aria-label="Cycle background" c="white">
                {bgIcons[background]}
              </ActionIcon>
            </Tooltip>
            <Tooltip label={isFullscreen ? "Exit fullscreen" : "Fullscreen"}>
              <ActionIcon variant="subtle" onClick={toggleFullscreen} aria-label="Fullscreen" c="white">
                {isFullscreen ? <IconMinimize size={16} /> : <IconMaximize size={16} />}
              </ActionIcon>
            </Tooltip>
          </Group>
        </Paper>

        {/* Parts panel (bottom-left, only when >1 part) */}
        {parts.length > 1 && (
          <Paper
            className="gcf-floating"
            style={{ position: "absolute", left: 8, bottom: 8, zIndex: 3, maxWidth: 320 }}
          >
            <Group
              justify="space-between"
              gap="xs"
              px="sm"
              py={6}
              style={{ cursor: "pointer", userSelect: "none" }}
              onClick={() => setPartsPanelOpen((v) => !v)}
            >
              <Text size="xs" fw={600} c="white" truncate style={{ maxWidth: 240 }}>
                {selectedIndex >= 0
                  ? `Part ${selectedIndex + 1}/${parts.length}: ${selectedMeta?.asset.original_filename ?? ""}`
                  : `Parts (${parts.length})`}
              </Text>
              {partsPanelOpen ? (
                <IconChevronDown size={14} color="white" />
              ) : (
                <IconChevronUp size={14} color="white" />
              )}
            </Group>
            <Collapse in={partsPanelOpen}>
              <Stack gap={2} px={6} pb={6} style={{ maxHeight: 260, overflowY: "auto" }}>
                {parts.map((p) => {
                  const isSelected = p.asset.id === selectedId;
                  return (
                    <UnstyledButton
                      key={p.asset.id}
                      onClick={() => setSelectedId(p.asset.id)}
                      style={{
                        padding: "4px 8px",
                        borderRadius: 4,
                        background: isSelected ? "rgba(167, 139, 250, 0.25)" : "transparent",
                      }}
                    >
                      <Text
                        size="xs"
                        c="white"
                        fw={isSelected ? 600 : 400}
                        truncate
                        style={{ minWidth: 0 }}
                      >
                        {p.asset.original_filename}
                      </Text>
                    </UnstyledButton>
                  );
                })}
              </Stack>
            </Collapse>
          </Paper>
        )}

        <Canvas camera={{ position: [80, 80, 120], fov: 45 }} gl={{ preserveDrawingBuffer: true }}>
          <CaptureBridge apiRef={captureRef} />
          <ambientLight intensity={0.55} />
          <directionalLight position={[100, 100, 100]} intensity={0.9} />
          <directionalLight position={[-100, -50, -100]} intensity={0.4} />
          <Suspense fallback={null}>
            <Bounds fit clip observe margin={1.4} key={boundsKey}>
              {selected && (
                <mesh geometry={selected.geometry}>
                  <meshStandardMaterial
                    color="#a78bfa"
                    metalness={0.1}
                    roughness={0.6}
                    wireframe={wireframe}
                  />
                </mesh>
              )}
            </Bounds>
          </Suspense>
          {showGrid && !capturing && (
            <Grid
              args={[400, 400]}
              cellSize={10}
              sectionSize={50}
              sectionThickness={1}
              cellColor={isDarkBg ? "#666" : "#bbb"}
              sectionColor={isDarkBg ? "#3a3a3f" : "#888"}
              infiniteGrid
              fadeDistance={500}
            />
          )}
          <OrbitControls makeDefault />
          {!capturing && (
            <GizmoHelper alignment="bottom-right" margin={[60, 60]}>
              <GizmoViewport
                axisColors={["#ff5c5c", "#5cff5c", "#5c5cff"]}
                labelColor={isDarkBg ? "white" : "#222"}
              />
            </GizmoHelper>
          )}
        </Canvas>
      </div>
    </Stack>
  );
}
