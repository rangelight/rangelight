// A handful of pages, so no router library: the path is the state, and links push it.
import { useSyncExternalStore } from "react";

/** The views' paths. */
export const WALL = "/wall";
export const UP_NEXT = "/up-next";
export const ARCHITECT = "/architect";

/** Every view, in the rail's order: the rail, the palette and the top bar read this list. `phone`:
 * kept in a phone's bottom bar (the others are a tap on More away). */
export const VIEWS = [
  { page: "sessions", path: "/", label: "Sessions", phone: true },
  { page: "architect", path: ARCHITECT, label: "Architect", phone: true },
  { page: "decisions", path: "/decisions", label: "Decisions", phone: true },
  { page: "reviews", path: "/reviews", label: "Reviews", phone: false },
  { page: "coordinator", path: "/coordinator", label: "Coordinator", phone: false },
  { page: "wall", path: WALL, label: "Fleet wall", phone: false },
  { page: "upnext", path: UP_NEXT, label: "Up next", phone: false },
] as const;

export type View = (typeof VIEWS)[number]["page"];
export type Route = { page: View } | { page: "session"; id: string } | { page: "missing" };

export function parse(path: string): Route {
  const view = VIEWS.find((v) => v.path === path);
  if (view) return { page: view.page };
  const m = /^\/sessions\/([^/]+)$/.exec(path);
  return m?.[1] ? { page: "session", id: decodeURIComponent(m[1]) } : { page: "missing" };
}

export const sessionPath = (id: string): string => `/sessions/${encodeURIComponent(id)}`;

export function navigate(path: string): void {
  history.pushState(null, "", path);
  dispatchEvent(new PopStateEvent("popstate"));
}

const subscribe = (l: () => void): (() => void) => {
  addEventListener("popstate", l);
  return () => {
    removeEventListener("popstate", l);
  };
};

export function useRoute(): Route {
  return parse(useSyncExternalStore(subscribe, () => location.pathname));
}
