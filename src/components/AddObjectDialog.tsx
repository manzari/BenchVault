import { Button, Group, Modal, Select, Stack, TextInput, Textarea, Text } from "@mantine/core";
import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { IconFile3d, IconPhoto, IconUpload } from "@tabler/icons-react";
import type { Folder } from "../lib/types";
import { api } from "../lib/api";
import { ensureCoverFromStl } from "../lib/cover";
import { notifyError } from "../lib/errors";

interface Props {
  opened: boolean;
  onClose: () => void;
  folders: Folder[];
  defaultFolderId: number | null;
  onCreated: () => void;
}

export function AddObjectDialog({ opened, onClose, folders, defaultFolderId, onCreated }: Props) {
  const [name, setName] = useState("");
  const [folderId, setFolderId] = useState<string | null>(
    defaultFolderId != null ? String(defaultFolderId) : null,
  );
  const [stlPaths, setStlPaths] = useState<string[]>([]);
  const [coverPath, setCoverPath] = useState<string | null>(null);
  const [notes, setNotes] = useState("");
  const [sourceUrl, setSourceUrl] = useState("");
  const [author, setAuthor] = useState("");
  const [license, setLicense] = useState("");
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => {
    if (opened) {
      setFolderId(defaultFolderId != null ? String(defaultFolderId) : null);
    }
  }, [opened, defaultFolderId]);

  const reset = () => {
    setName("");
    setStlPaths([]);
    setCoverPath(null);
    setNotes("");
    setSourceUrl("");
    setAuthor("");
    setLicense("");
  };

  const pickStls = async () => {
    let sel: string | string[] | null;
    try {
      sel = await open({
        multiple: true,
        filters: [{ name: "3D models", extensions: ["stl", "3mf", "obj"] }],
      });
    } catch (e) {
      notifyError("Could not open file picker", e);
      return;
    }
    if (!sel) return;
    setStlPaths(Array.isArray(sel) ? sel : [sel]);
  };

  const pickCover = async () => {
    let sel: string | string[] | null;
    try {
      sel = await open({
        multiple: false,
        filters: [{ name: "Image", extensions: ["png", "jpg", "jpeg", "webp", "gif"] }],
      });
    } catch (e) {
      notifyError("Could not open file picker", e);
      return;
    }
    if (!sel) return;
    setCoverPath(typeof sel === "string" ? sel : null);
  };

  const submit = async () => {
    if (!name.trim()) return;
    setSubmitting(true);
    try {
      const created = await api.createObject({
        name: name.trim(),
        folder_id: folderId ? Number(folderId) : null,
        notes: notes.trim() || null,
        source_url: sourceUrl.trim() || null,
        author: author.trim() || null,
        license: license.trim() || null,
        stl_paths: stlPaths,
        cover_image_path: coverPath,
      });
      // No picture supplied → snapshot the first STL so the object has a cover.
      if (!coverPath) await ensureCoverFromStl(created.id);
      onCreated();
      reset();
      onClose();
    } catch (e) {
      // Leave the dialog open so the user can correct the input and retry.
      notifyError("Could not create object", e);
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Modal opened={opened} onClose={onClose} title="New object" size="lg" centered>
      <Stack>
        <TextInput
          label="Name"
          required
          value={name}
          onChange={(e) => setName(e.currentTarget.value)}
          autoFocus
        />
        <Select
          label="Folder"
          placeholder="(root)"
          data={[{ value: "", label: "(root)" }, ...folders.map((f) => ({ value: String(f.id), label: f.name }))]}
          value={folderId ?? ""}
          onChange={(v) => setFolderId(v && v.length > 0 ? v : null)}
          clearable
        />
        <Stack gap={4}>
          <Text size="sm" fw={500}>
            STL parts
          </Text>
          <Group gap="xs">
            <Button leftSection={<IconUpload size={14} />} variant="light" onClick={pickStls} size="xs">
              Choose STL files…
            </Button>
            <Text size="xs" c="dimmed">
              {stlPaths.length === 0 ? "none" : `${stlPaths.length} file${stlPaths.length === 1 ? "" : "s"}`}
            </Text>
          </Group>
          {stlPaths.map((p) => (
            <Group key={p} gap={4}>
              <IconFile3d size={14} />
              <Text size="xs" truncate>
                {p}
              </Text>
            </Group>
          ))}
        </Stack>
        <Stack gap={4}>
          <Text size="sm" fw={500}>
            Cover image (optional)
          </Text>
          <Group gap="xs">
            <Button leftSection={<IconPhoto size={14} />} variant="light" onClick={pickCover} size="xs">
              Choose image…
            </Button>
            <Text size="xs" c="dimmed" truncate>
              {coverPath || "none"}
            </Text>
          </Group>
        </Stack>
        <Textarea
          label="Notes"
          autosize
          minRows={2}
          value={notes}
          onChange={(e) => setNotes(e.currentTarget.value)}
        />
        <TextInput
          label="Source URL"
          placeholder="https://printables.com/…"
          value={sourceUrl}
          onChange={(e) => setSourceUrl(e.currentTarget.value)}
        />
        <Group grow>
          <TextInput label="Author" value={author} onChange={(e) => setAuthor(e.currentTarget.value)} />
          <TextInput
            label="License"
            placeholder="CC-BY-4.0"
            value={license}
            onChange={(e) => setLicense(e.currentTarget.value)}
          />
        </Group>
        <Group justify="flex-end">
          <Button variant="subtle" onClick={onClose}>
            Cancel
          </Button>
          <Button onClick={submit} loading={submitting} disabled={!name.trim()}>
            Create object
          </Button>
        </Group>
      </Stack>
    </Modal>
  );
}
