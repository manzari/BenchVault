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
import { IconCube, IconPhoto, IconPlus, IconScissors, IconTrash } from "@tabler/icons-react";
import { open as openDialog, ask } from "@tauri-apps/plugin-dialog";
import { notifications } from "@mantine/notifications";
import type { AttachedAsset } from "../lib/types";
import { api } from "../lib/api";
import { notifyError, withErrorToast } from "../lib/errors";
import { renderStlToPng } from "../lib/stlRender";
import { formatBytes } from "../lib/format";
import { EmptyState } from "./EmptyState";

interface Props {
  objectId: number;
  parts: AttachedAsset[];
  onMutated: () => void;
  /** Called after the cover is updated from an auto-generated part render. */
  onCoverChanged?: () => void;
}

export function StlPartsList({ objectId, parts, onMutated, onCoverChanged }: Props) {
  const setCoverFromPart = async (p: AttachedAsset) => {
    const res = await withErrorToast(
      `Failed to make cover from ${p.asset.original_filename}`,
      async () => {
        const bytes = await api.readAssetBytes(p.asset.id);
        const png = renderStlToPng(bytes);
        return api.setCoverFromRender(objectId, png, `cover-${p.asset.original_filename}.png`);
      },
    );
    if (res.ok) {
      notifications.show({ color: "green", title: "Cover updated", message: `Rendered ${p.asset.original_filename} as the cover.` });
      (onCoverChanged ?? onMutated)();
    }
  };

  const addStl = async () => {
    const modelExts = ["stl", "3mf", "obj"];
    let selected: string | string[] | null;
    try {
      selected = await openDialog({
        multiple: true,
        filters: [
          { name: "3D models", extensions: modelExts },
          { name: "All files", extensions: ["*"] },
        ],
      });
    } catch (e) {
      notifyError("Could not open file picker", e);
      return;
    }
    if (!selected) return;
    const paths = Array.isArray(selected) ? selected : [selected];
    let added = 0;
    for (const p of paths) {
      // .scad sources get their own asset kind; other non-model picks (via "All
      // files") land as attachments.
      const lower = p.toLowerCase();
      const isModel = modelExts.some((ext) => lower.endsWith(`.${ext}`));
      const ok = await withErrorToast(`Failed to add ${baseName(p)}`, () =>
        isModel
          ? api.addStl(objectId, p)
          : lower.endsWith(".scad")
            ? api.addScad(objectId, p)
            : api.addAttachment(objectId, p),
      );
      if (ok.ok) added += 1;
    }
    if (added > 0) onMutated();
  };

  return (
    <Stack gap="sm">
      <Group justify="space-between" align="center">
        <Title order={4}>
          STL parts{" "}
          <Text component="span" c="dimmed" fw={500} size="sm">
            · {parts.length}
          </Text>
        </Title>
        <Button size="xs" variant="light" leftSection={<IconPlus size={14} />} onClick={addStl}>
          Add STL
        </Button>
      </Group>

      {parts.length === 0 ? (
        <EmptyState
          bare
          icon={<IconCube size={28} />}
          title="No STL parts yet"
          description="Drop an STL file here or use the Add STL button."
        />
      ) : (
        <Table.ScrollContainer minWidth={520}>
          <Table verticalSpacing="sm" highlightOnHover>
            <Table.Thead>
              <Table.Tr>
                <Table.Th w={36}>#</Table.Th>
                <Table.Th>Name</Table.Th>
                <Table.Th w={120}>Size</Table.Th>
                <Table.Th w={110} />
              </Table.Tr>
            </Table.Thead>
            <Table.Tbody>
              {parts.map((p, i) => (
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
                      {p.asset.original_filename.toLowerCase().endsWith(".3mf") && (
                        <Tooltip label="Open in OrcaSlicer">
                          <ActionIcon
                            variant="subtle"
                            color="orange"
                            size="sm"
                            aria-label="Open in OrcaSlicer"
                            onClick={() =>
                              withErrorToast("Could not open in OrcaSlicer", () =>
                                api.openAssetInOrca(p.asset.id),
                              )
                            }
                          >
                            <IconScissors size={14} />
                          </ActionIcon>
                        </Tooltip>
                      )}
                      <Tooltip label="Make cover from render">
                        <ActionIcon
                          variant="subtle"
                          size="sm"
                          aria-label="Make cover from render"
                          onClick={() => setCoverFromPart(p)}
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
                            if (!(await ask(`Remove ${p.asset.original_filename}?`, { title: "Remove part", kind: "warning" }))) return;
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
