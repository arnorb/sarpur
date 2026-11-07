import { useEffect, useState } from "react";
import { FolderOpen, Languages } from "lucide-react";
import type { DownloadMode, Show } from "@/api/types";
import { Button } from "@/components/ui/button";
import { ModalDialog, primaryButton, secondaryButton } from "@/components/common";
import { type DirectoryHandle, ensureWritable, folderPickerAvailable, loadDefaultFolder, pickFolder } from "@/lib/browser";
import { libraryDestination } from "@/lib/shows";
import { useStore } from "@/store";

export type DownloadChoice = { shows: Show[]; mode: DownloadMode };

const LANGUAGES = [
  { code: "is", label: "Icelandic", hint: "selected by default where available" },
  { code: "en", label: "English", hint: "select it to get it where available" },
];

export function DownloadDialog({ choice, onClose }: { choice: DownloadChoice; onClose: () => void }) {
  const { download, settings, fail } = useStore();
  const { shows, mode } = choice;
  const has = (code: string) => shows.some((s) => s.subtitleLanguages.includes(code));
  const [languages, setLanguages] = useState<string[]>(() => (has("is") ? ["is"] : []));
  const [defaultFolder, setDefaultFolder] = useState<DirectoryHandle | null>(null);
  const picker = folderPickerAvailable();

  useEffect(() => {
    if (picker) loadDefaultFolder().then(setDefaultFolder, () => undefined);
  }, [picker]);

  const start = async (folder?: DirectoryHandle) => {
    onClose();
    await download(shows, mode, languages, folder);
  };
  const withFolder = async (get: () => Promise<DirectoryHandle>) => {
    try {
      // Ask for access while the click still counts as a user action; when
      // the download finishes it's too late to prompt.
      await start(await get());
    } catch (error) {
      if (error instanceof DOMException && error.name === "AbortError") return;
      fail(error);
    }
  };

  return (
    <ModalDialog onClose={onClose} labelledBy="download-title" className="w-[calc(100%-2rem)] max-w-lg rounded-xl border border-slate-700 bg-slate-900 shadow-2xl">
      <div className="border-b border-slate-800 p-4">
        <h2 id="download-title" className="text-lg font-semibold text-slate-100">
          {mode === "web" ? "Download to this computer" : "Download to the library"}
        </h2>
        <p className="mt-1 line-clamp-2 text-sm text-slate-300">
          {shows.length === 1 ? shows[0].displayTitle : `${shows.length} episodes selected`}
        </p>
      </div>
      <div className="space-y-4 p-4">
        <fieldset>
          <legend className="mb-2 flex items-center gap-2 text-sm font-medium text-slate-200">
            <Languages className="h-4 w-4 text-sky-400" aria-hidden />
            Subtitle files
          </legend>
          <div className="space-y-2 rounded-lg border border-slate-800 bg-slate-950/50 p-3">
            {LANGUAGES.map(({ code, label, hint }) => (
              <label key={code} className={`flex items-center gap-2 text-sm ${has(code) ? "text-slate-100" : "text-slate-400"}`}>
                <input
                  type="checkbox"
                  className="h-4 w-4 accent-sky-500"
                  checked={languages.includes(code)}
                  disabled={!has(code)}
                  onChange={(e) =>
                    setLanguages((current) => (e.target.checked ? [...current.filter((l) => l !== code), code] : current.filter((l) => l !== code)))
                  }
                />
                {label} ({has(code) ? hint : "not available"})
              </label>
            ))}
          </div>
          {shows.some((s) => s.englishSubtitled) ? (
            <p className="mt-2 rounded-md border border-violet-500/40 bg-violet-500/10 px-3 py-2 text-xs text-violet-100">
              This is RÚV’s English-subtitled video version; its English subtitles are part of the picture.
            </p>
          ) : null}
        </fieldset>

        {mode === "web" ? (
          <div className="rounded-lg border border-slate-800 bg-slate-950/50 p-3">
            <p className="mb-1 flex items-center gap-2 text-sm font-medium text-slate-200">
              <FolderOpen className="h-4 w-4 text-sky-400" aria-hidden />
              Save on this computer
            </p>
            <p className="text-xs leading-relaxed text-slate-300">
              {picker
                ? defaultFolder
                  ? `Your default is “${defaultFolder.name}”. You can also use the browser’s download folder or choose another folder just this once.`
                  : "Use the browser’s download folder, or choose a folder for this download. You can set a default under Settings."
                : "This browser can’t save into a chosen folder. The file goes to the browser’s download folder."}
            </p>
            <p className="mt-2 text-xs text-slate-300">Keep this tab open; the file is saved when the download finishes.</p>
          </div>
        ) : (
          <div className="rounded-lg border border-sky-700/50 bg-sky-950/30 p-3">
            <p className="mb-1 flex items-center gap-2 text-sm font-medium text-slate-200">
              <FolderOpen className="h-4 w-4 text-sky-400" aria-hidden />
              Library folder
            </p>
            <code className="block break-all rounded bg-slate-950 px-2 py-1.5 text-xs text-sky-100">
              {libraryDestination(settings, shows)}
            </code>
            <p className="mt-2 text-xs text-slate-300">Change it under Settings. An episode already in the library isn’t downloaded again.</p>
          </div>
        )}

        <div className="flex flex-wrap justify-end gap-2">
          <Button className={secondaryButton} onClick={onClose}>
            Cancel
          </Button>
          {mode === "library" ? (
            <Button className={primaryButton} onClick={() => void start()}>
              Download to library
            </Button>
          ) : (
            <>
              {defaultFolder ? (
                <Button
                  className={primaryButton}
                  onClick={() =>
                    void withFolder(async () => {
                      await ensureWritable(defaultFolder);
                      return defaultFolder;
                    })
                  }
                >
                  <FolderOpen className="mr-2 h-4 w-4" aria-hidden />
                  Use {defaultFolder.name}
                </Button>
              ) : null}
              <Button className={defaultFolder || picker ? secondaryButton : primaryButton} onClick={() => void start()}>
                Use browser folder
              </Button>
              {picker ? (
                <Button className={defaultFolder ? secondaryButton : primaryButton} onClick={() => void withFolder(pickFolder)}>
                  <FolderOpen className="mr-2 h-4 w-4" aria-hidden />
                  Choose a folder
                </Button>
              ) : null}
            </>
          )}
        </div>
      </div>
    </ModalDialog>
  );
}
