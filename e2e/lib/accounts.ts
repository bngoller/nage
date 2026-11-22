// The account pages as the e2e suite drives them (PLAN M2 slice 7, decisions 128 to 137): creating
// an account or a category through its form, finding a section of the list and a line in it, the
// trail, the one-time message, a form sent with the status of its answer, and archiving.
//
// The tests of a chain share one server and run in parallel, so every test makes accounts of its
// own, named so that no other test of the chain uses the name, and no test expects a fixed count
// beside Show archived. A test that counts the archived accounts holds the chain's archive lock
// from its first count to its last, and every other test archives or unarchives only while holding
// it (`holdingArchives`), so that the count it reads rises by exactly what it did itself.
import { mkdirSync, rmSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { expect, type Locator, type Page } from '@playwright/test';

/** The four sections a household adds to, by the word of their addresses (decision 128). */
export type Section = 'assets' | 'liabilities' | 'income' | 'expenses';

/** The section's heading on the list (decision 129). */
export const SECTION_NAMES = {
    assets: 'Assets',
    liabilities: 'Liabilities',
    income: 'Income',
    expenses: 'Expenses',
} as const;

/** `text` as a regular expression that matches it exactly. */
export function exactly(text: string): RegExp {
    return new RegExp(`^${text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}$`);
}

/** The panel of a section on the list, by its heading (Assets, Liabilities, Income, Expenses, Equity). */
export function sectionPanel(page: Page, name: string): Locator {
    return page.getByRole('region', { name, exact: true });
}

/** The line of `scope` (a section's panel, the list of what is inside an account) that names `name`. */
export function line(scope: Locator, name: string): Locator {
    return scope.locator('ul.rows > li > a').filter({ has: scope.page().locator('span.name', { hasText: exactly(name) }) });
}

/** The trail at the top of the page in view (decision 130). */
export function trail(page: Page): Locator {
    return page.getByRole('navigation', { name: 'Trail' });
}

/** The one-time message or the refusal at the top of the page in view (decision 137). */
export function notice(page: Page): Locator {
    return page.locator('main p.notice');
}

/** The address of the account page in view, `/accounts/<number>`. */
export function accountPath(page: Page): string {
    const path = new URL(page.url()).pathname;
    expect(path, 'the address of an account page').toMatch(/^\/accounts\/[1-9][0-9]*$/);
    return path;
}

/**
 * Clicks `button`, which sends a form with POST, and returns the status of the answer to that
 * POST (a 303 for a save that is followed to its page, 422 or 409 for a form sent back), once the
 * page it leads to has loaded.
 */
export async function send(page: Page, button: Locator): Promise<number> {
    const answered = page.waitForResponse(
        (response) => response.request().method() === 'POST' && response.request().resourceType() === 'document',
    );
    await button.click();
    const status = (await answered).status();
    await page.waitForLoadState();
    return status;
}

/**
 * Creates an account or a category named `name` under `section` through the create form, inside
 * the place written in full as `inside` when one is given, and returns the address of its page,
 * which the form leads to.
 */
export async function create(page: Page, section: Section, name: string, inside?: string): Promise<string> {
    await page.goto(`/accounts/new?under=${section}`);
    await page.getByLabel('Name', { exact: true }).fill(name);
    if (inside !== undefined) {
        await page.getByLabel('Inside', { exact: true }).selectOption({ label: inside });
    }
    const status = await send(page, page.getByRole('button', { name: /^Create (account|category)$/ }));
    expect(status, `the create form of ${name} answers with the way to its page`).toBe(303);
    await expect(page.getByRole('heading', { level: 1, name, exact: true })).toBeVisible();
    return accountPath(page);
}

/** The count beside the list's Show archived link, 0 when the link is absent. */
export async function archivedCount(page: Page): Promise<number> {
    await page.goto('/accounts');
    await expect(page.getByRole('heading', { level: 1, name: 'Accounts', exact: true })).toBeVisible();
    const link = page.getByRole('link', { name: /^Show archived \(\d+\)$/ });
    if ((await link.count()) === 0) {
        return 0;
    }
    const text = (await link.textContent()) ?? '';
    return Number.parseInt(/\((\d+)\)/.exec(text)?.[1] ?? 'NaN', 10);
}

/**
 * Archives the account or category at `path` through its confirmation and checks that its page
 * says so in the word of its section (decision 129); the caller holds the archive lock.
 */
export async function archive(page: Page, path: string, word: 'account' | 'category'): Promise<void> {
    await page.goto(`${path}/archive`);
    const status = await send(page, page.getByRole('button', { name: 'Archive', exact: true }));
    expect(status, 'the archive answers with the way to the account page').toBe(303);
    await expect(notice(page)).toHaveText(`This ${word} was archived.`);
}

/** How long a test waits for the archive lock, and when a lock is taken as left by a killed run. */
const LOCK_WAIT_MS = 25_000;
const LOCK_STALE_MS = 60_000;

/**
 * Runs `work` while holding the archive lock of the server at `baseURL`: a directory under the
 * temporary directory, made by one worker at a time, removed when the work ends.
 */
export async function holdingArchives<T>(baseURL: string | undefined, work: () => Promise<T>): Promise<T> {
    const port = new URL(baseURL ?? 'http://127.0.0.1/').port;
    const lock = join(tmpdir(), `nage-archive-lock-${port}`);
    const deadline = Date.now() + LOCK_WAIT_MS;
    for (;;) {
        try {
            mkdirSync(lock);
            break;
        } catch (error) {
            if ((error as NodeJS.ErrnoException).code !== 'EEXIST') {
                throw error;
            }
        }
        try {
            if (Date.now() - statSync(lock).mtimeMs > LOCK_STALE_MS) {
                rmSync(lock, { recursive: true, force: true });
                continue;
            }
        } catch {
            continue;
        }
        if (Date.now() > deadline) {
            throw new Error(`the archive lock ${lock} was held by another test for ${LOCK_WAIT_MS} ms`);
        }
        await new Promise((resolve) => setTimeout(resolve, 50));
    }
    try {
        return await work();
    } finally {
        rmSync(lock, { recursive: true, force: true });
    }
}
