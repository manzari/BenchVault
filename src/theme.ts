import { createTheme, rem } from "@mantine/core";

export const theme = createTheme({
  primaryColor: "violet",
  primaryShade: { light: 6, dark: 5 },
  defaultRadius: "md",
  fontFamily:
    'Inter, -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Oxygen, Ubuntu, Cantarell, "Helvetica Neue", sans-serif',
  fontFamilyMonospace:
    'ui-monospace, SFMono-Regular, "JetBrains Mono", Menlo, Monaco, Consolas, monospace',
  headings: {
    fontWeight: "650",
    sizes: {
      h1: { fontSize: rem(28), lineHeight: "1.25" },
      h2: { fontSize: rem(22), lineHeight: "1.3" },
      h3: { fontSize: rem(18), lineHeight: "1.35" },
      h4: { fontSize: rem(15), lineHeight: "1.4" },
    },
  },
  components: {
    Paper: {
      defaultProps: { radius: "md" },
    },
    Button: {
      defaultProps: { radius: "md" },
    },
    TextInput: {
      defaultProps: { radius: "md" },
    },
    Textarea: {
      defaultProps: { radius: "md" },
    },
    Select: {
      defaultProps: { radius: "md" },
    },
    Modal: {
      defaultProps: { radius: "md", overlayProps: { blur: 3 } },
    },
    Card: {
      defaultProps: { radius: "md" },
    },
    Badge: {
      defaultProps: { radius: "sm" },
    },
  },
});
