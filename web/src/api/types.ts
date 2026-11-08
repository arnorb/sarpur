// Types for sarpur's HTTP API (docs/api.md).

export type ContentType = "movie_or_docu" | "sport" | "show";
export type DownloadMode = "web" | "library";

export type CatalogueStatus = {
  state: "empty" | "loading" | "refreshing" | "ready" | "failed";
  count: number;
  loadedAt: string | null;
  error: string | null;
};

export type Show = {
  pid: string;
  sid: string;
  title: string;
  displayTitle: string;
  seriesTitle: string;
  showtime: string;
  duration: number | null;
  durationLabel: string | null;
  contentType: ContentType;
  englishSubtitled: boolean;
  posterUrl: string | null;
  subtitleLanguages: string[];
  episodeDescription: string | null;
  seriesDescription: string | null;
  firstAppearedAt: string | null;
  expiresAt: string | null;
  webUrl: string;
  categories: string[];
  isFollowed: boolean;
  /** `library`: a file is known in the library; `recorded`: downloaded before, no file known. */
  downloadState: "library" | "recorded" | null;
};

export type JobState = "queued" | "running" | "completed" | "failed" | "cancelled";

export type Job = {
  id: string;
  clientId: string | null;
  pid: string;
  title: string;
  mode: DownloadMode;
  priority: "interactive" | "scheduled";
  state: JobState;
  progress: number;
  message: string | null;
  finalPath: string | null;
  files: { name: string; url: string }[];
  ffmpegExit: number | null;
  error: string | null;
  subtitleLanguages: string[] | null;
  createdAt: string;
  startedAt: string | null;
  finishedAt: string | null;
};

export type Quality = "Normal" | "HD720" | "HD1080";

export type Settings = {
  autoEnabled: boolean;
  autoIntervalMinutes: number;
  libraryRootDir: string;
  showsSubdir: string;
  moviesSubdir: string;
  sportsSubdir: string;
  plexBaseUrl: string;
  plexToken: string;
  plexLibrarySectionId: string;
  plexLibraryPath: string;
  quality: Quality;
};

export type FollowSid = {
  sid: string;
  seriesTitle: string;
  addedBy: "user" | "auto";
  addedAt: string;
};

export type Follow = {
  id: number;
  titleKey: string;
  label: string;
  englishSubtitled: boolean;
  createdAt: string;
  sids: FollowSid[];
};

export type RunReport = {
  dryRun: boolean;
  follows: number;
  episodes: number;
  addedSids: { followId: number; sid: string; seriesTitle: string }[];
  scheduled: { pid: string; title: string; path: string }[];
  skipped: { onDisk: number; nameMatch: number; recorded: number; inQueue: number };
  warnings: string[];
};

export type RunStatus = {
  running: boolean;
  lastRunAt: string | null;
  lastRunState: "ok" | "warning" | "error" | null;
  lastRunMessage: string | null;
  lastReport: RunReport | null;
};

export type ScanStatus = {
  running: boolean;
  lastScanAt: string | null;
  roots: string[];
  files: number;
  tagged: number;
  read: number;
  removed: number;
  errors: number;
  elapsedMs: number;
  error: string | null;
};

export type PlexResult = { state: "ok" | "skipped" | "failed"; message: string };

export type Health = {
  ok: boolean;
  version: string;
  dryRun: boolean;
  catalogue: CatalogueStatus;
  update: UpdateStatus;
};

/** The daily check against GitHub Releases. */
export type UpdateStatus = {
  available: string | null;
  url: string | null;
  checkedAt: string | null;
  error: string | null;
};

export type Session = { required: boolean; authenticated: boolean };

export type PathMapping = { key: string; from: string; to: string; changed: boolean };

export type ImportPreview = {
  source: string;
  settingsFile: boolean;
  recordedFile: boolean;
  eventLogs: number;
  settings: Record<string, unknown>;
  pathMapping: PathMapping[];
  followSids: string[];
  recorded: { total: number; library: number; web: number; unknown: number; alreadyKnown: number };
  copies: string[];
  warnings: string[];
};

export type ImportApplied = {
  preview: ImportPreview;
  recordedAdded: number;
  followsAdded: string[];
  warnings: string[];
};

export type ImportReport = {
  recordedButMissing: { pid: string; origin: string; title: string | null }[];
  onDiskNotRecorded: { pid: string; path: string }[];
  scanError: string | null;
};

/** What `/api/events` sends. */
export type ServerEvent =
  | { type: "job"; job: Job }
  | { type: "file_ready"; clientId: string; jobId: string; name: string; url: string }
  | { type: "follows"; status: RunStatus }
  | { type: "plex"; result: PlexResult };
