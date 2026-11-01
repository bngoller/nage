// Playwright configuration for the Nage e2e suite (decisions 31, 72, 73 and 75; PLAN M1 slice 7).
//
// Two projects, one server each (decision 73): chromium-desktop against 127.0.0.1:8791 and
// webkit-phone against 127.0.0.1:8792. serve.mjs starts each server on a fresh temporary data
// directory and Playwright stops it with SIGTERM, so nage closes its database cleanly.
import { defineConfig, devices } from '@playwright/test';

const CI = !!process.env.CI;

/** The port of each project's server (decision 73). */
const SERVERS = {
    'chromium-desktop': 8791,
    'webkit-phone': 8792,
} as const;

type Project = keyof typeof SERVERS;

function baseURL(project: Project): string {
    return `http://127.0.0.1:${SERVERS[project]}`;
}

/** One webServer entry per project: the launcher, readiness on /health, a clean SIGTERM stop. */
function server(project: Project) {
    return {
        name: project,
        command: `node serve.mjs ${SERVERS[project]}`,
        url: `${baseURL(project)}/health`,
        // A server already on the port is a stale one: fail at once instead of testing it.
        reuseExistingServer: false,
        timeout: 30_000,
        stdout: 'ignore' as const,
        stderr: 'pipe' as const,
        // SIGTERM to the process group, then up to 10 s for nage's clean stop (decision 53).
        gracefulShutdown: { signal: 'SIGTERM' as const, timeout: 10_000 },
    };
}

export default defineConfig({
    testDir: './tests',
    outputDir: './test-results',
    fullyParallel: true,
    // A forgotten `test.only` fails CI instead of silently narrowing the suite.
    forbidOnly: CI,
    // A flaky test fails loudly and is fixed; no retries anywhere (decision 73).
    retries: 0,
    // One worker in CI for stability (Playwright's own advice); Playwright's default locally.
    workers: CI ? 1 : undefined,
    // Playwright stops itself well before the e2e job's 30-minute timeout.
    globalTimeout: 10 * 60 * 1000,
    reporter: CI ? [['list'], ['github']] : [['list']],
    use: {
        trace: 'retain-on-failure',
        screenshot: 'only-on-failure',
        video: 'off',
        // bypassCSP is never set: every Nage page a test loads keeps its real security headers (SPEC 5.3).
    },
    projects: [
        {
            name: 'chromium-desktop',
            use: {
                ...devices['Desktop Chrome'],
                viewport: { width: 1280, height: 800 },
                baseURL: baseURL('chromium-desktop'),
            },
            // The phone checks (tests/phone/) run in the phone project only (SPEC 10.3). Playwright
            // matches the glob against the absolute path, hence the `tests/` anchor.
            testIgnore: '**/tests/phone/**',
        },
        {
            name: 'webkit-phone',
            // The iPhone 13 profile as Playwright defines it: a 390×844 screen, Safari's 390×664
            // viewport, device scale factor 3, mobile and touch emulation, WebKit (decision 73).
            use: {
                ...devices['iPhone 13'],
                baseURL: baseURL('webkit-phone'),
            },
        },
    ],
    webServer: [server('chromium-desktop'), server('webkit-phone')],
});
