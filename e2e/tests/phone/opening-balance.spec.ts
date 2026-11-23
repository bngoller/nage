// The phone check with both rules (decision 74, SPEC 10.3) on the opening balance's pages in
// webkit-phone (decisions 141 and 149): the form under Assets and under Liabilities, empty,
// refused and holding what is saved; the Remove confirmation; the clash pages; and an account's
// page with its Opening balance fact and its action. Each test enters its own accounts on the
// server the chain shares; the three checks use an account of a 64-character name holding one
// unbroken run of 40 with an opening balance of 999,999,999,999.99, so that both are checked in
// the trail, in Back to, in the sentences at the top, on the confirmation's figure line, under
// Saved now and beside the Balance (the pause's P8). The policy watch stays as it is: should
// WebKit report the browser's own parts of the date box or of the tick box, nothing is set
// aside; the builder halts and quotes the report word for word (the pause's P11).
import { create, notice, send, trail } from '../../lib/accounts';
import {
    LARGEST,
    TICK,
    TYPED_ZERO,
    amountBox,
    dateBox,
    enter,
    facts,
    longName,
    tickBox,
    yellowAction,
} from '../../lib/opening-balance';
import { expectPhoneLayout } from '../../lib/phone';
import { expect, test } from '../../lib/test';

/** The box of `what`, or a box that fails every comparison of position when it is not shown. */
const NOWHERE = { x: Number.NaN, y: Number.NaN, width: Number.NaN, height: Number.NaN };

/**
 * The change form's Save opening balance, Remove opening balance and the way back stand one under
 * the other in that order, Remove under Save and the way back last; the confirmation's yellow
 * Remove opening balance stands under Keep it, nearest the thumb.
 * Proves: Decisions 130, 141 and 143 in WebKit: the phone's seen order of the change form's buttons
 * and of the confirmation's two buttons.
 */
test("on a phone the form's Remove stands under Save and the confirmation's main button is the lower one", async ({ page }) => {
    const path = await create(page, 'assets', 'Phone opening order');
    await enter(page, path, { date: '2026-09-30', amount: '10.00' });

    await page.goto(`${path}/opening-balance`);
    await expect(page.getByRole('heading', { level: 1, name: 'Change opening balance', exact: true })).toBeVisible();
    const save = (await page.getByRole('button', { name: 'Save opening balance', exact: true }).boundingBox()) ?? NOWHERE;
    const remove = (await page.getByRole('link', { name: 'Remove opening balance', exact: true }).boundingBox()) ?? NOWHERE;
    const back = (await page.getByRole('link', { name: 'Back to Phone opening order', exact: true }).boundingBox()) ?? NOWHERE;
    expect(remove.y, 'Remove under Save').toBeGreaterThanOrEqual(save.y + save.height);
    expect(back.y, 'the way back under Remove, last').toBeGreaterThanOrEqual(remove.y + remove.height);

    await page.getByRole('link', { name: 'Remove opening balance', exact: true }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'Remove the opening balance?', exact: true })).toBeVisible();
    const main = (await page.getByRole('button', { name: 'Remove opening balance', exact: true }).boundingBox()) ?? NOWHERE;
    const keep = (await page.getByRole('link', { name: 'Keep it', exact: true }).boundingBox()) ?? NOWHERE;
    expect(main.y, 'Remove opening balance under Keep it').toBeGreaterThanOrEqual(keep.y + keep.height);
});

/**
 * The form passes both rules for an account under Assets whose name has 64 characters, 40 of them
 * one unbroken run: empty, with its date box and its tick box; refused in both boxes, with both
 * sentences under them and the line at the top; and, once 999,999,999,999.99 is saved, holding it.
 * The form of an account under Liabilities with such a name passes too, with Amount owed, its help
 * line for what the statement says was owed and its own tick box.
 * Proves: Decision 149 with SPEC 10.3 and the pause's P8: the form under Assets and under
 * Liabilities, empty, refused in both boxes and holding what is saved, for an account of a
 * 64-character name holding an unbroken run of 40 with an opening balance of 999,999,999,999.99,
 * passes both rules.
 */
test('the opening-balance form passes the phone check', async ({ page }) => {
    const name = longName('Phone ob form assets ok');
    const path = await create(page, 'assets', name);
    await page.goto(`${path}/opening-balance`);
    await expect(page.getByRole('heading', { level: 1, name: 'Opening balance', exact: true })).toBeVisible();
    await expect(trail(page).locator('li')).toHaveText(['Accounts', name, 'Opening balance']);
    await expect(dateBox(page)).toBeVisible();
    await expect(page.getByLabel(TICK.assets, { exact: true })).toBeVisible();
    await expectPhoneLayout(page);

    await amountBox(page).fill('1.234,56');
    expect(await send(page, page.getByRole('button', { name: 'Save opening balance', exact: true }))).toBe(422);
    await expect(notice(page)).toHaveAttribute('role', 'alert');
    await expect(page.locator('.field small.problem')).toHaveCount(2);
    await expectPhoneLayout(page);

    await enter(page, path, { date: '2026-09-30', amount: LARGEST });
    await page.goto(`${path}/opening-balance`);
    await expect(page.getByRole('heading', { level: 1, name: 'Change opening balance', exact: true })).toBeVisible();
    await expect(amountBox(page)).toHaveValue(LARGEST);
    await expect(page.getByRole('link', { name: `Back to ${name}`, exact: true })).toBeVisible();
    await expectPhoneLayout(page);

    const card = longName('Phone ob form card owed');
    const owed = await create(page, 'liabilities', card);
    await page.goto(`${owed}/opening-balance`);
    await expect(page.getByLabel('Amount owed', { exact: true })).toBeVisible();
    await expect(page.locator('.field small.help')).toHaveText([
        /^Not after today, \d{4}-\d{2}-\d{2}\.$/,
        'What the statement says you owed at the end of that day.',
    ]);
    await expect(page.getByLabel(TICK.liabilities, { exact: true })).toBeVisible();
    await expectPhoneLayout(page);
});

