import { useEffect, useState } from "react";
import { FolderOpen } from "lucide-react";
import { api } from "@/api/client";
import type { ImportApplied, ImportPreview, ImportReport, Quality, ScanStatus, Settings } from "@/api/types";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Field, LiveNotice, ModalDialog, NoticeBar } from "@/components/common";
import { type DirectoryHandle, folderPickerAvailable, loadDefaultFolder, pickFolder, storeDefaultFolder } from "@/lib/browser";
import { formatDate } from "@/lib/shows";
import { count } from "@/lib/utils";
import { useStore } from "@/store";

const SECTIONS = ["This browser", "Automatic downloads", "Library", "Plex", "Import"] as const;
export type Section = (typeof SECTIONS)[number];

const selectClass =
  "h-9 rounded-md border border-slate-500 bg-slate-800 px-2 text-base text-slate-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-400 sm:text-sm";

export function SettingsDialog({ onClose, initialSection = "This browser" }: { onClose: () => void; initialSection?: Section }) {
  const [section, setSection] = useState<Section>(initialSection);
  return (
    <ModalDialog onClose={onClose} labelledBy="settings-title" className="w-[calc(100%-1rem)] max-w-3xl rounded-xl border border-slate-700 bg-slate-900 shadow-2xl">
      <div className="flex items-center justify-between border-b border-slate-800 p-4">
        <h2 id="settings-title" className="text-lg font-semibold text-slate-100">
          Settings
        </h2>
        <Button size="sm" variant="secondary" onClick={onClose}>
          Close
        </Button>
      </div>
      {/* Results of actions in here; the page's own notice is behind the backdrop. */}
      <LiveNotice />
      <NoticeBar className="mx-4 mt-3" />
      <div className="flex flex-col md:flex-row">
        <nav aria-label="Settings sections" className="flex flex-wrap gap-1 border-b border-slate-800 p-2 md:w-48 md:flex-col md:flex-nowrap md:border-b-0 md:border-r">
          {SECTIONS.map((s) => (
            <button
              key={s}
              type="button"
              aria-current={section === s ? "page" : undefined}
              onClick={() => setSection(s)}
              className={`shrink-0 rounded-md px-3 py-2 text-left text-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-400 ${section === s ? "bg-sky-600/25 text-sky-100" : "text-slate-300 hover:bg-slate-800 hover:text-white"}`}
            >
              {s}
            </button>
          ))}
        </nav>
        {/* Every section stays mounted, so unsaved changes survive switching. */}
        <div className="max-h-[70vh] min-w-0 flex-1 overflow-y-auto overscroll-contain p-4">
          <div hidden={section !== "This browser"}>
            <BrowserSection />
          </div>
          <div hidden={section !== "Automatic downloads"}>
            <AutomationSection />
          </div>
          <div hidden={section !== "Library"}>
            <LibrarySection />
          </div>
          <div hidden={section !== "Plex"}>
            <PlexSection />
          </div>
          <div hidden={section !== "Import"}>
            <ImportSection />
          </div>
        </div>
      </div>
    </ModalDialog>
  );
}

/** Edits a copy of the settings and saves only the given keys. */
function useDraft<K extends keyof Settings>(keys: K[]) {
  const { settings, saveSettings } = useStore();
  const [draft, setDraft] = useState<Pick<Settings, K> | null>(null);
  useEffect(() => {
    if (settings && !draft) setDraft(Object.fromEntries(keys.map((k) => [k, settings[k]])) as Pick<Settings, K>);
  }, [settings, draft, keys]);
  const set = <F extends K>(key: F, value: Settings[F]) => setDraft((d) => (d ? { ...d, [key]: value } : d));
  const dirty = !!draft && !!settings && keys.some((k) => draft[k] !== settings[k]);
  const save = async (fix?: (d: Pick<Settings, K>) => Pick<Settings, K>) => {
    if (!draft) return;
    const value = fix ? fix(draft) : draft;
    if (value !== draft) setDraft(value);
    await saveSettings(value);
  };
  return { draft, set, save, dirty };
}

