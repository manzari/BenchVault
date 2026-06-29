import {
  Button,
  Group,
  SegmentedControl,
  SimpleGrid,
  Skeleton,
  Stack,
  Text,
  Title,
} from "@mantine/core";
import { IconFolderOff, IconPlus, IconWorldDownload } from "@tabler/icons-react";
import { useEffect, useMemo } from "react";
import { ObjectCard } from "../components/ObjectCard";
import { AddObjectDialog } from "../components/AddObjectDialog";
import { ImportFromUrlDialog } from "../components/ImportFromUrlDialog";
import { EmptyState } from "../components/EmptyState";
import { LibraryDashboard } from "../components/LibraryDashboard";
import { useLibraryStore } from "../lib/stores/library";
import { useUiStore } from "../lib/stores/ui";

export function LibraryPage() {
  const folders = useLibraryStore((s) => s.folders);
  const selectedFolderId = useLibraryStore((s) => s.selectedFolderId);
  const objects = useLibraryStore((s) => s.objects);
  const allObjects = useLibraryStore((s) => s.allObjects);
  const loading = useLibraryStore((s) => s.loading);
  const refreshObjects = useLibraryStore((s) => s.refreshObjects);

  const open = useUiStore((s) => s.addObjectOpen);
  const setOpen = useUiStore((s) => s.setAddObjectOpen);
  const importOpen = useUiStore((s) => s.importUrlOpen);
  const setImportOpen = useUiStore((s) => s.setImportUrlOpen);
  const sort = useUiStore((s) => s.folderSort);
  const setSort = useUiStore((s) => s.setFolderSort);

  useEffect(() => {
    refreshObjects();
  }, []);

  const sortedObjects = useMemo(() => {
    const arr = [...objects];
    if (sort === "name") {
      arr.sort((a, b) => a.name.localeCompare(b.name));
    } else {
      arr.sort((a, b) => Number(b.updated_at) - Number(a.updated_at));
    }
    return arr;
  }, [objects, sort]);

  const folder = selectedFolderId == null ? null : folders.find((f) => f.id === selectedFolderId);
  const isDashboard = selectedFolderId == null;

  return (
    <Stack gap="md">
      <Group justify="space-between" align="flex-end" wrap="wrap">
        <Stack gap={2}>
          <Title order={2}>{isDashboard ? "Library" : folder?.name ?? "Folder"}</Title>
          <Text size="sm" c="dimmed">
            {isDashboard
              ? `${allObjects.length} object${allObjects.length === 1 ? "" : "s"} across ${folders.length} folder${
                  folders.length === 1 ? "" : "s"
                }`
              : `${objects.length} object${objects.length === 1 ? "" : "s"}`}
          </Text>
        </Stack>
        <Group gap="sm">
          {!isDashboard && objects.length > 1 && (
            <SegmentedControl
              size="xs"
              value={sort}
              onChange={(v) => setSort(v as typeof sort)}
              data={[
                { label: "Recent", value: "updated" },
                { label: "Name", value: "name" },
              ]}
            />
          )}
          <Button
            variant="default"
            leftSection={<IconWorldDownload size={16} />}
            onClick={() => setImportOpen(true)}
          >
            Import from URL
          </Button>
          <Button leftSection={<IconPlus size={16} />} onClick={() => setOpen(true)}>
            New object
          </Button>
        </Group>
      </Group>

      {isDashboard ? (
        <LibraryDashboard
          folders={folders}
          allObjects={allObjects}
          loading={loading && allObjects.length === 0}
          onAdd={() => setOpen(true)}
        />
      ) : loading && objects.length === 0 ? (
        <SimpleGrid cols={{ base: 1, sm: 2, md: 3, lg: 4 }} spacing="md">
          {Array.from({ length: 4 }).map((_, i) => (
            <Skeleton key={i} h={220} radius="md" />
          ))}
        </SimpleGrid>
      ) : objects.length === 0 ? (
        <EmptyState
          icon={<IconFolderOff size={28} />}
          title="No objects in this folder"
          description="Drop an STL onto this folder in the sidebar, or create one manually."
          action={
            <Button variant="light" leftSection={<IconPlus size={16} />} onClick={() => setOpen(true)}>
              Add your first object
            </Button>
          }
        />
      ) : (
        <SimpleGrid cols={{ base: 1, sm: 2, md: 3, lg: 4 }} spacing="md">
          {sortedObjects.map((o) => (
            <ObjectCard key={o.id} obj={o} />
          ))}
        </SimpleGrid>
      )}

      <AddObjectDialog
        opened={open}
        onClose={() => setOpen(false)}
        folders={folders}
        defaultFolderId={selectedFolderId}
        onCreated={refreshObjects}
      />
      <ImportFromUrlDialog
        opened={importOpen}
        onClose={() => setImportOpen(false)}
        folders={folders}
        defaultFolderId={selectedFolderId}
        onCreated={refreshObjects}
      />
    </Stack>
  );
}