/**
 * For an account whose name has 64 characters, 40 of them one unbroken run, and whose opening
 * balance is 999,999,999,999.99, the Remove confirmation passes both rules as Remove opens it and
 * after a typed 0, with its line You typed 0; and so does the clash page after another page saved a
 * new date, with the sentence at the top, Saved now and Your change.
 * Proves: Decision 149 with SPEC 10.3 and the pause's P8: the confirmation, after Remove and after a
 * typed zero, and a clash page, for an account of a 64-character name holding an unbroken run of 40
 * with an opening balance of 999,999,999,999.99, pass both rules.
 */
test('the Remove confirmation and the clash page pass the phone check', async ({ page }) => {
    const name = longName('Phone ob remove clashes');
    const path = await create(page, 'assets', name);
    await enter(page, path, { date: '2026-09-30', amount: LARGEST });

    await page.goto(`${path}/opening-balance/remove`);
    await expect(page.getByRole('heading', { level: 1, name: 'Remove the opening balance?', exact: true })).toBeVisible();
    await expect(page.locator('p.gone')).toHaveText(`Assets:${name}: ${LARGEST} on 2026-09-30.`);
    await expectPhoneLayout(page);

    await page.goto(`${path}/opening-balance`);
    await amountBox(page).fill('0');
    expect(await send(page, page.getByRole('button', { name: 'Save opening balance', exact: true }))).toBe(200);
    await expect(page.locator('p.typed')).toHaveText(TYPED_ZERO);
    await expectPhoneLayout(page);

    await page.goto(`${path}/opening-balance`);
    const other = await page.context().newPage();
    await other.goto(`${path}/opening-balance`);
    await dateBox(other).fill('2026-09-29');
    expect(await send(other, other.getByRole('button', { name: 'Save opening balance', exact: true }))).toBe(303);
    await other.close();
    await amountBox(page).fill('999,999,999,999.00');
    expect(await send(page, page.getByRole('button', { name: 'Save opening balance', exact: true }))).toBe(409);
    await expect(notice(page)).toHaveText(
        `Your change to the opening balance of Assets:${name} was not saved because it was changed while you were working, by another user or in another window: it is now ${LARGEST} on 2026-09-29.`,
    );
    await expect(page.getByRole('region', { name: 'Saved now', exact: true }).locator('dd')).toHaveText(['2026-09-29', LARGEST]);
    await expect(amountBox(page.getByRole('region', { name: 'Your change', exact: true }))).toHaveValue('999,999,999,999.00');
    await expect(page.getByRole('button', { name: 'Save mine', exact: true })).toBeVisible();
    await expect(page.getByRole('link', { name: 'Keep what is saved', exact: true })).toBeVisible();
    await expectPhoneLayout(page);
});

/**
 * An account's page passes both rules with the fact None yet and Enter opening balance as its
 * yellow button; and, for an account whose name has 64 characters, 40 of them one unbroken run,
 * with 999,999,999,999.99 beside the Balance and Change opening balance outlined.
 * Proves: Decision 149 with SPEC 10.3 and the pause's P8: an account's page with None yet and its
 * yellow button, and, for an account of a 64-character name holding an unbroken run of 40, with its
 * figure of 999,999,999,999.99 beside the Balance and Change opening balance, passes both rules.
 */
test("an account's page with its opening balance passes the phone check", async ({ page }) => {
    const path = await create(page, 'assets', 'Phone opening none');
    await page.goto(path);
    await expect(facts(page).values).toHaveText(['Joint', '0.00', 'None yet']);
    await expect(yellowAction(page)).toHaveText(['Enter opening balance']);
    await expectPhoneLayout(page);

    const name = longName('Phone ob account viewed');
    const funded = await create(page, 'assets', name);
    await enter(page, funded, { date: '2026-09-30', amount: LARGEST });
    await page.reload();
    await expect(facts(page).names).toHaveText(['Owner', 'Balance', 'Opening balance']);
    await expect(facts(page).values).toHaveText(['Joint', LARGEST, `${LARGEST} on 2026-09-30`]);
    await expect(page.getByRole('link', { name: 'Change opening balance', exact: true })).toHaveClass(/\boutline\b/);
    await expect(yellowAction(page)).toHaveCount(0);
    await expect(tickBox(page)).toHaveCount(0);
    await expectPhoneLayout(page);
});