/** "Save changes" while the section has unsaved changes, else "Saved". */
function SaveButton({ dirty, onSave }: { dirty: boolean; onSave: () => void }) {
  return (
    <Button size="sm" variant={dirty ? "default" : "secondary"} aria-disabled={!dirty} onClick={() => dirty && onSave()}>
      {dirty ? "Save changes" : "Saved"}
    </Button>
  );
}

function BrowserSection() {
  const { mainAction, setMainAction, say, fail, session, logout } = useStore();
  const [folder, setFolder] = useState<DirectoryHandle | null>(null);
  const picker = folderPickerAvailable();
  useEffect(() => {
    if (picker) loadDefaultFolder().then(setFolder, () => undefined);
  }, [picker]);

  return (
    <div className="space-y-5">
      <fieldset>
        <legend className="mb-2 text-sm font-medium text-slate-200">Main button on episodes</legend>
        <div className="flex flex-wrap gap-4 text-sm text-slate-100">
          {(["web", "library"] as const).map((mode) => (
            <label key={mode} className="flex items-center gap-2">
              <input type="radio" name="main-action" className="accent-sky-500" checked={mainAction === mode} onChange={() => setMainAction(mode)} />
              {mode === "web" ? "Download to this computer" : "Download to library"}
            </label>
          ))}
        </div>
        <p className="mt-1 text-xs text-slate-300">Remembered by this browser only.</p>
      </fieldset>

      <div>
        <p className="text-sm font-medium text-slate-200">Default folder for downloads to this computer</p>
        <p className="mt-1 text-xs text-slate-300">
          {picker
            ? folder
              ? `Currently “${folder.name}”, remembered by this browser.`
              : "None: downloads go to the browser’s download folder."
            : "This browser can’t remember a folder; its own download folder is used."}
        </p>
        <div className="mt-2 flex gap-2">
          {folder ? (
            <Button
              size="sm"
              variant="secondary"
              onClick={async () => {
                await storeDefaultFolder(null);
                setFolder(null);
                say("The default folder was removed.");
              }}
            >
              Forget
            </Button>
          ) : null}
          <Button
            size="sm"
            disabled={!picker}
            
            onClick={async () => {
              try {
                const picked = await pickFolder();
                await storeDefaultFolder(picked);
                setFolder(picked);
                say(`Default folder set to ${picked.name}.`);
              } catch (error) {
                if (!(error instanceof DOMException && error.name === "AbortError")) fail(error, "Couldn't set the folder");
              }
            }}
          >
            <FolderOpen className="mr-2 h-4 w-4" aria-hidden />
            {folder ? "Change folder" : "Choose folder"}
          </Button>
        </div>
      </div>

      {session?.required ? (
        <Button size="sm" variant="secondary" onClick={() => void logout()}>
          Log out
        </Button>
      ) : null}
    </div>
  );
}

const AUTOMATION_KEYS: ("autoEnabled" | "autoIntervalMinutes")[] = ["autoEnabled", "autoIntervalMinutes"];

const MIN_INTERVAL = 5;
const clampInterval = (minutes: number) => (minutes > 0 ? Math.max(MIN_INTERVAL, Math.round(minutes)) : 60);

function AutomationSection() {
  const { draft, set, save, dirty } = useDraft(AUTOMATION_KEYS);
  const { runStatus } = useStore();
  if (!draft) return <p className="text-sm text-slate-300">Loading…</p>;
  const tooShort = draft.autoIntervalMinutes > 0 && draft.autoIntervalMinutes < MIN_INTERVAL;
  return (
    <div className="space-y-4">
      <label className="flex items-center gap-2 text-sm text-slate-100">
        <input type="checkbox" className="h-4 w-4 accent-sky-500" checked={draft.autoEnabled} onChange={(e) => set("autoEnabled", e.target.checked)} />
        Download new episodes of followed shows automatically
      </label>
      {/* The typed number is kept as is; it's only corrected on leaving the field and on save. */}
      <Field label="Check every (minutes)" hint={tooShort ? "At least 5 minutes; it will be saved as 5." : "At least 5 minutes."}>
        <Input
          type="number"
          min={MIN_INTERVAL}
          inputMode="numeric"
          className="w-32"
          aria-invalid={tooShort || undefined}
          value={draft.autoIntervalMinutes || ""}
          onChange={(e) => set("autoIntervalMinutes", Number.isNaN(e.target.valueAsNumber) ? 0 : e.target.valueAsNumber)}
          onBlur={() => set("autoIntervalMinutes", clampInterval(draft.autoIntervalMinutes))}
        />
      </Field>
      <p className="text-xs text-slate-300">
        Last run: {runStatus?.lastRunAt ? `${formatDate(runStatus.lastRunAt)}, ${runStatus.lastRunMessage ?? ""}` : "never"}. See Following for details.
      </p>
      <SaveButton dirty={dirty} onSave={() => void save((d) => ({ ...d, autoIntervalMinutes: clampInterval(d.autoIntervalMinutes) }))} />
    </div>
  );
}

