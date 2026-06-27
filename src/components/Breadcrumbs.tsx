import { Anchor, Breadcrumbs as MBreadcrumbs, Text } from "@mantine/core";
import { IconHome } from "@tabler/icons-react";
import { useNavigate } from "@tanstack/react-router";
import type { Folder } from "../lib/types";
import { useLibraryStore } from "../lib/stores/library";

interface Crumb {
  label: string;
  onClick?: () => void;
  icon?: React.ReactNode;
  current?: boolean;
}

function buildFolderPath(folders: Folder[], folderId: number | null): Folder[] {
  if (folderId == null) return [];
  const byId = new Map(folders.map((f) => [f.id as number, f]));
  const path: Folder[] = [];
  let cursor: Folder | undefined = byId.get(folderId);
  while (cursor) {
    path.unshift(cursor);
    cursor = cursor.parent_id != null ? byId.get(cursor.parent_id as number) : undefined;
  }
  return path;
}

interface Props {
  /** When set, render an object detail breadcrumb chain ending in this name. */
  objectName?: string | null;
  /** Folder id of the current object (for chain). Falls back to selectedFolderId. */
  objectFolderId?: number | null;
}

export function Breadcrumbs({ objectName, objectFolderId }: Props) {
  const folders = useLibraryStore((s) => s.folders);
  const selectedFolderId = useLibraryStore((s) => s.selectedFolderId);
  const selectFolder = useLibraryStore((s) => s.selectFolder);
  const navigate = useNavigate();

  const folderId = objectName != null ? objectFolderId ?? null : selectedFolderId;
  const path = buildFolderPath(folders, folderId);

  const crumbs: Crumb[] = [
    {
      label: "Library",
      icon: <IconHome size={14} />,
      onClick: async () => {
        await selectFolder(null);
        navigate({ to: "/" });
      },
    },
    ...path.map<Crumb>((f) => ({
      label: f.name,
      onClick: async () => {
        await selectFolder(f.id as number);
        navigate({ to: "/" });
      },
    })),
  ];

  if (objectName) crumbs.push({ label: objectName, current: true });

  return (
    <MBreadcrumbs separator="/" separatorMargin={6} styles={{ separator: { color: "var(--mantine-color-dimmed)" } }}>
      {crumbs.map((c, i) =>
        c.current ? (
          <Text key={i} size="sm" fw={500} truncate maw={320}>
            {c.label}
          </Text>
        ) : (
          <Anchor
            key={i}
            size="sm"
            c="dimmed"
            underline="hover"
            onClick={(e) => {
              e.preventDefault();
              c.onClick?.();
            }}
            href="#"
            style={{ display: "inline-flex", alignItems: "center", gap: 4 }}
          >
            {c.icon}
            {c.label}
          </Anchor>
        ),
      )}
    </MBreadcrumbs>
  );
}
