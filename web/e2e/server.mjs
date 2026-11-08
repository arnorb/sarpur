// Starts sarpur for the Playwright tests: a fresh data folder, the fake
// worker from the Rust tests with a small catalogue, and --dry-run so
// downloads finish without ffmpeg. The UI is whatever `npm run build` last
// wrote to dist/ (debug builds read it from disk).
import { spawn } from "node:child_process";
import { existsSync, mkdtempSync, writeFileSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const data = mkdtempSync(path.join(tmpdir(), "sarpur-e2e-"));
const port = process.env.E2E_PORT ?? "8099";

const poster = (name) => `https://myndir.ruv.is/${name}.jpg`;
const show = (pid, sid, seriesTitle, n, extra = {}) => ({
  pid,
  sid,
  title: `${seriesTitle} (${n} af 3)`,
  display_title: `${seriesTitle} s01e0${n}`,
  series_title: seriesTitle,
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
  show("m1", "400", "Bíómynd með mjög löngu nafni sem nær yfir margar línur", 1, { is_movie: true, duration: 6000 }),
];
writeFileSync(path.join(data, "fake_catalogue.json"), JSON.stringify({ items }));

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
      SARPUR_WORKER: path.join(repo, "crates/sarpur-lib/testdata/fake_follow_worker.py"),
      RUST_LOG: process.env.RUST_LOG ?? "warn",
      SARPUR_UPDATE_REPO: "",
    },
  },
);
const stop = () => child.kill("SIGTERM");
process.on("SIGTERM", stop);
process.on("SIGINT", stop);
child.on("exit", (code) => process.exit(code ?? 0));
