// The phone check on the account pages (decisions 74, 130 and 139), in webkit-phone with the state
// the set-up project saved: every account page passes both rules, no sideways scrolling and every
// line, link and button at least 44 by 44 CSS px, also with a category whose name has 64 characters
// with one unbroken run of 40; and the phone's order: a section's New line is its panel's last line
// and a confirmation's main button the lower one. Every test makes accounts of its own, named
// "Phone ..." so that tests/accounts.spec.ts, which shares this server, uses none of them; an archive
// or an unarchive happens only while holding the archive lock (lib/accounts.ts).
import { archive, create, holdingArchives, line, notice, sectionPanel, send } from '../../lib/accounts';
import { expectPhoneLayout } from '../../lib/phone';
import { expect, test } from '../../lib/test';

/** An unbroken run of 40 letters. */
const RUN = 'Abcdefghij'.repeat(4);

/** A category name of 64 characters that holds the run of 40 (decision 139). */
const LONG_NAME = `${RUN} long category for phone`;

/** The box of `what`, or a box that fails every comparison of position when it is not shown. */
const NOWHERE = { x: Number.NaN, y: Number.NaN, width: Number.NaN, height: Number.NaN };

/**
 * On the list each of Assets, Liabilities, Income and Expenses ends with its + New line: it stands
 * under the section's lines and is the panel's last line. A confirmation's Archive, the yellow main
 * button, stands under Keep it, nearest the thumb.
 * Proves: Decision 130 with P10 and P12 in WebKit: the phone's seen order of the list's New lines
 * and of a confirmation's buttons.
 */
test("on a phone a section's New line is its panel's last line and a confirmation's main button the lower one", async ({ page }) => {
    const path = await create(page, 'expenses', 'Phone order');
    await page.goto('/accounts');
    for (const name of ['Assets', 'Liabilities', 'Income', 'Expenses']) {
        const panel = sectionPanel(page, name);
        const box = (await panel.boundingBox()) ?? NOWHERE;
        const add = (await panel.locator('a.add').boundingBox()) ?? NOWHERE;
        const rows = (await panel.locator(':scope > ul.rows, :scope > p.empty').boundingBox()) ?? NOWHERE;
        expect(add.y, `the New line of ${name} under its lines`).toBeGreaterThanOrEqual(rows.y + rows.height - 1);
        expect(box.y + box.height - (add.y + add.height), `the New line of ${name} the panel's last line`).toBeLessThanOrEqual(3);
        expect(add.width, `the New line of ${name} as wide as its panel`).toBeGreaterThanOrEqual(box.width - 4);
    }

    await page.goto(`${path}/archive`);
    await expect(page.getByRole('heading', { level: 1, name: 'Archive Phone order?' })).toBeVisible();
    const main = (await page.getByRole('button', { name: 'Archive', exact: true }).boundingBox()) ?? NOWHERE;
    const keep = (await page.getByRole('link', { name: 'Keep it', exact: true }).boundingBox()) ?? NOWHERE;
    expect(main.y, 'Archive under Keep it').toBeGreaterThanOrEqual(keep.y + keep.height);
});

/**
 * The list passes both rules with its archived accounts hidden, Show archived at its foot, and with
 * them shown in their places, Hide archived at its foot.
 * Proves: Decision 139 with P12 and SPEC 10.3: the list, with archived hidden and shown, has no
 * sideways scrolling and every line, link and button is at least 44 by 44 CSS px.
 */
test('the accounts list passes the phone check', async ({ page, baseURL }) => {
    const path = await create(page, 'assets', 'Phone shelf');
    await holdingArchives(baseURL, () => archive(page, path, 'account'));
    await page.goto('/accounts');
    await expect(page.getByRole('heading', { level: 1, name: 'Accounts', exact: true })).toBeVisible();
    await expect(page.getByRole('link', { name: /^Show archived \(\d+\)$/ })).toBeVisible();
    await expect(line(sectionPanel(page, 'Assets'), 'Phone shelf')).toHaveCount(0);
    await expectPhoneLayout(page);

    await page.goto('/accounts?archived=shown');
    await expect(line(sectionPanel(page, 'Assets'), 'Phone shelf').locator('.tag')).toHaveText(['archived']);
    await expect(page.getByRole('link', { name: 'Hide archived', exact: true })).toBeVisible();
    await expectPhoneLayout(page);
});

