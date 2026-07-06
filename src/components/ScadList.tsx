import {
  ActionIcon,
  Button,
  Group,
  Stack,
  Table,
  Text,
  Title,
  Tooltip,
} from "@mantine/core";
import {
  IconEye,
  IconFileCode,
  IconPencil,
  IconPhoto,
  IconPlus,
  IconTrash,
} from "@tabler/icons-react";
import { useEffect, useState } from "react";
import { open, ask } from "@tauri-apps/plugin-dialog";
import { notifications } from "@mantine/notifications";
import type { AttachedAsset } from "../lib/types";
import { api } from "../lib/api";
import { notifyError, withErrorToast } from "../lib/errors";
import { renderStlToPng } from "../lib/stlRender";
import { formatBytes } from "../lib/format";
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

  const addScad = async () => {
    let selected: string | string[] | null;
    try {
      selected = await open({
        multiple: true,
        filters: [{ name: "OpenSCAD", extensions: ["scad"] }],
      });
    } catch (e) {
      notifyError("Could not open file picker", e);
      return;
    }
    if (!selected) return;
    const paths = Array.isArray(selected) ? selected : [selected];
    let added = 0;
    for (const p of paths) {
      const ok = await withErrorToast(`Failed to add ${baseName(p)}`, () =>
        api.addScad(objectId, p),
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
          OpenSCAD{" "}
          <Text component="span" c="dimmed" fw={500} size="sm">
            · {scads.length}
          </Text>
        </Title>
        <Button size="xs" variant="light" leftSection={<IconPlus size={14} />} onClick={addScad}>
          Add SCAD
        </Button>
      </Group>

      {scads.length === 0 ? (
        <EmptyState
          bare
          icon={<IconFileCode size={28} />}
          title="No OpenSCAD files yet"
          description="Drop a .scad file onto this object, or use the Add SCAD button."
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
              {scads.map((p, i) => (
                <Table.Tr key={p.asset.id}>
                  <Table.Td>
                    <Text size="sm" c="dimmed" ff="monospace">
                      {i + 1}
                    </Text>
                  </Table.Td>
                  <Table.Td>
                    <Text size="sm" truncate fw={500}>
                      {p.asset.original_filename}
                    </Text>
                  </Table.Td>
                  <Table.Td>
                    <Text size="sm" ff="monospace">
                      {formatBytes(p.asset.size_bytes)}
                    </Text>
                  </Table.Td>
                  <Table.Td>
                    <Group gap={4} wrap="nowrap" justify="flex-end">
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
                      <Tooltip label="Remove">
                        <ActionIcon
                          variant="subtle"
                          color="red"
                          size="sm"
                          aria-label="Remove"
                          onClick={async () => {
                            if (!(await ask(`Remove ${p.asset.original_filename}?`, { title: "Remove SCAD", kind: "warning" }))) return;
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
              ))}
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
