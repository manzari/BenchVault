import {
  Anchor,
  Badge,
  Button,
  Checkbox,
  Code,
  Group,
  Modal,
  NumberInput,
  Paper,
  PasswordInput,
  Progress,
  Select,
  Stack,
  Text,
  TextInput,
  Title,
} from "@mantine/core";
import { notifications } from "@mantine/notifications";
import {
  IconBrandThingiverse,
  IconCube,
  IconFileCode,
  IconRefresh,
  IconScissors,
  IconUsb,
  IconWand,
} from "@tabler/icons-react";
import { open as openDialog, ask } from "@tauri-apps/plugin-dialog";
import { useEffect, useMemo, useState } from "react";
import type { MountedDrive } from "../lib/types";
import { api } from "../lib/api";
import { useUsbStore } from "../lib/stores/usb";
import { useUiStore } from "../lib/stores/ui";

export function SettingsPage() {
  const target = useUsbStore((s) => s.target);
  const connected = useUsbStore((s) => s.connected);
  const syncing = useUsbStore((s) => s.syncing);
  const syncPercent = useUsbStore((s) => s.syncPercent);
  const syncMessage = useUsbStore((s) => s.syncMessage);
  const lastError = useUsbStore((s) => s.lastError);
  const lastReport = useUsbStore((s) => s.lastReport);

  const load = useUsbStore((s) => s.load);
  const pickUsb = useUsbStore((s) => s.pickUsb);
  const clearUsb = useUsbStore((s) => s.clearUsb);
  const syncNow = useUsbStore((s) => s.syncNow);

  const [pickerOpen, setPickerOpen] = useState(false);
  const [drives, setDrives] = useState<MountedDrive[]>([]);
  const [selectedMount, setSelectedMount] = useState<string | null>(null);
  const [label, setLabel] = useState("");
  const [registering, setRegistering] = useState(false);
  const [autoSync, setAutoSync] = useState(true);
  const [autoSyncBusy, setAutoSyncBusy] = useState(true);

  useEffect(() => {
    load();
    api
      .getUsbAutoSync()
      .then((v) => {
        setAutoSync(v);
        setAutoSyncBusy(false);
      })
      .catch(() => setAutoSyncBusy(false));
  }, [load]);

  const toggleAutoSync = async (next: boolean) => {
    setAutoSync(next); // optimistic
    setAutoSyncBusy(true);
    try {
      await api.setUsbAutoSync(next);
    } catch (e) {
      setAutoSync(!next); // revert
      notifications.show({ color: "red", title: "Save failed", message: String(e) });
    } finally {
      setAutoSyncBusy(false);
    }
  };

  const openPicker = async () => {
    setRegistering(false);
    setLabel("");
    setSelectedMount(null);
    try {
      const ds = await api.listMountedDrives();
      setDrives(ds);
      if (ds.length > 0) {
        setSelectedMount(ds[0].mount_path);
        setLabel(ds[0].label ?? "BenchVault USB");
      }
      setPickerOpen(true);
    } catch (e) {
      notifications.show({ color: "red", title: "Could not list drives", message: String(e) });
    }
  };

  const refreshDrives = async () => {
    try {
      const ds = await api.listMountedDrives();
      setDrives(ds);
    } catch (e) {
      notifications.show({ color: "red", title: "Could not list drives", message: String(e) });
    }
  };

  const tryRegister = async (force: boolean) => {
    if (!selectedMount) return;
    setRegistering(true);
    try {
      await pickUsb(selectedMount, label.trim() || "BenchVault USB", force);
      setPickerOpen(false);
      notifications.show({ color: "green", title: "USB registered", message: selectedMount });
    } catch (e) {
      const msg = String(e);
      if (msg.includes("foreign marker") && !force) {
        if (
          await ask(
            "This USB is already registered to another BenchVault install. Reclaim it for this one?",
            { title: "Reclaim USB", kind: "warning" },
          )
        ) {
          await tryRegister(true);
          return;
        }
      } else {
        notifications.show({ color: "red", title: "Registration failed", message: msg });
      }
    } finally {
      setRegistering(false);
    }
  };

  const onForget = async () => {
    if (!(await ask("Forget this USB? Existing files on the drive are not deleted.", { title: "Forget USB", kind: "warning" }))) return;
    try {
      await clearUsb();
      notifications.show({ color: "blue", title: "USB forgotten", message: "" });
    } catch (e) {
      notifications.show({ color: "red", title: "Failed to forget", message: String(e) });
    }
  };

  const doSync = async (deep: boolean) => {
    try {
      const report = await syncNow(deep);
      if (report.skipped) {
        notifications.show({
          color: "yellow",
          title: "Sync skipped",
          message: report.reason ?? "unknown",
        });
        return;
      }
      const summary = `${report.copied} copied, ${report.updated} updated, ${report.deleted} deleted`;
      const errs = report.errors.length;
      notifications.show({
        color: errs > 0 ? "yellow" : "green",
        title: errs > 0 ? `Sync finished with ${errs} error${errs === 1 ? "" : "s"}` : "Sync complete",
        message: summary,
      });
    } catch (e) {
      notifications.show({ color: "red", title: "Sync failed", message: String(e) });
    }
  };

  const lastSyncText = useMemo(() => {
    if (!lastReport || lastReport.skipped) return null;
    const ts = lastReport.finished_at;
    if (!ts) return null;
    return new Date(ts * 1000).toLocaleString();
  }, [lastReport]);

  const status = !target
    ? { color: "gray" as const, text: "no USB selected" }
    : syncing
      ? { color: "blue" as const, text: "syncing…" }
      : connected
        ? { color: "green" as const, text: "connected" }
        : { color: "yellow" as const, text: "not detected" };

  return (
    <Stack gap="lg" maw={780}>
      <Title order={2}>Settings</Title>

      <ThingiverseTokenCard />

      <OrcaSlicerCard />

      <OpenScadCard />

      <KobraCleanCard />

      <StlPreviewCard />

      <Paper withBorder p="md">
        <Stack gap="md">
          <Group justify="space-between" align="flex-start" wrap="nowrap">
            <Group gap="sm" align="center">
              <IconUsb size={20} />
              <Title order={4} m={0}>
                USB sync
              </Title>
            </Group>
            <Badge color={status.color} variant="light">
              {status.text}
            </Badge>
          </Group>

          <Text size="sm" c="dimmed">
            Pick one USB drive once. Every GCode with the &ldquo;Export to USB&rdquo; switch on
            (default) will be mirrored to that drive — automatically when the library changes and
            the drive is plugged in, or on demand via Sync now. Files on the USB outside the
            app&rsquo;s tracked set are never touched.
          </Text>

          <Checkbox
            label="Sync automatically when the library changes or the USB is plugged in"
            checked={autoSync}
            disabled={autoSyncBusy}
            onChange={(e) => toggleAutoSync(e.currentTarget.checked)}
          />

          {target ? (
            <Group gap="md" wrap="nowrap">
              <Stack gap={2} style={{ flex: 1, minWidth: 0 }}>
                <Text size="xs" c="dimmed" tt="uppercase">
                  Registered
                </Text>
                <Text fw={600} truncate>
                  {target.label}
                </Text>
                <Code>{target.uuid.slice(0, 8)}…</Code>
              </Stack>
              <Group gap="xs">
                <Button variant="light" onClick={openPicker}>
                  Change USB
                </Button>
                <Button variant="subtle" color="red" onClick={onForget}>
                  Forget
                </Button>
              </Group>
            </Group>
          ) : (
            <Group>
              <Button onClick={openPicker} leftSection={<IconUsb size={16} />}>
                Pick USB
              </Button>
              <Text size="sm" c="dimmed">
                No USB selected.
              </Text>
            </Group>
          )}

          {target && (
            <Group>
              <Button
                onClick={() => doSync(false)}
                disabled={syncing}
                leftSection={<IconRefresh size={16} />}
              >
                Sync now
              </Button>
              <Button onClick={() => doSync(true)} disabled={syncing} variant="light">
                Deep sync
              </Button>
            </Group>
          )}

          {syncing && (
            <Stack gap={4}>
              <Progress value={syncPercent} animated />
              {syncMessage && (
                <Text size="xs" c="dimmed" truncate>
                  {syncMessage}
                </Text>
              )}
            </Stack>
          )}

          {lastSyncText && !syncing && (
            <Text size="xs" c="dimmed">
              Last sync: {lastSyncText} — {lastReport?.copied ?? 0} copied,{" "}
              {lastReport?.updated ?? 0} updated, {lastReport?.deleted ?? 0} deleted
            </Text>
          )}
          {lastError && (
            <Text size="xs" c="red">
              {lastError}
            </Text>
          )}
        </Stack>
      </Paper>

      <Modal opened={pickerOpen} onClose={() => setPickerOpen(false)} title="Pick a USB drive" centered>
        <Stack gap="md">
          {drives.length === 0 ? (
            <Text size="sm" c="dimmed">
              No removable drives detected. Plug in a USB and click Refresh.
            </Text>
          ) : (
            <Select
              label="Mounted drive"
              data={drives.map((d) => ({
                value: d.mount_path,
                label: `${d.label ?? d.mount_path}${d.fs_kind ? ` (${d.fs_kind})` : ""} — ${d.mount_path}`,
              }))}
              value={selectedMount}
              onChange={(v) => {
                setSelectedMount(v);
                const drive = drives.find((d) => d.mount_path === v);
                if (drive && !label) setLabel(drive.label ?? "BenchVault USB");
              }}
            />
          )}
          <TextInput
            label="Label"
            placeholder="BenchVault USB"
            value={label}
            onChange={(e) => setLabel(e.currentTarget.value)}
          />
          <Group justify="space-between">
            <Button variant="subtle" onClick={refreshDrives}>
              Refresh
            </Button>
            <Group>
              <Button variant="default" onClick={() => setPickerOpen(false)}>
                Cancel
              </Button>
              <Button
                disabled={!selectedMount || registering}
                onClick={() => tryRegister(false)}
                loading={registering}
              >
                Register
              </Button>
            </Group>
          </Group>
        </Stack>
      </Modal>
    </Stack>
  );
}

