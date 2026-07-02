import {
  ActionIcon,
  Badge,
  Box,
  Button,
  Group,
  Menu,
  Modal,
  Paper,
  ScrollArea,
  Stack,
  Text,
  Title,
  Tooltip,
} from "@mantine/core";
import {
  IconCube,
  IconDots,
  IconFile,
  IconFileText,
  IconPhoto,
  IconPhotoPlus,
  IconPin,
  IconPinFilled,
  IconScissors,
  IconStar,
  IconStarFilled,
  IconTrash,
  IconUpload,
} from "@tabler/icons-react";
import { useState } from "react";
import { open as openDialog, ask } from "@tauri-apps/plugin-dialog";
import type { AttachedAsset } from "../lib/types";
import { api } from "../lib/api";
import { notifyError, withErrorToast } from "../lib/errors";
import { formatBytes } from "../lib/format";
import { AssetImage } from "./AssetImage";
import { EmptyState } from "./EmptyState";

interface Props {
  objectId: number;
  assets: AttachedAsset[];
  coverAssetId: number | null;
  /** The object's main `.3mf` project (highlighted in the files list). */
  mainProjectId?: number | null;
  /** Pin a `.3mf` as the main project. */
  onSetMainProject?: (assetId: number) => void | Promise<void>;
  onMutated: () => void | Promise<void>;
}

const IMAGE_EXTS = ["png", "jpg", "jpeg", "webp", "gif", "bmp", "svg"];

function basename(p: string): string {
  const i = Math.max(p.lastIndexOf("/"), p.lastIndexOf("\\"));
  return i >= 0 ? p.slice(i + 1) : p;
}

export function AttachmentStrip({
  objectId,
  assets,
  coverAssetId,
  mainProjectId,
  onSetMainProject,
  onMutated,
}: Props) {
  const [lightbox, setLightbox] = useState<AttachedAsset | null>(null);

  const attachments = assets.filter((a) => a.role === "attachment");
  const images = attachments.filter((a) => a.asset.kind === "image");
  const files = attachments.filter((a) => a.asset.kind !== "image");

  const pickAndAdd = async (filters?: { name: string; extensions: string[] }[]) => {
    let sel: string | string[] | null;
    try {
      sel = await openDialog({ multiple: true, filters });
    } catch (e) {
      notifyError("Could not open file picker", e);
      return;
    }
    if (!sel) return;
    const paths = Array.isArray(sel) ? sel : [sel];
    let added = 0;
    for (const p of paths) {
      const ok = await withErrorToast(`Failed to attach ${basename(p)}`, () =>
        api.addAttachment(objectId, p),
      );
      if (ok.ok) added += 1;
    }
    if (added > 0) await onMutated();
  };

  const addImage = () => pickAndAdd([{ name: "Images", extensions: IMAGE_EXTS }]);
  const addFile = () => pickAndAdd();

  const setCover = async (assetId: number) => {
    const ok = await withErrorToast("Failed to set cover", () =>
      api.setCoverFromAsset(objectId, assetId),
    );
    if (ok.ok) await onMutated();
  };

  const remove = async (a: AttachedAsset) => {
    if (!(await ask(`Remove ${a.asset.original_filename}?`, { title: "Remove attachment", kind: "warning" }))) return;
    const ok = await withErrorToast(`Failed to remove ${a.asset.original_filename}`, () =>
      api.removeAsset(a.asset.id),
    );
    if (ok.ok) await onMutated();
  };

  const openInOrca = (a: AttachedAsset) =>
    withErrorToast("Could not open in OrcaSlicer", () => api.openAssetInOrca(a.asset.id));

  return (
    <Stack gap="lg">
      {/* Images section */}
      <Stack gap="sm">
        <Group justify="space-between" align="center">
          <Title order={4}>
            Images{" "}
            <Text component="span" c="dimmed" fw={500} size="sm">
              · {images.length}
            </Text>
          </Title>
          <Button size="xs" variant="light" leftSection={<IconPhotoPlus size={14} />} onClick={addImage}>
            Add image
          </Button>
        </Group>

        {images.length === 0 ? (
          <EmptyState
            bare
            icon={<IconPhoto size={28} />}
            title="No images yet"
            description="Add reference photos or renders to display alongside this object."
          />
        ) : (
          <ScrollArea type="auto" offsetScrollbars>
            <Group wrap="nowrap" gap="sm" p={2}>
              {images.map((a) => (
                <ImageCard
                  key={a.asset.id}
                  attached={a}
                  isCover={coverAssetId === a.asset.id}
                  onOpen={() => setLightbox(a)}
                  onSetCover={() => setCover(a.asset.id)}
                  onRemove={() => remove(a)}
                />
              ))}
            </Group>
          </ScrollArea>
        )}
      </Stack>

      {/* Files section */}
      <Stack gap="sm">
        <Group justify="space-between" align="center">
          <Title order={4}>
            Files{" "}
            <Text component="span" c="dimmed" fw={500} size="sm">
              · {files.length}
            </Text>
          </Title>
          <Button size="xs" variant="light" leftSection={<IconUpload size={14} />} onClick={addFile}>
            Add file
          </Button>
        </Group>

        {files.length === 0 ? (
          <EmptyState
            bare
            icon={<IconFile size={28} />}
            title="No additional files yet"
            description="Attach PDFs, CAD sources, notes, or any other related files."
          />
        ) : (
          <Stack gap={4}>
            {files.map((a) => {
              const is3mf = a.asset.original_filename.toLowerCase().endsWith(".3mf");
              return (
                <FileRow
                  key={a.asset.id}
                  attached={a}
                  onRemove={() => remove(a)}
                  onOpenInOrca={is3mf ? () => openInOrca(a) : undefined}
                  isMainProject={is3mf && a.asset.id === mainProjectId}
                  onSetMainProject={
                    is3mf && onSetMainProject && a.asset.id !== mainProjectId
                      ? () => onSetMainProject(a.asset.id)
                      : undefined
                  }
                />
              );
            })}
          </Stack>
        )}
      </Stack>

      <Modal
        opened={lightbox !== null}
        onClose={() => setLightbox(null)}
        size="xl"
        centered
        padding="xs"
        title={lightbox?.asset.original_filename ?? ""}
      >
        {lightbox && (
          <Box style={{ maxHeight: "80vh", display: "flex", justifyContent: "center" }}>
            <AssetImage assetId={lightbox.asset.id} alt={lightbox.asset.original_filename} fit="contain" height="80vh" />
          </Box>
        )}
      </Modal>
    </Stack>
  );
}

