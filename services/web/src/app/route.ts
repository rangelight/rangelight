// A handful of pages, so no router library: the path is the state, and links push it.
import { useSyncExternalStore } from "react";

export type Route =
  | { page: "sessions" }
  | { page: "wall" }
  | { page: "upnext" }
  | { page: "session"; id: string }
  | { page: "missing" };

/** The views' paths. */
export const WALL = "/wall";
export const UP_NEXT = "/up-next";

export function parse(path: string): Route {
  if (path === "/") return { page: "sessions" };
  if (path === WALL) return { page: "wall" };
  if (path === UP_NEXT) return { page: "upnext" };
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
