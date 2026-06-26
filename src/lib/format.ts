export function formatDuration(seconds: number | null | undefined): string {
  if (seconds == null) return "—";
  const s = Math.max(0, Math.floor(seconds));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  if (h > 0) return `${h}h ${m}m`;
  if (m > 0) return `${m}m ${sec}s`;
  return `${sec}s`;
}

export function formatBytes(bytes: number | null | undefined): string {
  if (bytes == null) return "—";
  const units = ["B", "KB", "MB", "GB"];
  let n = bytes;
  let i = 0;
  while (n >= 1024 && i < units.length - 1) {
    n /= 1024;
    i++;
  }
  return `${n.toFixed(n < 10 && i > 0 ? 1 : 0)} ${units[i]}`;
}

export function formatGrams(g: number | null | undefined): string {
  if (g == null) return "—";
  return `${g.toFixed(1)} g`;
}

export function formatLengthMm(mm: number | null | undefined): string {
  if (mm == null) return "—";
  if (mm > 1000) return `${(mm / 1000).toFixed(2)} m`;
  return `${mm.toFixed(0)} mm`;
}

/** Combined filament summary: "28.4 g · 9.6 m" when both are present, else whichever is. */
export function formatFilament(
  weightGrams: number | null | undefined,
  lengthMm: number | null | undefined,
): string {
  const haveW = weightGrams != null;
  const haveL = lengthMm != null;
  if (!haveW && !haveL) return "—";
  if (haveW && haveL) return `${formatGrams(weightGrams)} · ${formatLengthMm(lengthMm)}`;
  return haveW ? formatGrams(weightGrams) : formatLengthMm(lengthMm);
}

/** Mantine color name for a slicer name. Falls back to gray. */
export function slicerColor(name: string | null | undefined): string {
  if (!name) return "gray";
  const lower = name.toLowerCase();
  if (lower.includes("prusa")) return "orange";
  if (lower.includes("super")) return "yellow";
  if (lower.includes("orca")) return "teal";
  if (lower.includes("bambu")) return "lime";
  if (lower.includes("cura")) return "blue";
  return "gray";
}
