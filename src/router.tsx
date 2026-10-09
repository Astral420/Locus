import { createRootRoute, createRoute, createRouter } from "@tanstack/react-router";
import { AppShell } from "./components/layout/AppShell";
import { MeetingsView } from "./routes/MeetingsView";
import { RecordView } from "./routes/RecordView";
import { MeetingDetailView } from "./routes/MeetingDetailView";
import { KnowledgeView } from "./routes/KnowledgeView";
import { ModelsView } from "./routes/ModelsView";
import { SettingsView } from "./routes/SettingsView";
import { NotFoundView } from "./routes/NotFoundView";

const rootRoute = createRootRoute({
  component: AppShell,
  notFoundComponent: NotFoundView,
});

const indexRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  component: MeetingsView,
});

const recordRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/record",
  component: RecordView,
});

const meetingDetailRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/meeting/$id",
  component: MeetingDetailView,
});

const knowledgeRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/knowledge",
  component: KnowledgeView,
});

const modelsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/models",
  component: ModelsView,
});

const settingsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/settings",
  component: SettingsView,
});

export const routeTree = rootRoute.addChildren([
  indexRoute,
  recordRoute,
  meetingDetailRoute,
  knowledgeRoute,
  modelsRoute,
  settingsRoute,
]);

export const router = createRouter({
  routeTree,
  defaultPreload: "intent",
  defaultNotFoundComponent: NotFoundView,
});

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}