function KobraCleanCard() {
  const [enabled, setEnabled] = useState(false);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    api
      .getCleanKobraGcode()
      .then((v) => {
        setEnabled(v);
        setLoading(false);
      })
      .catch(() => setLoading(false));
  }, []);

  const toggle = async (next: boolean) => {
    setEnabled(next); // optimistic
    setSaving(true);
    try {
      await api.setCleanKobraGcode(next);
    } catch (e) {
      setEnabled(!next); // revert
      notifications.show({ color: "red", title: "Save failed", message: String(e) });
    } finally {
      setSaving(false);
    }
  };

  return (
    <Paper withBorder p="md">
      <Stack gap="md">
        <Group gap="sm" align="center">
          <IconWand size={20} />
          <Title order={4} m={0}>
            Anycubic Kobra fix
          </Title>
        </Group>

        <Text size="sm" c="dimmed">
          OrcaSlicer 2.3.2 emits a <Code>; filament_colour_type = 1;1;1;1</Code> comment that breaks
          Anycubic Kobra gcode parsing (
          <Anchor href="https://github.com/OrcaSlicer/OrcaSlicer/issues/12444" target="_blank">
            issue #12444
          </Anchor>
          ). With this on, that line is stripped from every gcode when it&rsquo;s first discovered in
          the library and when it&rsquo;s copied to a USB drive.
        </Text>

        <Checkbox
          label="Strip the Kobra-breaking line from gcode"
          checked={enabled}
          disabled={loading || saving}
          onChange={(e) => toggle(e.currentTarget.checked)}
        />
      </Stack>
    </Paper>
  );
}

