// The opening balance's pages in both chains (decisions 140 to 146): an account's opening balance
// entered from its page, refused with every problem at once, changed, met by another save, and
// removed after its confirmation, each on accounts of the test's own on the server the chain
// shares, so that no test depends on another's data or on a figure another test can change. In
// the computer chain the refused test also types a date after today, which the form's novalidate
// lets through to Nage's sentence under the box (the pause's P4). The suite is built last and
// proven by planted mistakes (decision 149).
import { accountPath, create, notice, send, trail } from '../lib/accounts';
import {
    COMMA_OUT_OF_PLACE,
    EMPTY_DATE,
    EXAMPLE,
    REFUSED,
    REMOVING,
    TICK,
    TYPED_ZERO,
    WARNING,
    actions,
    afterToday,
    amountBox,
    dateBox,
    dayAfter,
    enter,
    facts,
    fill,
    tickBox,
    today,
    yellowAction,
} from '../lib/opening-balance';
import { expect, test } from '../lib/test';

/** The box of `what`, or a box that fails every comparison of position when it is not shown. */
const NOWHERE = { x: Number.NaN, y: Number.NaN, width: Number.NaN, height: Number.NaN };

/**
 * A new account under Assets shows under Balance the fact Opening balance, None yet, and Enter
 * opening balance as its one yellow button, first among its actions. The button opens the form
 * under the trail Accounts, the account, Opening balance: the heading, the account's full place,
 * the warning and its example, Date with no default and its help naming today (the real day in
 * UTC, the zone the suite's server runs in), Balance with its
 * help and the tick box, the form carrying novalidate and the amount box asking for the number
 * keypad, Save opening balance and Back to the account, and no Remove. A date and an amount saved
 * there lead to the account's page, which says The opening balance was saved. once, shows the
 * figure and its date beside the Balance, and offers Change opening balance outlined, with no
 * yellow button left.
 * Proves: Decisions 140, 141 and 146 in Chromium and in WebKit: Enter opening balance from a new
 * account's page, the warning, the saved line once, the fact and Change opening balance.
 */
test("an opening balance is entered from the account's page and the page says so once", async ({ page }) => {
    const path = await create(page, 'assets', 'Opening entered');
    await expect(facts(page).names).toHaveText(['Owner', 'Balance', 'Opening balance']);
    await expect(facts(page).values).toHaveText(['Joint', '0.00', 'None yet']);
    await expect(actions(page)).toHaveText(['Enter opening balance', 'Change account', 'New account inside Opening entered', 'Archive']);
    await expect(yellowAction(page)).toHaveText(['Enter opening balance']);

    // The served binary runs the real clock in UTC (serve.mjs hands it --tz UTC): the day the
    // form names is the UTC day of this moment, either side of midnight.
    const before = new Date().toISOString().slice(0, 10);
    await page.getByRole('link', { name: 'Enter opening balance', exact: true }).click();
    expect(new URL(page.url()).pathname).toBe(`${path}/opening-balance`);
    await expect(page.getByRole('heading', { level: 1, name: 'Opening balance', exact: true })).toBeVisible();
    await expect(trail(page).locator('li')).toHaveText(['Accounts', 'Opening entered', 'Opening balance']);
    await expect(page.locator('p.place')).toHaveText('Assets:Opening entered');
    await expect(page.locator('.caution p')).toHaveText([WARNING, EXAMPLE]);
    await expect(page.locator('main form')).toHaveAttribute('novalidate', '');
    await expect(dateBox(page)).toHaveAttribute('type', 'date');
    await expect(dateBox(page)).toHaveValue('');
    const day = await today(page);
    const after = new Date().toISOString().slice(0, 10);
    expect([before, after], "today is the real day in UTC, the served binary's zone").toContain(day);
    await expect(dateBox(page)).toHaveAttribute('max', day);
    await expect(dateBox(page)).toHaveAttribute('min', '1900-01-01');
    await expect(amountBox(page)).toHaveAttribute('inputmode', 'decimal');
    await expect(page.getByLabel('Balance', { exact: true })).toHaveValue('');
    await expect(page.locator('.field small.help')).toHaveText([`Not after today, ${day}.`, 'What the account held at the end of that day.']);
    await expect(page.getByLabel(TICK.assets, { exact: true })).not.toBeChecked();
    await expect(page.getByRole('link', { name: 'Back to Opening entered', exact: true })).toHaveAttribute('href', path);
    await expect(page.getByRole('link', { name: 'Remove opening balance', exact: true })).toHaveCount(0);

    await fill(page, { date: '2026-09-30', amount: '2,480.15' });
    expect(await send(page, page.getByRole('button', { name: 'Save opening balance', exact: true }))).toBe(303);
    await expect(page.getByRole('heading', { level: 1, name: 'Opening entered', exact: true })).toBeVisible();
    expect(accountPath(page)).toBe(path);
    await expect(notice(page)).toHaveText('The opening balance was saved.');
    await expect(notice(page)).toHaveAttribute('role', 'status');
    await expect(facts(page).names).toHaveText(['Owner', 'Balance', 'Opening balance']);
    await expect(facts(page).values).toHaveText(['Joint', '2,480.15', '2,480.15 on 2026-09-30']);
    await expect(actions(page)).toHaveText(['Change opening balance', 'Change account', 'New account inside Opening entered', 'Archive']);
    await expect(page.getByRole('link', { name: 'Change opening balance', exact: true })).toHaveClass(/\boutline\b/);
    await expect(yellowAction(page), 'no yellow button once it has one').toHaveCount(0);
    await page.reload();
    await expect(page.getByRole('heading', { level: 1, name: 'Opening entered', exact: true })).toBeVisible();
    await expect(notice(page), 'the message shown once').toHaveCount(0);
    await expect(facts(page).values).toHaveText(['Joint', '2,480.15', '2,480.15 on 2026-09-30']);
});

