import React from "react";
import ReactDOM from "react-dom/client";
import { MantineProvider } from "@mantine/core";
import { Notifications } from "@mantine/notifications";
import { RouterProvider, createRouter, createRootRoute, createRoute, Outlet } from "@tanstack/react-router";
import "@mantine/core/styles.css";
import "@mantine/notifications/styles.css";
import "@mantine/dropzone/styles.css";
import "./theme.css";

import { App } from "./App";
import { theme } from "./theme";
import { LibraryPage } from "./routes/LibraryPage";
import { ObjectDetailPage } from "./routes/ObjectDetailPage";
import { SettingsPage } from "./routes/SettingsPage";

const rootRoute = createRootRoute({
  component: () => (
    <App>
      <Outlet />
    </App>
  ),
});

const libraryRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  component: LibraryPage,
});

const objectDetailRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/objects/$objectId",
  component: ObjectDetailPage,
});

const settingsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/settings",
  component: SettingsPage,
});

const router = createRouter({
  routeTree: rootRoute.addChildren([libraryRoute, objectDetailRoute, settingsRoute]),
});

declare module "@tanstack/react-router" {
  interface Register { router: typeof router; }
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <MantineProvider theme={theme} defaultColorScheme="dark">
      <Notifications position="top-center" autoClose={3500} />
      <RouterProvider router={router} />
    </MantineProvider>
  </React.StrictMode>,
);
