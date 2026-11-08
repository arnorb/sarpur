import { existsSync, readdirSync } from "node:fs";
import { homedir } from "node:os";
import path from "node:path";
import { defineConfig, devices } from "@playwright/test";

const port = process.env.E2E_PORT ?? "8099";

/** PW_CHROMIUM, else the newest complete Chromium in Playwright's macOS
 * cache (any version), else Playwright's own (`npx playwright install`). */
function chromium(): string | undefined {
  if (process.env.PW_CHROMIUM) return process.env.PW_CHROMIUM;
  const cache = path.join(homedir(), "Library", "Caches", "ms-playwright");
  if (!existsSync(cache)) return undefined;
  const builds = readdirSync(cache)
    .filter((d) => /^chromium-\d+$/.test(d) && existsSync(path.join(cache, d, "INSTALLATION_COMPLETE")))
    .sort((a, b) => Number(b.split("-")[1]) - Number(a.split("-")[1]));
  for (const build of builds) {
    for (const app of ["Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing", "Chromium.app/Contents/MacOS/Chromium"]) {
      for (const dir of ["chrome-mac-arm64", "chrome-mac"]) {
        const file = path.join(cache, build, dir, app);
        if (existsSync(file)) return file;
      }
    }
  }
  return undefined;
}

// Criterion 17. Runs the built UI (npm run build) against a sarpur in
// dry-run mode with the fake worker (e2e/server.mjs).
export default defineConfig({
  testDir: "e2e",
  fullyParallel: false,
  workers: 1,
  reporter: "list",
  use: {
    baseURL: `http://127.0.0.1:${port}`,
    ...devices["Desktop Chrome"],
    launchOptions: { executablePath: chromium() },
  },
  webServer: {
    command: "npm run build && node e2e/server.mjs",
    url: `http://127.0.0.1:${port}/api/health`,
    timeout: 300_000,
    reuseExistingServer: false,
    env: { E2E_PORT: port },
  },
});
