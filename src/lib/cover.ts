// Auto-cover policy for newly created objects: prefer a real picture, but fall
// back to a snapshot of the first STL part so every object has a thumbnail.

import { api } from "./api";
import { renderStlToPng } from "./stlRender";
import { useUiStore } from "./stores/ui";

/**
 * Ensure `objectId` has a cover image. If the object already has one (a picture
 * was supplied as the cover or attached during import), this is a no-op. Otherwise
 * it renders the first STL part to a PNG snapshot off-screen and sets that as the
 * cover.
 *
 * Best-effort: any failure (no STL, unreadable bytes, WebGL unavailable) is
 * swallowed and logged, so it never breaks the object-creation flow that calls it.
 * Returns `true` only when a snapshot cover was actually generated.
 */
export async function ensureCoverFromStl(objectId: number): Promise<boolean> {
  try {
    const full = await api.getObject(objectId);
    if (full.object.cover_asset_id != null) return false;

    // First STL part by sort order (the first one added gets sort_order 0).
    const stl = full.assets
      .filter((a) => a.role === "stl_part")
      .sort((a, b) => a.sort_order - b.sort_order)[0];
    if (!stl) return false;

    // Honor the same size ceiling the interactive viewer uses: pulling a huge
    // STL across IPC as number[] and parsing it on the main thread can OOM the
    // webview, and this runs unprompted during object creation.
    const maxBytes = useUiStore.getState().stlPreviewMaxBytes;
    if ((stl.asset.size_bytes ?? 0) > maxBytes) return false;

    const bytes = await api.readAssetBytes(stl.asset.id);
    const png = renderStlToPng(bytes);
    await api.setCoverFromRender(objectId, png, `cover-${stl.asset.original_filename}.png`);
    return true;
  } catch (e) {
    console.warn(`auto cover from STL failed for object ${objectId}`, e);
    return false;
  }
}
