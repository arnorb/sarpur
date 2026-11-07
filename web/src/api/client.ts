// A typed client for sarpur's API. Same origin: in development Vite passes
// /api to the server (vite.config.ts).
import type {
  CatalogueStatus,
  DownloadMode,
  Follow,
  Health,
  ImportApplied,
  ImportPreview,
  ImportReport,
  Job,
  PlexResult,
  RunStatus,
  ScanStatus,
  Session,
  Settings,
  Show,
} from "./types";

export class ApiError extends Error {
  constructor(
    message: string,
    readonly status: number,
  ) {
    super(message);
  }
}

/** Called on a 401, so the app can show the login form. */
let onUnauthorized: () => void = () => undefined;
export function setUnauthorizedHandler(handler: () => void) {
  onUnauthorized = handler;
}

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  let response: Response;
  try {
    response = await fetch(path, {
      method,
      headers: body === undefined ? undefined : { "Content-Type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
      credentials: "same-origin",
    });
  } catch {
    throw new ApiError("Can't reach sarpur. Is it running?", 0);
  }
  const text = await response.text();
  let data: unknown = null;
  try {
    data = text ? JSON.parse(text) : null;
  } catch {
    // Not JSON (a proxy error page, say); handled below.
  }
  if (!response.ok) {
    if (response.status === 401 && !path.startsWith("/api/login")) onUnauthorized();
    const serverError =
      data !== null && typeof data === "object" && "error" in data && typeof data.error === "string" ? data.error : "";
    const message =
      serverError ||
      ([502, 503, 504].includes(response.status)
        ? "sarpur isn't answering yet. Try again in a moment."
        : `Request failed (${response.status})`);
    throw new ApiError(message, response.status);
  }
  return data as T;
}

const get = <T>(path: string) => request<T>("GET", path);
const post = <T>(path: string, body?: unknown) => request<T>("POST", path, body ?? {});
const del = <T>(path: string) => request<T>("DELETE", path);
const enc = encodeURIComponent;

export const api = {
  health: () => get<Health>("/api/health"),
  session: () => get<Session>("/api/session"),
  login: (password: string) => post<Session>("/api/login", { password }),
  logout: () => post<{ authenticated: false }>("/api/logout"),

  shows: (query = "") =>
    get<{ shows: Show[]; status: CatalogueStatus }>(`/api/shows${query.trim() ? `?q=${enc(query.trim())}` : ""}`),
  catalogueStatus: () => get<CatalogueStatus>("/api/catalogue/status"),
  refreshCatalogue: () => post<CatalogueStatus>("/api/catalogue/refresh"),

  settings: () => get<Settings>("/api/settings"),
  saveSettings: (settings: Partial<Settings>) => post<Settings>("/api/settings", settings),

  download: (pid: string, mode: DownloadMode, clientId: string, subtitleLanguages: string[]) =>
    post<Job>("/api/downloads", { pid, mode, clientId, subtitleLanguages }),
  jobs: () => get<{ jobs: Job[] }>("/api/jobs"),
  removeJob: (id: string) => del<{ removed: boolean; job: Job | null }>(`/api/jobs/${enc(id)}`),
  retryJob: (id: string) => post<Job>(`/api/jobs/${enc(id)}/retry`),

  follows: () => get<{ follows: Follow[]; status: RunStatus }>("/api/follows"),
  follow: (sid: string) => post<Follow>("/api/follows", { sid }),
  unfollow: (id: number) => del<{ removed: boolean }>(`/api/follows/${id}`),
  removeFollowSid: (id: number, sid: string) =>
    del<{ follow: Follow | null }>(`/api/follows/${id}/sids/${enc(sid)}`),
  followStatus: () => get<RunStatus>("/api/follows/status"),
  runFollows: (dryRun: boolean) => post<RunStatus>("/api/follows/run", { dryRun }),

  forget: (pid: string) => post<{ pid: string; forgottenAt: string | null }>(`/api/recorded/${enc(pid)}/forget`),

  scanStatus: () => get<ScanStatus>("/api/library/scan"),
  scan: () => post<ScanStatus>("/api/library/scan"),
  plexStatus: () => get<{ last: PlexResult | null }>("/api/plex/status"),
  refreshPlex: () => post<PlexResult>("/api/plex/refresh"),

  importCandidates: () => get<{ candidates: string[] }>("/api/import/candidates"),
  importPreview: (source: string) => post<ImportPreview>("/api/import/preview", { source }),
  importApply: (source: string) => post<ImportApplied>("/api/import/apply", { source }),
  importReport: () => get<ImportReport>("/api/import/report"),
};
