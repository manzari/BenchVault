import {
  ActionIcon,
  Badge,
  Button,
  Group,
  Stack,
  Switch,
  Table,
  Text,
  Title,
  Tooltip,
  Collapse,
} from "@mantine/core";
import { IconChevronRight, IconFileCode, IconPlus, IconTrash } from "@tabler/icons-react";
import { useState } from "react";
import { open, ask } from "@tauri-apps/plugin-dialog";
import type { AttachedAsset } from "../lib/types";
import { api } from "../lib/api";
import { notifyError, withErrorToast } from "../lib/errors";
import {
  formatBytes,
  formatDuration,
  formatFilament,
  formatLengthMm,
  slicerColor,
} from "../lib/format";
import { useUsbStore } from "../lib/stores/usb";
import { EmptyState } from "./EmptyState";

interface Props {
  objectId: number;
  gcodes: AttachedAsset[];
  onMutated: () => void;
}

export function GcodeList({ objectId, gcodes, onMutated }: Props) {
  const [expanded, setExpanded] = useState<Record<number, boolean>>({});
  const usbTarget = useUsbStore((s) => s.target);

  const addGcode = async () => {
    let selected: string | string[] | null;
    try {
      selected = await open({
        multiple: true,
        filters: [
          { name: "GCode", extensions: ["gcode", "bgcode", "gco", "g"] },
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
      const ok = await withErrorToast(`Failed to add ${baseName(p)}`, () =>
        api.addGcode(objectId, p),
      );
      if (ok.ok) added += 1;
    }
    if (added > 0) onMutated();
  };

  return (
    <Stack gap="sm">
      <Group justify="space-between" align="center">
        <Title order={4}>
          GCode files{" "}
          <Text component="span" c="dimmed" fw={500} size="sm">
            · {gcodes.length}
          </Text>
        </Title>
        <Button size="xs" variant="light" leftSection={<IconPlus size={14} />} onClick={addGcode}>
          Add GCode
        </Button>
      </Group>

      {gcodes.length === 0 ? (
        <EmptyState
          bare
          icon={<IconFileCode size={28} />}
          title="No GCode files yet"
          description="Drop a sliced .gcode file onto this object, or use the Add GCode button."
        />
      ) : (
        <Table.ScrollContainer minWidth={720}>
          <Table verticalSpacing="sm" highlightOnHover>
            <Table.Thead>
              <Table.Tr>
                <Table.Th w={36} />
                <Table.Th>Name</Table.Th>
                <Table.Th>Slicer</Table.Th>
                <Table.Th>Layer</Table.Th>
                <Table.Th>Time</Table.Th>
                <Table.Th>Filament</Table.Th>
                <Table.Th w={70}>USB</Table.Th>
                <Table.Th w={36} />
              </Table.Tr>
            </Table.Thead>
            <Table.Tbody>
              {gcodes.map((g) => {
                const m = g.gcode_metadata;
                const isOpen = expanded[g.asset.id] ?? false;
                return (
                  <RowFragment
                    key={g.asset.id}
                    g={g}
                    isOpen={isOpen}
                    onToggle={() => setExpanded((e) => ({ ...e, [g.asset.id]: !isOpen }))}
                    usbEnabled={!!usbTarget}
                    onUsb={async (next) => {
                      const ok = await withErrorToast(
                        "Could not update USB export setting",
                        () => api.setGcodeExport(g.asset.id, next),
                      );
                      if (ok.ok) onMutated();
                    }}
                    onRemove={async () => {
                      if (!(await ask(`Remove ${g.asset.original_filename}?`, { title: "Remove gcode", kind: "warning" }))) return;
                      const ok = await withErrorToast(
                        `Failed to remove ${g.asset.original_filename}`,
                        () => api.removeAsset(g.asset.id),
                      );
                      if (ok.ok) onMutated();
                    }}
                    layer={m?.layer_height_mm ? `${m.layer_height_mm} mm` : "—"}
                    time={formatDuration(m?.estimated_time_seconds)}
                    filament={formatFilament(m?.filament_weight_g, m?.filament_length_mm)}
                  />
                );
              })}
            </Table.Tbody>
          </Table>
        </Table.ScrollContainer>
      )}
    </Stack>
  );
}

interface RowProps {
  g: AttachedAsset;
  isOpen: boolean;
  onToggle: () => void;
  usbEnabled: boolean;
  onUsb: (next: boolean) => Promise<void> | void;
  onRemove: () => Promise<void> | void;
  layer: string;
  time: string;
  filament: string;
}

function RowFragment({ g, isOpen, onToggle, usbEnabled, onUsb, onRemove, layer, time, filament }: RowProps) {
  const m = g.gcode_metadata;
  return (
    <>
      <Table.Tr>
        <Table.Td>
          <ActionIcon
            variant="subtle"
            size="sm"
            onClick={onToggle}
            aria-label={isOpen ? "Collapse details" : "Expand details"}
          >
            <IconChevronRight
              size={14}
              style={{
                transform: isOpen ? "rotate(90deg)" : "rotate(0deg)",
                transition: "transform 140ms ease",
              }}
            />
          </ActionIcon>
        </Table.Td>
        <Table.Td>
          <Text size="sm" truncate fw={500}>
            {g.asset.original_filename}
          </Text>
        </Table.Td>
        <Table.Td>
          {m?.slicer_name ? (
            <Badge variant="light" color={slicerColor(m.slicer_name)} radius="sm">
              {m.slicer_name}
              {m.slicer_version ? ` ${m.slicer_version}` : ""}
            </Badge>
          ) : (
            <Text c="dimmed" size="xs">
              —
            </Text>
          )}
        </Table.Td>
        <Table.Td>
          <Text size="sm" ff="monospace">
            {layer}
          </Text>
        </Table.Td>
        <Table.Td>
          <Text size="sm" ff="monospace">
            {time}
          </Text>
        </Table.Td>
        <Table.Td>
          <Text size="sm" ff="monospace">
            {filament}
          </Text>
        </Table.Td>
        <Table.Td>
          <Tooltip
            label={
              usbEnabled
                ? "Export this GCode to the registered USB on next sync"
                : "Register a USB in Settings to enable sync"
            }
          >
            <Switch
              size="sm"
              checked={m?.export_to_usb ?? true}
              disabled={!usbEnabled}
              onChange={(e) => onUsb(e.currentTarget.checked)}
              aria-label="Export to USB"
            />
          </Tooltip>
        </Table.Td>
        <Table.Td>
          <Tooltip label="Remove">
            <ActionIcon variant="subtle" color="red" size="sm" onClick={onRemove} aria-label="Remove">
              <IconTrash size={14} />
            </ActionIcon>
          </Tooltip>
        </Table.Td>
      </Table.Tr>
      <Table.Tr>
        <Table.Td colSpan={8} p={0} style={{ borderTop: "none" }}>
          <Collapse in={isOpen}>
            <Group gap="lg" p="md" bg="var(--mantine-color-default-hover)">
              <Detail label="Profile" value={m?.profile_name} />
              <Detail label="Nozzle" value={m?.nozzle_temp_c ? `${m.nozzle_temp_c}°C` : null} />
              <Detail label="Bed" value={m?.bed_temp_c ? `${m.bed_temp_c}°C` : null} />
              <Detail label="Filament type" value={m?.filament_type} />
              <Detail label="Filament length" value={formatLengthMm(m?.filament_length_mm)} />
              <Detail label="File size" value={formatBytes(g.asset.size_bytes)} />
            </Group>
          </Collapse>
        </Table.Td>
      </Table.Tr>
    </>
  );
}

function baseName(p: string): string {
  const i = Math.max(p.lastIndexOf("/"), p.lastIndexOf("\\"));
  return i >= 0 ? p.slice(i + 1) : p;
}

function Detail({ label, value }: { label: string; value: string | null | undefined }) {
  return (
    <Stack gap={0}>
      <Text size="xs" c="dimmed" tt="uppercase" fw={600}>
        {label}
      </Text>
      <Text size="sm">{value || "—"}</Text>
    </Stack>
  );
}
