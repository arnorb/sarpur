import { useEffect, useState } from "react";
import { FolderOpen } from "lucide-react";
import { api } from "@/api/client";
import type { ImportApplied, ImportPreview, ImportReport, Quality, ScanStatus, Settings } from "@/api/types";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Field, inputClass, ModalDialog, primaryButton, secondaryButton } from "@/components/common";
import { type DirectoryHandle, folderPickerAvailable, loadDefaultFolder, pickFolder, storeDefaultFolder } from "@/lib/browser";
import { formatDate } from "@/lib/shows";
import { useStore } from "@/store";

const SECTIONS = ["This browser", "Automatic downloads", "Library", "Plex", "Import"] as const;
type Section = (typeof SECTIONS)[number];

export function SettingsDialog({ onClose }: { onClose: () => void }) {
  const [section, setSection] = useState<Section>("This browser");
  return (
    <ModalDialog onClose={onClose} labelledBy="settings-title" className="w-[calc(100%-1rem)] max-w-3xl rounded-xl border border-slate-700 bg-slate-900 shadow-2xl">
      <div className="flex items-center justify-between border-b border-slate-800 p-4">
        <h2 id="settings-title" className="text-lg font-semibold text-slate-100">
          Settings
        </h2>
        <Button size="sm" className={secondaryButton} onClick={onClose}>
          Close
        </Button>
      </div>
      <div className="flex flex-col md:flex-row">
        <nav aria-label="Settings sections" className="flex gap-1 overflow-x-auto border-b border-slate-800 p-2 md:w-48 md:flex-col md:border-b-0 md:border-r">
          {SECTIONS.map((s) => (
            <button
              key={s}
              type="button"
              aria-current={section === s ? "page" : undefined}
              onClick={() => setSection(s)}
              className={`shrink-0 rounded-md px-3 py-2 text-left text-sm ${section === s ? "bg-sky-600/25 text-sky-100" : "text-slate-300 hover:bg-slate-800 hover:text-white"}`}
            >
              {s}
            </button>
          ))}
        </nav>
        <div className="max-h-[70vh] min-w-0 flex-1 overflow-y-auto p-4">
          {section === "This browser" ? <BrowserSection /> : null}
          {section === "Automatic downloads" ? <AutomationSection /> : null}
          {section === "Library" ? <LibrarySection /> : null}
          {section === "Plex" ? <PlexSection /> : null}
          {section === "Import" ? <ImportSection /> : null}
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
  const save = async () => {
    if (draft) await saveSettings(draft);
  };
  return { draft, set, save };
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
              {mode === "web" ? "Download to this computer" : "Download to the library"}
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
              className={secondaryButton}
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
            className={primaryButton}
            onClick={async () => {
              try {
                const picked = await pickFolder();
                await storeDefaultFolder(picked);
                setFolder(picked);
                say(`Default folder set to ${picked.name}.`);
              } catch (error) {
                if (!(error instanceof DOMException && error.name === "AbortError")) fail(error);
              }
            }}
          >
            <FolderOpen className="mr-2 h-4 w-4" aria-hidden />
            {folder ? "Change folder" : "Choose folder"}
          </Button>
        </div>
      </div>

      {session?.required ? (
        <Button size="sm" className={secondaryButton} onClick={() => void logout()}>
          Log out
        </Button>
      ) : null}
    </div>
  );
}

const AUTOMATION_KEYS: ("autoEnabled" | "autoIntervalMinutes")[] = ["autoEnabled", "autoIntervalMinutes"];

