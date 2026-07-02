import { Badge, Group, TagsInput } from "@mantine/core";
import { useEffect, useState } from "react";
import { api } from "../lib/api";
import { notifyError } from "../lib/errors";
import type { Tag } from "../lib/types";

interface Props {
  entityKind: string;
  entityId: number;
  initialTags: Tag[];
  onChanged?: () => void;
}

export function TagInput({ entityKind, entityId, initialTags, onChanged }: Props) {
  const [available, setAvailable] = useState<Tag[]>([]);
  const [selected, setSelected] = useState<string[]>(initialTags.map((t) => t.name));

  useEffect(() => {
    api
      .listTags()
      .then(setAvailable)
      .catch((e) => notifyError("Failed to load tags", e));
  }, []);

  useEffect(() => {
    setSelected(initialTags.map((t) => t.name));
  }, [initialTags.map((t) => t.id).join(",")]);

  const handleChange = async (next: string[]) => {
    const prev = selected;
    // Optimistically update the UI so typing feels instant. We roll back to
    // `prev` if any backend call fails.
    setSelected(next);

    try {
      // Attach any names that weren't there before.
      for (const name of next) {
        if (prev.includes(name)) continue;
        let tag = available.find((t) => t.name.toLowerCase() === name.toLowerCase());
        if (!tag) {
          tag = await api.createTag(name, null);
          setAvailable((a) => [...a, tag!]);
        }
        await api.attachTag(entityKind, entityId, tag.id);
      }
      // Detach anything that was removed. Match case-insensitively to mirror the
      // attach side — otherwise a tag typed as "PLA" but stored as "pla" can
      // never be removed (the lookup misses and the chip reappears on reload).
      for (const name of prev) {
        if (next.includes(name)) continue;
        const tag = available.find((t) => t.name.toLowerCase() === name.toLowerCase());
        if (tag) await api.detachTag(entityKind, entityId, tag.id);
      }
      onChanged?.();
    } catch (e) {
      notifyError("Failed to update tags", e);
      // Roll the optimistic state back so the chip set matches the backend.
      setSelected(prev);
    }
  };

  return (
    <TagsInput
      label="Tags"
      placeholder="Type and press Enter…"
      data={available.map((t) => t.name)}
      value={selected}
      onChange={handleChange}
      clearable
    />
  );
}

export function TagBadges({ tags }: { tags: Tag[] }) {
  if (tags.length === 0) return null;
  return (
    <Group gap={4}>
      {tags.map((t) => (
        <Badge key={t.id} color={t.color || undefined} variant="light">
          {t.name}
        </Badge>
      ))}
    </Group>
  );
}
