import { create } from "zustand";
import { api } from "../api";

export type ViewerBackground = "dark-grid" | "light-grid" | "solid";
export type FolderSort = "updated" | "name";

/** Mirrors the backend default (`service::settings::DEFAULT_STL_PREVIEW_MAX_BYTES`). */
export const DEFAULT_STL_PREVIEW_MAX_BYTES = 100 * 1024 * 1024;

interface UiState {
  addObjectOpen: boolean;
  setAddObjectOpen: (v: boolean) => void;
  importUrlOpen: boolean;
  setImportUrlOpen: (v: boolean) => void;

  viewerBackground: ViewerBackground;
  cycleViewerBackground: () => void;
  viewerWireframe: boolean;
  setViewerWireframe: (v: boolean) => void;

  folderSort: FolderSort;
  setFolderSort: (v: FolderSort) => void;

  /** Largest STL the preview will read; bigger ones are refused to avoid OOM. */
  stlPreviewMaxBytes: number;
  loadStlPreviewMaxBytes: () => Promise<void>;
  setStlPreviewMaxBytes: (bytes: number) => Promise<void>;
}

const BG_CYCLE: ViewerBackground[] = ["dark-grid", "light-grid", "solid"];

export const useUiStore = create<UiState>((set, get) => ({
  addObjectOpen: false,
  setAddObjectOpen: (v) => set({ addObjectOpen: v }),
  importUrlOpen: false,
  setImportUrlOpen: (v) => set({ importUrlOpen: v }),

  viewerBackground: "dark-grid",
  cycleViewerBackground: () => {
    const i = BG_CYCLE.indexOf(get().viewerBackground);
    set({ viewerBackground: BG_CYCLE[(i + 1) % BG_CYCLE.length] });
  },
  viewerWireframe: false,
  setViewerWireframe: (v) => set({ viewerWireframe: v }),

  folderSort: "updated",
  setFolderSort: (v) => set({ folderSort: v }),

  stlPreviewMaxBytes: DEFAULT_STL_PREVIEW_MAX_BYTES,
  loadStlPreviewMaxBytes: async () => {
    try {
      set({ stlPreviewMaxBytes: await api.getStlPreviewMaxBytes() });
    } catch {
      // Keep the default if the backend can't be reached.
    }
  },
  setStlPreviewMaxBytes: async (bytes) => {
    await api.setStlPreviewMaxBytes(bytes);
    set({ stlPreviewMaxBytes: bytes });
  },
}));
