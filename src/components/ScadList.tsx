import {
  ActionIcon,
  Badge,
  Button,
  Code,
  Collapse,
  Group,
  Stack,
  Table,
  Text,
  Title,
  Tooltip,
} from "@mantine/core";
import {
  IconChevronRight,
  IconEye,
  IconFileCode,
  IconPencil,
  IconPhoto,
  IconPlus,
  IconTrash,
} from "@tabler/icons-react";
import { Fragment, useEffect, useState } from "react";
import { open, ask } from "@tauri-apps/plugin-dialog";
import { notifications } from "@mantine/notifications";
import type { AttachedAsset } from "../lib/types";
import { api } from "../lib/api";
import { notifyError, withErrorToast } from "../lib/errors";
import { renderStlToPng } from "../lib/stlRender";
import { formatBytes, formatDimensions, formatTriangleCount, formatVolume } from "../lib/format";
import {
  groupScadParameters,
  parseScadMetadata,
  scadHasDetail,
  scadSummary,
  type ScadMetadata,
} from "../lib/scad";
import { EmptyState } from "./EmptyState";

interface Props {
  objectId: number;
  scads: AttachedAsset[];
  onMutated: () => void;
  /** Called after the cover is updated from a rendered SCAD. */
  onCoverChanged?: () => void;
  /** Select this asset in the STL viewer (rendering it if needed). */
  onPreview: (asset: AttachedAsset["asset"]) => void;
}