/**
 * An account's page passes both rules with the trail of an account inside another, its facts (Owner
 * Joint, Balance 0.00, Opening balance None yet), what is inside it and its four actions, Enter
 * opening balance first (decision 141); with the refusal of an archive at its top; a built-in
 * account's page with its sentence and Change name; and an archived account's page with Unarchive
 * and Change name.
 * Proves: Decision 139 with P12 and SPEC 10.3: an account's page with its trail and actions, with a
 * refusal at its top, a built-in account's and an archived account's with its two buttons pass both
 * rules.
 */
test("an account's page passes the phone check", async ({ page, baseURL }) => {
    const wallet = await create(page, 'assets', 'Phone wallet');
    const purse = await create(page, 'assets', 'Phone purse', 'Assets:Phone wallet');
    await page.goto(purse);
    await expect(page.getByRole('navigation', { name: 'Trail' }).locator('li')).toHaveText(['Accounts', 'Phone wallet', 'Phone purse']);
    await expect(page.locator('dl.facts dt')).toHaveText(['Owner', 'Balance', 'Opening balance']);
    await expect(page.locator('dl.facts dd')).toHaveText(['Joint', '0.00', 'None yet']);
    await expect(page.locator('.doing .actions > a')).toHaveText([
        'Enter opening balance',
        'Change account',
        'New account inside Phone purse',
        'Archive',
    ]);
    await expectPhoneLayout(page);

    await page.goto(wallet);
    await expect(page.getByRole('region', { name: 'Inside it', exact: true }).locator('span.name')).toHaveText(['Phone purse']);
    await expectPhoneLayout(page);
    await page.goto(`${wallet}/archive`);
    expect(await send(page, page.getByRole('button', { name: 'Archive', exact: true }))).toBe(422);
    await expect(notice(page)).toHaveAttribute('role', 'alert');
    await expect(notice(page)).toContainText('Assets:Phone wallet was not archived');
    await expectPhoneLayout(page);

    await page.goto('/accounts');
    await line(sectionPanel(page, 'Equity'), 'Valuation Changes').click();
    await expect(page.getByRole('heading', { level: 1, name: 'Valuation Changes', exact: true })).toBeVisible();
    await expect(page.locator('.doing .actions > a')).toHaveText(['Change name']);
    await expectPhoneLayout(page);

    const retired = await create(page, 'assets', 'Phone retired');
    await holdingArchives(baseURL, () => archive(page, retired, 'account'));
    await page.goto(retired);
    await expect(page.locator('.doing .actions > a')).toHaveText(['Unarchive', 'Change name']);
    await expectPhoneLayout(page);
});

/**
 * What do you want to add? passes both rules with its four choices; the create form of an account,
 * with Name, Inside and Owner, empty and refused with a problem under its box; the change form of an
 * account, as it opens and refused, the refused one saying at its top that the account was not
 * saved and marking its name box.
 * Proves: Decision 139 with P12 and SPEC 10.3: What do you want to add and the forms, empty and
 * refused, pass both rules.
 */
test('the choice page and the create and change forms pass the phone check', async ({ page }) => {
    await page.goto('/accounts/new');
    await expect(page.locator('ul.choose a')).toHaveCount(4);
    await expectPhoneLayout(page);

    await page.goto('/accounts/new?under=assets');
    await expect(page.getByRole('heading', { level: 1, name: 'New account under Assets' })).toBeVisible();
    await expect(page.getByLabel('Owner', { exact: true })).toBeVisible();
    await expectPhoneLayout(page);
    await page.getByLabel('Name', { exact: true }).fill('Phone: refused');
    expect(await send(page, page.getByRole('button', { name: 'Create account', exact: true }))).toBe(422);
    await expect(page.locator('.field small.problem')).toHaveCount(1);
    await expectPhoneLayout(page);

    const path = await create(page, 'liabilities', 'Phone card');
    await page.goto(`${path}/edit`);
    await expect(page.getByRole('heading', { level: 1, name: 'Change account' })).toBeVisible();
    await expect(page.locator('.field small.help')).toHaveCount(1);
    await expectPhoneLayout(page);
    await page.getByLabel('Name', { exact: true }).fill('Phone: card');
    expect(await send(page, page.getByRole('button', { name: 'Save changes', exact: true }))).toBe(422);
    await expect(page.locator('.field small.problem')).toHaveCount(1);
    await expect(notice(page)).toHaveText(
        'The account was not saved, because some of the boxes below need a change. Each one says what to change; then send the form again.',
    );
    await expect(notice(page)).toHaveAttribute('role', 'alert');
    await expect(page.getByLabel('Name', { exact: true })).toHaveAttribute('aria-invalid', 'true');
    await expectPhoneLayout(page);
});