const LIBRARY_KEYS: ("libraryRootDir" | "showsSubdir" | "moviesSubdir" | "sportsSubdir" | "quality")[] = [
  "libraryRootDir",
  "showsSubdir",
  "moviesSubdir",
  "sportsSubdir",
  "quality",
];

function LibrarySection() {
  const { draft, set, save, dirty } = useDraft(LIBRARY_KEYS);
  const [scan, setScan] = useState<ScanStatus | null>(null);
  const { fail } = useStore();
  useEffect(() => {
    api.scanStatus().then(setScan, () => undefined);
  }, []);
  useEffect(() => {
    if (!scan?.running) return;
    const timer = window.setInterval(() => api.scanStatus().then(setScan, () => undefined), 1000);
    return () => window.clearInterval(timer);
  }, [scan?.running]);
  if (!draft) return <p className="text-sm text-slate-300">Loading…</p>;
  return (
    <div className="space-y-4">
      <Field label="Library folder" hint="On the machine sarpur runs on, e.g. /media.">
        <Input value={draft.libraryRootDir} onChange={(e) => set("libraryRootDir", e.target.value)} />
      </Field>
      <div className="grid gap-3 sm:grid-cols-3">
        <Field label="Shows subfolder">
          <Input value={draft.showsSubdir} onChange={(e) => set("showsSubdir", e.target.value)} />
        </Field>
        <Field label="Movies subfolder">
          <Input value={draft.moviesSubdir} onChange={(e) => set("moviesSubdir", e.target.value)} />
        </Field>
        <Field label="Sports subfolder">
          <Input value={draft.sportsSubdir} onChange={(e) => set("sportsSubdir", e.target.value)} />
        </Field>
      </div>
      <Field label="Quality">
        <select
          className={selectClass}
          value={draft.quality}
          onChange={(e) => set("quality", e.target.value as Quality)}
        >
          <option value="HD1080">1080p</option>
          <option value="HD720">720p</option>
          <option value="Normal">Normal</option>
        </select>
      </Field>
      <SaveButton dirty={dirty} onSave={() => void save()} />

      <div className="border-t border-slate-800 pt-4">
        <p className="text-sm font-medium text-slate-200">Library scan</p>
        <p className="mt-1 text-xs text-slate-300">
          Finds RÚV files in the library by their tags, so nothing already there is downloaded again. It runs at startup and before each follow run.
        </p>
        {scan?.lastScanAt ? (
          <p className="mt-1 text-xs text-slate-300">
            Last scan {formatDate(scan.lastScanAt)}: {count(scan.files, "file", "files")}, {scan.tagged} from RÚV, {scan.errors} unreadable ({scan.elapsedMs} ms).
          </p>
        ) : null}
        <Button
          size="sm"
          variant="secondary"
          className="mt-2"
          aria-disabled={!!scan?.running}
          onClick={() => !scan?.running && api.scan().then(setScan, (error) => fail(error, "Couldn't start the library scan"))}
        >
          {scan?.running ? "Scanning…" : "Scan now"}
        </Button>
      </div>
    </div>
  );
}

const PLEX_KEYS: ("plexBaseUrl" | "plexToken" | "plexLibrarySectionId" | "plexLibraryPath")[] = [
  "plexBaseUrl",
  "plexToken",
  "plexLibrarySectionId",
  "plexLibraryPath",
];

