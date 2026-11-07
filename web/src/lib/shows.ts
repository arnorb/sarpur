// Grouping, labels and formatting for shows, as the S1 UI did it.
import type { ContentType, Settings, Show } from "@/api/types";

export const CONTENT_ORDER: ContentType[] = ["movie_or_docu", "sport", "show"];
export const CONTENT_LABEL: Record<ContentType, string> = {
  movie_or_docu: "Movies & Docs",
  sport: "Sports",
  show: "Shows",
};

const TRAILING_PARENTHESIS = /\s*\([^)]*\)\s*$/;

/** Removes trailing "(…)" groups for display. Never a follow key. */
export function seriesLabel(raw: string): string {
  let title = (raw ?? "").trim();
  for (;;) {
    const shorter = title.replace(TRAILING_PARENTHESIS, "").trim();
    if (shorter === title || shorter === "") return title;
    title = shorter;
  }
}

/** One series: its sid and its episodes, newest first. */
export type Group = { sid: string; shows: Show[] };

export function groupBySid(shows: Show[]): Group[] {
  const bySid = new Map<string, Show[]>();
  for (const show of shows) {
    const list = bySid.get(show.sid);
    if (list) list.push(show);
    else bySid.set(show.sid, [show]);
  }
  return Array.from(bySid, ([sid, list]) => ({
    sid,
    shows: [...list].sort((a, b) => time(b.showtime) - time(a.showtime)),
  }));
}

export function time(iso?: string | null, fallback = 0): number {
  if (!iso) return fallback;
  const value = new Date(iso).getTime();
  return Number.isNaN(value) ? fallback : value;
}

export function groupTitle(group: Group): string {
  for (const pick of [(s: Show) => s.seriesTitle, (s: Show) => s.title]) {
    const found = group.shows.map((s) => seriesLabel(pick(s))).find((t) => t.length > 0);
    if (found) return found;
  }
  return "Untitled series";
}

export const groupType = (group: Group): ContentType => group.shows[0]?.contentType ?? "show";
export const groupPoster = (group: Group) => group.shows.find((s) => s.posterUrl)?.posterUrl ?? null;
export const groupFollowed = (group: Group) => group.shows.some((s) => s.isFollowed);

/** The longest description any episode carries for the series. */
export function groupDescription(group: Group): string | null {
  const texts = group.shows
    .flatMap((s) => [s.seriesDescription, s.episodeDescription])
    .map((t) => (t ?? "").trim())
    .filter((t) => t.length > 0);
  return texts.sort((a, b) => b.length - a.length)[0] ?? null;
}

export function episodeDescription(show: Show): string | null {
  const text = (show.episodeDescription ?? show.seriesDescription ?? "").trim();
  return text || null;
}

const dateTime = new Intl.DateTimeFormat("is-IS", {
  year: "numeric",
  month: "2-digit",
  day: "2-digit",
  hour: "2-digit",
  minute: "2-digit",
});
const dateOnly = new Intl.DateTimeFormat("is-IS", { year: "numeric", month: "2-digit", day: "2-digit" });

export function formatDate(iso?: string | null): string {
  const t = time(iso, NaN);
  return Number.isNaN(t) ? "Unknown date" : dateTime.format(t);
}

export function formatDay(iso?: string | null): string | null {
  const t = time(iso, NaN);
  return Number.isNaN(t) ? null : dateOnly.format(t);
}

export function formatDuration(show: Show): string | null {
  const seconds = Number(show.duration ?? 0);
  if (seconds > 0) {
    const minutes = Math.max(1, Math.round(seconds / 60));
    const hours = Math.floor(minutes / 60);
    const rest = minutes % 60;
    return hours > 0 ? `${hours} hr${rest > 0 ? ` ${rest} min` : ""}` : `${rest} min`;
  }
  return show.durationLabel?.trim() || null;
}

/** "date · duration · expires …" for an episode line. */
export function episodeMeta(show: Show): string {
  const parts = [formatDate(show.showtime)];
  const duration = formatDuration(show);
  if (duration) parts.push(duration);
  const expires = formatDay(show.expiresAt);
  if (expires) parts.push(`expires ${expires}`);
  return parts.join(" · ");
}

export type SortBy = "name" | "duration" | "firstAppearance" | "lastAppearance" | "expiry";
export type SubtitleFilter = "all" | "is" | "en" | "english-version";

export type Filters = {
  contentType: ContentType;
  categories: string[];
  subtitles: SubtitleFilter;
  sortBy: SortBy;
  direction: "asc" | "desc";
};

export const DEFAULT_FILTERS: Filters = {
  contentType: "show",
  categories: [],
  subtitles: "all",
  sortBy: "lastAppearance",
  direction: "desc",
};

export function filterAndSort(groups: Group[], f: Filters): Group[] {
  const kept = groups.filter((g) => {
    if (groupType(g) !== f.contentType) return false;
    if (f.categories.length > 0 && !g.shows.some((s) => s.categories.some((c) => f.categories.includes(c)))) return false;
    if (f.subtitles === "is" && !g.shows.some((s) => s.subtitleLanguages.includes("is"))) return false;
    if (f.subtitles === "en" && !g.shows.some((s) => s.subtitleLanguages.includes("en"))) return false;
    if (f.subtitles === "english-version" && !g.shows.some((s) => s.englishSubtitled)) return false;
    return true;
  });
  const missing = f.direction === "asc" ? Number.MAX_SAFE_INTEGER : -1;
  const value = (g: Group): string | number => {
    switch (f.sortBy) {
      case "name":
        return groupTitle(g).toLocaleLowerCase("is");
      case "duration":
        return Math.max(...g.shows.map((s) => Number(s.duration ?? 0)));
      case "firstAppearance":
        return Math.min(...g.shows.map((s) => time(s.firstAppearedAt ?? s.showtime, missing)));
      case "expiry":
        return Math.min(...g.shows.map((s) => time(s.expiresAt, missing)));
      default:
        return Math.max(...g.shows.map((s) => time(s.showtime)));
    }
  };
  return kept
    .map((g) => ({ g, v: value(g) }))
    .sort((a, b) => {
      const c =
        typeof a.v === "string" && typeof b.v === "string" ? a.v.localeCompare(b.v, "is") : Number(a.v) - Number(b.v);
      return f.direction === "asc" ? c : -c;
    })
    .map(({ g }) => g);
}

/** Where a library download of these shows goes (for the dialog). */
export function libraryDestination(settings: Settings | null, shows: Show[]): string {
  if (!settings) return "the library folder";
  const type = shows[0]?.contentType ?? "show";
  const sub = type === "movie_or_docu" ? settings.moviesSubdir : type === "sport" ? settings.sportsSubdir : settings.showsSubdir;
  const root = (settings.libraryRootDir || "the data folder’s downloads").replace(/[\\/]+$/, "");
  const child = (sub || "").trim();
  if (!child) return root;
  if (child.startsWith("/") || /^[A-Za-z]:[\\/]/.test(child)) return child;
  return `${root}/${child.replace(/^[\\/]+/, "")}`;
}
