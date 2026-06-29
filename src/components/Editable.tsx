import { Text, TextInput, Textarea } from "@mantine/core";
import { useEffect, useRef, useState } from "react";

interface BaseProps {
  value: string | null;
  onSave: (next: string) => Promise<void> | void;
  placeholder?: string;
  emptyText?: string;
  size?: "xs" | "sm" | "md" | "lg";
  fw?: number;
  c?: string;
}

export function EditableText({
  value,
  onSave,
  placeholder,
  emptyText = "—",
  size = "sm",
  fw,
  c,
}: BaseProps) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(value ?? "");
  const ref = useRef<HTMLInputElement>(null);

  useEffect(() => {
    setDraft(value ?? "");
  }, [value]);

  useEffect(() => {
    if (editing) ref.current?.select();
  }, [editing]);

  const commit = async () => {
    const next = draft.trim();
    if (next !== (value ?? "")) await onSave(next);
    setEditing(false);
  };

  if (editing) {
    return (
      <TextInput
        ref={ref}
        size={size === "lg" ? "md" : "xs"}
        value={draft}
        placeholder={placeholder}
        onChange={(e) => setDraft(e.currentTarget.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === "Enter") commit();
          if (e.key === "Escape") {
            setDraft(value ?? "");
            setEditing(false);
          }
        }}
      />
    );
  }

  const isEmpty = value == null || value.length === 0;
  return (
    <Text
      size={size}
      fw={fw}
      c={isEmpty ? "dimmed" : c}
      style={{ cursor: "text", minHeight: "1.2em" }}
      onClick={() => setEditing(true)}
      title="Click to edit"
    >
      {isEmpty ? emptyText : value}
    </Text>
  );
}

export function EditableTextarea({
  value,
  onSave,
  placeholder,
  emptyText = "—",
  size = "sm",
}: BaseProps) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(value ?? "");
  const ref = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    setDraft(value ?? "");
  }, [value]);

  useEffect(() => {
    if (editing) ref.current?.select();
  }, [editing]);

  const commit = async () => {
    const next = draft.trim();
    if (next !== (value ?? "")) await onSave(next);
    setEditing(false);
  };

  if (editing) {
    return (
      <Textarea
        ref={ref}
        size="xs"
        autosize
        minRows={2}
        value={draft}
        placeholder={placeholder}
        onChange={(e) => setDraft(e.currentTarget.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) commit();
          if (e.key === "Escape") {
            setDraft(value ?? "");
            setEditing(false);
          }
        }}
      />
    );
  }

  const isEmpty = value == null || value.length === 0;
  return (
    <Text
      size={size}
      c={isEmpty ? "dimmed" : undefined}
      style={{ cursor: "text", whiteSpace: "pre-wrap", minHeight: "1.2em" }}
      onClick={() => setEditing(true)}
      title="Click to edit (Ctrl+Enter to save)"
    >
      {isEmpty ? emptyText : value}
    </Text>
  );
}