/**
 * The form sent with an empty date and an amount whose comma is out of place answers 422: the line
 * at the top says the opening balance was not saved, each box carries its own sentence in Nage's
 * words and is marked, the amount typed and the tick as sent are kept, and the account's page still
 * says None yet. Once one is saved, a refused change form comes back as the change form, with
 * Remove and the entry it names, and the corrected form changes the opening balance. In the
 * computer chain a date after today, typed into the date box, is sent all the
 * same, since the form carries novalidate, and refused in Nage's words under the box, naming today.
 * Proves: Decision 145 and the pause's P4 in Chromium: an empty date and a comma out of place shown
 * at once under their boxes, what was typed kept, nothing saved; then a date after today typed into
 * the date box is sent, the form carrying novalidate, and refused in Nage's words under the box.
 * Decision 145 in WebKit: the same first half.
 */
test('a refused opening balance shows every problem at once and keeps what was typed', async ({ page }, testInfo) => {
    const path = await create(page, 'assets', 'Opening refused');
    await page.goto(`${path}/opening-balance`);
    await amountBox(page).fill('1.234,56');
    await tickBox(page).check();
    expect(await send(page, page.getByRole('button', { name: 'Save opening balance', exact: true }))).toBe(422);
    await expect(page.getByRole('heading', { level: 1, name: 'Opening balance', exact: true })).toBeVisible();
    await expect(notice(page)).toHaveText(REFUSED);
    await expect(notice(page)).toHaveAttribute('role', 'alert');
    await expect(page.locator('.field small.problem')).toHaveText([EMPTY_DATE, COMMA_OUT_OF_PLACE]);
    await expect(dateBox(page)).toHaveValue('');
    await expect(dateBox(page)).toHaveAttribute('aria-invalid', 'true');
    await expect(amountBox(page)).toHaveValue('1.234,56');
    await expect(amountBox(page)).toHaveAttribute('aria-invalid', 'true');
    await expect(tickBox(page), 'the tick kept as sent').toBeChecked();

    await page.goto(path);
    await expect(facts(page).values, 'nothing saved').toHaveText(['Joint', '0.00', 'None yet']);
    await expect(yellowAction(page)).toHaveText(['Enter opening balance']);

    // A refused change form comes back as the change form, naming the entry it changes, so that
    // the corrected form changes it.
    await enter(page, path, { date: '2026-09-30', amount: '20.00' });
    await page.goto(`${path}/opening-balance`);
    await amountBox(page).fill('1.234,56');
    expect(await send(page, page.getByRole('button', { name: 'Save opening balance', exact: true }))).toBe(422);
    await expect(page.getByRole('heading', { level: 1, name: 'Change opening balance', exact: true })).toBeVisible();
    await expect(notice(page)).toHaveText(REFUSED);
    await expect(page.locator('main form input[type="hidden"][name="entry"]')).toHaveCount(1);
    await expect(page.getByRole('link', { name: 'Remove opening balance', exact: true })).toBeVisible();
    await expect(amountBox(page)).toHaveValue('1.234,56');
    await amountBox(page).fill('25.00');
    expect(await send(page, page.getByRole('button', { name: 'Save opening balance', exact: true }))).toBe(303);
    await expect(notice(page)).toHaveText('The opening balance was saved.');
    await expect(facts(page).values, 'the corrected change saved').toHaveText(['Joint', '25.00', '25.00 on 2026-09-30']);

    if (testInfo.project.name !== 'chromium-desktop') {
        return;
    }
    await page.goto(`${path}/opening-balance`);
    await expect(page.locator('main form')).toHaveAttribute('novalidate', '');
    const day = await today(page);
    const tomorrow = dayAfter(day);
    await fill(page, { date: tomorrow, amount: '12.00' });
    await expect(dateBox(page)).toHaveValue(tomorrow);
    expect(await send(page, page.getByRole('button', { name: 'Save opening balance', exact: true })), 'the date after today sent and refused by Nage').toBe(422);
    await expect(notice(page)).toHaveText(REFUSED);
    await expect(page.locator('.field small.problem')).toHaveText([afterToday(day)]);
    await expect(dateBox(page)).toHaveValue(tomorrow);
    await expect(dateBox(page)).toHaveAttribute('aria-invalid', 'true');
    await expect(amountBox(page)).toHaveValue('12.00');
    await expect(amountBox(page)).not.toHaveAttribute('aria-invalid', 'true');
});

