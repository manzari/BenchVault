import {
  ActionIcon,
  Anchor,
  AspectRatio,
  Box,
  Button,
  Divider,
  Grid,
  Group,
  Image,
  Menu,
  Paper,
  Select,
  Skeleton,
  Stack,
  Text,
  Title,
  Tooltip,
} from "@mantine/core";
import {
  IconChevronDown,
  IconCube,
  IconFolderOpen,
  IconPin,
  IconPinFilled,
  IconPlus,
  IconScissors,
  IconTrash,
  IconUpload,
} from "@tabler/icons-react";
import { useNavigate, useParams } from "@tanstack/react-router";
import { useCallback, useEffect, useState } from "react";
import { open as openDialog, ask } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import { notifications } from "@mantine/notifications";
import { api } from "../lib/api";
import { notifyError, withErrorToast } from "../lib/errors";
import type { ObjectFull } from "../lib/types";
import { StlViewer } from "../components/StlViewer";
import { StlPartsList } from "../components/StlPartsList";
import { ScadList } from "../components/ScadList";
import { MarkdownDocs } from "../components/MarkdownDocs";
import { GcodeList } from "../components/GcodeList";
import { AttachmentStrip } from "../components/AttachmentStrip";
import { TagInput, TagBadges } from "../components/TagInput";
import { EditableText, EditableTextarea } from "../components/Editable";
import { EmptyState } from "../components/EmptyState";
import { useLibraryStore } from "../lib/stores/library";

const MODEL_EXTS = ["stl", "3mf", "obj", "step", "stp"];

