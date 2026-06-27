import { ActionIcon, Group, NavLink, Stack, Text, TextInput, Tooltip } from "@mantine/core";
import {
  IconChevronDown,
  IconChevronRight,
  IconCube,
  IconFolder,
  IconFolderOpen,
  IconFolderPlus,
  IconHome,
  IconTrash,
} from "@tabler/icons-react";
import { ask } from "@tauri-apps/plugin-dialog";
import { useEffect, useMemo, useRef, useState } from "react";
import type { Folder, ObjectSummary } from "../lib/types";
import { api } from "../lib/api";
import { withErrorToast } from "../lib/errors";

interface Props {
  folders: Folder[];
  objects: ObjectSummary[];
  selectedFolderId: number | null;
  activeObjectId: number | null;
  onSelectFolder: (id: number | null) => void;
  onSelectObject: (id: number) => void;
  onMutated: () => void;
}

interface FolderNode {
  folder: Folder;
  children: FolderNode[];
}

function buildTree(folders: Folder[]): FolderNode[] {
  const byId = new Map<number, FolderNode>();
  folders.forEach((f) => byId.set(f.id, { folder: f, children: [] }));
  const roots: FolderNode[] = [];
  folders.forEach((f) => {
    const node = byId.get(f.id)!;
    if (f.parent_id != null && byId.has(f.parent_id)) {
      byId.get(f.parent_id)!.children.push(node);
    } else {
      roots.push(node);
    }
  });
  const sortRec = (ns: FolderNode[]) => {
    ns.sort((a, b) => a.folder.name.localeCompare(b.folder.name));
    ns.forEach((n) => sortRec(n.children));
  };
  sortRec(roots);
  return roots;
}

function ancestorsOfFolder(folders: Folder[], folderId: number | null): Set<number> {
  const out = new Set<number>();
  if (folderId == null) return out;
  const byId = new Map(folders.map((f) => [f.id, f]));
  let cur = byId.get(folderId);
  while (cur) {
    out.add(cur.id);
    if (cur.parent_id == null) break;
    cur = byId.get(cur.parent_id);
  }
  return out;
}

type RenameTarget = { kind: "folder" | "object"; id: number; initial: string };

// Manual mouse-based drag. We can't use HTML5 DnD here because Tauri 2 captures
// drag events at the webview level (`dragDropEnabled: true`, needed by App.tsx's
// OS-file-drop importer) and doesn't forward them to HTML elements.
const DRAG_THRESHOLD_PX = 5;
type DragState = {
  id: number;
  name: string;
  startX: number;
  startY: number;
  x: number;
  y: number;
  started: boolean;
  // null = root, number = folder id, undefined = no drop target under cursor.
  hoverFolderId: number | null | undefined;
};