/**
 * An account under Assets whose balance was below zero: its amount typed as a plain number with
 * The account was overdrawn ticked is saved below zero, and the account's page writes the figure
 * and the Balance with a minus. The change form then shows the amount without its minus and the box
 * ticked.
 * Proves: Decision 141 in Chromium and in WebKit: the tick box makes the amount below zero, the
 * fact writes the minus, and the change form shows the amount without it, ticked.
 */
test("an overdrawn account's opening balance is entered with the tick box", async ({ page }) => {
    const path = await create(page, 'assets', 'Opening overdrawn');
    await page.getByRole('link', { name: 'Enter opening balance', exact: true }).click();
    await dateBox(page).fill('2026-09-30');
    await amountBox(page).fill('35.20');
    await page.getByLabel(TICK.assets, { exact: true }).check();
    expect(await send(page, page.getByRole('button', { name: 'Save opening balance', exact: true }))).toBe(303);
    await expect(notice(page)).toHaveText('The opening balance was saved.');
    await expect(facts(page).values).toHaveText(['Joint', '-35.20', '-35.20 on 2026-09-30']);

    await page.getByRole('link', { name: 'Change opening balance', exact: true }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'Change opening balance', exact: true })).toBeVisible();
    await expect(trail(page).locator('li')).toHaveText(['Accounts', 'Opening overdrawn', 'Opening balance']);
    await expect(dateBox(page)).toHaveValue('2026-09-30');
    await expect(amountBox(page)).toHaveValue('35.20');
    await expect(page.getByLabel(TICK.assets, { exact: true })).toBeChecked();
});

/**
 * The change form holds what is saved and saves a new date and amount in place, the page saying
 * The opening balance was saved. with the new figure; sent again as it stands it leads to the
 * account's page with no line, since nothing was written. An amount of 0 typed there writes nothing and
 * answers the Remove confirmation with You typed 0 above the figure still saved; its Keep it leads
 * back to the account's page, which still shows that figure and says nothing.
 * Proves: Decisions 141 and 143 in Chromium and in WebKit: the change form saves a new figure; a
 * typed 0 leads to the confirmation with You typed 0, and Keep it keeps the figure.
 */