function StlPreviewCard() {
  const maxBytes = useUiStore((s) => s.stlPreviewMaxBytes);
  const setMaxBytes = useUiStore((s) => s.setStlPreviewMaxBytes);
  const currentMb = Math.round(maxBytes / (1024 * 1024));

  const [mb, setMb] = useState<number | string>(currentMb);
  const [saving, setSaving] = useState(false);

  // Reflect external changes (e.g. the value loaded from the backend at startup).
  useEffect(() => {
    setMb(Math.round(maxBytes / (1024 * 1024)));
  }, [maxBytes]);

  const parsed = typeof mb === "number" ? mb : parseInt(mb, 10);
  const valid = Number.isFinite(parsed) && parsed > 0;
  const dirty = valid && parsed !== currentMb;

  const save = async () => {
    if (!valid) return;
    setSaving(true);
    try {
      await setMaxBytes(Math.round(parsed * 1024 * 1024));
      notifications.show({
        color: "green",
        title: "Preview limit saved",
        message: `${parsed} MB`,
      });
    } catch (e) {
      notifications.show({ color: "red", title: "Save failed", message: String(e) });
    } finally {
      setSaving(false);
    }
  };

  return (
    <Paper withBorder p="md">
      <Stack gap="md">
        <Group gap="sm" align="center">
          <IconCube size={20} />
          <Title order={4} m={0}>
            STL preview
          </Title>
        </Group>

        <Text size="sm" c="dimmed">
          The 3D preview loads an entire STL into memory. Parts larger than this limit aren&rsquo;t
          rendered — loading them would exhaust the renderer and crash the view. Default is 100 MB.
        </Text>

        <Group align="flex-end" gap="sm">
          <NumberInput
            label="Maximum size"
            value={mb}
            onChange={setMb}
            min={1}
            step={10}
            suffix=" MB"
            style={{ width: 200 }}
          />
          <Button onClick={save} loading={saving} disabled={!dirty}>
            Save
          </Button>
        </Group>
      </Stack>
    </Paper>
  );
}

