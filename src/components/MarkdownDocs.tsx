import {
  ActionIcon,
  Accordion,
  Button,
  Group,
  Loader,
  Stack,
  Text,
  Title,
  Tooltip,
  TypographyStylesProvider,
} from "@mantine/core";
import { IconFileText, IconPlus, IconTrash } from "@tabler/icons-react";
import { useEffect, useState } from "react";
import { open, ask } from "@tauri-apps/plugin-dialog";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type { AttachedAsset } from "../lib/types";
import { api } from "../lib/api";
import { notifyError, withErrorToast } from "../lib/errors";
import { formatBytes } from "../lib/format";
import { EmptyState } from "./EmptyState";

interface Props {
  objectId: number;
  docs: AttachedAsset[];
  onMutated: () => void;
}

export function MarkdownDocs({ objectId, docs, onMutated }: Props) {
  // The currently expanded doc, and a cache of already-loaded markdown text.
  const [open_, setOpen] = useState<string | null>(null);
  const [content, setContent] = useState<Map<number, string>>(() => new Map());
  const [loading, setLoading] = useState(false);

  const activeId = open_ != null ? Number(open_) : null;

  // Lazily fetch the expanded doc's text through the same read_asset_bytes IPC
  // the STL viewer uses, decoding the bytes to UTF-8.
  useEffect(() => {
    if (activeId == null || content.has(activeId)) return;
    let cancelled = false;
    setLoading(true);
    api
      .readAssetBytes(activeId)
      .then((bytes) => {
        if (cancelled) return;
        const text = new TextDecoder().decode(new Uint8Array(bytes));
        setContent((prev) => new Map(prev).set(activeId, text));
      })
      .catch((e) => {
        if (!cancelled) setContent((prev) => new Map(prev).set(activeId, `_Failed to load: ${String(e)}_`));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [activeId]);

  const addDoc = async () => {
    let selected: string | string[] | null;
    try {
      selected = await open({
        multiple: true,
        filters: [{ name: "Markdown", extensions: ["md", "markdown"] }],
      });
    } catch (e) {
      notifyError("Could not open file picker", e);
      return;
    }
    if (!selected) return;
    const paths = Array.isArray(selected) ? selected : [selected];
    let added = 0;
    for (const p of paths) {
      const ok = await withErrorToast(`Failed to add ${baseName(p)}`, () => api.addDoc(objectId, p));
      if (ok.ok) added += 1;
    }
    if (added > 0) onMutated();
  };

  const remove = async (p: AttachedAsset) => {
    if (!(await ask(`Remove ${p.asset.original_filename}?`, { title: "Remove document", kind: "warning" }))) return;
    const ok = await withErrorToast(`Failed to remove ${p.asset.original_filename}`, () =>
      api.removeAsset(p.asset.id),
    );
    if (ok.ok) onMutated();
  };

  return (
    <Stack gap="sm">
      <Group justify="space-between" align="center">
        <Title order={4}>
          Docs / Notes{" "}
          <Text component="span" c="dimmed" fw={500} size="sm">
            · {docs.length}
          </Text>
        </Title>
        <Button size="xs" variant="light" leftSection={<IconPlus size={14} />} onClick={addDoc}>
          Add doc
        </Button>
      </Group>

      {docs.length === 0 ? (
        <EmptyState
          bare
          icon={<IconFileText size={28} />}
          title="No docs yet"
          description="Drop a .md file onto this object, or use the Add doc button."
        />
      ) : (
        <Accordion variant="separated" value={open_} onChange={setOpen}>
          {docs.map((p) => (
            <Accordion.Item key={p.asset.id} value={String(p.asset.id)}>
              <Group gap={0} wrap="nowrap" pr="xs">
                <Accordion.Control>
                  <Group gap="xs" wrap="nowrap">
                    <IconFileText size={16} />
                    <Text size="sm" fw={500} truncate>
                      {p.asset.original_filename}
                    </Text>
                    <Text size="xs" c="dimmed" ff="monospace">
                      {formatBytes(p.asset.size_bytes)}
                    </Text>
                  </Group>
                </Accordion.Control>
                <Tooltip label="Remove">
                  <ActionIcon variant="subtle" color="red" size="sm" aria-label="Remove" onClick={() => remove(p)}>
                    <IconTrash size={14} />
                  </ActionIcon>
                </Tooltip>
              </Group>
              <Accordion.Panel>
                {activeId === p.asset.id && !content.has(p.asset.id) && loading ? (
                  <Group justify="center" py="md">
                    <Loader size="sm" />
                  </Group>
                ) : (
                  <TypographyStylesProvider>
                    <Markdown remarkPlugins={[remarkGfm]}>{content.get(p.asset.id) ?? ""}</Markdown>
                  </TypographyStylesProvider>
                )}
              </Accordion.Panel>
            </Accordion.Item>
          ))}
        </Accordion>
      )}
    </Stack>
  );
}

function baseName(p: string): string {
  const i = Math.max(p.lastIndexOf("/"), p.lastIndexOf("\\"));
  return i >= 0 ? p.slice(i + 1) : p;
}
