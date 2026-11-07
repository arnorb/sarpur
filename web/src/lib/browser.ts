// What this browser remembers: its client id (which browser a finished
// download belongs to; routing, not security), the browser downloads it
// already saved, the main action, and a default download folder.
import type { DownloadMode } from "@/api/types";

const CLIENT_ID_KEY = "sarpur.clientId";
const HANDLED_JOBS_KEY = "sarpur.handledDownloadJobs";
const HANDLED_JOBS_LIMIT = 200;
const MAIN_ACTION_KEY = "sarpur.mainAction";
const POSTER_COLUMNS_KEY = "sarpur.posterColumns";

let memoryClientId: string | null = null;

function read(key: string): string | null {
  try {
    return window.localStorage.getItem(key);
  } catch {
    return null;
  }
}

function write(key: string, value: string) {
  try {
    window.localStorage.setItem(key, value);
  } catch {
    // Kept in memory only.
  }
}

export function getClientId(): string {
  if (memoryClientId) return memoryClientId;
  const stored = read(CLIENT_ID_KEY);
  if (stored) return (memoryClientId = stored);
  const generated =
    typeof crypto !== "undefined" && typeof crypto.randomUUID === "function"
      ? crypto.randomUUID()
      : `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
  memoryClientId = generated;
  write(CLIENT_ID_KEY, generated);
  return generated;
}

export function loadHandledJobs(): Set<string> {
  try {
    const parsed: unknown = JSON.parse(read(HANDLED_JOBS_KEY) ?? "[]");
    if (Array.isArray(parsed)) return new Set(parsed.filter((id): id is string => typeof id === "string"));
  } catch {
    // Unreadable: start empty.
  }
  return new Set();
}

export function persistHandledJobs(handled: Set<string>) {
  write(HANDLED_JOBS_KEY, JSON.stringify(Array.from(handled).slice(-HANDLED_JOBS_LIMIT)));
}

/** Which button is the main one on episodes: Download (this computer) or Library. */
export function loadMainAction(): DownloadMode {
  return read(MAIN_ACTION_KEY) === "library" ? "library" : "web";
}

export function saveMainAction(mode: DownloadMode) {
  write(MAIN_ACTION_KEY, mode);
}

export function loadPosterColumns(): number {
  const saved = Number(read(POSTER_COLUMNS_KEY));
  return saved >= 5 && saved <= 15 ? saved : 10;
}

export function savePosterColumns(columns: number) {
  write(POSTER_COLUMNS_KEY, String(columns));
}

// A default folder for browser downloads (Chrome's File System Access API),
// kept in IndexedDB because a folder handle can't go in localStorage.

export type WritableFile = { write(data: Blob): Promise<void>; close(): Promise<void> };
export type DirectoryHandle = {
  name: string;
  queryPermission?(options: { mode: "readwrite" }): Promise<PermissionState>;
  requestPermission?(options: { mode: "readwrite" }): Promise<PermissionState>;
  getFileHandle(name: string, options: { create: boolean }): Promise<{ createWritable(): Promise<WritableFile> }>;
};

declare global {
  interface Window {
    showDirectoryPicker?: (options?: { mode?: "read" | "readwrite" }) => Promise<DirectoryHandle>;
  }
}

export const folderPickerAvailable = () =>
  window.isSecureContext && typeof window.showDirectoryPicker === "function";

const FOLDER_DB = "sarpur-browser-settings";
const FOLDER_STORE = "handles";

function openFolderDb() {
  return new Promise<IDBDatabase>((resolve, reject) => {
    const request = indexedDB.open(FOLDER_DB, 1);
    request.onupgradeneeded = () => request.result.createObjectStore(FOLDER_STORE);
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

export async function loadDefaultFolder(): Promise<DirectoryHandle | null> {
  const db = await openFolderDb();
  try {
    return await new Promise((resolve, reject) => {
      const request = db.transaction(FOLDER_STORE).objectStore(FOLDER_STORE).get("default");
      request.onsuccess = () => resolve((request.result as DirectoryHandle | undefined) ?? null);
      request.onerror = () => reject(request.error);
    });
  } finally {
    db.close();
  }
}

export async function storeDefaultFolder(folder: DirectoryHandle | null) {
  const db = await openFolderDb();
  try {
    await new Promise<void>((resolve, reject) => {
      const store = db.transaction(FOLDER_STORE, "readwrite").objectStore(FOLDER_STORE);
      const request = folder ? store.put(folder, "default") : store.delete("default");
      request.onsuccess = () => resolve();
      request.onerror = () => reject(request.error);
    });
  } finally {
    db.close();
  }
}

/** Asks for write access; must run while a click still counts as a user action. */
export async function ensureWritable(folder: DirectoryHandle) {
  let permission = folder.queryPermission ? await folder.queryPermission({ mode: "readwrite" }) : "granted";
  if (permission !== "granted" && folder.requestPermission) {
    permission = await folder.requestPermission({ mode: "readwrite" });
  }
  if (permission !== "granted") throw new Error(`Permission to save in “${folder.name}” was not granted.`);
}

export async function pickFolder(): Promise<DirectoryHandle> {
  if (!window.showDirectoryPicker) throw new Error("This browser can't choose a folder.");
  const folder = await window.showDirectoryPicker({ mode: "readwrite" });
  await ensureWritable(folder);
  return folder;
}

export async function saveFilesToFolder(files: { name: string; url: string }[], folder: DirectoryHandle) {
  for (const file of files) {
    const response = await fetch(file.url, { credentials: "same-origin" });
    if (!response.ok) throw new Error(`Could not get ${file.name}`);
    const target = await folder.getFileHandle(file.name, { create: true });
    const writable = await target.createWritable();
    await writable.write(await response.blob());
    await writable.close();
  }
}

/** Starts the browser's own downloads, one after another. */
export function startBrowserDownloads(files: { name: string; url: string }[]) {
  files.forEach((file, index) => {
    window.setTimeout(() => {
      const anchor = document.createElement("a");
      anchor.href = file.url;
      anchor.download = file.name;
      document.body.appendChild(anchor);
      anchor.click();
      anchor.remove();
    }, index * 250);
  });
}
