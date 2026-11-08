import { useEffect } from "react";

/** Sets the browser tab's title for the current page. */
export function usePageTitle(title: string) {
  useEffect(() => {
    document.title = title ? `${title} – RÚV Sarpur` : "RÚV Sarpur";
  }, [title]);
}

/** Smooth scrolling, unless the user asked for reduced motion. */
export function scrollBehavior(): ScrollBehavior {
  return window.matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth";
}
