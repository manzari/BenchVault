import {
  AppShell,
  Burger,
  Combobox,
  Group,
  Kbd,
  Loader,
  Paper,
  ScrollArea,
  Stack,
  Text,
  TextInput,
  ActionIcon,
  Tooltip,
  useCombobox,
} from "@mantine/core";
import { useDisclosure, useHotkeys } from "@mantine/hooks";
import { notifications } from "@mantine/notifications";
import {
  IconCube,
  IconFolder,
  IconFile,
  IconSearch,
  IconSettings,
  IconDeviceUsb,
  IconRefresh,
} from "@tabler/icons-react";
import { withErrorToast } from "./lib/errors";
import { Link, useNavigate, useMatches } from "@tanstack/react-router";
import { useEffect, useRef, useState, type ReactNode } from "react";
import type { SearchHit, SyncReport } from "./lib/types";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { listen } from "@tauri-apps/api/event";
import { FolderTree } from "./components/FolderTree";
import { Breadcrumbs } from "./components/Breadcrumbs";
import { api } from "./lib/api";
import { ensureCoverFromStl } from "./lib/cover";
import { useLibraryStore } from "./lib/stores/library";
import { useUsbStore } from "./lib/stores/usb";
import { useUiStore } from "./lib/stores/ui";

type AppEventPayload =
  | { kind: "library_changed" }
  | { kind: "job_progress"; job_id: string; percent: number; message: string | null }
  | { kind: "usb_sync_finished"; report: SyncReport }
  | { kind: string; [k: string]: unknown };

function basename(path: string): string {
  const sep = path.lastIndexOf("/") >= 0 ? "/" : "\\";
  const base = path.slice(path.lastIndexOf(sep) + 1);
  const dot = base.lastIndexOf(".");
  return dot > 0 ? base.slice(0, dot) : base;
}

const isMac =
  typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent || "");

function searchHitIcon(kind: string): ReactNode {
  if (kind === "folder") return <IconFolder size={16} />;
  if (kind === "object") return <IconCube size={16} />;
  return <IconFile size={16} />;
}

