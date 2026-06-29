import { Group, Paper, SimpleGrid, Skeleton, Stack, Text, Title } from "@mantine/core";
import {
  Icon3dCubeSphere,
  IconCube,
  IconFileCode,
  IconFolder,
  IconPlus,
  IconSparkles,
} from "@tabler/icons-react";
import type { ReactNode } from "react";
import type { Folder, ObjectSummary } from "../lib/types";
import { ObjectCard } from "./ObjectCard";
import { EmptyState } from "./EmptyState";

interface Props {
  folders: Folder[];
  allObjects: ObjectSummary[];
  loading: boolean;
  onAdd: () => void;
}

export function LibraryDashboard({ folders, allObjects, loading, onAdd }: Props) {
  if (allObjects.length === 0 && !loading) {
    return (
      <EmptyState
        icon={<IconSparkles size={28} />}
        title="Your library is empty"
        description="Drop an STL anywhere in the window to start a new object — or click below."
        action={
          <button
            type="button"
            onClick={onAdd}
            style={{
              display: "inline-flex",
              alignItems: "center",
              gap: 6,
              padding: "8px 14px",
              borderRadius: "var(--mantine-radius-md)",
              background: "var(--mantine-color-violet-6)",
              color: "white",
              border: 0,
              cursor: "pointer",
              fontWeight: 600,
              fontSize: 14,
            }}
          >
            <IconPlus size={16} /> New object
          </button>
        }
      />
    );
  }

  const totalStls = allObjects.reduce((acc, o) => acc + Number(o.stl_count || 0), 0);
  const totalGcodes = allObjects.reduce((acc, o) => acc + Number(o.gcode_count || 0), 0);

  const recent = [...allObjects]
    .sort((a, b) => Number(b.updated_at) - Number(a.updated_at))
    .slice(0, 8);

  return (
    <Stack gap="lg">
      <SimpleGrid cols={{ base: 2, sm: 4 }} spacing="md">
        {loading ? (
          Array.from({ length: 4 }).map((_, i) => <Skeleton key={i} h={92} radius="md" />)
        ) : (
          <>
            <StatCard label="Objects" value={allObjects.length} icon={<IconCube size={18} />} />
            <StatCard label="STL parts" value={totalStls} icon={<Icon3dCubeSphere size={18} />} />
            <StatCard label="GCode files" value={totalGcodes} icon={<IconFileCode size={18} />} accent="grape" />
            <StatCard label="Folders" value={folders.length} icon={<IconFolder size={18} />} accent="cyan" />
          </>
        )}
      </SimpleGrid>

      <Stack gap="sm">
        <Group justify="space-between" align="baseline">
          <Title order={3}>Recent</Title>
          <Text size="xs" c="dimmed">
            Most recently updated
          </Text>
        </Group>
        {loading && recent.length === 0 ? (
          <SimpleGrid cols={{ base: 1, sm: 2, md: 3, lg: 4 }} spacing="md">
            {Array.from({ length: 4 }).map((_, i) => (
              <Skeleton key={i} h={220} radius="md" />
            ))}
          </SimpleGrid>
        ) : (
          <SimpleGrid cols={{ base: 1, sm: 2, md: 3, lg: 4 }} spacing="md">
            {recent.map((o) => (
              <ObjectCard key={o.id} obj={o} />
            ))}
          </SimpleGrid>
        )}
      </Stack>
    </Stack>
  );
}

function StatCard({
  label,
  value,
  icon,
  accent = "violet",
}: {
  label: string;
  value: number;
  icon: ReactNode;
  accent?: string;
}) {
  return (
    <Paper withBorder p="md">
      <Group justify="space-between" align="flex-start" wrap="nowrap">
        <Stack gap={2}>
          <Text size="xs" c="dimmed" tt="uppercase" fw={600}>
            {label}
          </Text>
          <Text fz={28} fw={700} lh={1.1}>
            {value}
          </Text>
        </Stack>
        <div
          style={{
            width: 32,
            height: 32,
            borderRadius: 8,
            display: "grid",
            placeItems: "center",
            background: `var(--mantine-color-${accent}-light)`,
            color: `var(--mantine-color-${accent}-light-color)`,
          }}
        >
          {icon}
        </div>
      </Group>
    </Paper>
  );
}
