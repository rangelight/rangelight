// A handful of pages, so no router library: the path is the state, and links push it.
import { useSyncExternalStore } from "react";

/** The views' paths. */
export const WALL = "/wall";
export const UP_NEXT = "/up-next";
export const ARCHITECT = "/architect";

/** Every view, in the rail's order: the rail, the palette and the top bar read this list. `rail`:
 * it has a rail icon (the others are in the palette, where the mock-up puts them); `phone`: kept in a
 * phone's bottom bar (the others are a tap on More away). */
export const VIEWS = [
  { page: "home", path: "/home", label: "Home", rail: true, phone: false },
  { page: "sessions", path: "/", label: "Sessions", rail: true, phone: true },
  { page: "architect", path: ARCHITECT, label: "Architect", rail: true, phone: true },
  { page: "decisions", path: "/decisions", label: "Decisions", rail: true, phone: true },
  { page: "reviews", path: "/reviews", label: "Reviews", rail: true, phone: false },
  { page: "coordinator", path: "/coordinator", label: "Coordinator", rail: true, phone: false },
  { page: "wall", path: WALL, label: "Fleet wall", rail: true, phone: false },
  { page: "upnext", path: UP_NEXT, label: "Up next", rail: true, phone: false },
  { page: "fork", path: "/fork", label: "Fork compare", rail: true, phone: false },
  { page: "repos", path: "/repos", label: "Repositories", rail: true, phone: false },
  { page: "settings", path: "/settings", label: "Settings", rail: true, phone: false },
  { page: "access", path: "/access", label: "Claude access", rail: false, phone: false },
  { page: "retro", path: "/retro", label: "Retro", rail: false, phone: false },
] as const;

export type View = (typeof VIEWS)[number]["page"];
export type Route =
  | { page: View }
  | { page: "session"; id: string }
  | { page: "lane"; key: string }
  | { page: "missing" };

export function parse(path: string): Route {
  const view = VIEWS.find((v) => v.path === path);
  if (view) return { page: view.page };
  const lane = /^\/lanes\/([^/]+)$/.exec(path)?.[1];
  if (lane) return { page: "lane", key: decodeURIComponent(lane) };
  const m = /^\/sessions\/([^/]+)$/.exec(path);
  return m?.[1] ? { page: "session", id: decodeURIComponent(m[1]) } : { page: "missing" };
}

export const sessionPath = (id: string): string => `/sessions/${encodeURIComponent(id)}`;

/** A sample lane's session view (the preview facade's lanes, never a real session). */
export const lanePath = (key: string): string => `/lanes/${encodeURIComponent(key)}`;

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
