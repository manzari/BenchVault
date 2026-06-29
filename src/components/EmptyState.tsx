import { Paper, Stack, Text, ThemeIcon } from "@mantine/core";
import type { ReactNode } from "react";

interface Props {
  icon: ReactNode;
  title: string;
  description?: string;
  action?: ReactNode;
  /** When true, render without the bordered Paper wrapper. */
  bare?: boolean;
}

export function EmptyState({ icon, title, description, action, bare }: Props) {
  const body = (
    <Stack align="center" gap="xs" py="xl">
      <ThemeIcon size={56} radius="xl" variant="light" color="violet">
        {icon}
      </ThemeIcon>
      <Text fw={600} size="md" mt="xs">
        {title}
      </Text>
      {description && (
        <Text c="dimmed" size="sm" ta="center" maw={420}>
          {description}
        </Text>
      )}
      {action && <div style={{ marginTop: 8 }}>{action}</div>}
    </Stack>
  );
  if (bare) return body;
  return (
    <Paper withBorder p="lg">
      {body}
    </Paper>
  );
}
