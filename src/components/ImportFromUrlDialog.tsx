import {
  Alert,
  Button,
  Group,
  Modal,
  Select,
  Stack,
  Text,
  TextInput,
} from "@mantine/core";
import { notifications } from "@mantine/notifications";
import { IconBrandThingiverse, IconKey, IconWorldDownload } from "@tabler/icons-react";
import { useEffect, useState } from "react";
import type { Folder } from "../lib/types";
import { api } from "../lib/api";
import { ensureCoverFromStl } from "../lib/cover";

interface Props {
  opened: boolean;
  onClose: () => void;
  folders: Folder[];
  defaultFolderId: number | null;
  onCreated: () => void;
}

const PLACEHOLDER = "https://www.thingiverse.com/thing:763622";

export function ImportFromUrlDialog({
  opened,
  onClose,
  folders,
  defaultFolderId,
  onCreated,
}: Props) {
  const [url, setUrl] = useState("");
  const [folderId, setFolderId] = useState<string | null>(
    defaultFolderId != null ? String(defaultFolderId) : null,
  );
  const [submitting, setSubmitting] = useState(false);
  const [hasToken, setHasToken] = useState<boolean | null>(null);

  useEffect(() => {
    if (!opened) return;
    setFolderId(defaultFolderId != null ? String(defaultFolderId) : null);
    setUrl("");
    api
      .getThingiverseToken()
      .then((t) => setHasToken(!!t))
      .catch(() => setHasToken(false));
  }, [opened, defaultFolderId]);

  const submit = async () => {
    const trimmed = url.trim();
    if (!trimmed) return;
    setSubmitting(true);
    try {
      const report = await api.importFromUrl(trimmed, folderId ? Number(folderId) : null);
      const parts: string[] = [];
      if (report.model_files_imported > 0) {
        parts.push(
          `${report.model_files_imported} model file${report.model_files_imported === 1 ? "" : "s"}`,
        );
      }
      if (report.cover_imported) parts.push("cover image");
      if (report.files_skipped > 0) parts.push(`${report.files_skipped} skipped`);
      const summary = parts.length > 0 ? parts.join(", ") : "metadata only";

      // No cover image came with the import → snapshot the first STL part.
      if (!report.cover_imported) await ensureCoverFromStl(report.object.id);

      notifications.show({
        color: report.auth_required ? "yellow" : "green",
        title: report.auth_required ? "Imported metadata only" : `Imported "${report.object.name}"`,
        message: report.warning ?? summary,
      });
      onCreated();
      onClose();
    } catch (e) {
      notifications.show({ color: "red", title: "Import failed", message: String(e) });
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Modal
      opened={opened}
      onClose={onClose}
      title={
        <Group gap="xs">
          <IconWorldDownload size={18} />
          <Text fw={600}>Import from URL</Text>
        </Group>
      }
      size="lg"
      centered
    >
      <Stack>
        <Text size="sm" c="dimmed">
          Paste a Thingiverse <Text span ff="monospace">thing:NNN</Text> URL. We'll fetch the
          title, author, license, and cover image from the public page.
        </Text>

        <TextInput
          label="URL"
          placeholder={PLACEHOLDER}
          value={url}
          onChange={(e) => setUrl(e.currentTarget.value)}
          autoFocus
          leftSection={<IconBrandThingiverse size={16} />}
        />

        <Select
          label="Destination folder"
          placeholder="(root)"
          data={[
            { value: "", label: "(root)" },
            ...folders.map((f) => ({ value: String(f.id), label: f.name })),
          ]}
          value={folderId ?? ""}
          onChange={(v) => setFolderId(v && v.length > 0 ? v : null)}
          clearable
        />

        {hasToken === false && (
          <Alert color="yellow" icon={<IconKey size={16} />} title="No Thingiverse API token">
            Without a token, only the page metadata and cover image are imported — no STL/3MF
            files. Add a token in <Text span fw={600}>Settings → Thingiverse</Text> to enable
            full file downloads.
          </Alert>
        )}

        <Group justify="flex-end">
          <Button variant="subtle" onClick={onClose}>
            Cancel
          </Button>
          <Button onClick={submit} loading={submitting} disabled={!url.trim()}>
            Import
          </Button>
        </Group>
      </Stack>
    </Modal>
  );
}