test('an opening balance is changed, and a zero asks to remove it', async ({ page }) => {
    const path = await create(page, 'assets', 'Opening changed');
    await enter(page, path, { date: '2026-09-30', amount: '100.00' });

    await page.getByRole('link', { name: 'Change opening balance', exact: true }).click();
    expect(new URL(page.url()).pathname).toBe(`${path}/opening-balance`);
    await expect(page.getByRole('heading', { level: 1, name: 'Change opening balance', exact: true })).toBeVisible();
    await expect(dateBox(page)).toHaveValue('2026-09-30');
    await expect(amountBox(page)).toHaveValue('100.00');
    await expect(tickBox(page)).not.toBeChecked();
    await expect(page.getByRole('link', { name: 'Remove opening balance', exact: true })).toHaveAttribute('href', `${path}/opening-balance/remove`);
    await fill(page, { date: '2026-09-29', amount: '150.00' });
    expect(await send(page, page.getByRole('button', { name: 'Save opening balance', exact: true }))).toBe(303);
    await expect(notice(page)).toHaveText('The opening balance was saved.');
    expect(accountPath(page)).toBe(path);
    await expect(facts(page).values).toHaveText(['Joint', '150.00', '150.00 on 2026-09-29']);

    await page.getByRole('link', { name: 'Change opening balance', exact: true }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'Change opening balance', exact: true })).toBeVisible();
    expect(await send(page, page.getByRole('button', { name: 'Save opening balance', exact: true })), 'the form sent as it stands').toBe(303);
    await expect(page.getByRole('heading', { level: 1, name: 'Opening changed', exact: true })).toBeVisible();
    expect(accountPath(page)).toBe(path);
    await expect(notice(page), 'nothing written, nothing said').toHaveCount(0);
    await expect(facts(page).values).toHaveText(['Joint', '150.00', '150.00 on 2026-09-29']);

    await page.getByRole('link', { name: 'Change opening balance', exact: true }).click();
    await amountBox(page).fill('0');
    expect(await send(page, page.getByRole('button', { name: 'Save opening balance', exact: true })), 'the zero answers the confirmation').toBe(200);
    await expect(page.getByRole('heading', { level: 1, name: 'Remove the opening balance?', exact: true })).toBeVisible();
    await expect(page.locator('p.typed')).toHaveText(TYPED_ZERO);
    await expect(page.locator('p.gone')).toHaveText('Assets:Opening changed: 150.00 on 2026-09-29.');
    await expect(page.getByText(REMOVING, { exact: true })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Remove opening balance', exact: true })).toBeVisible();
    await page.getByRole('link', { name: 'Keep it', exact: true }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'Opening changed', exact: true })).toBeVisible();
    expect(accountPath(page)).toBe(path);
    await expect(notice(page), 'nothing done, nothing said').toHaveCount(0);
    await expect(facts(page).values, 'the figure kept').toHaveText(['Joint', '150.00', '150.00 on 2026-09-29']);
});

/**
 * Remove opening balance on the change form opens its confirmation, under the trail Accounts, the
 * account, Remove opening balance: the question, the figure in full, what removing does and that a
 * removed one cannot be brought back yet, with no You typed 0. When another page changes the
 * figure meanwhile, its Remove answers 409 with the confirmation again, the sentence at the top and
 * the figure as it is now. Its Remove opening balance there removes
 * it: the account's page says The opening balance was removed. once, its Balance is back at 0.00,
 * the fact says None yet and Enter opening balance is its yellow button again.
 * Proves: Decisions 143 and 146 in Chromium and in WebKit: Remove asks first, the removed line once,
 * the fact None yet and Enter opening balance back.
 */