function PlexSection() {
  const { draft, set, save, dirty } = useDraft(PLEX_KEYS);
  const { lastPlex, fail } = useStore();
  const [result, setResult] = useState(lastPlex);
  useEffect(() => setResult(lastPlex), [lastPlex]);
  if (!draft) return <p className="text-sm text-slate-300">Loading…</p>;
  return (
    <div className="space-y-4">
      <p className="text-xs text-slate-300">After library downloads, sarpur asks Plex to rescan once the queue is empty.</p>
      <Field label="Plex address" hint="e.g. http://truenas:32400">
        <Input value={draft.plexBaseUrl} onChange={(e) => set("plexBaseUrl", e.target.value)} />
      </Field>
      <div className="grid gap-3 sm:grid-cols-2">
        <Field label="Library section id" hint="The number after source= in the library’s Plex Web address.">
          <Input value={draft.plexLibrarySectionId} onChange={(e) => set("plexLibrarySectionId", e.target.value)} />
        </Field>
        <Field label="Token" hint="In Plex Web: an episode’s ⋯ → Get Info → View XML; the X-Plex-Token value in the address.">
          <Input type="password" autoComplete="off"  value={draft.plexToken} onChange={(e) => set("plexToken", e.target.value)} />
        </Field>
      </div>
      <Field label="Only this folder (optional)" hint="As Plex sees it.">
        <Input value={draft.plexLibraryPath} onChange={(e) => set("plexLibraryPath", e.target.value)} />
      </Field>
      <div className="flex flex-wrap items-center gap-2">
        <SaveButton dirty={dirty} onSave={() => void save()} />
        <Button size="sm" variant="secondary" onClick={() => api.refreshPlex().then(setResult, (error) => fail(error, "Couldn't reach Plex"))}>
          Refresh Plex now
        </Button>
        {result ? (
          <span role="status" className={`text-xs ${result.state === "failed" ? "text-rose-200" : "text-slate-300"}`}>
            {result.message}
          </span>
        ) : null}
      </div>
    </div>
  );
}

const SETTING_LABEL: Record<string, string> = {
  libraryRootDir: "Library folder",
  showsSubdir: "Shows subfolder",
  moviesSubdir: "Movies subfolder",
  sportsSubdir: "Sports subfolder",
  plexLibraryPath: "Plex folder",
};

