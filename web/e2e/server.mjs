// Starts sarpur for the Playwright tests: a fresh data folder with a small
// schedule, RÚV answered from recordings written here (SARPUR_RUV_REPLAY),
// and --dry-run so downloads finish without ffmpeg. The UI is whatever `npm run build` last
// wrote to dist/ (debug builds read it from disk).
import { spawn } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const data = mkdtempSync(path.join(tmpdir(), "sarpur-e2e-"));
const port = process.env.E2E_PORT ?? "8099";

const poster = (name) => `https://myndir.ruv.is/${name}.jpg`;
const file = (pid) => `https://ruv-vod.akamaized.net/e2e/${pid}/${pid}.m3u8`;
const show = (pid, sid, seriesTitle, n, extra = {}) => ({
  pid,
  sid,
  title: `${seriesTitle} (${n} af 3)`,
  series_title: seriesTitle,
  episode_title: `Þáttur ${n}`,
  ep_num: String(n),
  ep_total: "3",
  season_num: "1",
  multiple_episodes: true,
  is_movie: false,
  is_docu: false,
  is_sport: false,
  "original-title": null,
  series_desc: null,
  episode: { description: `Þáttur ${n}.` },
  file: file(pid),
  showtime: `2026-03-0${n}T20:00:00`,
  duration: 1800,
  portrait_image: poster(sid),
  series_sdesc: `${seriesTitle} er íslensk þáttaröð.`,
  desc: `Þáttur ${n}.`,
  categories: ["Leikið efni"],
  subtitles: [{ name: "is", value: "https://example.invalid/is.vtt" }],
  slug: seriesTitle.toLowerCase(),
  ...extra,
});
const items = [
  show("a1", "100", "Ófærð", 1),
  show("a2", "100", "Ófærð", 2),
  show("a3", "100", "Ófærð", 3),
  show("b1", "200", "Fréttir", 1, { categories: ["Fréttir"] }),
  show("c1", "300", "Krakkafréttir", 1, { categories: ["Barnaefni"] }),
  show("m1", "400", "Bíómynd með mjög löngu nafni sem nær yfir margar línur", 1, {
    title: "Bíómynd með mjög löngu nafni sem nær yfir margar línur",
    ep_total: "1",
    multiple_episodes: false,
    is_movie: true,
    duration: 6000,
  }),
];
// Dated today, so a refresh only asks for series with new episodes (none).
const today = new Date().toISOString().slice(0, 10);
writeFileSync(
  path.join(data, "tvschedule.json"),
  JSON.stringify({ date: today, ...Object.fromEntries(items.map((i) => [i.pid, i])) }),
);

// RÚV's answers: an empty featured feed and each item's playlists.
const replay = path.join(data, "ruv-replay");
mkdirSync(replay);
const entries = [];
const record = (url, body) => {
  const name = `${entries.length}.txt`;
  writeFileSync(path.join(replay, name), body);
  entries.push({ method: "GET", url, file: name, status: 200 });
};
record("https://api.ruv.is/api/programs/featured/tv", JSON.stringify({ panels: [] }));
for (const item of items) {
  const variant = item.file.replace(/[^/]+$/, "1080p/stream.m3u8");
  record(item.file, "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1,RESOLUTION=1920x1080\n1080p/stream.m3u8\n");
  record(variant, "#EXTM3U\nseg.ts\n");
}
writeFileSync(path.join(replay, "index.json"), JSON.stringify({ entries }));

// rustup's cargo when it isn't on PATH.
const rustupCargo = path.join(homedir(), ".cargo", "bin", "cargo");
const cargo = process.env.CARGO ?? (existsSync(rustupCargo) ? rustupCargo : "cargo");
const child = spawn(
  cargo,
  ["run", "--quiet", "-p", "sarpur", "--", "--data-dir", data, "--port", port, "--dry-run"],
  {
    cwd: repo,
    stdio: "inherit",
    env: {
      ...process.env,
      SARPUR_RUV_REPLAY: replay,
      RUST_LOG: process.env.RUST_LOG ?? "warn",
      SARPUR_UPDATE_REPO: "",
    },
  },
);
const stop = () => child.kill("SIGTERM");
process.on("SIGTERM", stop);
process.on("SIGINT", stop);
child.on("exit", (code) => process.exit(code ?? 0));
