// The phone check on the home page, a named page of SPEC 10.3 (decisions 74, 116, 119 and 121), in
// webkit-phone with the state the set-up project saved. Rule 2 measures real elements here: the Menu
// button, and with the menu open its two links and its Sign out button. The page shows the starter
// accounts at zero; its check with a 64-character account name and a large amount comes with the
// account forms of PLAN M2 slice 7.
import { openMenu } from '../../lib/menu';
import { MIN_TAP_TARGET_PX, expectPhoneLayout } from '../../lib/phone';
import { expect, test } from '../../lib/test';

/** The rendered interactive elements of the bar (decision 74's set). */
const BAR_TARGETS = 'header.bar a[href], header.bar button, header.bar summary';

/**
 * As the home page opens on a phone, the bar shows Nage's name and the Menu button and none of the
 * menu's entries; the page passes both rules, and the Menu button is the one tap target of the bar.
 * Proves: SPEC 10.3 "Phone layout (G6)" for the home page, which discharges decision 115's
 * assignment of the home page's own check; decision 116 (on a phone the entries stay behind the
 * Menu button until it is tapped, and the button is at least 44×44 CSS px); decision 74.
 */
test('the home page passes the phone check with its menu closed', async ({ page }) => {
    const response = await page.goto('/');
    expect(response?.status()).toBe(200);
    await expect(page.getByRole('heading', { level: 1, name: 'Home' })).toBeVisible();
    await expect(page.locator('section.balances li')).toHaveCount(4);
    const button = page.getByRole('button', { name: 'Menu' });
    await expect(button).toBeVisible();
    await expect(page.locator(BAR_TARGETS).filter({ visible: true })).toHaveCount(1);
    const box = await button.boundingBox();
    expect(box?.width ?? 0, 'width of the Menu button').toBeGreaterThanOrEqual(MIN_TAP_TARGET_PX);
    expect(box?.height ?? 0, 'height of the Menu button').toBeGreaterThanOrEqual(MIN_TAP_TARGET_PX);
    await expectPhoneLayout(page);
});

/**
 * After a tap on Menu the entries stand under the bar, one per line, and the page still passes both
 * rules: the Menu button, the links Home and Change password and the Sign out button are each at
 * least 44×44 CSS px, and nothing widens the page.
 * Proves: decision 116 (`app.css` sizes the Menu button and every link and button of the menu to at
 * least 44×44 CSS px, so the home page passes rule 2 of decision 74 with its menu); SPEC 10.3 "Phone
 * layout (G6)" for the home page with the menu open.
 */
test('the home page passes the phone check with its menu open', async ({ page }) => {
    await page.goto('/');
    const menu = await openMenu(page);
    await expect(menu.getByRole('link')).toHaveText(['Home', 'Change password']);
    await expect(menu.getByRole('button', { name: 'Sign out' })).toBeVisible();
    const targets = page.locator(BAR_TARGETS).filter({ visible: true });
    await expect(targets).toHaveCount(4);
    for (const target of await targets.all()) {
        const box = await target.boundingBox();
        const what = (await target.textContent())?.trim() ?? '';
        expect(box?.width ?? 0, `width of "${what}" in the bar`).toBeGreaterThanOrEqual(MIN_TAP_TARGET_PX);
        expect(box?.height ?? 0, `height of "${what}" in the bar`).toBeGreaterThanOrEqual(MIN_TAP_TARGET_PX);
    }
    await expectPhoneLayout(page);
});
