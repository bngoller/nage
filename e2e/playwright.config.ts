// Playwright configuration for the Nage e2e suite (decisions 31, 72, 73, 75 and 121; PLAN M1 slice 7
// and M2 slice 6).
//
// Two servers (decision 73): chromium-desktop on 127.0.0.1:8791 and webkit-phone on 127.0.0.1:8792.
// serve.mjs starts each one on a fresh temporary data directory and Playwright stops it with SIGTERM,
// so nage closes its database cleanly.
//
// Each server has a chain of three projects, which Playwright runs one after the other (decision
// 121): first the checks that need a server nobody has set up (fresh-desktop, fresh-phone), then the
// set-up of that server through `/setup` in the browser, which saves the signed-in state to that
// server's file (setup-desktop, setup-phone), then every other test, which starts from that state
// unless it asks for none (chromium-desktop, webkit-phone). `--project chromium-desktop` or
// `--project webkit-phone` runs that server's whole chain; a chain stops at its first project that
// has a failed test.
import { join } from 'node:path';
import { defineConfig, devices } from '@playwright/test';
import type { SuiteOptions } from './lib/test';

const CI = !!process.env.CI;

/** The port of each server (decision 73), under the name of the project that ends its chain. */
const SERVERS = {
    'chromium-desktop': 8791,
    'webkit-phone': 8792,
} as const;

type Server = keyof typeof SERVERS;

function baseURL(server: Server): string {
    return `http://127.0.0.1:${SERVERS[server]}`;
}

/**
 * The file that holds a server's signed-in state (decision 121): one per server, since browsers do
 * not keep cookies apart by port and each server has its own database. Git ignores the directory,
 * and it lies outside `test-results/`, which a failed CI job uploads.
 */
function stateFile(server: Server): string {
    return join(__dirname, 'playwright', '.auth', `${server}.json`);
}

/** One webServer entry per server: the launcher, readiness on /health, a clean SIGTERM stop. */
function server(name: Server) {
    return {
        name,
        command: `node serve.mjs ${SERVERS[name]}`,
        url: `${baseURL(name)}/health`,
        // A server already on the port is a stale one: fail at once instead of testing it.
        reuseExistingServer: false,
        timeout: 30_000,
        stdout: 'ignore' as const,
        stderr: 'pipe' as const,
        // SIGTERM to the process group, then up to 10 s for nage's clean stop (decision 53).
        gracefulShutdown: { signal: 'SIGTERM' as const, timeout: 10_000 },
    };
}

/** What the three projects of the desktop server's chain share. */
const desktop = {
    ...devices['Desktop Chrome'],
    viewport: { width: 1280, height: 800 },
    baseURL: baseURL('chromium-desktop'),
    stateFile: stateFile('chromium-desktop'),
};

/**
 * What the three projects of the phone server's chain share: the iPhone 13 profile as Playwright
 * defines it, a 390×844 screen, Safari's 390×664 viewport, device scale factor 3, mobile and touch
 * emulation, WebKit (decision 73).
 */
const phone = {
    ...devices['iPhone 13'],
    baseURL: baseURL('webkit-phone'),
    stateFile: stateFile('webkit-phone'),
};

// Playwright matches a glob against the absolute path, hence the `tests/` anchors.
/** The checks that need a server nobody has set up; they run first in their chain. */
const FRESH = ['**/tests/smoke.spec.ts', '**/tests/phone/setup.spec.ts'];
/** The set-up of a server through the browser. */
const SETUP = '**/tests/*.setup.ts';
/** The phone checks (tests/phone/), which run in the phone server's chain only (SPEC 10.3). */
const PHONE_CHECKS = '**/tests/phone/**';
/**
 * A test that measures the computer's layout alone, such as the rail of decision 123, has a title
 * that begins "on a computer": the phone server's chain leaves it out, so it is neither run nor
 * reported as skipped there. (A tag would do the same, but the list reporter prints a test's tags
 * after its title.)
 */
const COMPUTER_ONLY = /(^|\s)on a computer /;

export default defineConfig<SuiteOptions>({
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
            name: 'fresh-desktop',
            testMatch: FRESH,
            testIgnore: PHONE_CHECKS,
            use: desktop,
        },
        {
            name: 'setup-desktop',
            testMatch: SETUP,
            dependencies: ['fresh-desktop'],
            use: desktop,
        },
        {
            name: 'chromium-desktop',
            dependencies: ['setup-desktop'],
            testIgnore: [...FRESH, PHONE_CHECKS],
            use: { ...desktop, storageState: desktop.stateFile },
        },
        {
            name: 'fresh-phone',
            testMatch: FRESH,
            use: phone,
        },
        {
            name: 'setup-phone',
            testMatch: SETUP,
            dependencies: ['fresh-phone'],
            use: phone,
        },
        {
            name: 'webkit-phone',
            dependencies: ['setup-phone'],
            testIgnore: FRESH,
            grepInvert: COMPUTER_ONLY,
            use: { ...phone, storageState: phone.stateFile },
        },
    ],
    webServer: [server('chromium-desktop'), server('webkit-phone')],
});