function AutomationSection() {
  const { draft, set, save } = useDraft(AUTOMATION_KEYS);
  const { runStatus } = useStore();
  if (!draft) return <p className="text-sm text-slate-300">Loading…</p>;
  return (
    <div className="space-y-4">
      <label className="flex items-center gap-2 text-sm text-slate-100">
        <input type="checkbox" className="h-4 w-4 accent-sky-500" checked={draft.autoEnabled} onChange={(e) => set("autoEnabled", e.target.checked)} />
        Download new episodes of followed shows automatically
      </label>
      <Field label="Check every (minutes)" hint="At least 5.">
        <Input
          type="number"
          min={5}
          className={`${inputClass} w-32`}
          value={String(draft.autoIntervalMinutes)}
          onChange={(e) => set("autoIntervalMinutes", Math.max(5, Number(e.target.value) || 60))}
        />
      </Field>
      <p className="text-xs text-slate-300">
        Last run: {runStatus?.lastRunAt ? `${formatDate(runStatus.lastRunAt)}, ${runStatus.lastRunMessage ?? ""}` : "never"}. See Following for details.
      </p>
      <Button size="sm" className={primaryButton} onClick={() => void save()}>
        Save
      </Button>
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
  const { draft, set, save } = useDraft(LIBRARY_KEYS);
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
        <Input className={inputClass} value={draft.libraryRootDir} onChange={(e) => set("libraryRootDir", e.target.value)} />
      </Field>
      <div className="grid gap-3 sm:grid-cols-3">
        <Field label="Shows subfolder">
          <Input className={inputClass} value={draft.showsSubdir} onChange={(e) => set("showsSubdir", e.target.value)} />
        </Field>
        <Field label="Movies subfolder">
          <Input className={inputClass} value={draft.moviesSubdir} onChange={(e) => set("moviesSubdir", e.target.value)} />
        </Field>
        <Field label="Sports subfolder">
          <Input className={inputClass} value={draft.sportsSubdir} onChange={(e) => set("sportsSubdir", e.target.value)} />
        </Field>
      </div>
      <Field label="Quality">
        <select
          className="h-9 rounded-md border border-slate-600 bg-slate-800 px-2 text-sm text-slate-100"
          value={draft.quality}
          onChange={(e) => set("quality", e.target.value as Quality)}
        >
          <option value="HD1080">1080p</option>
          <option value="HD720">720p</option>
          <option value="Normal">Normal</option>
        </select>
      </Field>
      <Button size="sm" className={primaryButton} onClick={() => void save()}>
        Save
      </Button>

      <div className="border-t border-slate-800 pt-4">
        <p className="text-sm font-medium text-slate-200">Library scan</p>
        <p className="mt-1 text-xs text-slate-300">
          Finds RÚV files in the library by their tags, so nothing already there is downloaded again. It runs at startup and before each follow run.
        </p>
        {scan?.lastScanAt ? (
          <p className="mt-1 text-xs text-slate-300">
            Last scan {formatDate(scan.lastScanAt)}: {scan.files} files, {scan.tagged} from RÚV, {scan.errors} unreadable ({scan.elapsedMs} ms).
          </p>
        ) : null}
        <Button
          size="sm"
          className={`mt-2 ${secondaryButton}`}
          disabled={!!scan?.running}
          onClick={() => api.scan().then(setScan, fail)}
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
  const { draft, set, save } = useDraft(PLEX_KEYS);
  const { lastPlex, fail } = useStore();
  const [result, setResult] = useState(lastPlex);
  useEffect(() => setResult(lastPlex), [lastPlex]);
  if (!draft) return <p className="text-sm text-slate-300">Loading…</p>;
  return (
    <div className="space-y-4">
      <p className="text-xs text-slate-300">After library downloads, sarpur asks Plex to rescan once the queue is empty.</p>
      <Field label="Plex address" hint="e.g. http://truenas:32400">
        <Input className={inputClass} value={draft.plexBaseUrl} onChange={(e) => set("plexBaseUrl", e.target.value)} />
      </Field>
      <div className="grid gap-3 sm:grid-cols-2">
        <Field label="Library section id">
          <Input className={inputClass} value={draft.plexLibrarySectionId} onChange={(e) => set("plexLibrarySectionId", e.target.value)} />
        </Field>
        <Field label="Token">
          <Input type="password" autoComplete="off" className={inputClass} value={draft.plexToken} onChange={(e) => set("plexToken", e.target.value)} />
        </Field>
      </div>
      <Field label="Only this folder (optional)" hint="As Plex sees it.">
        <Input className={inputClass} value={draft.plexLibraryPath} onChange={(e) => set("plexLibraryPath", e.target.value)} />
      </Field>
      <div className="flex flex-wrap items-center gap-2">
        <Button size="sm" className={primaryButton} onClick={() => void save()}>
          Save
        </Button>
        <Button size="sm" className={secondaryButton} onClick={() => api.refreshPlex().then(setResult, fail)}>
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

function ImportSection() {
  const { fail, say } = useStore();
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
  const run = async <T,>(work: () => Promise<T>, done: (value: T) => void) => {
    setBusy(true);
    try {
      done(await work());
    } catch (error) {
      fail(error);
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
          className={inputClass}
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
        <Button size="sm" className={primaryButton} disabled={busy || !source.trim()} onClick={() => void run(() => api.importPreview(source), setPreview)}>
          Preview
        </Button>
        <Button size="sm" className={secondaryButton} disabled={busy} onClick={() => void run(() => api.importReport(), setReport)}>
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
                      <td className="py-1 pr-2">{m.key}</td>
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
            History: {preview.recorded.total} episodes ({preview.recorded.library} library, {preview.recorded.web} browser only, {preview.recorded.unknown} unknown;{" "}
            {preview.recorded.alreadyKnown} already known). Follows: {preview.followSids.length}.
            {preview.copies.length > 0 ? ` Copies ${preview.copies.join(" and ")}.` : ""}
          </p>
          {preview.warnings.map((w) => (
            <p key={w} className="text-xs text-amber-200">
              {w}
            </p>
          ))}
          {applied ? (
            <p role="status" className="text-emerald-200">
              Imported {applied.recordedAdded} episodes and {applied.followsAdded.length} follows.
              {applied.warnings.length > 0 ? ` ${applied.warnings.join(" ")}` : ""}
            </p>
          ) : (
            <Button
              size="sm"
              className={primaryButton}
              disabled={busy}
              onClick={() =>
                void run(
                  () => api.importApply(source),
                  (result) => {
                    setApplied(result);
                    say("Import done. Reload the page to see the imported settings and follows.");
                  },
                )
              }
            >
              Import
            </Button>
          )}
        </div>
      ) : null}

      {report ? (
        <div className="space-y-2 rounded-lg border border-slate-700 bg-slate-950/60 p-3 text-xs text-slate-200">
          {report.scanError ? <p className="text-rose-200">Library scan failed: {report.scanError}</p> : null}
          <details>
            <summary className="cursor-pointer text-sm">Downloaded before but not in the library: {report.recordedButMissing.length}</summary>
            <p className="mt-1 text-slate-300">Probably deleted on purpose. Follow runs skip these until you use “Allow re-download”.</p>
            <ul className="mt-1 max-h-48 overflow-y-auto">
              {report.recordedButMissing.map((m) => (
                <li key={m.pid}>
                  {m.title ?? m.pid} <span className="text-slate-400">({m.pid}, {m.origin})</span>
                </li>
              ))}
            </ul>
          </details>
          <details>
            <summary className="cursor-pointer text-sm">In the library but not in the history: {report.onDiskNotRecorded.length}</summary>
            <ul className="mt-1 max-h-48 overflow-y-auto">
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