test('an opening balance is removed after its confirmation', async ({ page }) => {
    const path = await create(page, 'assets', 'Opening removed');
    await enter(page, path, { date: '2026-09-30', amount: '75.50' });
    await expect(facts(page).values).toHaveText(['Joint', '75.50', '75.50 on 2026-09-30']);

    await page.getByRole('link', { name: 'Change opening balance', exact: true }).click();
    await page.getByRole('link', { name: 'Remove opening balance', exact: true }).click();
    expect(new URL(page.url()).pathname).toBe(`${path}/opening-balance/remove`);
    await expect(page.getByRole('heading', { level: 1, name: 'Remove the opening balance?', exact: true })).toBeVisible();
    await expect(trail(page).locator('li')).toHaveText(['Accounts', 'Opening removed', 'Remove opening balance']);
    await expect(page.locator('p.typed')).toHaveCount(0);
    await expect(page.locator('p.gone')).toHaveText('Assets:Opening removed: 75.50 on 2026-09-30.');
    await expect(page.getByText(REMOVING, { exact: true })).toBeVisible();
    await expect(page.getByRole('link', { name: 'Keep it', exact: true })).toHaveAttribute('href', path);

    // Another page changes the figure while the confirmation is open: Remove meets the change and
    // is asked again with the figure as it is now.
    const other = await page.context().newPage();
    await other.goto(`${path}/opening-balance`);
    await amountBox(other).fill('80.00');
    expect(await send(other, other.getByRole('button', { name: 'Save opening balance', exact: true }))).toBe(303);
    await other.close();
    expect(await send(page, page.getByRole('button', { name: 'Remove opening balance', exact: true })), 'a removal that met a change').toBe(409);
    await expect(page.getByRole('heading', { level: 1, name: 'Remove the opening balance?', exact: true })).toBeVisible();
    await expect(notice(page)).toHaveText(
        'The opening balance of Assets:Opening removed was not removed because it was changed while you were working, by another user or in another window: it is now 80.00 on 2026-09-30.',
    );
    await expect(notice(page)).toHaveAttribute('role', 'alert');
    await expect(page.locator('p.gone')).toHaveText('Assets:Opening removed: 80.00 on 2026-09-30.');
    await expect(page.locator('p.typed')).toHaveCount(0);

    expect(await send(page, page.getByRole('button', { name: 'Remove opening balance', exact: true }))).toBe(303);
    await expect(page.getByRole('heading', { level: 1, name: 'Opening removed', exact: true })).toBeVisible();
    expect(accountPath(page)).toBe(path);
    await expect(notice(page)).toHaveText('The opening balance was removed.');
    await expect(notice(page)).toHaveAttribute('role', 'status');
    await expect(facts(page).values).toHaveText(['Joint', '0.00', 'None yet']);
    await expect(yellowAction(page)).toHaveText(['Enter opening balance']);
    await expect(actions(page).first()).toHaveText('Enter opening balance');
    await page.reload();
    await expect(page.getByRole('heading', { level: 1, name: 'Opening removed', exact: true })).toBeVisible();
    await expect(notice(page), 'the message shown once').toHaveCount(0);
    await expect(facts(page).values).toHaveText(['Joint', '0.00', 'None yet']);
});

/**
 * Two pages show the change form of one opening balance. The first saves a new amount; the second,
 * still showing the old figure, saves another and is refused with 409: the sentence at the top names
 * the account by its full place and the figure saved now, the trail ends in Opening balance and the
 * page names the account's full place, Saved now shows that date and amount, and Your change keeps
 * both boxes as typed with no help line. Save mine then saves the second figure. A change sent after
 * another page removed it meets removed meanwhile: No opening balance, Keep it removed and Save as a
 * new opening balance, which saves it. Two pages entering a first one at once leave the later one
 * on entered meanwhile, headed Opening balance, whose Save mine changes the one entered.
 * Proves: Decision 144 in Chromium and in WebKit: two pages of one form, the later save refused with
 * what is saved, then Save mine saved.
 */