export function ScadList({ objectId, scads, onMutated, onCoverChanged, onPreview }: Props) {
  // OpenSCAD is optional; when it's absent the render-dependent actions degrade.
  const [openscadAvailable, setOpenscadAvailable] = useState<boolean | null>(null);
  const [expanded, setExpanded] = useState<Set<number>>(new Set());

  const toggleExpanded = (id: number) =>
    setExpanded((prev) => {
      const next = new Set(prev);
      next.has(id) ? next.delete(id) : next.add(id);
      return next;
    });

  useEffect(() => {
    let cancelled = false;
    api
      .getOpenscadPath()
      .then((p) => {
        if (!cancelled) setOpenscadAvailable(p != null);
      })
      .catch(() => {
        if (!cancelled) setOpenscadAvailable(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const hasOpenscad = openscadAvailable !== false;

  const addScript = async () => {
    let selected: string | string[] | null;
    try {
      selected = await open({
        multiple: true,
        filters: [{ name: "CAD scripts", extensions: ["scad", "py"] }],
      });
    } catch (e) {
      notifyError("Could not open file picker", e);
      return;
    }
    if (!selected) return;
    const paths = Array.isArray(selected) ? selected : [selected];
    let added = 0;
    for (const p of paths) {
      // .py is routed through add_attachment, which sniffs it and only files it
      // as a CadQuery script when it actually imports cadquery.
      const isPy = p.toLowerCase().endsWith(".py");
      const ok = await withErrorToast(`Failed to add ${baseName(p)}`, () =>
        isPy ? api.addCadquery(objectId, p) : api.addScad(objectId, p),
      );
      if (ok.ok) added += 1;
    }
    if (added > 0) onMutated();
  };

  const makeCover = async (p: AttachedAsset) => {
    const res = await withErrorToast(
      `Failed to make cover from ${p.asset.original_filename}`,
      async () => {
        const stl = await api.renderScadPreview(p.asset.id);
        const png = renderStlToPng(stl);
        return api.setCoverFromRender(objectId, png, `cover-${p.asset.original_filename}.png`);
      },
    );
    if (res.ok) {
      notifications.show({
        color: "green",
        title: "Cover updated",
        message: `Rendered ${p.asset.original_filename} as the cover.`,
      });
      (onCoverChanged ?? onMutated)();
    }
  };

  const noOpenscad = "OpenSCAD not found — set the path in Settings";

  return (
    <Stack gap="sm">
      <Group justify="space-between" align="center">
        <Title order={4}>
          CAD Scripts{" "}
          <Text component="span" c="dimmed" fw={500} size="sm">
            · {scads.length}
          </Text>
        </Title>
        <Button size="xs" variant="light" leftSection={<IconPlus size={14} />} onClick={addScript}>
          Add script
        </Button>
      </Group>

      {scads.length === 0 ? (
        <EmptyState
          bare
          icon={<IconFileCode size={28} />}
          title="No CAD scripts yet"
          description="Drop a .scad or a CadQuery .py file onto this object, or use the Add script button."
        />
      ) : (
        <Table.ScrollContainer minWidth={520}>
          <Table verticalSpacing="sm" highlightOnHover>
            <Table.Thead>
              <Table.Tr>
                <Table.Th w={36}>#</Table.Th>
                <Table.Th>Name</Table.Th>
                <Table.Th w={120}>Size</Table.Th>
                <Table.Th w={150} />
              </Table.Tr>
            </Table.Thead>
            <Table.Tbody>
              {scads.map((p, i) => {
                const meta = parseScadMetadata(p.asset.metadata_json);
                const summary = scadSummary(meta);
                const hasDetail = scadHasDetail(meta);
                const isOpen = expanded.has(p.asset.id);
                const m = p.mesh_metadata;
                const tris = formatTriangleCount(m?.triangle_count);
                const geom = m
                  ? [
                      formatDimensions(m.bbox_x_mm, m.bbox_y_mm, m.bbox_z_mm),
                      formatVolume(m.volume_mm3),
                      tris ? `△ ${tris}` : null,
                      m.is_watertight == null
                        ? null
                        : m.is_watertight
                          ? "watertight"
                          : "not watertight",
                    ]
                      .filter((x) => x && x !== "—")
                      .join(" · ")
                  : "";
                return (
                <Fragment key={p.asset.id}>
                <Table.Tr>
                  <Table.Td>
                    <Text size="sm" c="dimmed" ff="monospace">
                      {i + 1}
                    </Text>
                  </Table.Td>
                  <Table.Td>
                    <Group gap={6} wrap="nowrap" align="flex-start">
                      {hasDetail ? (
                        <ActionIcon
                          variant="subtle"
                          color="gray"
                          size="sm"
                          aria-label={isOpen ? "Hide details" : "Show details"}
                          onClick={() => toggleExpanded(p.asset.id)}
                        >
                          <IconChevronRight
                            size={14}
                            style={{
                              transition: "transform 150ms",
                              transform: isOpen ? "rotate(90deg)" : "none",
                            }}
                          />
                        </ActionIcon>
                      ) : (
                        <span style={{ display: "inline-block", width: 28 }} />
                      )}
                      <div style={{ minWidth: 0 }}>
                        <Group gap={6} wrap="nowrap">
                          <Text size="sm" truncate fw={500}>
                            {p.asset.original_filename}
                          </Text>
                          <Badge size="xs" variant="light" color={p.asset.kind === "cadquery" ? "teal" : "grape"}>
                            {p.asset.kind === "cadquery" ? "CadQuery" : "OpenSCAD"}
                          </Badge>
                        </Group>
                        {geom && (
                          <Text size="xs" c="dimmed" ff="monospace">
                            {geom}
                          </Text>
                        )}
                        {summary && (
                          <Text size="xs" c="dimmed">
                            {summary}
                          </Text>
                        )}
                      </div>
                    </Group>
                  </Table.Td>
                  <Table.Td>
                    <Text size="sm" ff="monospace">
                      {formatBytes(p.asset.size_bytes)}
                    </Text>
                  </Table.Td>
                  <Table.Td>
                    <Group gap={4} wrap="nowrap" justify="flex-end">
                      {p.asset.kind === "scad" && (
                        <>
                          <Tooltip label={hasOpenscad ? "Preview render" : noOpenscad}>
                            <ActionIcon
                              variant="subtle"
                              size="sm"
                              aria-label="Preview render"
                              disabled={!hasOpenscad}
                              onClick={() => onPreview(p.asset)}
                            >
                              <IconEye size={14} />
                            </ActionIcon>
                          </Tooltip>
                          <Tooltip label={hasOpenscad ? "Open in OpenSCAD" : noOpenscad}>
                            <ActionIcon
                              variant="subtle"
                              color="grape"
                              size="sm"
                              aria-label="Open in OpenSCAD"
                              disabled={!hasOpenscad}
                              onClick={() =>
                                withErrorToast("Could not open in OpenSCAD", () =>
                                  api.openAssetInOpenscad(p.asset.id),
                                )
                              }
                            >
                              <IconPencil size={14} />
                            </ActionIcon>
                          </Tooltip>
                          <Tooltip label={hasOpenscad ? "Make cover from render" : noOpenscad}>
                            <ActionIcon
                              variant="subtle"
                              size="sm"
                              aria-label="Make cover from render"
                              disabled={!hasOpenscad}
                              onClick={() => makeCover(p)}
                            >
                              <IconPhoto size={14} />
                            </ActionIcon>
                          </Tooltip>
                        </>
                      )}
                      <Tooltip label="Remove">
                        <ActionIcon
                          variant="subtle"
                          color="red"
                          size="sm"
                          aria-label="Remove"
                          onClick={async () => {
                            if (!(await ask(`Remove ${p.asset.original_filename}?`, { title: "Remove script", kind: "warning" }))) return;
                            const ok = await withErrorToast(
                              `Failed to remove ${p.asset.original_filename}`,
                              () => api.removeAsset(p.asset.id),
                            );
                            if (ok.ok) onMutated();
                          }}
                        >
                          <IconTrash size={14} />
                        </ActionIcon>
                      </Tooltip>
                    </Group>
                  </Table.Td>
                </Table.Tr>
                {hasDetail && meta && (
                  <Table.Tr>
                    <Table.Td colSpan={4} p={0} style={{ borderTop: "none" }}>
                      <Collapse in={isOpen}>
                        <ScadDetail meta={meta} />
                      </Collapse>
                    </Table.Td>
                  </Table.Tr>
                )}
                </Fragment>
                );
              })}
            </Table.Tbody>
          </Table>
        </Table.ScrollContainer>
      )}
    </Stack>
  );
}

function baseName(p: string): string {
  const i = Math.max(p.lastIndexOf("/"), p.lastIndexOf("\\"));
  return i >= 0 ? p.slice(i + 1) : p;
}

function ScadDetail({ meta }: { meta: ScadMetadata }) {
  const deps = [...meta.includes, ...meta.uses];
  const groups = groupScadParameters(meta.parameters);
  return (
    <Stack gap="sm" px="md" py="sm" bg="var(--mantine-color-default-hover)">
      {meta.parameters.length > 0 && (
        <Stack gap={6}>
          {groups.map((g, gi) => (
            <div key={gi}>
              {g.group && (
                <Text size="xs" tt="uppercase" fw={700} c="dimmed" mb={2}>
                  {g.group}
                </Text>
              )}
              <Stack gap={2}>
                {g.params.map((param) => (
                  <Group key={param.name} gap={8} wrap="nowrap">
                    <Text size="xs" ff="monospace">
                      <Text span c="grape">
                        {param.name}
                      </Text>{" "}
                      = {param.default}
                    </Text>
                    {param.hint && (
                      <Text size="xs" c="dimmed" ff="monospace">
                        [{param.hint}]
                      </Text>
                    )}
                  </Group>
                ))}
              </Stack>
            </div>
          ))}
        </Stack>
      )}
      <MetaLine label="Depends on" items={deps} />
      <MetaLine label="Modules" items={meta.modules} />
      <MetaLine label="Functions" items={meta.functions} />
    </Stack>
  );
}

function MetaLine({ label, items }: { label: string; items: string[] }) {
  if (items.length === 0) return null;
  return (
    <Group gap={6} wrap="wrap" align="center">
      <Text size="xs" c="dimmed" fw={600}>
        {label}
      </Text>
      {items.map((it) => (
        <Code key={it}>{it}</Code>
      ))}
    </Group>
  );
}
