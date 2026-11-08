import type { ComponentProps, MouseEvent } from "react";
import { navigate } from "@/app/route";

/** A link inside the Workbench: a real <a> (keyboard, middle-click, copy link) that navigates in place. */
export function AppLink({ href, onClick, ...props }: ComponentProps<"a"> & { href: string }) {
  const go = (e: MouseEvent<HTMLAnchorElement>): void => {
    onClick?.(e);
    if (e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey)
      return;
    e.preventDefault();
    navigate(href);
  };
  return <a href={href} onClick={go} {...props} />;
}