function ThingiverseTokenCard() {
  const [token, setToken] = useState("");
  const [stored, setStored] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    api
      .getThingiverseToken()
      .then((t) => {
        setStored(t);
        setLoading(false);
      })
      .catch(() => setLoading(false));
  }, []);

  const save = async () => {
    setSaving(true);
    try {
      const t = token.trim();
      await api.setThingiverseToken(t.length > 0 ? t : null);
      setStored(t.length > 0 ? t : null);
      setToken("");
      notifications.show({
        color: "green",
        title: t ? "Thingiverse token saved" : "Thingiverse token cleared",
        message: "",
      });
    } catch (e) {
      notifications.show({ color: "red", title: "Save failed", message: String(e) });
    } finally {
      setSaving(false);
    }
  };

  const clear = async () => {
    setSaving(true);
    try {
      await api.setThingiverseToken(null);
      setStored(null);
      setToken("");
      notifications.show({ color: "blue", title: "Thingiverse token cleared", message: "" });
    } finally {
      setSaving(false);
    }
  };

  return (
    <Paper withBorder p="md">
      <Stack gap="md">
        <Group justify="space-between" align="flex-start" wrap="nowrap">
          <Group gap="sm" align="center">
            <IconBrandThingiverse size={20} />
            <Title order={4} m={0}>
              Thingiverse
            </Title>
          </Group>
          <Badge color={stored ? "green" : "gray"} variant="light">
            {loading ? "…" : stored ? "token set" : "no token"}
          </Badge>
        </Group>

        <Text size="sm" c="dimmed">
          Required to download STL/3MF files when importing a Thingiverse URL. Get a personal
          access token by registering an app at{" "}
          <Anchor href="https://www.thingiverse.com/developers/my-apps" target="_blank">
            thingiverse.com/developers/my-apps
          </Anchor>
          . Without a token, imports save metadata + cover image only.
        </Text>

        <PasswordInput
          label="API token"
          placeholder={stored ? "•••••••• (token set)" : "paste token here"}
          value={token}
          onChange={(e) => setToken(e.currentTarget.value)}
        />

        <Group>
          <Button onClick={save} loading={saving} disabled={token.trim().length === 0}>
            Save token
          </Button>
          {stored && (
            <Button variant="subtle" color="red" onClick={clear} disabled={saving}>
              Clear
            </Button>
          )}
        </Group>
      </Stack>
    </Paper>
  );
}

