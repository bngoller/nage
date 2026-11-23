// The phone check on the home page, a named page of SPEC 10.3 (decisions 74, 116, 119, 121, 124 and
// 125), in webkit-phone with the state the set-up project saved. Rule 2 measures real elements here:
// the mark and the name, one link to the home page, and the Menu button, and with the menu open its
// three links, the three choices of its Appearance group and its Sign out button. The page shows the
// starter accounts among the accounts the chain's other tests add; its check with a 64-character
// account name at the largest amount a form takes stands below, the account funded through the
// opening-balance form of PLAN M2 slice 7a (decision 149).
import { create, line, sectionPanel } from '../../lib/accounts';
import { openMenu } from '../../lib/menu';
import { LARGEST, enter, longName } from '../../lib/opening-balance';
import { MIN_TAP_TARGET_PX, expectPhoneLayout } from '../../lib/phone';
import { expect, test } from '../../lib/test';

/** The box of `what`, or a box that fails every comparison of position when it is not shown. */
const NOWHERE = { x: Number.NaN, y: Number.NaN, width: Number.NaN, height: Number.NaN };

/** The rendered interactive elements of the bar (decision 74's set). */
const BAR_TARGETS = 'header.bar a[href], header.bar button, header.bar summary';

/**
 * As the home page opens on a phone, the bar shows the mark and the name, one link to the home
 * page, and the Menu button, and none of the menu's entries; the page passes both rules, and the
 * link and the Menu button are the two tap targets of the bar, each at least 44×44 CSS px.
 * Proves: SPEC 10.3 "Phone layout (G6)" for the home page, which discharges decision 115's
 * assignment of the home page's own check; decision 116 (on a phone the entries stay behind the
 * Menu button until it is tapped, and the button is at least 44×44 CSS px); decision 125 (the mark
 * and the name one link in the bar); decision 74.
 */
test('the home page passes the phone check with its menu closed', async ({ page }) => {
    const response = await page.goto('/');
    expect(response?.status()).toBe(200);
    await expect(page.getByRole('heading', { level: 1, name: 'Home' })).toBeVisible();
    // The account tests of this chain add accounts on the server these tests share, so the page
    // holds the four starter lines at least.
    expect(await page.locator('section.balances li').count()).toBeGreaterThanOrEqual(4);
    const button = page.getByRole('button', { name: 'Menu' });
    await expect(button).toBeVisible();
    const targets = page.locator(BAR_TARGETS).filter({ visible: true });
    await expect(targets).toHaveCount(2);
    await expect(targets.nth(0)).toHaveText('Nage');
    await expect(targets.nth(0)).toHaveAttribute('href', '/');
    await expect(targets.nth(1)).toHaveText('Menu');
    for (const target of await targets.all()) {
        const box = await target.boundingBox();
        const what = (await target.textContent())?.trim() ?? '';
        expect(box?.width ?? 0, `width of "${what}" in the bar`).toBeGreaterThanOrEqual(MIN_TAP_TARGET_PX);
        expect(box?.height ?? 0, `height of "${what}" in the bar`).toBeGreaterThanOrEqual(MIN_TAP_TARGET_PX);
    }
    await expectPhoneLayout(page);
});

/**
 * After a tap on Menu the entries stand under the bar, one per line, and the page still passes both
 * rules: the link of the mark and the name, the Menu button, the links Home, Accounts and Change password,
 * the three choices of the Appearance group and the Sign out button are each at least 44×44 CSS
 * px, and nothing widens the page.
 * Proves: decision 116 (`app.css` sizes the Menu button and every link and button of the menu, the
 * three choices of the Appearance group included, to at least 44×44 CSS px, so the home page passes
 * rule 2 of decision 74 with its menu); SPEC 10.3 "Phone layout (G6)" for the home page with the
 * menu open.
 */
test('the home page passes the phone check with its menu open', async ({ page }) => {
    await page.goto('/');
    const menu = await openMenu(page);
    await expect(menu.getByRole('link')).toHaveText(['Home', 'Accounts', 'Change password']);
    await expect(menu.getByRole('group', { name: 'Appearance' }).getByRole('button')).toHaveText(['Automatic', 'Light', 'Dark']);
    await expect(menu.getByRole('button', { name: 'Sign out' })).toBeVisible();
    const targets = page.locator(BAR_TARGETS).filter({ visible: true });
    await expect(targets).toHaveCount(9);
    for (const target of await targets.all()) {
        const box = await target.boundingBox();
        const what = (await target.textContent())?.trim() ?? '';
        expect(box?.width ?? 0, `width of "${what}" in the bar`).toBeGreaterThanOrEqual(MIN_TAP_TARGET_PX);
        expect(box?.height ?? 0, `height of "${what}" in the bar`).toBeGreaterThanOrEqual(MIN_TAP_TARGET_PX);
    }
    await expectPhoneLayout(page);
});

/**
 * An account under Assets whose name has 64 characters, 40 of them one unbroken run, funded through
 * its opening-balance form at 999,999,999,999.99, the largest amount a form takes: the home page
 * lists it with that figure, the name wrapping inside its line and the figure inside the page, and
 * passes both rules; so does the accounts list, where its line keeps the name inside it.
 * Proves: Decisions 139, 142 and 149 with SPEC 10.3 and the pause's P8: an account with a
 * 64-character name holding an unbroken run of 40, funded at 999,999,999,999.99 through the form,
 * Home and the list pass both rules.
 */
test('the home page passes the phone check with a 64-character name at the largest amount', async ({ page }) => {
    const name = longName('Phone ob home large sum');
    const path = await create(page, 'assets', name);
    await enter(page, path, { date: '2026-09-30', amount: LARGEST });

    await page.goto('/');
    await expect(page.getByRole('heading', { level: 1, name: 'Home' })).toBeVisible();
    const row = page.locator('section.balances li').filter({ has: page.locator('.name', { hasText: name }) });
    await expect(row).toHaveCount(1);
    await expect(row.locator('.amount')).toHaveText(LARGEST);
    const width = page.viewportSize()?.width ?? 0;
    const nameBox = (await row.locator('.name').boundingBox()) ?? NOWHERE;
    const figureBox = (await row.locator('.amount').boundingBox()) ?? NOWHERE;
    expect(nameBox.x + nameBox.width, 'the name inside the page').toBeLessThanOrEqual(width);
    expect(figureBox.x + figureBox.width, 'the figure inside the page').toBeLessThanOrEqual(width);
    await expectPhoneLayout(page);

    await page.goto('/accounts');
    const listed = line(sectionPanel(page, 'Assets'), name);
    await expect(listed.locator('.amount')).toHaveText(LARGEST);
    const lineBox = (await listed.boundingBox()) ?? NOWHERE;
    const listedName = (await listed.locator('span.name').boundingBox()) ?? NOWHERE;
    expect(listedName.x + listedName.width, 'the name wrapped inside its line').toBeLessThanOrEqual(lineBox.x + lineBox.width);
    await expectPhoneLayout(page);
});