test('two saves of one opening balance clash and the later one is shown what is saved', async ({ page }) => {
    const path = await create(page, 'assets', 'Opening clash');
    await enter(page, path, { date: '2026-09-30', amount: '500.00' });
    await page.goto(`${path}/opening-balance`);
    await expect(page.getByRole('heading', { level: 1, name: 'Change opening balance', exact: true })).toBeVisible();

    const other = await page.context().newPage();
    await other.goto(`${path}/opening-balance`);
    await amountBox(other).fill('600.00');
    expect(await send(other, other.getByRole('button', { name: 'Save opening balance', exact: true }))).toBe(303);
    await expect(notice(other)).toHaveText('The opening balance was saved.');
    await other.close();

    await amountBox(page).fill('550.00');
    expect(await send(page, page.getByRole('button', { name: 'Save opening balance', exact: true }))).toBe(409);
    await expect(page.getByRole('heading', { level: 1, name: 'Change opening balance', exact: true })).toBeVisible();
    await expect(notice(page)).toHaveText(
        'Your change to the opening balance of Assets:Opening clash was not saved because it was changed while you were working, by another user or in another window: it is now 600.00 on 2026-09-30.',
    );
    await expect(notice(page)).toHaveAttribute('role', 'alert');
    await expect(trail(page).locator('li')).toHaveText(['Accounts', 'Opening clash', 'Opening balance']);
    await expect(page.locator('p.place')).toHaveText('Assets:Opening clash');
    const saved = page.getByRole('region', { name: 'Saved now', exact: true });
    await expect(saved.locator('dt')).toHaveText(['Date', 'Balance']);
    await expect(saved.locator('dd')).toHaveText(['2026-09-30', '600.00']);
    const yours = page.getByRole('region', { name: 'Your change', exact: true });
    await expect(dateBox(yours)).toHaveValue('2026-09-30');
    await expect(amountBox(yours)).toHaveValue('550.00');
    await expect(tickBox(yours)).not.toBeChecked();
    await expect(yours.locator('.field small.help'), 'no help line under Your change').toHaveCount(0);
    await expect(page.getByRole('link', { name: 'Keep what is saved', exact: true })).toHaveAttribute('href', path);

    expect(await send(page, page.getByRole('button', { name: 'Save mine', exact: true }))).toBe(303);
    await expect(notice(page)).toHaveText('The opening balance was saved.');
    expect(accountPath(page)).toBe(path);
    await expect(facts(page).values).toHaveText(['Joint', '550.00', '550.00 on 2026-09-30']);

    // Removed meanwhile: another page removes it while the change form is open.
    await page.goto(`${path}/opening-balance`);
    const remover = await page.context().newPage();
    await remover.goto(`${path}/opening-balance/remove`);
    expect(await send(remover, remover.getByRole('button', { name: 'Remove opening balance', exact: true }))).toBe(303);
    await remover.close();
    await amountBox(page).fill('560.00');
    expect(await send(page, page.getByRole('button', { name: 'Save opening balance', exact: true })), 'a change that met a removal').toBe(409);
    await expect(page.getByRole('heading', { level: 1, name: 'Change opening balance', exact: true })).toBeVisible();
    await expect(notice(page)).toHaveText(
        'Your change to the opening balance of Assets:Opening clash was not saved because it was removed while you were working, by another user or in another window.',
    );
    await expect(page.getByRole('region', { name: 'Saved now', exact: true }).locator('p')).toHaveText('No opening balance');
    await expect(page.getByRole('link', { name: 'Keep it removed', exact: true })).toHaveAttribute('href', path);
    await expect(page.getByRole('button', { name: 'Save mine', exact: true })).toHaveCount(0);
    expect(await send(page, page.getByRole('button', { name: 'Save as a new opening balance', exact: true }))).toBe(303);
    await expect(notice(page)).toHaveText('The opening balance was saved.');
    await expect(facts(page).values).toHaveText(['Joint', '560.00', '560.00 on 2026-09-30']);

    // Entered meanwhile: two pages of the form that enters one, both saving.
    const fresh = await create(page, 'assets', 'Opening entered clash');
    await page.goto(`${fresh}/opening-balance`);
    const first = await page.context().newPage();
    await first.goto(`${fresh}/opening-balance`);
    await fill(first, { date: '2026-09-30', amount: '70.00' });
    expect(await send(first, first.getByRole('button', { name: 'Save opening balance', exact: true }))).toBe(303);
    await first.close();
    await fill(page, { date: '2026-09-30', amount: '75.00' });
    expect(await send(page, page.getByRole('button', { name: 'Save opening balance', exact: true })), 'a second entry').toBe(409);
    await expect(page.getByRole('heading', { level: 1, name: 'Opening balance', exact: true })).toBeVisible();
    await expect(notice(page)).toHaveText(
        'Your opening balance for Assets:Opening entered clash was not saved because one was entered while you were working, by another user or in another window: it is 70.00 on 2026-09-30.',
    );
    await expect(trail(page).locator('li')).toHaveText(['Accounts', 'Opening entered clash', 'Opening balance']);
    await expect(page.getByRole('region', { name: 'Saved now', exact: true }).locator('dd')).toHaveText(['2026-09-30', '70.00']);
    expect(await send(page, page.getByRole('button', { name: 'Save mine', exact: true }))).toBe(303);
    await expect(notice(page)).toHaveText('The opening balance was saved.');
    expect(accountPath(page)).toBe(fresh);
    await expect(facts(page).values).toHaveText(['Joint', '75.00', '75.00 on 2026-09-30']);
});

