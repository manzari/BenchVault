import { ActionIcon, AspectRatio, Card, Group, Text, Badge, Image, Stack, Tooltip } from "@mantine/core";
import { Icon3dCubeSphere, IconFileCode, IconFolderOpen, IconCube } from "@tabler/icons-react";
import { Link } from "@tanstack/react-router";
import { useEffect, useState, type MouseEvent } from "react";
import type { ObjectSummary } from "../lib/types";
import { api } from "../lib/api";

export function ObjectCard({ obj }: { obj: ObjectSummary }) {
  const [coverUrl, setCoverUrl] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    let revoke: string | null = null;
    if (obj.cover_asset_id != null) {
      api
        .readAssetBytes(obj.cover_asset_id)
        .then((bytes) => {
          if (cancelled) return;
          const url = URL.createObjectURL(new Blob([new Uint8Array(bytes)]));
          revoke = url;
          setCoverUrl(url);
        })
        .catch(() => {
          if (!cancelled) setCoverUrl(null);
        });
    } else {
      setCoverUrl(null);
    }
    return () => {
      cancelled = true;
      if (revoke) URL.revokeObjectURL(revoke);
    };
  }, [obj.cover_asset_id]);

  const reveal = (e: MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    api.revealObjectInExplorer(obj.id);
  };

  return (
    <Card
      component={Link}
      to="/objects/$objectId"
      params={{ objectId: String(obj.id) } as any}
      withBorder
      padding="sm"
      className="gcf-card"
      style={{ textDecoration: "none", color: "inherit", height: "100%" }}
    >
      <Card.Section style={{ position: "relative" }}>
        <AspectRatio ratio={16 / 10}>
          {coverUrl ? (
            <Image src={coverUrl} fit="cover" alt={obj.name} />
          ) : (
            <div
              style={{
                background:
                  "linear-gradient(135deg, var(--mantine-color-violet-9) 0%, var(--mantine-color-dark-7) 100%)",
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
              }}
            >
              <IconCube size={56} stroke={1.2} color="var(--mantine-color-violet-3)" opacity={0.55} />
            </div>
          )}
        </AspectRatio>
        <Tooltip label="Open in file explorer">
          <ActionIcon
            variant="default"
            size="sm"
            onClick={reveal}
            aria-label="Open in file explorer"
            style={{ position: "absolute", top: 6, right: 6 }}
          >
            <IconFolderOpen size={14} />
          </ActionIcon>
        </Tooltip>
      </Card.Section>
      <Stack gap={6} mt="sm">
        <Text fw={600} lineClamp={2} style={{ wordBreak: "break-word" }}>
          {obj.name}
        </Text>
        <Group gap="xs">
          <Badge variant="light" leftSection={<Icon3dCubeSphere size={12} />}>
            {obj.stl_count}
          </Badge>
          <Badge variant="light" color="grape" leftSection={<IconFileCode size={12} />}>
            {obj.gcode_count}
          </Badge>
        </Group>
      </Stack>
    </Card>
  );
}
