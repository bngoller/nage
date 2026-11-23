// The opening balance's pages as the e2e suite drives them (PLAN M2 slice 7a, decisions 140 to
// 146): the form an account's page leads to, its two boxes and its tick box, the fact on the
// account's page, and the day the form names as today. Every test enters opening balances on
// accounts of its own (lib/accounts.ts), so that no figure another test can change is read.
import { expect, type Locator, type Page } from '@playwright/test';
import { send } from './accounts';

/** The two sentences of SPEC 6.6 the form shows as one block, word for word (decision 141). */
export const WARNING =
    'Enter the balance at the end of the day before the first transaction you will import. Imported rows dated on or before this date would be counted twice.';

/** The example line under them (decision 141). */
export const EXAMPLE =
    'For example: if the first statement you will import starts on 2026-10-01, enter the balance at the end of 2026-09-30.';

/** The line at the top of a refused form (decision 145). */
export const REFUSED =
    'The opening balance was not saved, because some of the boxes below need a change. Each one says what to change; then send the form again.';

/** The sentence under an empty date box (decision 145). */
export const EMPTY_DATE =
    'The opening balance was not saved because the date box is empty, and an opening balance needs the day it is for: Nage never guesses it. Pick the day, then save again.';

/** The sentence under an amount whose comma is out of place, in a book of two decimals (decision 145). */
export const COMMA_OUT_OF_PLACE =
    'The opening balance was not saved because the comma in the amount is out of place: Nage writes amounts as 1,234.56, with commas between thousands and a point before the cents. Type it that way, then save again.';

/** The sentence under a date after today, `today` being the day the form names (decision 145). */
export function afterToday(today: string): string {
    return `The opening balance was not saved because the date is after today, ${today}, and an opening balance cannot be for a day that has not come. Pick today or an earlier day, then save again.`;
}

/** The line of the confirmation that a typed zero leads to (decision 143). */
export const TYPED_ZERO = 'You typed 0: an account that starts at zero needs no opening balance.';

/** What the confirmation says a removal does (decision 143). */
export const REMOVING =
    'Removing it takes its amount out of every balance. You can then enter a new one. A removed opening balance cannot be brought back yet: note the amount and the date if you may need them.';

/** The tick box's words under Assets and under Liabilities (decision 141). */
export const TICK = {
    assets: 'The account was overdrawn (it held less than nothing)',
    liabilities: 'The account was in credit (the amount was owed to you)',
} as const;

/** The largest amount a form takes at two decimals (decision 142). */
export const LARGEST = '999,999,999,999.99';

/** An unbroken run of 40 letters (decision 139). */
export const RUN = 'Abcdefghij'.repeat(4);

/** A name of 64 characters, the run of 40 then `words`, which must be 23 characters long. */
export function longName(words: string): string {
    const name = `${RUN} ${words}`;
    expect(name, `the name ending in "${words}" has 64 characters`).toHaveLength(64);
    return name;
}

/** The date box of the form in view, or of `scope`. */
export function dateBox(scope: Page | Locator): Locator {
    return scope.getByLabel('Date', { exact: true });
}

/** The amount box of the form in view, or of `scope`: Balance under Assets, Amount owed under Liabilities. */
export function amountBox(scope: Page | Locator): Locator {
    return scope.getByLabel(/^(Balance|Amount owed)$/);
}

/** The tick box of the form in view, or of `scope`. */
export function tickBox(scope: Page | Locator): Locator {
    return scope.getByRole('checkbox');
}

/** The account page's facts, the names and the values, in the order the page shows them. */
export function facts(page: Page): { names: Locator; values: Locator } {
    return { names: page.locator('dl.facts dt'), values: page.locator('dl.facts dd') };
}

/** The account page's actions, in the order of its HTML. */
export function actions(page: Page): Locator {
    return page.locator('.doing .actions > a');
}

/** The account page's yellow action, the one link of its actions drawn as a button without an outline. */
export function yellowAction(page: Page): Locator {
    return page.locator('.doing .actions > a.button:not(.outline)');
}

/** The day the form in view names as today, read from the date box's help line. */
export async function today(page: Page): Promise<string> {
    const help = page.locator('.field small.help').filter({ hasText: /^Not after today, / });
    await expect(help).toHaveText(/^Not after today, \d{4}-\d{2}-\d{2}\.$/);
    const text = (await help.textContent()) ?? '';
    return /(\d{4}-\d{2}-\d{2})/.exec(text)?.[1] ?? '';
}

/** The day after `day`, both written as 2026-09-30. */
export function dayAfter(day: string): string {
    const next = new Date(`${day}T00:00:00Z`);
    next.setUTCDate(next.getUTCDate() + 1);
    return next.toISOString().slice(0, 10);
}

/** What a form is sent with: the two boxes and the tick. */
export interface Figure {
    date: string;
    amount: string;
    belowZero?: boolean;
}

/** Fills the boxes of the form in view, or of `scope`, with `figure`. */
export async function fill(scope: Page | Locator, figure: Figure): Promise<void> {
    await dateBox(scope).fill(figure.date);
    await amountBox(scope).fill(figure.amount);
    await tickBox(scope).setChecked(figure.belowZero ?? false);
}

/**
 * Enters `figure` as the opening balance of the account at `path` through its form and checks
 * that the account's page says so.
 */
export async function enter(page: Page, path: string, figure: Figure): Promise<void> {
    await page.goto(`${path}/opening-balance`);
    await fill(page, figure);
    const status = await send(page, page.getByRole('button', { name: 'Save opening balance', exact: true }));
    expect(status, 'the save answers with the way to the account page').toBe(303);
    // The notice is waited for first: in WebKit the address can still be the form's for a moment
    // after the answer to the POST, until the account's page has replaced it.
    await expect(page.locator('main p.notice')).toHaveText('The opening balance was saved.');
    expect(new URL(page.url()).pathname).toBe(path);
}