function ImportSection() {
  const { fail } = useStore();
  const [source, setSource] = useState("");
  const [candidates, setCandidates] = useState<string[]>([]);
  const [preview, setPreview] = useState<ImportPreview | null>(null);
  const [applied, setApplied] = useState<ImportApplied | null>(null);
  const [report, setReport] = useState<ImportReport | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    api.importCandidates().then((d) => {
      setCandidates(d.candidates);
      setSource((s) => s || d.candidates[0] || "");
    }, () => undefined);
  }, []);
  const run = async <T,>(action: string, work: () => Promise<T>, done: (value: T) => void) => {
    if (busy) return;
    setBusy(true);
    try {
      done(await work());
    } catch (error) {
      fail(error, action);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="space-y-4">
      <p className="text-xs text-slate-300">
        Brings in ruvsarpur’s settings, followed shows and download history from its folder (the one with <code>gui-settings.json</code> and{" "}
        <code>prevrecorded.log</code>). You see what changes before anything is imported.
      </p>
      <Field label="ruvsarpur’s folder" hint={candidates.length > 0 ? `Found: ${candidates.join(", ")}` : undefined}>
        <Input
          
          value={source}
          list="import-candidates"
          onChange={(e) => {
            setSource(e.target.value);
            setPreview(null);
            setApplied(null);
          }}
        />
        <datalist id="import-candidates">
          {candidates.map((c) => (
            <option key={c} value={c} />
          ))}
        </datalist>
      </Field>
      <div className="flex flex-wrap gap-2">
        <Button
          size="sm"
          aria-disabled={busy || !source.trim()}
          onClick={() => (source.trim() ? void run("Couldn't preview the import", () => api.importPreview(source), setPreview) : fail("Enter ruvsarpur’s folder first."))}
        >
          Preview
        </Button>
        <Button size="sm" variant="secondary" aria-disabled={busy} onClick={() => void run("Couldn't compare the history", () => api.importReport(), setReport)}>
          Compare history with the library
        </Button>
      </div>

      {preview ? (
        <div className="space-y-3 rounded-lg border border-slate-700 bg-slate-950/60 p-3 text-sm">
          {preview.pathMapping.length > 0 ? (
            <div className="overflow-x-auto">
              <table className="w-full text-left text-xs">
                <caption className="mb-1 text-left text-sm font-medium text-slate-200">Folders</caption>
                <thead className="text-slate-300">
                  <tr>
                    <th className="py-1 pr-2 font-medium">Setting</th>
                    <th className="py-1 pr-2 font-medium">In ruvsarpur</th>
                    <th className="py-1 font-medium">In sarpur</th>
                  </tr>
                </thead>
                <tbody className="text-slate-100">
                  {preview.pathMapping.map((m) => (
                    <tr key={m.key} className="border-t border-slate-800 align-top">
                      <td className="py-1 pr-2">{SETTING_LABEL[m.key] ?? m.key}</td>
                      <td className="break-all py-1 pr-2">
                        <code>{m.from || "—"}</code>
                      </td>
                      <td className={`break-all py-1 ${m.changed ? "text-amber-200" : ""}`}>
                        <code>{m.to || "—"}</code>
                        {m.changed ? " (changed)" : ""}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          ) : null}
          <p className="text-slate-200">
            History: {count(preview.recorded.total, "episode", "episodes")} ({preview.recorded.library} in the library, {preview.recorded.web} downloaded to a browser,{" "}
            {preview.recorded.unknown} from automatic runs; {preview.recorded.alreadyKnown} sarpur already knows). Follows: {preview.followSids.length}.
            {preview.copies.length > 0 ? ` Copies ${preview.copies.join(" and ")}.` : ""}
          </p>
          <p className="text-xs text-slate-300">
            After the import, follow runs won’t download these episodes again, except the {preview.recorded.web} downloaded to a browser.
          </p>
          {preview.warnings.map((w) => (
            <p key={w} className="text-xs text-amber-200">
              {w}
            </p>
          ))}
          {applied ? (
            <div role="status" className="flex flex-wrap items-center gap-3 text-emerald-200">
              <p>
                Imported {count(applied.recordedAdded, "episode", "episodes")} and {count(applied.followsAdded.length, "follow", "follows")}. Reload to see them.
                {applied.warnings.length > 0 ? ` ${applied.warnings.join(" ")}` : ""}
              </p>
              <Button size="sm" onClick={() => window.location.reload()}>
                Reload now
              </Button>
            </div>
          ) : (
            <Button size="sm" aria-disabled={busy} onClick={() => void run("Couldn't import", () => api.importApply(source), setApplied)}>
              Import {count(preview.recorded.total, "episode", "episodes")} and {count(preview.followSids.length, "follow", "follows")}
            </Button>
          )}
        </div>
      ) : null}

      {report ? (
        <div className="space-y-2 rounded-lg border border-slate-700 bg-slate-950/60 p-3 text-xs text-slate-200">
          {report.scanError ? <p className="text-rose-200">Library scan failed: {report.scanError}</p> : null}
          <details>
            <summary className="cursor-pointer rounded text-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-400">Downloaded before but not in the library: {report.recordedButMissing.length}</summary>
            <p className="mt-1 text-slate-300">Probably deleted on purpose. Follow runs skip these until you use “Include in follow runs”.</p>
            <ul className="mt-1 max-h-48 overflow-y-auto overscroll-contain">
              {report.recordedButMissing.map((m) => (
                <li key={m.pid}>
                  {m.title ?? m.pid} <span className="text-slate-400">({m.pid}, {m.origin})</span>
                </li>
              ))}
            </ul>
          </details>
          <details>
            <summary className="cursor-pointer rounded text-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-sky-400">In the library but not in the history: {report.onDiskNotRecorded.length}</summary>
            <ul className="mt-1 max-h-48 overflow-y-auto overscroll-contain">
              {report.onDiskNotRecorded.map((m) => (
                <li key={m.path} className="break-all">
                  <code>{m.path}</code>
                </li>
              ))}
            </ul>
          </details>
        </div>
      ) : null}
    </div>
  );
}
