// Playwright configuration for the browser check of the Docker image (PLAN M2 slice 9; decisions 71
// and 121; the Deployment row of SPEC 10.3). `scripts/container-e2e.sh <image>` starts the image on
// an empty volume and runs this file inside e2e/:
//
//     npx playwright test --config container.config.ts
//
// It has no webServer: the server under test is the container, which the script starts and
// removes, and whose address it hands in through NAGE_CONTAINER_URL (default
// http://127.0.0.1:8798). One chain of two projects, modelled on the desktop chain of
// playwright.config.ts and reusing its files: first the checks that need a server nobody has set up
// (tests/smoke.spec.ts), then the set-up through `/setup` in the browser, which signs the first user
// in, proves the home page and saves the signed-in state to a file of its own (tests/server.setup.ts).
// The server set up here is the image's binary with the image's own files, so a file missing only
// from the image (a template, a served asset, a migration) fails a test. playwright.config.ts, which
// ordinary runs use, does not read this file.
import { join } from 'node:path';
import { defineConfig, devices } from '@playwright/test';
import type { SuiteOptions } from './lib/test';

const CI = !!process.env.CI;

/**
 * The container's published address. The set-up test expects the session cookie for the host
 * 127.0.0.1, which the script always publishes on.
 */
const BASE_URL = new URL(process.env.NAGE_CONTAINER_URL ?? 'http://127.0.0.1:8798').origin;

/** What the two projects share: the desktop chain's browser and window, the container's address. */
const container = {
    ...devices['Desktop Chrome'],
    viewport: { width: 1280, height: 800 },
    baseURL: BASE_URL,
    // A name of its own beside the two servers' files (git ignores the directory).
    stateFile: join(__dirname, 'playwright', '.auth', 'container-desktop.json'),
};

export default defineConfig<SuiteOptions>({
    testDir: './tests',
    outputDir: './test-results',
    // One container, one book: the chain runs one test at a time.
    workers: 1,
    // A forgotten `test.only` fails CI instead of silently narrowing the check.
    forbidOnly: CI,
    // A flaky test fails loudly and is fixed; no retries anywhere (decision 73).
    retries: 0,
    globalTimeout: 5 * 60 * 1000,
    reporter: CI ? [['list'], ['github']] : [['list']],
    use: {
        trace: 'retain-on-failure',
        screenshot: 'only-on-failure',
        video: 'off',
        // bypassCSP is never set: every Nage page a test loads keeps its real security headers (SPEC 5.3).
    },
    projects: [
        {
            name: 'fresh-container',
            testMatch: '**/tests/smoke.spec.ts',
            use: container,
        },
        {
            name: 'setup-container',
            testMatch: '**/tests/server.setup.ts',
            dependencies: ['fresh-container'],
            use: container,
        },
    ],
});
