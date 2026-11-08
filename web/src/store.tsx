// The app's shared state and actions. Jobs and follow-run status stay live
// through the server's event stream (/api/events) instead of polling.
import { createContext, type ReactNode, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import { api, ApiError, setUnauthorizedHandler } from "@/api/client";
import type {
  CatalogueStatus,
  DownloadMode,
  Follow,
  Health,
  Job,
  PlexResult,
  RunStatus,
  ServerEvent,
  Session,
  Settings,
  Show,
} from "@/api/types";
import {
  type DirectoryHandle,
  getClientId,
  loadHandledJobs,
  loadMainAction,
  persistHandledJobs,
  saveFilesToFolder,
  saveMainAction,
  startBrowserDownloads,
} from "@/lib/browser";
import { count } from "@/lib/utils";

/** A message for the user; `undo` puts back what the action just removed. */
type Notice = { kind: "status" | "error"; text: string; undo?: () => Promise<void> } | null;

type Store = {
  session: Session | null;
  health: Health | null;
  shows: Show[];
  showsLoading: boolean;
  query: string;
  catalogue: CatalogueStatus | null;
  settings: Settings | null;
  follows: Follow[];
  runStatus: RunStatus | null;
  jobs: Job[];
  lastPlex: PlexResult | null;
  notice: Notice;
  mainAction: DownloadMode;

  login(password: string): Promise<void>;
  logout(): Promise<void>;
  loadShows(query: string): Promise<void>;
  refreshCatalogue(): Promise<void>;
  saveSettings(patch: Partial<Settings>): Promise<boolean>;
  setFollowed(sid: string, follow: boolean): Promise<void>;
  removeFollowSid(follow: Follow, sid: string): Promise<void>;
  unfollow(follow: Follow): Promise<void>;
  runFollows(dryRun: boolean): Promise<void>;
  download(shows: Show[], mode: DownloadMode, languages: string[], folder?: DirectoryHandle): Promise<void>;
  removeJob(job: Job): Promise<void>;
  retryJob(job: Job): Promise<void>;
  clearFinished(): Promise<void>;
  allowRedownload(show: Show): Promise<void>;
  setMainAction(mode: DownloadMode): void;
  say(text: string, undo?: () => Promise<void>): void;
  /** Shows an error; `action` says what failed ("Couldn't save the settings"). */
  fail(error: unknown, action?: string): void;
  clearNotice(): void;
};

const StoreContext = createContext<Store | null>(null);

export function useStore(): Store {
  const store = useContext(StoreContext);
  if (!store) throw new Error("useStore outside StoreProvider");
  return store;
}

const message = (error: unknown) => (error instanceof Error ? error.message : String(error));
const finished = (job: Job) => ["completed", "failed", "cancelled"].includes(job.state);

export function StoreProvider({ children }: { children: ReactNode }) {
  const [session, setSession] = useState<Session | null>(null);
  const [health, setHealth] = useState<Health | null>(null);
  const [shows, setShows] = useState<Show[]>([]);
  const [showsLoading, setShowsLoading] = useState(false);
  const [query, setQuery] = useState("");
  const [catalogue, setCatalogue] = useState<CatalogueStatus | null>(null);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [follows, setFollows] = useState<Follow[]>([]);
  const [runStatus, setRunStatus] = useState<RunStatus | null>(null);
  const [jobs, setJobs] = useState<Job[]>([]);
  const [lastPlex, setLastPlex] = useState<PlexResult | null>(null);
  const [notice, setNotice] = useState<Notice>(null);
  const [mainAction, setMainActionState] = useState<DownloadMode>(loadMainAction);
  const handled = useRef<Set<string>>(loadHandledJobs());
  const folders = useRef<Record<string, DirectoryHandle>>({});

  const say = useCallback((text: string, undo?: () => Promise<void>) => setNotice({ kind: "status", text, undo }), []);
  const fail = useCallback(
    (error: unknown, action?: string) =>
      setNotice({ kind: "error", text: action ? `${action}. ${message(error)}` : message(error) }),
    [],
  );
  const clearNotice = useCallback(() => setNotice(null), []);

  useEffect(() => {
    setUnauthorizedHandler(() => setSession({ required: true, authenticated: false }));
    api.session().then(setSession, () => setSession({ required: false, authenticated: true }));
  }, []);
  const ready = !!session && (!session.required || session.authenticated);

  /** Saves a finished browser download of this browser, once (also after a reload). */
  const deliver = useCallback(
    async (job: Job) => {
      if (job.state !== "completed" || job.mode !== "web" || job.clientId !== getClientId()) return;
      if (handled.current.has(job.id) || job.files.length === 0) return;
      handled.current.add(job.id);
      persistHandledJobs(handled.current);
      try {
        const folder = folders.current[job.id];
        if (folder) {
          await saveFilesToFolder(job.files, folder);
          delete folders.current[job.id];
        } else {
          startBrowserDownloads(job.files);
        }
      } catch (error) {
        fail(error, "Couldn't save the downloaded file");
      }
    },
    [fail],
  );

  const markFollowed = useCallback((list: Follow[]) => {
    const sids = new Set(list.flatMap((f) => f.sids.map((s) => s.sid)));
    setShows((current) => current.map((s) => (s.isFollowed === sids.has(s.sid) ? s : { ...s, isFollowed: sids.has(s.sid) })));
  }, []);

  const reloadFollows = useCallback(async () => {
    const data = await api.follows();
    setFollows(data.follows);
    setRunStatus(data.status);
    markFollowed(data.follows);
  }, [markFollowed]);

  const loadShows = useCallback(
    async (q: string) => {
      setShowsLoading(true);
      try {
        const data = await api.shows(q);
        // Only now: a failed search must not label the old list as its results.
        setQuery(q);
        setShows(data.shows);
        setCatalogue(data.status);
      } catch (error) {
        fail(error, q ? `Couldn't search for “${q}”` : "Couldn't load the catalogue");
      } finally {
        setShowsLoading(false);
      }
    },
    [fail],
  );

  // Initial data once logged in.
  useEffect(() => {
    if (!ready) return;
    api.health().then(setHealth, () => undefined);
    void loadShows("");
    api.settings().then(setSettings, (error) => fail(error, "Couldn't load the settings"));
    reloadFollows().catch((error) => fail(error, "Couldn't load the followed shows"));
    api.jobs().then((data) => {
      setJobs(data.jobs);
      data.jobs.forEach((job) => void deliver(job));
    }, (error) => fail(error, "Couldn't load the download queue"));
    api.plexStatus().then((data) => setLastPlex(data.last), () => undefined);
  }, [ready, loadShows, reloadFollows, deliver, fail]);

  // The update check runs daily on the server; pick its answer up hourly.
  useEffect(() => {
    if (!ready) return;
    const timer = window.setInterval(() => api.health().then(setHealth, () => undefined), 3_600_000);
    return () => window.clearInterval(timer);
  }, [ready]);

  // The catalogue loads in the background at startup: reload the shows when it's ready.
  useEffect(() => {
    if (!ready || !catalogue || !["loading", "refreshing", "empty"].includes(catalogue.state)) return;
    const timer = window.setInterval(async () => {
      try {
        const status = await api.catalogueStatus();
        setCatalogue(status);
        if (status.state === "ready" || status.state === "failed") {
          window.clearInterval(timer);
          if (status.state === "ready") void loadShows(query);
        }
      } catch {
        // Try again on the next tick.
      }
    }, 2000);
    return () => window.clearInterval(timer);
  }, [ready, catalogue, loadShows, query]);

  // Live updates.
  useEffect(() => {
    if (!ready) return;
    const source = new EventSource(`/api/events?client_id=${encodeURIComponent(getClientId())}`);
    const onEvent = (event: MessageEvent<string>) => {
      let data: ServerEvent;
      try {
        data = JSON.parse(event.data) as ServerEvent;
      } catch {
        return;
      }
      if (data.type === "job") {
        const job = data.job;
        setJobs((current) => {
          const index = current.findIndex((j) => j.id === job.id);
          if (index === -1) return [job, ...current];
          const next = [...current];
          next[index] = job;
          return next;
        });
        if (job.state === "completed") {
          if (job.mode === "library") {
            setShows((current) =>
              current.map((s) => (s.pid === job.pid ? { ...s, downloadState: "library" } : s)),
            );
          }
          void deliver(job);
        }
      } else if (data.type === "follows") {
        setRunStatus(data.status);
        if (!data.status.running) reloadFollows().catch(() => undefined);
      } else if (data.type === "plex") {
        setLastPlex(data.result);
      }
    };
    for (const name of ["job", "follows", "plex"]) source.addEventListener(name, onEvent as EventListener);
    // After a dropped connection, catch up on what was missed.
    source.onopen = () => {
      api.jobs().then((data) => {
        setJobs(data.jobs);
        data.jobs.forEach((job) => void deliver(job));
      }, () => undefined);
    };
    return () => source.close();
  }, [ready, deliver, reloadFollows]);

  const store: Store = useMemo(
    () => ({
      session,
      health,
      shows,
      showsLoading,
      query,
      catalogue,
      settings,
      follows,
      runStatus,
      jobs,
      lastPlex,
      notice,
      mainAction,

      async login(password) {
        setSession(await api.login(password));
      },
      async logout() {
        try {
          await api.logout();
          setSession({ required: true, authenticated: false });
        } catch (error) {
          fail(error, "Couldn't log out");
        }
      },
      loadShows,
      async refreshCatalogue() {
        try {
          setCatalogue(await api.refreshCatalogue());
          say("Fetching the catalogue from RÚV. This takes a few minutes.");
        } catch (error) {
          fail(error, "Couldn't refresh the catalogue");
        }
      },
      async saveSettings(patch) {
        try {
          setSettings(await api.saveSettings(patch));
          say("Settings saved.");
          return true;
        } catch (error) {
          fail(error, "Couldn't save the settings");
          return false;
        }
      },
      async setFollowed(sid, follow) {
        try {
          if (follow) {
            await api.follow(sid);
            await reloadFollows();
          } else {
            const owner = follows.find((f) => f.sids.some((s) => s.sid === sid));
            if (!owner) return;
            await api.removeFollowSid(owner.id, sid);
            await reloadFollows();
            say(`Stopped following ${owner.sids.find((s) => s.sid === sid)?.seriesTitle ?? owner.label}.`, async () => {
              await api.follow(sid);
              await reloadFollows();
            });
          }
        } catch (error) {
          fail(error, follow ? "Couldn't follow the show" : "Couldn't stop following the show");
        }
      },
      async removeFollowSid(follow, sid) {
        try {
          await api.removeFollowSid(follow.id, sid);
          await reloadFollows();
          say(`Removed ${follow.sids.find((s) => s.sid === sid)?.seriesTitle ?? sid} from ${follow.label}.`, async () => {
            await api.follow(sid);
            await reloadFollows();
          });
        } catch (error) {
          fail(error, "Couldn't remove the season");
        }
      },
      async unfollow(follow) {
        try {
          await api.unfollow(follow.id);
          await reloadFollows();
          say(`Stopped following ${follow.label}.`, async () => {
            for (const { sid } of follow.sids) await api.follow(sid);
            await reloadFollows();
          });
        } catch (error) {
          fail(error, `Couldn't stop following ${follow.label}`);
        }
      },
      async runFollows(dryRun) {
        try {
          setRunStatus(await api.runFollows(dryRun));
        } catch (error) {
          fail(error, dryRun ? "Couldn't preview the follow run" : "Couldn't start the follow run");
        }
      },
      async download(list, mode, languages, folder) {
        try {
          const queued: Job[] = [];
          for (const show of list) {
            const wanted = languages.filter((l) => show.subtitleLanguages.includes(l));
            const job = await api.download(show.pid, mode, getClientId(), wanted);
            if (folder) folders.current[job.id] = folder;
            queued.push(job);
          }
          // The event stream may already have newer states (a dry run or an
          // episode already in the library finishes at once): keep those.
          setJobs((current) => {
            const fresh = queued.reverse().filter((q) => !current.some((j) => j.id === q.id));
            return [...fresh, ...current];
          });
          say(`${count(list.length, "episode", "episodes")} added to the download queue.`);
        } catch (error) {
          fail(error, "Couldn't add the download");
        }
      },
      async removeJob(job) {
        try {
          const result = await api.removeJob(job.id);
          setJobs((current) =>
            result.removed ? current.filter((j) => j.id !== job.id) : current.map((j) => (j.id === job.id && result.job ? result.job : j)),
          );
        } catch (error) {
          fail(error, `Couldn't stop “${job.title}”`);
        }
      },
      async retryJob(job) {
        try {
          const retried = await api.retryJob(job.id);
          handled.current.delete(job.id);
          setJobs((current) => {
            const known = current.find((j) => j.id === retried.id);
            const latest = known && known.state !== job.state ? known : retried;
            return [latest, ...current.filter((j) => j.id !== job.id)];
          });
          say(`${job.title} is queued again.`);
        } catch (error) {
          fail(error, `Couldn't retry “${job.title}”`);
        }
      },
      async clearFinished() {
        // Completed only: failed and cancelled jobs keep their Retry button.
        const done = jobs.filter((job) => job.state === "completed");
        try {
          for (const job of done) await api.removeJob(job.id);
          setJobs((current) => current.filter((j) => !done.some((d) => d.id === j.id)));
        } catch (error) {
          fail(error, "Couldn't clear the completed downloads");
        }
      },
      async allowRedownload(show) {
        try {
          await api.forget(show.pid);
          setShows((current) => current.map((s) => (s.pid === show.pid ? { ...s, downloadState: null } : s)));
          say(`Follow runs will include “${show.displayTitle}” again.`);
        } catch (error) {
          if (error instanceof ApiError && error.status === 404) {
            setShows((current) => current.map((s) => (s.pid === show.pid ? { ...s, downloadState: null } : s)));
          } else {
            fail(error, "Couldn't include the episode in follow runs again");
          }
        }
      },
      setMainAction(mode) {
        saveMainAction(mode);
        setMainActionState(mode);
      },
      say,
      fail,
      clearNotice,
    }),
    [
      session,
      health,
      shows,
      showsLoading,
      query,
      catalogue,
      settings,
      follows,
      runStatus,
      jobs,
      lastPlex,
      notice,
      mainAction,
      loadShows,
      reloadFollows,
      say,
      fail,
      clearNotice,
    ],
  );

  return <StoreContext.Provider value={store}>{children}</StoreContext.Provider>;
}

export const isActive = (job: Job) => job.state === "queued" || job.state === "running";
export const isFinished = finished;