/**
 * At 1280 px the change form's Save opening balance and Back to the account stand in a row with
 * Remove opening balance at its far right, apart from the way back, for an account whose short
 * name leaves the row room to spare; the confirmation's yellow Remove opening balance stands
 * first, left of Keep it on the same row; and the clash page's Saved now and Your change stand side
 * by side, Save mine left of Keep what is saved.
 * Proves: Decisions 130, 141 and 144 in Chromium: the computer's seen order of the change form's
 * buttons, the confirmation's main button first and the clash page's two panels.
 */
test("on a computer the form's Remove stands at the right of its row and the clash's panels side by side", async ({ page }) => {
    expect(page.viewportSize(), "the computer's screen").toEqual({ width: 1280, height: 800 });
    // A short name leaves the row room to spare, so that Remove stands apart only when its rule
    // takes effect, not because the three buttons happen to fill the row.
    const path = await create(page, 'assets', 'Desk order');
    await enter(page, path, { date: '2026-09-30', amount: '10.00' });

    await page.goto(`${path}/opening-balance`);
    const row = (await page.locator('main form .actions').boundingBox()) ?? NOWHERE;
    const save = (await page.getByRole('button', { name: 'Save opening balance', exact: true }).boundingBox()) ?? NOWHERE;
    const back = (await page.getByRole('link', { name: 'Back to Desk order', exact: true }).boundingBox()) ?? NOWHERE;
    const remove = (await page.getByRole('link', { name: 'Remove opening balance', exact: true }).boundingBox()) ?? NOWHERE;
    const middle = (box: typeof NOWHERE): number => box.y + box.height / 2;
    expect(save.x + save.width, 'Save left of the way back').toBeLessThanOrEqual(back.x);
    expect(back.x + back.width, 'Remove right of the way back').toBeLessThanOrEqual(remove.x);
    expect(Math.abs(middle(save) - middle(back)), 'Save and the way back on one row').toBeLessThanOrEqual(4);
    expect(Math.abs(middle(save) - middle(remove)), 'Save and Remove on one row').toBeLessThanOrEqual(4);
    // Beside the way back Remove would stand one gap of the row (1rem) from it.
    expect(remove.x - (back.x + back.width), 'Remove apart from the way back, not beside it').toBeGreaterThan(24);
    // Only the button's own pressed shadow (3 px) stands between it and the row's end.
    expect(row.x + row.width - (remove.x + remove.width), "Remove at the row's far right").toBeLessThanOrEqual(4);
    await page.goto(`${path}/opening-balance/remove`);
    const main = (await page.getByRole('button', { name: 'Remove opening balance', exact: true }).boundingBox()) ?? NOWHERE;
    const keep = (await page.getByRole('link', { name: 'Keep it', exact: true }).boundingBox()) ?? NOWHERE;
    expect(main.x + main.width, 'Remove opening balance left of Keep it').toBeLessThanOrEqual(keep.x);
    expect(Math.abs(main.y - keep.y), 'Remove opening balance and Keep it on one row').toBeLessThanOrEqual(2);

    await page.goto(`${path}/opening-balance`);
    const other = await page.context().newPage();
    await other.goto(`${path}/opening-balance`);
    await amountBox(other).fill('20.00');
    expect(await send(other, other.getByRole('button', { name: 'Save opening balance', exact: true }))).toBe(303);
    await other.close();
    await amountBox(page).fill('30.00');
    expect(await send(page, page.getByRole('button', { name: 'Save opening balance', exact: true }))).toBe(409);
    const saved = (await page.getByRole('region', { name: 'Saved now', exact: true }).boundingBox()) ?? NOWHERE;
    const yours = (await page.getByRole('region', { name: 'Your change', exact: true }).boundingBox()) ?? NOWHERE;
    expect(saved.x + saved.width, 'Saved now left of Your change').toBeLessThanOrEqual(yours.x);
    expect(Math.abs(saved.y - yours.y), 'the two panels side by side').toBeLessThanOrEqual(2);
    const mine = (await page.getByRole('button', { name: 'Save mine', exact: true }).boundingBox()) ?? NOWHERE;
    const kept = (await page.getByRole('link', { name: 'Keep what is saved', exact: true }).boundingBox()) ?? NOWHERE;
    expect(mine.x + mine.width, 'Save mine left of Keep what is saved').toBeLessThanOrEqual(kept.x);
});