export function FolderTree({
  folders,
  objects,
  selectedFolderId,
  activeObjectId,
  onSelectFolder,
  onSelectObject,
  onMutated,
}: Props) {
  const [adding, setAdding] = useState<{ parent: number | null } | null>(null);
  const [name, setName] = useState("");
  const [expanded, setExpanded] = useState<Set<number>>(new Set());
  const [renaming, setRenaming] = useState<RenameTarget | null>(null);
  const [renameDraft, setRenameDraft] = useState("");
  const [drag, setDrag] = useState<DragState | null>(null);
  const dragRef = useRef<DragState | null>(null);
  // Set briefly when a drag completes so the synthetic click that follows mouseup
  // doesn't trigger select/rename. Cleared on next macrotask.
  const suppressClickRef = useRef(false);

  const startRename = (t: RenameTarget) => {
    setRenaming(t);
    setRenameDraft(t.initial);
  };

  const commitRename = async () => {
    if (!renaming) return;
    const next = renameDraft.trim();
    const target = renaming;
    setRenaming(null);
    if (!next || next === target.initial) return;
    const ok =
      target.kind === "folder"
        ? await withErrorToast("Could not rename folder", () =>
            api.renameFolder(target.id, next),
          )
        : await withErrorToast("Could not rename object", () =>
            api.updateObject(target.id, { name: next }),
          );
    if (ok.ok) onMutated();
  };

  const renameInputProps = {
    size: "xs" as const,
    autoFocus: true,
    value: renameDraft,
    onChange: (e: React.ChangeEvent<HTMLInputElement>) => setRenameDraft(e.currentTarget.value),
    onBlur: () => commitRename(),
    onKeyDown: (e: React.KeyboardEvent) => {
      if (e.key === "Enter") commitRename();
      if (e.key === "Escape") {
        setRenaming(null);
      }
    },
  };
  const tree = useMemo(() => buildTree(folders), [folders]);
  const objectsByFolder = useMemo(() => {
    const m = new Map<number | null, ObjectSummary[]>();
    objects.forEach((o) => {
      const k = o.folder_id ?? null;
      const arr = m.get(k) ?? [];
      arr.push(o);
      m.set(k, arr);
    });
    m.forEach((arr) => arr.sort((a, b) => a.name.localeCompare(b.name)));
    return m;
  }, [objects]);

  const activeObject = activeObjectId != null ? objects.find((o) => o.id === activeObjectId) : undefined;
  const activeFolderId = activeObject?.folder_id ?? selectedFolderId;

  // Auto-expand ancestors so the active folder/object is visible.
  useEffect(() => {
    if (activeFolderId == null) return;
    const anc = ancestorsOfFolder(folders, activeFolderId);
    setExpanded((prev) => {
      const next = new Set(prev);
      anc.forEach((id) => next.add(id));
      return next;
    });
  }, [activeFolderId, folders]);

  const toggle = (id: number) =>
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const addFolder = async (parent: number | null) => {
    if (!name.trim()) {
      setAdding(null);
      return;
    }
    const ok = await withErrorToast("Could not create folder", () =>
      api.createFolder(parent, name.trim()),
    );
    setName("");
    setAdding(null);
    if (ok.ok) onMutated();
  };

  const startObjectDrag = (e: React.PointerEvent<HTMLElement>, o: ObjectSummary) => {
    if (e.button !== 0) return;
    // preventDefault stops the browser's native drag (link/text/image drag), which on
    // Tauri 2 (dragDropEnabled: true) would be swallowed and never reach us.
    e.preventDefault();
    const node = e.currentTarget;
    try {
      node.setPointerCapture(e.pointerId);
    } catch {
      // best-effort; some Tauri webviews may not support all pointer types
    }

    const start: DragState = {
      id: o.id,
      name: o.name,
      startX: e.clientX,
      startY: e.clientY,
      x: e.clientX,
      y: e.clientY,
      started: false,
      hoverFolderId: undefined,
    };
    dragRef.current = start;
    setDrag(start);

    const computeHover = (x: number, y: number): number | null | undefined => {
      const el = document.elementFromPoint(x, y);
      const dropTarget = (el as Element | null)?.closest(
        "[data-drop-folder-id]",
      ) as HTMLElement | null;
      if (!dropTarget) return undefined;
      const v = dropTarget.dataset.dropFolderId!;
      return v === "root" ? null : Number(v);
    };

    const onMove = (ev: PointerEvent) => {
      const cur = dragRef.current;
      if (!cur || ev.pointerId !== e.pointerId) return;
      const x = ev.clientX;
      const y = ev.clientY;
      let started = cur.started;
      if (!started) {
        const dx = x - cur.startX;
        const dy = y - cur.startY;
        if (dx * dx + dy * dy >= DRAG_THRESHOLD_PX * DRAG_THRESHOLD_PX) {
          started = true;
        }
      }
      const hoverFolderId = started ? computeHover(x, y) : undefined;
      const next: DragState = { ...cur, x, y, started, hoverFolderId };
      dragRef.current = next;
      setDrag(next);
    };

    const cleanup = () => {
      window.removeEventListener("pointermove", onMove, true);
      window.removeEventListener("pointerup", onUp, true);
      window.removeEventListener("pointercancel", onUp, true);
      try {
        node.releasePointerCapture(e.pointerId);
      } catch {
        // ignore
      }
    };

    const onUp = async (ev: PointerEvent) => {
      if (ev.pointerId !== e.pointerId) return;
      cleanup();
      const cur = dragRef.current;
      dragRef.current = null;
      setDrag(null);
      if (!cur) return;

      // The browser fires a synthetic click right after pointerup. Because we
      // called setPointerCapture on pointerdown, that click is dispatched to the
      // wrapper div (the capture element) rather than the inner NavLink, so the
      // NavLink's onClick never runs. We therefore drive both taps (select) and
      // drags (move) from here, and swallow that stray click so it can't double-
      // fire or race our refresh with stale data fetched mid-move.
      const swallowClick = (ce: MouseEvent) => {
        ce.preventDefault();
        ce.stopPropagation();
        ce.stopImmediatePropagation();
        window.removeEventListener("click", swallowClick, true);
      };
      window.addEventListener("click", swallowClick, true);
      setTimeout(() => {
        window.removeEventListener("click", swallowClick, true);
      }, 50);
      suppressClickRef.current = true;
      setTimeout(() => {
        suppressClickRef.current = false;
      }, 0);

      // A tap (pointer didn't move past the drag threshold) opens the object —
      // or renames it if it's already the active one, matching the NavLink's old
      // click behavior.
      if (!cur.started) {
        if (activeObjectId === o.id) {
          startRename({ kind: "object", id: o.id, initial: o.name });
        } else {
          onSelectObject(o.id);
        }
        return;
      }

      const target = computeHover(ev.clientX, ev.clientY);
      if (target === undefined) return;
      const dragged = objects.find((x) => x.id === cur.id);
      if (dragged && (dragged.folder_id ?? null) === target) return;
      const ok = await withErrorToast("Could not move object", () =>
        api.moveObject(cur.id, target),
      );
      if (ok.ok) {
        // Expand the destination folder (and its ancestors) so the moved object
        // is immediately visible — otherwise a collapsed target hides the move
        // and looks like nothing happened.
        if (target != null) {
          const anc = ancestorsOfFolder(folders, target);
          setExpanded((prev) => {
            const next = new Set(prev);
            anc.forEach((id) => next.add(id));
            return next;
          });
        }
        onMutated();
      }
    };

    // Capture-phase listeners on window: fire before any child element can
    // stopPropagation, and unaffected by which element setPointerCapture targets.
    window.addEventListener("pointermove", onMove, true);
    window.addEventListener("pointerup", onUp, true);
    window.addEventListener("pointercancel", onUp, true);
  };

  const renderObject = (o: ObjectSummary, depth: number) => {
    if (renaming?.kind === "object" && renaming.id === o.id) {
      return (
        <TextInput
          key={`o-${o.id}`}
          {...renameInputProps}
          ml={8 + depth * 16}
          mr={8}
        />
      );
    }
    const isActive = activeObjectId === o.id;
    const isDragging = drag?.started && drag.id === o.id;
    return (
      <div
        key={`o-${o.id}`}
        data-drop-object-id={o.id}
        draggable={false}
        onPointerDown={(e) => startObjectDrag(e, o)}
        style={{
          opacity: isDragging ? 0.4 : 1,
          userSelect: "none",
          touchAction: "none",
          cursor: drag?.id === o.id && drag.started ? "grabbing" : "grab",
        }}
      >
        <NavLink
          style={{ paddingLeft: 8 + depth * 16 }}
          active={isActive}
          label={o.name}
          leftSection={<IconCube size={14} />}
          onClick={() => {
            // Synthetic click that follows a completed drag's mouseup — skip select/rename.
            if (suppressClickRef.current) return;
            if (isActive) startRename({ kind: "object", id: o.id, initial: o.name });
            else onSelectObject(o.id);
          }}
        />
      </div>
    );
  };

  const renderFolder = (n: FolderNode, depth: number) => {
    const isOpen = expanded.has(n.folder.id);
    const kids = n.children;
    const objs = objectsByFolder.get(n.folder.id) ?? [];
    const hasContent = kids.length > 0 || objs.length > 0;
    const isDropHover = drag?.started && drag.hoverFolderId === n.folder.id;
    return (
      // data-drop-folder-id wraps the entire folder subtree so drops on children
      // (objects, sub-folders, empty content area) all resolve to this folder.
      <div key={`f-${n.folder.id}`} data-drop-folder-id={n.folder.id}>
        <Group
          gap={0}
          wrap="nowrap"
          align="center"
          style={{
            backgroundColor: isDropHover ? "var(--mantine-color-blue-light)" : undefined,
            outline: isDropHover ? "2px dashed var(--mantine-color-blue-5)" : undefined,
            outlineOffset: -2,
            borderRadius: 4,
          }}
        >
          <ActionIcon
            variant="subtle"
            size="sm"
            onClick={() => toggle(n.folder.id)}
            style={{ marginLeft: 8 + depth * 16, visibility: hasContent ? "visible" : "hidden" }}
            aria-label={isOpen ? "Collapse" : "Expand"}
          >
            {isOpen ? <IconChevronDown size={12} /> : <IconChevronRight size={12} />}
          </ActionIcon>
          {renaming?.kind === "folder" && renaming.id === n.folder.id ? (
            <TextInput {...renameInputProps} style={{ flex: 1 }} mr={8} />
          ) : (
            <NavLink
              style={{ flex: 1 }}
              active={selectedFolderId === n.folder.id}
              label={n.folder.name}
              leftSection={isOpen ? <IconFolderOpen size={16} /> : <IconFolder size={16} />}
              onClick={() => {
                if (selectedFolderId === n.folder.id) {
                  startRename({ kind: "folder", id: n.folder.id, initial: n.folder.name });
                } else {
                  onSelectFolder(n.folder.id);
                }
              }}
            />
          )}
          <Tooltip label="New subfolder">
            <ActionIcon variant="subtle" size="sm" onClick={() => setAdding({ parent: n.folder.id })}>
              <IconFolderPlus size={14} />
            </ActionIcon>
          </Tooltip>
          <Tooltip label="Delete">
            <ActionIcon
              variant="subtle"
              color="red"
              size="sm"
              onClick={async () => {
                if (!(await ask(`Delete folder "${n.folder.name}"? Objects inside drop to root.`, { title: "Delete folder", kind: "warning" }))) return;
                const ok = await withErrorToast("Could not delete folder", () =>
                  api.deleteFolder(n.folder.id),
                );
                if (ok.ok) onMutated();
              }}
            >
              <IconTrash size={14} />
            </ActionIcon>
          </Tooltip>
        </Group>
        {adding?.parent === n.folder.id && (
          <TextInput
            size="xs"
            autoFocus
            placeholder="Folder name"
            value={name}
            onChange={(e) => setName(e.currentTarget.value)}
            onBlur={() => addFolder(n.folder.id)}
            onKeyDown={(e) => {
              if (e.key === "Enter") addFolder(n.folder.id);
              if (e.key === "Escape") {
                setAdding(null);
                setName("");
              }
            }}
            ml={8 + (depth + 1) * 16}
            mr={8}
          />
        )}
        {isOpen && (
          <>
            {kids.map((c) => renderFolder(c, depth + 1))}
            {objs.map((o) => renderObject(o, depth + 1))}
          </>
        )}
      </div>
    );
  };

  const rootObjects = objectsByFolder.get(null) ?? [];

  return (
    <Stack gap={0} style={{ minWidth: 240 }}>
      <Group justify="space-between" px="sm" py="xs">
        <Text fw={600} size="sm" c="dimmed">
          LIBRARY
        </Text>
        <Tooltip label="New folder">
          <ActionIcon variant="subtle" size="sm" onClick={() => setAdding({ parent: null })}>
            <IconFolderPlus size={14} />
          </ActionIcon>
        </Tooltip>
      </Group>
      <div
        data-drop-folder-id="root"
        style={{
          backgroundColor:
            drag?.started && drag.hoverFolderId === null
              ? "var(--mantine-color-blue-light)"
              : undefined,
          outline:
            drag?.started && drag.hoverFolderId === null
              ? "2px dashed var(--mantine-color-blue-5)"
              : undefined,
          outlineOffset: -2,
          borderRadius: 4,
        }}
      >
        <NavLink
          active={selectedFolderId === null && activeObjectId === null}
          label="All objects"
          leftSection={<IconHome size={16} />}
          onClick={() => onSelectFolder(null)}
        />
      </div>
      {adding?.parent === null && (
        <TextInput
          size="xs"
          autoFocus
          placeholder="Folder name"
          value={name}
          onChange={(e) => setName(e.currentTarget.value)}
          onBlur={() => addFolder(null)}
          onKeyDown={(e) => {
            if (e.key === "Enter") addFolder(null);
            if (e.key === "Escape") {
              setAdding(null);
              setName("");
            }
          }}
          mx="sm"
        />
      )}
      {tree.map((n) => renderFolder(n, 0))}
      {rootObjects.map((o) => renderObject(o, 0))}
      {drag?.started && (
        <div
          style={{
            position: "fixed",
            left: drag.x + 12,
            top: drag.y + 12,
            pointerEvents: "none",
            zIndex: 1000,
            background: "var(--mantine-color-body)",
            border: "1px solid var(--mantine-color-blue-5)",
            borderRadius: 4,
            padding: "2px 8px",
            fontSize: 12,
            boxShadow: "0 2px 8px rgba(0,0,0,0.2)",
            display: "flex",
            alignItems: "center",
            gap: 6,
          }}
        >
          <IconCube size={12} />
          {drag.name}
        </div>
      )}
    </Stack>
  );
}
