import { create } from "zustand";
import type { Folder, ObjectSummary } from "../types";
import { api } from "../api";

interface LibraryState {
  folders: Folder[];
  selectedFolderId: number | null;
  objects: ObjectSummary[];
  allObjects: ObjectSummary[];
  loading: boolean;
  error: string | null;
  loadFolders: () => Promise<void>;
  selectFolder: (id: number | null) => Promise<void>;
  refreshObjects: () => Promise<void>;
  refreshAllObjects: () => Promise<void>;
}

export const useLibraryStore = create<LibraryState>((set, get) => ({
  folders: [],
  selectedFolderId: null,
  objects: [],
  allObjects: [],
  loading: false,
  error: null,

  loadFolders: async () => {
    set({ loading: true, error: null });
    try {
      const folders = await api.listFolders();
      set({ folders, loading: false });
    } catch (e: any) {
      set({ error: String(e), loading: false });
    }
  },

  selectFolder: async (id) => {
    set({ selectedFolderId: id });
    await get().refreshObjects();
  },

  refreshObjects: async () => {
    const { selectedFolderId } = get();
    set({ loading: true, error: null });
    try {
      const [objects, allObjects] = await Promise.all([
        api.listObjects(selectedFolderId),
        api.listAllObjects(),
      ]);
      // Drop the result if the selection changed while we were fetching — a
      // slow response for an old folder must not clobber the current one.
      if (get().selectedFolderId !== selectedFolderId) return;
      set({ objects, allObjects, loading: false });
    } catch (e: any) {
      if (get().selectedFolderId !== selectedFolderId) return;
      set({ error: String(e), loading: false });
    }
  },

  refreshAllObjects: async () => {
    try {
      const allObjects = await api.listAllObjects();
      set({ allObjects });
    } catch (e: any) {
      set({ error: String(e) });
    }
  },
}));