function ImageCard({
  attached,
  isCover,
  onOpen,
  onSetCover,
  onRemove,
}: {
  attached: AttachedAsset;
  isCover: boolean;
  onOpen: () => void;
  onSetCover: () => void;
  onRemove: () => void;
}) {
  const SIZE = 140;
  return (
    <Paper
      withBorder
      radius="sm"
      style={{
        position: "relative",
        flex: "0 0 auto",
        width: SIZE,
        height: SIZE,
        overflow: "hidden",
        cursor: "pointer",
      }}
      onClick={onOpen}
    >
      <AssetImage assetId={attached.asset.id} alt={attached.asset.original_filename} fit="cover" height={SIZE} width={SIZE} />

      {isCover && (
        <Badge
          size="xs"
          color="violet"
          variant="filled"
          leftSection={<IconStarFilled size={10} />}
          style={{ position: "absolute", top: 6, left: 6, pointerEvents: "none" }}
        >
          cover
        </Badge>
      )}

      <Box style={{ position: "absolute", top: 4, right: 4 }} onClick={(e) => e.stopPropagation()}>
        <Menu position="bottom-end" withinPortal>
          <Menu.Target>
            <ActionIcon
              variant="filled"
              color="dark"
              size="sm"
              aria-label="Image actions"
              style={{ opacity: 0.85 }}
            >
              <IconDots size={14} />
            </ActionIcon>
          </Menu.Target>
          <Menu.Dropdown>
            <Menu.Item
              leftSection={<IconStar size={14} />}
              onClick={onSetCover}
              disabled={isCover}
            >
              {isCover ? "Already the cover" : "Set as cover"}
            </Menu.Item>
            <Menu.Divider />
            <Menu.Item color="red" leftSection={<IconTrash size={14} />} onClick={onRemove}>
              Delete
            </Menu.Item>
          </Menu.Dropdown>
        </Menu>
      </Box>
    </Paper>
  );
}

function FileRow({
  attached,
  onRemove,
  onOpenInOrca,
  isMainProject,
  onSetMainProject,
}: {
  attached: AttachedAsset;
  onRemove: () => void;
  onOpenInOrca?: () => void;
  isMainProject?: boolean;
  onSetMainProject?: () => void;
}) {
  const Icon = iconForKind(attached.asset.kind);
  return (
    <Paper withBorder p="xs" radius="sm">
      <Group gap="sm" wrap="nowrap">
        <Icon size={20} style={{ flexShrink: 0, color: "var(--mantine-color-dimmed)" }} />
        <Box style={{ flex: 1, minWidth: 0 }}>
          <Group gap={6} wrap="nowrap">
            <Text size="sm" fw={500} truncate>
              {attached.asset.original_filename}
            </Text>
            {isMainProject && (
              <Badge
                size="xs"
                color="orange"
                variant="light"
                leftSection={<IconPinFilled size={10} />}
                style={{ flexShrink: 0 }}
              >
                main
              </Badge>
            )}
          </Group>
          <Text size="xs" c="dimmed">
            {formatBytes(attached.asset.size_bytes)}
            {attached.asset.kind !== "other" ? ` · ${attached.asset.kind}` : ""}
          </Text>
        </Box>
        {onSetMainProject && (
          <Tooltip label="Set as main project">
            <ActionIcon
              variant="subtle"
              color="orange"
              size="sm"
              onClick={onSetMainProject}
              aria-label="Set as main project"
            >
              <IconPin size={14} />
            </ActionIcon>
          </Tooltip>
        )}
        {onOpenInOrca && (
          <Tooltip label="Open in OrcaSlicer">
            <ActionIcon
              variant="subtle"
              color="orange"
              size="sm"
              onClick={onOpenInOrca}
              aria-label="Open in OrcaSlicer"
            >
              <IconScissors size={14} />
            </ActionIcon>
          </Tooltip>
        )}
        <Tooltip label="Remove">
          <ActionIcon variant="subtle" color="red" size="sm" onClick={onRemove} aria-label="Remove">
            <IconTrash size={14} />
          </ActionIcon>
        </Tooltip>
      </Group>
    </Paper>
  );
}

function iconForKind(kind: string) {
  switch (kind) {
    case "cad":
      return IconCube;
    case "image":
      return IconPhoto;
    case "stl":
      return IconCube;
    case "gcode":
      return IconFileText;
    default:
      return IconFile;
  }
}