type OrcaPresetOption = { group: string; items: { value: string; label: string }[] };

/** Group presets by vendor into Mantine Select's grouped-data shape. */
function groupPresets(
  presets: { name: string; vendor: string; path: string }[],
): OrcaPresetOption[] {
  const byVendor = new Map<string, { value: string; label: string }[]>();
  for (const p of presets) {
    const list = byVendor.get(p.vendor) ?? [];
    list.push({ value: p.path, label: p.name });
    byVendor.set(p.vendor, list);
  }
  return [...byVendor.entries()]
    .sort((a, b) => a[0].localeCompare(b[0]))
    .map(([group, items]) => ({ group, items }));
}

function OrcaSlicerCard() {
  const [path, setPath] = useState("");
  const [detected, setDetected] = useState(false);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);

  const [machineOpts, setMachineOpts] = useState<OrcaPresetOption[]>([]);
  const [filamentOpts, setFilamentOpts] = useState<OrcaPresetOption[]>([]);
  const [machine, setMachine] = useState<string | null>(null);
  const [filament, setFilament] = useState<string | null>(null);

  // The backend returns the saved path, or an auto-detected install if none is
  // saved. We can't tell the two apart from the value alone, so we just show it
  // pre-filled either way; saving it pins whatever is in the field.
  useEffect(() => {
    Promise.all([api.getOrcaPath(), api.listOrcaPresets(), api.getOrcaDefaults()])
      .then(([p, presets, defaults]) => {
        setPath(p ?? "");
        setDetected(p != null);
        setMachineOpts(groupPresets(presets.machines));
        setFilamentOpts(groupPresets(presets.filaments));
        setMachine(defaults.machine_path);
        setFilament(defaults.filament_path);
        setLoading(false);
      })
      .catch(() => setLoading(false));
  }, []);

  const browse = async () => {
    try {
      const sel = await openDialog({
        multiple: false,
        directory: false,
        title: "Select the OrcaSlicer executable / AppImage",
      });
      if (typeof sel === "string") setPath(sel);
    } catch (e) {
      notifications.show({ color: "red", title: "Could not open file picker", message: String(e) });
    }
  };

  const save = async () => {
    setSaving(true);
    try {
      const v = path.trim();
      await api.setOrcaPath(v.length > 0 ? v : null);
      await api.setOrcaDefaults(machine, filament);
      notifications.show({
        color: "green",
        title: "OrcaSlicer settings saved",
        message: v || "Path will auto-detect on next use.",
      });
    } catch (e) {
      notifications.show({ color: "red", title: "Save failed", message: String(e) });
    } finally {
      setSaving(false);
    }
  };

  const presetsFound = machineOpts.length > 0 || filamentOpts.length > 0;

  return (
    <Paper withBorder p="md">
      <Stack gap="md">
        <Group justify="space-between" align="flex-start" wrap="nowrap">
          <Group gap="sm" align="center">
            <IconScissors size={20} />
            <Title order={4} m={0}>
              OrcaSlicer
            </Title>
          </Group>
          <Badge color={path ? "green" : "gray"} variant="light">
            {loading ? "…" : path ? (detected ? "detected" : "set") : "not found"}
          </Badge>
        </Group>

        <Text size="sm" c="dimmed">
          Path to the OrcaSlicer executable or AppImage, used by the &ldquo;Open in OrcaSlicer&rdquo;
          and &ldquo;New Orca project&rdquo; buttons. Leave blank to auto-detect. You can also enter
          a command such as <Code>flatpak run io.github.softfever.OrcaSlicer</Code>.
        </Text>

        <Group gap="xs" wrap="nowrap" align="flex-end">
          <TextInput
            style={{ flex: 1 }}
            label="OrcaSlicer path"
            placeholder="/path/to/OrcaSlicer.AppImage"
            value={path}
            onChange={(e) => {
              setPath(e.currentTarget.value);
              setDetected(false);
            }}
          />
          <Button variant="default" onClick={browse}>
            Browse…
          </Button>
        </Group>

        <Text size="sm" c="dimmed">
          Default printer and filament for new Orca projects. When set, the generated{" "}
          <Code>.3mf</Code> opens with these presets selected (a compatible print profile is chosen
          automatically). Leave blank to use OrcaSlicer&rsquo;s own defaults.
        </Text>

        {!loading && !presetsFound ? (
          <Text size="xs" c="dimmed">
            No OrcaSlicer presets found on disk — open OrcaSlicer once so it writes its config, then
            reopen this page.
          </Text>
        ) : (
          <Group gap="xs" grow align="flex-start">
            <Select
              label="Default printer"
              placeholder="OrcaSlicer default"
              data={machineOpts}
              value={machine}
              onChange={setMachine}
              searchable
              clearable
              limit={50}
              nothingFoundMessage="No match"
              disabled={loading || machineOpts.length === 0}
            />
            <Select
              label="Default filament"
              placeholder="OrcaSlicer default"
              data={filamentOpts}
              value={filament}
              onChange={setFilament}
              searchable
              clearable
              limit={50}
              nothingFoundMessage="No match"
              disabled={loading || filamentOpts.length === 0}
            />
          </Group>
        )}

        <Group>
          <Button onClick={save} loading={saving}>
            Save
          </Button>
        </Group>
      </Stack>
    </Paper>
  );
}