export function App({ children }: { children: React.ReactNode }) {
  const [opened, { toggle }] = useDisclosure();
  const [q, setQ] = useState("");
  const [hits, setHits] = useState<SearchHit[]>([]);
  const [searching, setSearching] = useState(false);
  const navigate = useNavigate();
  const matches = useMatches();
  const searchRef = useRef<HTMLInputElement>(null);
  const combobox = useCombobox();

  const folders = useLibraryStore((s) => s.folders);
  const allObjects = useLibraryStore((s) => s.allObjects);
  const selectedFolderId = useLibraryStore((s) => s.selectedFolderId);
  const loadFolders = useLibraryStore((s) => s.loadFolders);
  const refreshAllObjects = useLibraryStore((s) => s.refreshAllObjects);
  const refreshObjects = useLibraryStore((s) => s.refreshObjects);
  const selectFolder = useLibraryStore((s) => s.selectFolder);
  const libraryError = useLibraryStore((s) => s.error);

  const usbSyncing = useUsbStore((s) => s.syncing);
  const [rescanning, setRescanning] = useState(false);

  // Sweep the on-disk library for files/objects not yet tracked. The backend emits
  // `library_changed` on success (which refreshes the tree); we toast the count.
  const handleRescan = async () => {
    setRescanning(true);
    const res = await withErrorToast("Library rescan failed", () => api.rescanLibrary());
    setRescanning(false);
    if (!res.ok) return;
    const n = res.value;
    notifications.show({
      color: n > 0 ? "green" : "blue",
      title: n > 0 ? "Rescan complete" : "Library up to date",
      message: n > 0 ? `Discovered ${n} new file${n === 1 ? "" : "s"}.` : "No new files found on disk.",
    });
  };

  // Push exportable gcode to the registered USB drive. Mirrors SettingsPage's sync.
  const handleUsbSync = async () => {
    try {
      const report = await useUsbStore.getState().syncNow(false);
      if (report.skipped) {
        notifications.show({ color: "yellow", title: "Sync skipped", message: report.reason ?? "unknown" });
        return;
      }
      const errs = report.errors.length;
      notifications.show({
        color: errs > 0 ? "yellow" : "green",
        title: errs > 0 ? `Sync finished with ${errs} error${errs === 1 ? "" : "s"}` : "Sync complete",
        message: `${report.copied} copied, ${report.updated} updated, ${report.deleted} deleted`,
      });
    } catch (e) {
      notifications.show({ color: "red", title: "Sync failed", message: String(e) });
    }
  };

  useEffect(() => {
    loadFolders();
    refreshAllObjects();
    useUsbStore.getState().load();
    useUiStore.getState().loadStlPreviewMaxBytes();
  }, []);

  useEffect(() => {
    const unlistenPromise = listen<AppEventPayload>("app-event", (e) => {
      const ev = e.payload;
      if (ev.kind === "library_changed") {
        // The watcher (or a rescan) ingested/changed files on disk. Pull fresh
        // folders + objects so newly-discovered gcode etc. shows up immediately.
        void useLibraryStore.getState().loadFolders();
        void useLibraryStore.getState().refreshObjects();
      } else if (ev.kind === "job_progress" && (ev as any).job_id === "usb-sync") {
        useUsbStore.getState().applyProgress(
          Number((ev as any).percent ?? 0),
          ((ev as any).message as string | null) ?? null,
        );
      } else if (ev.kind === "usb_sync_finished") {
        const report = (ev as { report: SyncReport }).report;
        useUsbStore.getState().applyAutoReport(report);
        // Toast only when the auto-sync actually moved files; errors surface via
        // the lastError subscription below (once per distinct error, not per tick).
        const changes = report.skipped ? 0 : report.copied + report.updated + report.deleted;
        if (changes > 0) {
          notifications.show({
            color: "green",
            title: "USB auto-synced",
            message: `${report.copied} copied, ${report.updated} updated, ${report.deleted} deleted`,
          });
        }
      }
    });
    return () => {
      unlistenPromise.then((un) => un());
    };
  }, []);

  // Surface library errors as toasts so they don't get swallowed silently.
  useEffect(() => {
    if (libraryError) {
      notifications.show({ color: "red", title: "Library error", message: libraryError });
    }
  }, [libraryError]);

  // Surface USB errors the same way.
  useEffect(() => {
    return useUsbStore.subscribe((state, prev) => {
      if (state.lastError && state.lastError !== prev.lastError) {
        notifications.show({ color: "red", title: "USB sync error", message: state.lastError });
      }
    });
  }, []);

  // Cmd/Ctrl+K to focus the header search.
  useHotkeys([
    ["mod+K", () => searchRef.current?.focus()],
  ]);

  // Live search: debounce 200ms, fire while the user types.
  useEffect(() => {
    const trimmed = q.trim();
    if (!trimmed) {
      setHits([]);
      setSearching(false);
      return;
    }
    setSearching(true);
    let cancelled = false;
    const t = setTimeout(() => {
      api
        .search(trimmed, null)
        .then((h) => {
          if (cancelled) return;
          setHits(h);
        })
        .catch(() => {
          if (cancelled) return;
          setHits([]);
        })
        .finally(() => {
          if (!cancelled) setSearching(false);
        });
    }, 200);
    return () => {
      cancelled = true;
      clearTimeout(t);
    };
  }, [q]);

  const activateHit = async (hit: SearchHit) => {
    if (hit.entity_kind === "object") {
      navigate({ to: "/objects/$objectId", params: { objectId: String(hit.entity_id) } });
    } else if (hit.entity_kind === "folder") {
      await selectFolder(hit.entity_id);
      navigate({ to: "/" });
    } else {
      // assets: no destination route yet — leave inert.
      return;
    }
    setQ("");
    setHits([]);
    combobox.closeDropdown();
    searchRef.current?.blur();
  };

  // Global drop router for STL/GCode files and folders.
  //   - Folder onto an object → ingest its contents into that object
  //   - Folder onto a folder row (or empty space) → import as a new object in that folder
  //   - STL onto an object → added as part
  //   - GCode onto an object → added as gcode
  //   - STL onto a folder row (or empty space) → new object in that folder
  useEffect(() => {
    const unlistenPromise = getCurrentWebview().onDragDropEvent(async (event) => {
      if (event.payload.type !== "drop") return;
      if (event.payload.paths.length === 0) return;

      // Backend tells us which paths are directories — extension alone can't.
      let kinds: ("dir" | "file" | "missing")[];
      try {
        kinds = await api.classifyPaths(event.payload.paths);
      } catch {
        kinds = event.payload.paths.map(() => "file" as const);
      }

      const dirs: string[] = [];
      const stls: string[] = [];
      const gcodes: string[] = [];
      event.payload.paths.forEach((p, i) => {
        if (kinds[i] === "dir") {
          dirs.push(p);
          return;
        }
        if (kinds[i] === "missing") return;
        const lower = p.toLowerCase();
        if (lower.endsWith(".stl")) stls.push(p);
        else if (lower.endsWith(".gcode") || lower.endsWith(".gco") || lower.endsWith(".g")) gcodes.push(p);
      });
      if (dirs.length === 0 && stls.length === 0 && gcodes.length === 0) return;

      const dpr = window.devicePixelRatio || 1;
      const x = event.payload.position.x / dpr;
      const y = event.payload.position.y / dpr;
      const el = document.elementFromPoint(x, y) as Element | null;
      const objectEl = el?.closest("[data-drop-object-id]") ?? null;
      const folderEl = !objectEl ? el?.closest("[data-drop-folder-id]") ?? null : null;

      if (objectEl) {
        const objectId = Number(objectEl.getAttribute("data-drop-object-id"));
        let addedStl = 0;
        let addedGcode = 0;
        let addedFromDirs = 0;
        for (const path of stls) {
          try {
            await api.addStl(objectId, path);
            addedStl += 1;
          } catch (e) {
            notifications.show({
              color: "red",
              title: "Failed to add STL",
              message: `${basename(path)}: ${String(e)}`,
            });
          }
        }
        for (const path of gcodes) {
          try {
            await api.addGcode(objectId, path);
            addedGcode += 1;
          } catch (e) {
            notifications.show({
              color: "red",
              title: "Failed to add GCode",
              message: `${basename(path)}: ${String(e)}`,
            });
          }
        }
        for (const path of dirs) {
          try {
            const r = await api.importFolderIntoObject(objectId, path);
            addedFromDirs += r.stl_count + r.gcode_count + r.image_count + r.other_count;
          } catch (e) {
            notifications.show({
              color: "red",
              title: "Failed to import folder",
              message: `${basename(path)}: ${String(e)}`,
            });
          }
        }
        if (addedStl + addedGcode + addedFromDirs > 0) {
          window.dispatchEvent(new CustomEvent("gcf:object-mutated", { detail: { id: objectId } }));
          await refreshAllObjects();
          await refreshObjects();
          const parts: string[] = [];
          if (addedStl > 0) parts.push(`${addedStl} STL part${addedStl === 1 ? "" : "s"}`);
          if (addedGcode > 0) parts.push(`${addedGcode} GCode file${addedGcode === 1 ? "" : "s"}`);
          if (addedFromDirs > 0) parts.push(`${addedFromDirs} file${addedFromDirs === 1 ? "" : "s"} from folder`);
          notifications.show({
            color: "green",
            title: "Added",
            message: parts.join(" and ") + ".",
          });
        }
        return;
      }

      // Not over an object. Loose GCodes need one — warn if any were dropped without a target.
      if (gcodes.length > 0) {
        notifications.show({
          color: "yellow",
          title: "Drop GCode onto an object",
          message: `GCode files need an object. Drop them on an object in the tree or its detail page.`,
        });
      }

      const targetFolderId = folderEl
        ? Number(folderEl.getAttribute("data-drop-folder-id"))
        : useLibraryStore.getState().selectedFolderId;

      let createdObjects = 0;

      for (const path of dirs) {
        try {
          const report = await api.importFolderAsObject(path, targetFolderId);
          // Folder import sets a cover when it found an image; otherwise snapshot the first STL.
          await ensureCoverFromStl(report.object.id);
          createdObjects += 1;
        } catch (e) {
          notifications.show({
            color: "red",
            title: "Failed to import folder",
            message: `${basename(path)}: ${String(e)}`,
          });
        }
      }

      for (const path of stls) {
        try {
          const created = await api.createObject({
            name: basename(path),
            folder_id: targetFolderId,
            notes: null,
            source_url: null,
            author: null,
            license: null,
            stl_paths: [path],
            cover_image_path: null,
          });
          // STL-only object → snapshot it for the cover.
          await ensureCoverFromStl(created.id);
          createdObjects += 1;
        } catch (e) {
          notifications.show({
            color: "red",
            title: "Failed to import STL",
            message: `${basename(path)}: ${String(e)}`,
          });
        }
      }

      if (createdObjects > 0) {
        await refreshAllObjects();
        await refreshObjects();
        notifications.show({
          color: "green",
          title: "Imported",
          message: `Created ${createdObjects} object${createdObjects === 1 ? "" : "s"}.`,
        });
      }
    });
    return () => {
      unlistenPromise.then((unlisten) => unlisten());
    };
  }, [refreshAllObjects, refreshObjects]);

  // Active object id from the URL when on /objects/:objectId(/edit)
  const activeObjectId = (() => {
    for (const m of matches) {
      const params = m.params as Record<string, string | undefined>;
      if (params?.objectId) return Number(params.objectId);
    }
    return null;
  })();

  return (
    <AppShell
      header={{ height: 56 }}
      navbar={{ width: 280, breakpoint: "sm", collapsed: { mobile: !opened } }}
      padding="md"
    >
      <AppShell.Header>
        <Group h="100%" px="md" justify="space-between" gap="md">
          <Group gap="xs" wrap="nowrap">
            <Burger opened={opened} onClick={toggle} hiddenFrom="sm" size="sm" />
            <IconCube size={22} color="var(--mantine-color-violet-5)" />
            <Text component={Link} to="/" fw={700} td="none" c="inherit">
              BenchVault
            </Text>
          </Group>
          <div style={{ flex: 1, maxWidth: 520 }}>
            <Combobox
              store={combobox}
              withinPortal
              position="bottom-start"
              middlewares={{ flip: false, shift: true }}
              onOptionSubmit={(value) => {
                const hit = hits.find((h) => `${h.entity_kind}:${h.entity_id}` === value);
                if (hit) activateHit(hit);
              }}
            >
              <Combobox.Target>
                <TextInput
                  ref={searchRef}
                  placeholder="Search objects, files…"
                  leftSection={<IconSearch size={16} />}
                  value={q}
                  onChange={(e) => {
                    setQ(e.currentTarget.value);
                    combobox.openDropdown();
                    combobox.updateSelectedOptionIndex();
                  }}
                  onFocus={() => {
                    if (q.trim()) combobox.openDropdown();
                  }}
                  onBlur={() => combobox.closeDropdown()}
                  onKeyDown={(e) => {
                    if (e.key === "Escape") {
                      if (q) {
                        e.preventDefault();
                        setQ("");
                      }
                      combobox.closeDropdown();
                    }
                  }}
                  rightSectionWidth={q.trim() ? 28 : 56}
                  rightSection={
                    q.trim() ? (
                      searching ? <Loader size={14} /> : null
                    ) : (
                      <Kbd size="xs" style={{ marginRight: 6 }}>
                        {isMac ? "⌘K" : "Ctrl+K"}
                      </Kbd>
                    )
                  }
                />
              </Combobox.Target>

              <Combobox.Dropdown hidden={!q.trim()}>
                {hits.length === 0 ? (
                  <Combobox.Empty>
                    {searching ? "Searching…" : `No matches for "${q.trim()}"`}
                  </Combobox.Empty>
                ) : (
                  <ScrollArea.Autosize mah={360} type="scroll">
                    <Combobox.Options>
                      {hits.map((h) => (
                        <Combobox.Option
                          key={`${h.entity_kind}-${h.entity_id}`}
                          value={`${h.entity_kind}:${h.entity_id}`}
                          disabled={h.entity_kind !== "object" && h.entity_kind !== "folder"}
                        >
                          <Group gap="sm" wrap="nowrap">
                            <div style={{ color: "var(--mantine-color-violet-4)", flexShrink: 0 }}>
                              {searchHitIcon(h.entity_kind)}
                            </div>
                            <Text size="sm" truncate style={{ flex: 1 }}>
                              {h.name}
                            </Text>
                            <Text size="xs" c="dimmed">
                              {h.entity_kind}
                            </Text>
                          </Group>
                        </Combobox.Option>
                      ))}
                    </Combobox.Options>
                  </ScrollArea.Autosize>
                )}
              </Combobox.Dropdown>
            </Combobox>
          </div>
          <Group gap="xs" wrap="nowrap">
            <Tooltip label="Sync to USB">
              <ActionIcon
                variant="subtle"
                onClick={handleUsbSync}
                loading={usbSyncing}
                aria-label="Sync to USB"
              >
                <IconDeviceUsb size={20} />
              </ActionIcon>
            </Tooltip>
            <Tooltip label="Rescan library files">
              <ActionIcon
                variant="subtle"
                onClick={handleRescan}
                loading={rescanning}
                aria-label="Rescan library files"
              >
                <IconRefresh size={20} />
              </ActionIcon>
            </Tooltip>
            <Tooltip label="Settings">
              <ActionIcon component={Link} to="/settings" variant="subtle" aria-label="Settings">
                <IconSettings size={20} />
              </ActionIcon>
            </Tooltip>
          </Group>
        </Group>
      </AppShell.Header>
      <AppShell.Navbar p={0}>
        <Paper withBorder={false} radius={0} style={{ height: "100%", overflow: "auto" }}>
          <FolderTree
            folders={folders}
            objects={allObjects}
            selectedFolderId={selectedFolderId}
            activeObjectId={activeObjectId}
            onSelectFolder={async (id) => {
              await selectFolder(id);
              await refreshAllObjects();
              navigate({ to: "/" });
            }}
            onSelectObject={(id) => navigate({ to: "/objects/$objectId", params: { objectId: String(id) } })}
            onMutated={async () => {
              await loadFolders();
              await refreshObjects();
              await refreshAllObjects();
            }}
          />
        </Paper>
      </AppShell.Navbar>
      <AppShell.Main>
        <Stack gap="md">
          <BreadcrumbsForRoute />
          {children}
        </Stack>
      </AppShell.Main>
    </AppShell>
  );
}

/** Reads the current route params and renders the right breadcrumb chain. */
function BreadcrumbsForRoute() {
  const matches = useMatches();
  const allObjects = useLibraryStore((s) => s.allObjects);

  const objectIdParam = (() => {
    for (const m of matches) {
      const params = m.params as Record<string, string | undefined>;
      if (params?.objectId) return Number(params.objectId);
    }
    return null;
  })();

  if (objectIdParam != null) {
    const obj = allObjects.find((o) => o.id === objectIdParam);
    return (
      <Breadcrumbs
        objectName={obj?.name ?? null}
        objectFolderId={obj?.folder_id ?? null}
      />
    );
  }

  return <Breadcrumbs />;
}