export function ObjectDetailPage() {
  const { objectId } = useParams({ from: "/objects/$objectId" });
  const id = Number(objectId);
  const navigate = useNavigate();
  const [data, setData] = useState<ObjectFull | null>(null);
  const [coverUrl, setCoverUrl] = useState<string | null>(null);
  const [orcaBusy, setOrcaBusy] = useState(false);
  // Bumped to ask the viewer to select a specific part (from the SCAD list).
  const [previewRequest, setPreviewRequest] = useState<{ id: number; nonce: number } | null>(null);

  const folders = useLibraryStore((s) => s.folders);
  const refreshAllObjects = useLibraryStore((s) => s.refreshAllObjects);
  const refreshObjects = useLibraryStore((s) => s.refreshObjects);

  const load = useCallback(async () => {
    try {
      const full = await api.getObject(id);
      setData(full);
    } catch (e) {
      notifyError("Could not load object", e);
    }
  }, [id]);

  // Primary load, keyed on id. Reset data and guard against an out-of-order
  // response: navigating A→B must never let A's slow response overwrite B's,
  // which would render — and let you save edits against — the wrong object.
  useEffect(() => {
    let cancelled = false;
    setData(null);
    (async () => {
      try {
        const full = await api.getObject(id);
        if (!cancelled) setData(full);
      } catch (e) {
        if (!cancelled) notifyError("Could not load object", e);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [id]);

  // After a cover change, refetch the object (updates the detail page) and the object
  // lists (updates card thumbnails elsewhere).
  const refreshAfterCover = useCallback(async () => {
    await load();
    await refreshAllObjects();
    await refreshObjects();
  }, [load, refreshAllObjects, refreshObjects]);

  useEffect(() => {
    const handler = (e: Event) => {
      const detail = (e as CustomEvent<{ id: number }>).detail;
      if (detail?.id === id) load();
    };
    window.addEventListener("gcf:object-mutated", handler);
    return () => window.removeEventListener("gcf:object-mutated", handler);
  }, [id, load]);

  // The file watcher ingested/changed files on disk (e.g. a slicer saved gcode into
  // this object's folder while the page is open). Refetch so new parts show up.
  useEffect(() => {
    const unlistenPromise = listen<{ kind: string }>("app-event", (e) => {
      if (e.payload.kind === "library_changed") load();
    });
    return () => {
      unlistenPromise.then((un) => un());
    };
  }, [load]);

  useEffect(() => {
    let cancelled = false;
    let url: string | null = null;
    if (data?.object.cover_asset_id != null) {
      api
        .readAssetBytes(data.object.cover_asset_id)
        .then((bytes) => {
          if (cancelled) return;
          url = URL.createObjectURL(new Blob([new Uint8Array(bytes)]));
          setCoverUrl(url);
        })
        .catch((e) => {
          if (!cancelled) notifyError("Could not load cover image", e);
        });
    } else {
      setCoverUrl(null);
    }
    return () => {
      cancelled = true;
      if (url) URL.revokeObjectURL(url);
    };
  }, [data?.object.cover_asset_id]);

  if (!data) {
    return <DetailSkeleton />;
  }

  const stls = data.assets.filter((a) => a.role === "stl_part");
  const gcodes = data.assets.filter((a) => a.role === "gcode");
  const scads = data.assets.filter((a) => a.role === "scad");
  const docs = data.assets.filter((a) => a.role === "document");
  // The 3D viewer parses STL (STLLoader) and OBJ (OBJLoader). Other model files
  // sharing the stl_part role (.3mf, .step) have no in-app loader, so keep them
  // out of the viewer — they still show in the parts list below.
  const previewableStls = stls.filter((a) => {
    const n = a.asset.original_filename.toLowerCase();
    return n.endsWith(".stl") || n.endsWith(".obj");
  });
  // SCAD entries render to STL on demand via `render_scad_preview`, so they can
  // ride in the same viewer alongside real STL parts.
  const previewParts = [...previewableStls, ...scads];

  // An object can hold several .3mf projects; one is the "main" (the user's pick,
  // else the most recent). With a project present the button opens it; otherwise
  // it builds a fresh one from the STL parts.
  const projects = data.assets.filter((a) =>
    a.asset.original_filename.toLowerCase().endsWith(".3mf"),
  );
  const mainProjectId = data.main_project_asset_id;
  const mainProject = projects.find((a) => a.asset.id === mainProjectId) ?? null;

  const patch = async (p: Parameters<typeof api.updateObject>[1]) => {
    const ok = await withErrorToast("Could not save changes", () => api.updateObject(id, p));
    if (!ok.ok) {
      // The backend rejected the patch; reload to revert local optimistic state.
      await load();
      return;
    }
    await load();
    await refreshAllObjects();
    await refreshObjects();
  };

  const addStl = async () => {
    let sel: string | string[] | null;
    try {
      sel = await openDialog({
        multiple: true,
        filters: [
          { name: "3D models", extensions: MODEL_EXTS },
          { name: "All files", extensions: ["*"] },
        ],
      });
    } catch (e) {
      notifyError("Could not open file picker", e);
      return;
    }
    if (!sel) return;
    const paths = Array.isArray(sel) ? sel : [sel];
    let added = 0;
    for (const p of paths) {
      // Route each pick to its proper section by extension; other picks (via
      // "All files") land as attachments (which itself sniffs cadquery .py).
      const lower = p.toLowerCase();
      const isModel = MODEL_EXTS.some((ext) => lower.endsWith(`.${ext}`));
      const ok = await withErrorToast(`Failed to add ${p}`, () =>
        isModel
          ? api.addStl(id, p)
          : lower.endsWith(".scad")
            ? api.addScad(id, p)
            : lower.endsWith(".py")
              ? api.addCadquery(id, p)
              : lower.endsWith(".md") || lower.endsWith(".markdown")
                ? api.addDoc(id, p)
                : api.addAttachment(id, p),
      );
      if (ok.ok) added += 1;
    }
    if (added > 0) await load();
  };

  const newOrcaProject = async () => {
    setOrcaBusy(true);
    const res = await withErrorToast("Could not open OrcaSlicer", () => api.newOrcaProject(id));
    setOrcaBusy(false);
    if (!res.ok) return;
    switch (res.value) {
      case "opened_existing":
        notifications.show({
          color: "blue",
          title: "Opened in OrcaSlicer",
          message: `Reopened the existing ${data.object.name}.3mf project.`,
        });
        break;
      case "built_and_opened":
        notifications.show({
          color: "green",
          title: "Opened in OrcaSlicer",
          message: `Built ${data.object.name}.3mf from the STL parts and opened it.`,
        });
        break;
      case "fell_back_to_stls":
        notifications.show({
          color: "yellow",
          title: "Loaded STLs in OrcaSlicer",
          message:
            "Couldn't build the project file automatically — loaded the STL parts instead. Use Save Project in Orca to keep it.",
        });
        break;
    }
    // The new .3mf lands in the watched library; give the watcher a beat, then reload.
    setTimeout(load, 1200);
  };

  const openProject = (assetId: number) =>
    withErrorToast("Could not open in OrcaSlicer", () => api.openAssetInOrca(assetId));

  const chooseMainProject = async (assetId: number) => {
    const ok = await withErrorToast("Could not set main project", () =>
      api.setMainProject(id, assetId),
    );
    if (ok.ok) await load();
  };

  const handleDelete = async () => {
    if (!(await ask(`Delete object "${data.object.name}"? It will be hidden from the library.`, { title: "Delete object", kind: "warning" }))) return;
    const ok = await withErrorToast("Could not delete object", () => api.deleteObject(id));
    if (!ok.ok) return;
    await refreshAllObjects();
    await refreshObjects();
    navigate({ to: "/" });
  };

  return (
    <Stack gap="md" data-drop-object-id={id}>
      {/* Header */}
      <Group justify="space-between" align="flex-start" wrap="nowrap" gap="md">
        <Stack gap={6} style={{ flex: 1, minWidth: 0 }}>
          <Box>
            <EditableText
              value={data.object.name}
              size="lg"
              fw={700}
              onSave={(next) => patch({ name: next || data.object.name })}
            />
          </Box>
          {data.tags.length > 0 && <TagBadges tags={data.tags} />}
        </Stack>
        <Group gap="xs">
          <Tooltip label="Open in file explorer">
            <ActionIcon
              variant="subtle"
              onClick={() =>
                withErrorToast("Could not open file explorer", () =>
                  api.revealObjectInExplorer(id),
                )
              }
              aria-label="Open in file explorer"
            >
              <IconFolderOpen size={18} />
            </ActionIcon>
          </Tooltip>
          <Tooltip label="Delete">
            <ActionIcon variant="subtle" color="red" onClick={handleDelete} aria-label="Delete">
              <IconTrash size={18} />
            </ActionIcon>
          </Tooltip>
        </Group>
      </Group>

      {/* Main: metadata sidebar + STL viewer */}
      <Grid gutter="md">
        <Grid.Col span={{ base: 12, md: 5, lg: 4 }}>
          <Paper withBorder p="md">
            <Stack gap="sm">
              <AspectRatio ratio={16 / 10}>
                {coverUrl ? (
                  <Image src={coverUrl} alt="cover" radius="sm" fit="cover" />
                ) : (
                  <div
                    style={{
                      borderRadius: "var(--mantine-radius-sm)",
                      background:
                        "linear-gradient(135deg, var(--mantine-color-violet-9) 0%, var(--mantine-color-dark-7) 100%)",
                      display: "flex",
                      alignItems: "center",
                      justifyContent: "center",
                    }}
                  >
                    <IconCube size={56} stroke={1.2} color="var(--mantine-color-violet-3)" opacity={0.5} />
                  </div>
                )}
              </AspectRatio>

              <Section label="Location">
                <Select
                  size="xs"
                  placeholder="(root)"
                  data={[
                    { value: "", label: "(root)" },
                    ...folders.map((f) => ({ value: String(f.id), label: f.name })),
                  ]}
                  value={data.object.folder_id != null ? String(data.object.folder_id) : ""}
                  onChange={async (v) => {
                    const ok = await withErrorToast("Could not move object", () =>
                      api.moveObject(id, v && v.length > 0 ? Number(v) : null),
                    );
                    if (!ok.ok) return;
                    await load();
                    await refreshAllObjects();
                    await refreshObjects();
                  }}
                  clearable
                />
              </Section>

              <Divider />

              <Section label="Notes">
                <Box className="gcf-inset" p="xs">
                  <EditableTextarea
                    value={data.object.notes}
                    placeholder="Add notes…"
                    onSave={(next) => patch({ notes: next })}
                  />
                </Box>
              </Section>

              <Section label="Source">
                <Group gap={4} wrap="nowrap" align="center">
                  <div style={{ flex: 1, minWidth: 0 }}>
                    <EditableText
                      value={data.object.source_url}
                      placeholder="https://…"
                      truncate
                      onSave={(next) => patch({ source_url: next })}
                    />
                  </div>
                  {data.object.source_url && (
                    <Anchor
                      href={data.object.source_url}
                      target="_blank"
                      size="xs"
                      onClick={(e) => e.stopPropagation()}
                    >
                      open
                    </Anchor>
                  )}
                </Group>
              </Section>

              <Section label="Author">
                <EditableText
                  value={data.object.author}
                  placeholder="Author"
                  onSave={(next) => patch({ author: next })}
                />
              </Section>

              <Section label="License">
                <EditableText
                  value={data.object.license}
                  placeholder="CC-BY-4.0"
                  onSave={(next) => patch({ license: next })}
                />
              </Section>

              <Divider />

              <Section label="Tags">
                <TagInput
                  entityKind="object"
                  entityId={id}
                  initialTags={data.tags}
                  onChanged={load}
                />
              </Section>
            </Stack>
          </Paper>
        </Grid.Col>

        <Grid.Col span={{ base: 12, md: 7, lg: 8 }}>
          <Paper withBorder p="md" style={{ minHeight: 540, height: "100%" }}>
            <Group justify="space-between" mb="sm">
              <Title order={4}>
                STL preview{" "}
                <Text component="span" c="dimmed" fw={500}>
                  · {previewParts.length} part{previewParts.length === 1 ? "" : "s"}
                </Text>
              </Title>
              <Group gap="xs">
                {projects.length > 0 ? (
                  <>
                    <Tooltip
                      label={
                        mainProject
                          ? `Open ${mainProject.asset.original_filename} in OrcaSlicer`
                          : "Open project in OrcaSlicer"
                      }
                    >
                      <Button
                        size="xs"
                        variant="light"
                        color="orange"
                        leftSection={<IconScissors size={14} />}
                        onClick={() => mainProjectId != null && openProject(mainProjectId)}
                      >
                        Open project in Orca
                      </Button>
                    </Tooltip>
                    <Menu position="bottom-end" withinPortal>
                      <Menu.Target>
                        <Button
                          size="xs"
                          variant="light"
                          color="orange"
                          px={6}
                          aria-label="Project options"
                        >
                          <IconChevronDown size={14} />
                        </Button>
                      </Menu.Target>
                      <Menu.Dropdown>
                        <Menu.Label>Main project</Menu.Label>
                        {projects.map((p) => (
                          <Menu.Item
                            key={p.asset.id}
                            leftSection={
                              p.asset.id === mainProjectId ? (
                                <IconPinFilled size={14} />
                              ) : (
                                <IconPin size={14} />
                              )
                            }
                            onClick={() => chooseMainProject(p.asset.id)}
                          >
                            {p.asset.original_filename}
                          </Menu.Item>
                        ))}
                        <Menu.Divider />
                        <Menu.Item
                          leftSection={<IconPlus size={14} />}
                          onClick={newOrcaProject}
                          disabled={stls.length === 0 || orcaBusy}
                        >
                          New project from STL parts
                        </Menu.Item>
                      </Menu.Dropdown>
                    </Menu>
                  </>
                ) : (
                  <Tooltip
                    label={
                      stls.length === 0
                        ? "Add STL parts first"
                        : "Create a new OrcaSlicer project from all STL parts"
                    }
                  >
                    <Button
                      size="xs"
                      variant="light"
                      color="orange"
                      leftSection={<IconScissors size={14} />}
                      onClick={newOrcaProject}
                      loading={orcaBusy}
                      disabled={stls.length === 0}
                    >
                      Create project in Orca
                    </Button>
                  </Tooltip>
                )}
                <Button size="xs" variant="light" leftSection={<IconUpload size={14} />} onClick={addStl}>
                  Add STL
                </Button>
              </Group>
            </Group>
            {previewParts.length > 0 ? (
              <StlViewer
                parts={previewParts}
                objectId={id}
                onCoverChanged={refreshAfterCover}
                previewRequest={previewRequest}
              />
            ) : (
              <EmptyState
                bare
                icon={<IconCube size={28} />}
                title="No STL parts yet"
                description="Drop an STL file here or use the Add STL button."
                action={
                  <Button size="xs" variant="light" leftSection={<IconUpload size={14} />} onClick={addStl}>
                    Add STL
                  </Button>
                }
              />
            )}
          </Paper>
        </Grid.Col>
      </Grid>

      {/* STL parts list */}
      <Paper withBorder p="md">
        <StlPartsList objectId={id} parts={stls} onMutated={load} onCoverChanged={refreshAfterCover} />
      </Paper>

      {/* CAD Scripts list (.scad + cadquery .py; only when present) */}
      {scads.length > 0 && (
        <Paper withBorder p="md">
          <ScadList
            objectId={id}
            scads={scads}
            onMutated={load}
            onCoverChanged={refreshAfterCover}
            onPreview={(asset) => setPreviewRequest({ id: asset.id, nonce: Date.now() })}
          />
        </Paper>
      )}

      {/* GCode list */}
      <Paper withBorder p="md">
        <GcodeList objectId={id} gcodes={gcodes} onMutated={load} />
      </Paper>

      {/* Docs / Notes (rendered markdown; only when present) */}
      {docs.length > 0 && (
        <Paper withBorder p="md">
          <MarkdownDocs
            objectId={id}
            docs={docs}
            images={data.assets.filter((a) => a.asset.kind === "image")}
            onMutated={load}
          />
        </Paper>
      )}

      {/* Attachments: images strip + files list */}
      <Paper withBorder p="md">
        <AttachmentStrip
          objectId={id}
          assets={data.assets}
          coverAssetId={data.object.cover_asset_id}
          mainProjectId={mainProjectId}
          onSetMainProject={chooseMainProject}
          onMutated={async () => {
            await load();
            await refreshAllObjects();
            await refreshObjects();
          }}
        />
      </Paper>
    </Stack>
  );
}

function Section({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <Stack gap={4}>
      <Text size="xs" c="dimmed" tt="uppercase" fw={600} lts={0.4}>
        {label}
      </Text>
      {children}
    </Stack>
  );
}

function DetailSkeleton() {
  return (
    <Stack gap="md">
      <Group justify="space-between" align="flex-start">
        <Stack gap={6} style={{ flex: 1 }}>
          <Skeleton h={28} w="40%" />
          <Skeleton h={16} w="20%" />
        </Stack>
        <Group gap="xs">
          <Skeleton h={28} w={28} circle />
          <Skeleton h={28} w={28} circle />
        </Group>
      </Group>
      <Grid gutter="md">
        <Grid.Col span={{ base: 12, md: 5, lg: 4 }}>
          <Skeleton h={520} radius="md" />
        </Grid.Col>
        <Grid.Col span={{ base: 12, md: 7, lg: 8 }}>
          <Skeleton h={520} radius="md" />
        </Grid.Col>
      </Grid>
      <Skeleton h={140} radius="md" />
    </Stack>
  );
}