function OpenScadCard() {
  const [path, setPath] = useState("");
  const [detected, setDetected] = useState(false);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);

  // Like Orca: the backend returns the saved path or an auto-detected install; we
  // pre-fill either way and pin whatever is in the field on save.
  useEffect(() => {
    api
      .getOpenscadPath()
      .then((p) => {
        setPath(p ?? "");
        setDetected(p != null);
        setLoading(false);
      })
      .catch(() => setLoading(false));
  }, []);

  const browse = async () => {
    try {
      const sel = await openDialog({
        multiple: false,
        directory: false,
        title: "Select the OpenSCAD executable",
      });
      if (typeof sel === "string") setPath(sel);
    } catch (e) {
      notifications.show({ color: "red", title: "Could not open file picker", message: String(e) });
    }
  };

  const save = async () => {
    setSaving(true);
    try {
      const v = path.trim();
      await api.setOpenscadPath(v.length > 0 ? v : null);
      notifications.show({
        color: "green",
        title: "OpenSCAD path saved",
        message: v || "Path will auto-detect on next use.",
      });
    } catch (e) {
      notifications.show({ color: "red", title: "Save failed", message: String(e) });
    } finally {
      setSaving(false);
    }
  };

  return (
    <Paper withBorder p="md">
      <Stack gap="md">
        <Group justify="space-between" align="flex-start" wrap="nowrap">
          <Group gap="sm" align="center">
            <IconFileCode size={20} />
            <Title order={4} m={0}>
              OpenSCAD
            </Title>
          </Group>
          <Badge color={path ? "green" : "gray"} variant="light">
            {loading ? "…" : path ? (detected ? "detected" : "set") : "not found"}
          </Badge>
        </Group>

        <Text size="sm" c="dimmed">
          Path to the OpenSCAD executable, used to render <Code>.scad</Code> previews and the
          &ldquo;Open in OpenSCAD&rdquo; action. Leave blank to auto-detect. You can also enter a
          command such as <Code>flatpak run org.openscad.OpenSCAD</Code>.
        </Text>

        <Group gap="xs" wrap="nowrap" align="flex-end">
          <TextInput
            style={{ flex: 1 }}
            label="OpenSCAD path"
            placeholder="/usr/bin/openscad"
            value={path}
            onChange={(e) => {
              setPath(e.currentTarget.value);
              setDetected(false);
            }}
          />
          <Button variant="default" onClick={browse}>
            Browse…
          </Button>
        </Group>

        <Group>
          <Button onClick={save} loading={saving}>
            Save
          </Button>
        </Group>
      </Stack>
    </Paper>
  );
}
