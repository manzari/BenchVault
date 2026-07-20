// Typed view over the static OpenSCAD metadata the backend stashes in
// `assets.metadata_json` (see src-tauri/src/parsers/scad.rs). Loose data by
// design, so it's parsed here rather than surfaced as a typed IPC sidecar.

export interface ScadParameter {
  name: string;
  default: string;
  group: string | null;
  hint: string | null;
}

export interface ScadMetadata {
  parameters: ScadParameter[];
  includes: string[];
  uses: string[];
  modules: string[];
  functions: string[];
}

export function parseScadMetadata(json: string | null | undefined): ScadMetadata | null {
  if (!json) return null;
  try {
    const raw = JSON.parse(json);
    if (!raw || typeof raw !== "object" || !Array.isArray(raw.parameters)) return null;
    return {
      parameters: raw.parameters ?? [],
      includes: raw.includes ?? [],
      uses: raw.uses ?? [],
      modules: raw.modules ?? [],
      functions: raw.functions ?? [],
    };
  } catch {
    return null;
  }
}

export function scadHasDetail(m: ScadMetadata | null): boolean {
  return (
    !!m &&
    m.parameters.length + m.includes.length + m.uses.length + m.modules.length + m.functions.length > 0
  );
}

/** Compact one-liner for under the filename, e.g. "3 params · 2 deps · 1 module". */
export function scadSummary(m: ScadMetadata | null): string | null {
  if (!m) return null;
  const parts: string[] = [];
  const plural = (n: number, s: string) => `${n} ${s}${n === 1 ? "" : "s"}`;
  if (m.parameters.length) parts.push(plural(m.parameters.length, "param"));
  const deps = m.includes.length + m.uses.length;
  if (deps) parts.push(plural(deps, "dep"));
  if (m.modules.length) parts.push(plural(m.modules.length, "module"));
  if (m.functions.length) parts.push(plural(m.functions.length, "fn"));
  return parts.length ? parts.join(" · ") : null;
}

/** Group parameters into contiguous runs by customizer group, preserving order. */
export function groupScadParameters(
  params: ScadParameter[],
): { group: string | null; params: ScadParameter[] }[] {
  const groups: { group: string | null; params: ScadParameter[] }[] = [];
  for (const p of params) {
    const g = p.group ?? null;
    const last = groups[groups.length - 1];
    if (last && last.group === g) last.params.push(p);
    else groups.push({ group: g, params: [p] });
  }
  return groups;
}
