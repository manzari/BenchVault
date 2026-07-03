import { create } from "zustand";
import type { SyncReport, UsbTarget } from "../types";
import { api } from "../api";

interface UsbState {
  target: UsbTarget | null;
  connected: boolean;
  syncing: boolean;
  syncPercent: number;
  syncMessage: string | null;
  lastError: string | null;
  lastReport: SyncReport | null;

  load: () => Promise<void>;
  pickUsb: (mountPath: string, label: string, force?: boolean) => Promise<UsbTarget>;
  clearUsb: () => Promise<void>;
  syncNow: (deep?: boolean) => Promise<SyncReport>;
  setExportFlag: (assetId: number, flag: boolean) => Promise<void>;

  applyProgress: (percent: number, message: string | null) => void;
  /** Fold in the report of a backend-initiated (auto) sync. */
  applyAutoReport: (report: SyncReport) => void;
}

export const useUsbStore = create<UsbState>((set, get) => ({
  target: null,
  connected: false,
  syncing: false,
  syncPercent: 0,
  syncMessage: null,
  lastError: null,
  lastReport: null,

  load: async () => {
    try {
      const target = await api.getUsbTarget();
      set({ target });
    } catch (e: any) {
      set({ lastError: String(e) });
    }
  },

  pickUsb: async (mountPath, label, force) => {
    const target = await api.registerUsbTarget(mountPath, label, force);
    set({ target, lastError: null });
    return target;
  },

  clearUsb: async () => {
    await api.clearUsbTarget();
    set({ target: null, connected: false, lastReport: null });
  },

  syncNow: async (deep) => {
    // A sync is already running: return a well-formed "skipped" report rather
    // than `lastReport!`, which is null before the first sync and crashes
    // callers that read report.skipped.
    if (get().syncing) {
      return {
        copied: 0,
        updated: 0,
        deleted: 0,
        skipped: true,
        reason: "Sync already in progress",
        errors: [],
        started_at: 0,
        finished_at: 0,
      };
    }
    set({ syncing: true, syncPercent: 0, syncMessage: null });
    try {
      const report = await api.syncUsbNow(deep);
      const connected = !report.skipped || report.reason !== "USB not detected";
      set({
        syncing: false,
        syncPercent: report.skipped ? 0 : 100,
        syncMessage: null,
        lastReport: report,
        lastError: report.errors[0] ?? (report.skipped ? report.reason : null),
        connected: report.skipped ? get().connected && connected : true,
      });
      if (report.skipped && report.reason === "USB not detected") {
        set({ connected: false });
      }
      return report;
    } catch (e: any) {
      set({ syncing: false, lastError: String(e) });
      throw e;
    }
  },

  setExportFlag: async (assetId, flag) => {
    await api.setGcodeExport(assetId, flag);
  },

  applyProgress: (percent, message) => {
    set({ syncPercent: percent, syncMessage: message });
  },

  applyAutoReport: (report) => {
    if (report.skipped) {
      // Keep the last real report; only track (dis)connection. Other skip
      // reasons ("already syncing") say nothing about the drive.
      if (report.reason === "USB not detected") set({ connected: false });
      return;
    }
    set({
      connected: true,
      lastReport: report,
      lastError: report.errors[0] ?? null,
      syncMessage: null,
    });
  },
}));