/**
 * The Archive confirmation and the Unarchive confirmation each pass both rules, their main button
 * under the other.
 * Proves: Decision 139 with P12 and SPEC 10.3: the Archive and Unarchive confirmations pass both
 * rules, the main button last.
 */
test('the confirmations pass the phone check', async ({ page, baseURL }) => {
    const path = await create(page, 'expenses', 'Phone tidy');
    for (const [action, keep] of [
        ['Archive', 'Keep it'],
        ['Unarchive', 'Keep it archived'],
    ] as const) {
        if (action === 'Unarchive') {
            await holdingArchives(baseURL, () => archive(page, path, 'category'));
        }
        await page.goto(`${path}/${action.toLowerCase()}`);
        await expect(page.getByRole('heading', { level: 1, name: `${action} Phone tidy?` })).toBeVisible();
        const main = (await page.getByRole('button', { name: action, exact: true }).boundingBox()) ?? NOWHERE;
        const other = (await page.getByRole('link', { name: keep, exact: true }).boundingBox()) ?? NOWHERE;
        expect(main.y, `${action} under ${keep}`).toBeGreaterThanOrEqual(other.y + other.height);
        await expectPhoneLayout(page);
    }
});

/**
 * The clash page, after another page saved a new name of the same account, passes both rules with
 * its message, Saved now, Your change, Save mine and Keep what is saved.
 * Proves: Decision 139 with P12 and SPEC 10.3: the clash page, its two panels and two buttons,
 * passes both rules.
 */
test('the clash page passes the phone check', async ({ page }) => {
    const path = await create(page, 'assets', 'Phone clash');
    await page.goto(`${path}/edit`);
    const other = await page.context().newPage();
    await other.goto(`${path}/edit`);
    await other.getByLabel('Name', { exact: true }).fill('Phone clash first');
    expect(await send(other, other.getByRole('button', { name: 'Save changes', exact: true }))).toBe(303);
    await other.close();

    await page.getByLabel('Name', { exact: true }).fill('Phone clash second');
    expect(await send(page, page.getByRole('button', { name: 'Save changes', exact: true }))).toBe(409);
    await expect(page.getByRole('region', { name: 'Saved now', exact: true })).toBeVisible();
    await expect(page.getByRole('region', { name: 'Your change', exact: true })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Save mine', exact: true })).toBeVisible();
    await expect(page.getByRole('link', { name: 'Keep what is saved', exact: true })).toBeVisible();
    await expectPhoneLayout(page);
});

/**
 * A category whose name has 64 characters, 40 of them one unbroken run, passes both rules on its
 * line of the list, where the name wraps inside its line instead of running past it into the
 * panel's clipped edge, on its page, on its change form and on its Archive confirmation, and as a
 * choice of another category's Inside list.
 * Proves: Decision 139 with P12 and SPEC 10.3: a 64-character category name with an unbroken run of
 * 40 on its line, its page, its change form, its Archive confirmation and another category's Inside
 * list.
 */
test('a category whose name has 64 characters passes the phone check wherever it stands', async ({ page }) => {
    expect(LONG_NAME).toHaveLength(64);
    const path = await create(page, 'expenses', LONG_NAME);
    await expect(page.locator('p.place')).toHaveText(`Expenses:${LONG_NAME}`);
    await expectPhoneLayout(page);

    await page.goto('/accounts');
    const row = line(sectionPanel(page, 'Expenses'), LONG_NAME);
    await expect(row).toBeVisible();
    const rowBox = (await row.boundingBox()) ?? NOWHERE;
    const nameBox = (await row.locator('span.name').boundingBox()) ?? NOWHERE;
    expect(nameBox.x + nameBox.width, 'the name wrapped inside its line, not cut off by the panel').toBeLessThanOrEqual(rowBox.x + rowBox.width);
    await expectPhoneLayout(page);

    await page.goto(`${path}/edit`);
    await expect(page.getByLabel('Name', { exact: true })).toHaveValue(LONG_NAME);
    await expectPhoneLayout(page);

    await page.goto(`${path}/archive`);
    await expect(page.getByRole('heading', { level: 1, name: `Archive ${LONG_NAME}?` })).toBeVisible();
    await expectPhoneLayout(page);

    const short = await create(page, 'expenses', 'Phone short');
    await page.goto(`${short}/edit`);
    await expect(page.getByLabel('Inside', { exact: true }).locator('option', { hasText: `Expenses:${LONG_NAME}` })).toHaveCount(1);
    await page.getByLabel('Inside', { exact: true }).selectOption({ label: `Expenses:${LONG_NAME}` });
    await expectPhoneLayout(page);
});
