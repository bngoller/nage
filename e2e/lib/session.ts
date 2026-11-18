// The made-up household of the e2e suite and the steps that set a server up and sign in (decision
// 121). Each server is set up once, by its set-up project, with these details; the tests that follow
// start from the signed-in state that project saved, or sign in themselves when they must not share
// it: a test that signs out or is refused uses a browser context without that state (`SIGNED_OUT`),
// a refused sign-in types a username no other test uses, and no test changes the password, since all
// the tests of a chain share one server.
import { mkdirSync } from 'node:fs';
import { dirname } from 'node:path';
import type { Page } from '@playwright/test';

/**
 * What the set-up form is filled with. The password is test-only: it opens nothing but a run's own
 * servers, which listen on the loopback address and are gone when the run ends.
 */
export const HOUSEHOLD = {
    bookName: 'Household',
    currency: 'CAD',
    username: 'sam',
    displayName: 'Sam',
    password: 'correct horse battery',
} as const;

/** The name of Nage's session cookie (decision 108). */
export const SESSION_COOKIE = 'nage_session';

/** The state of a browser nobody has signed in with: for `test.use({ storageState: SIGNED_OUT })`. */
export const SIGNED_OUT = { cookies: [], origins: [] };

/**
 * Fills the set-up form on the page in view with the household's details, leaves the starter
 * accounts ticked as the page offers them, and sends the form.
 */
export async function setUp(page: Page): Promise<void> {
    await page.getByLabel('Book name', { exact: true }).fill(HOUSEHOLD.bookName);
    await page.getByLabel('Currency', { exact: true }).selectOption(HOUSEHOLD.currency);
    await page.getByLabel('Username', { exact: true }).fill(HOUSEHOLD.username);
    await page.getByLabel('Display name', { exact: true }).fill(HOUSEHOLD.displayName);
    await page.getByLabel('Password', { exact: true }).fill(HOUSEHOLD.password);
    await page.getByLabel('Password again', { exact: true }).fill(HOUSEHOLD.password);
    await page.getByRole('button', { name: 'Set up Nage', exact: true }).click();
}

/** Types `username` and `password` into the sign-in form on the page in view and sends it. */
export async function signIn(page: Page, username: string, password: string): Promise<void> {
    await page.getByLabel('Username', { exact: true }).fill(username);
    await page.getByLabel('Password', { exact: true }).fill(password);
    await page.getByRole('button', { name: 'Sign in', exact: true }).click();
}

/** Saves the cookies of the page's browser context to `file`, creating its directory. */
export async function saveSignedInState(page: Page, file: string): Promise<void> {
    if (file === '') {
        throw new Error('no state file to save to: the project sets the option `stateFile` (playwright.config.ts)');
    }
    mkdirSync(dirname(file), { recursive: true });
    await page.context().storageState({ path: file });
}
