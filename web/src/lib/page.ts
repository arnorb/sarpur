import { useEffect, useLayoutEffect, useRef } from "react";
import { useLocation, useNavigationType } from "react-router-dom";

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

/**
 * Back and Forward return to where the page was scrolled; a new page starts
 * at the top. The browser's own restoring runs before React has drawn the
 * page, so it is turned off and done here, once the page is in the document.
 */
export function useScrollRestoration() {
  const location = useLocation();
  const type = useNavigationType();
  const positions = useRef(new Map<string, number>());
  const key = useRef(location.key);
  const first = useRef(true);

  useEffect(() => {
    const previous = window.history.scrollRestoration;
    window.history.scrollRestoration = "manual";
    const save = () => positions.current.set(key.current, window.scrollY);
    window.addEventListener("scroll", save, { passive: true });
    return () => {
      window.removeEventListener("scroll", save);
      window.history.scrollRestoration = previous;
    };
  }, []);

  useLayoutEffect(() => {
    key.current = location.key;
    if (first.current) {
      first.current = false;
      return;
    }
    const top = type === "POP" ? (positions.current.get(location.key) ?? 0) : 0;
    window.scrollTo({ top, behavior: "instant" });
  }, [location.key, type]);
}
