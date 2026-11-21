// The appearance switch in a browser (milestone 2 slice 6a, SPEC decisions 123 and 124), in both
// chains with the state the set-up project saved: each choice made in the menu, the palette the
// page then wears and the choice marked, and Automatic following the device's setting. Every test
// has its browser context of its own, so the appearance cookie a test sets is seen by no other
// test; the device's setting is emulated with `page.emulateMedia`.
import type { Page } from '@playwright/test';
import { openMenu } from '../lib/menu';
import { PALETTES, type Palette, painted } from '../lib/palette';
import { expect, test } from '../lib/test';

/** The three choices of the Appearance group, in the order the menu shows them. */
const CHOICES = ['Automatic', 'Light', 'Dark'] as const;

type Choice = (typeof CHOICES)[number];

/**
 * Waits until the page in view paints `palette`, failing with `message`; it asks again while a
 * form's answer is still on its way.
 */
async function expectPainted(page: Page, palette: Palette, message: string): Promise<void> {
    await expect.poll(() => painted(page), { message }).toEqual(PALETTES[palette]);
}

/** The yellow the choice in force is filled with, the accent of both palettes (`#ffd300`). */
const ACCENT = 'rgb(255, 211, 0)';

/**
 * The Appearance group of the menu as the screen shows it, opening the menu on a phone: its three
 * choices in order, side by side in one row of equal parts that spans the group.
 */
async function appearance(page: Page) {
    const menu = await openMenu(page);
    const group = menu.getByRole('group', { name: 'Appearance' });
    const buttons = group.getByRole('button');
    await expect(buttons, 'the three choices of the Appearance group').toHaveText([...CHOICES]);
    const boxes: { x: number; y: number; width: number; height: number }[] = [];
    for (const choice of CHOICES) {
        boxes.push((await group.getByRole('button', { name: choice, exact: true }).boundingBox()) ?? { x: 0, y: 0, width: 0, height: 0 });
    }
    for (const [index, box] of boxes.entries()) {
        const first = boxes[0] ?? box;
        expect(Math.abs(box.y - first.y), `${CHOICES[index]} in one row with Automatic`).toBeLessThanOrEqual(1);
        expect(Math.abs(box.width - first.width), `${CHOICES[index]} as wide as Automatic, one joined switch of equal parts`).toBeLessThanOrEqual(1.5);
        const previous = boxes[index - 1];
        if (previous !== undefined) {
            expect(box.x, `${CHOICES[index]} right of ${CHOICES[index - 1]}`).toBeGreaterThan(previous.x);
        }
    }
    return group;
}

/** The menu marks `chosen` as the appearance in force, filled with the accent, and the other two as not. */
async function expectMarked(page: Page, chosen: Choice): Promise<void> {
    const group = await appearance(page);
    for (const choice of CHOICES) {
        const button = group.getByRole('button', { name: choice, exact: true });
        await expect(button, `${choice} marked as ${choice === chosen ? '' : 'not '}in force`).toHaveAttribute(
            'aria-pressed',
            choice === chosen ? 'true' : 'false',
        );
        const fill = await button.evaluate((element) => getComputedStyle(element).backgroundColor);
        if (choice === chosen) {
            expect(fill, `${choice}, the choice in force, filled with the accent`).toBe(ACCENT);
        } else {
            expect(fill, `${choice}, not in force, not filled with the accent`).not.toBe(ACCENT);
        }
    }
}

/** Presses `choice` in the menu's Appearance group. */
async function choose(page: Page, choice: Choice): Promise<void> {
    await (await appearance(page)).getByRole('button', { name: choice, exact: true }).click();
}

/**
 * On a device set to dark the home page is dark with no choice made; Light chosen in the menu
 * (the rail on a computer, the open menu on a phone), whose three choices stand in one row of
 * equal parts, brings the person back to the home page, which then carries `data-theme="light"`,
 * paints the light palette although the device is dark, and marks Light as the choice in force,
 * filled with the accent while the other two are not. On a device set to light, Dark chosen on the
 * change-password page stays on that page, which carries `data-theme="dark"`, paints the dark
 * palette although the device is light and marks Dark; Automatic chosen then removes the mark, so
 * the page follows the light device again and marks Automatic.
 * Proves: decision 124 and decision 123: Light chosen in the rail on an emulated dark device and
 * Dark on an emulated light device set data-theme, mark the button and change the colours the page
 * really paints, so a missing data-theme block cannot pass; Automatic chosen removes the mark
 */
test('each appearance chosen in the menu paints the pages and is marked as chosen', async ({ page }) => {
    const html = page.locator('html');

    await page.emulateMedia({ colorScheme: 'dark' });
    await page.goto('/');
    await expectPainted(page, 'dark', 'the home page on a dark device before any choice');

    await choose(page, 'Light');
    await expectPainted(page, 'light', 'the page wears the palette chosen: Light on a dark device');
    await expect(html).toHaveAttribute('data-theme', 'light');
    expect(new URL(page.url()).pathname, 'the page after the choice').toBe('/');
    await expect(page.getByRole('heading', { level: 1, name: 'Home' })).toBeVisible();
    await expectMarked(page, 'Light');

    await page.emulateMedia({ colorScheme: 'light' });
    await page.goto('/account/password');
    await expect(page.getByRole('heading', { level: 1, name: 'Change your password' })).toBeVisible();
    await choose(page, 'Dark');
    await expectPainted(page, 'dark', 'the page wears the palette chosen: Dark on a light device');
    await expect(html).toHaveAttribute('data-theme', 'dark');
    expect(new URL(page.url()).pathname, 'the page after the choice').toBe('/account/password');
    await expect(page.getByRole('heading', { level: 1, name: 'Change your password' })).toBeVisible();
    await expectMarked(page, 'Dark');

    await choose(page, 'Automatic');
    await expect(html, 'no palette mark once Automatic is chosen').not.toHaveAttribute('data-theme');
    await expectPainted(page, 'light', 'the page follows the light device once Automatic is chosen');
    expect(new URL(page.url()).pathname, 'the page after the choice').toBe('/account/password');
    await expectMarked(page, 'Automatic');
});

/**
 * With no choice made, the home page carries no palette mark, marks Automatic as in force and
 * paints the dark palette on a device set to dark and the light one when the device turns light;
 * a page without a menu, the closed set-up page, follows the device the same way.
 * Proves: decision 124 and decision 123: without a choice the page is dark or light as the
 * emulated device prefers
 */
test("Automatic follows the device's setting", async ({ page }) => {
    const html = page.locator('html');

    await page.emulateMedia({ colorScheme: 'dark' });
    await page.goto('/');
    await expectPainted(page, 'dark', "the page follows the device's setting: dark");
    await page.emulateMedia({ colorScheme: 'light' });
    await expectPainted(page, 'light', "the page follows the device's setting: light");
    await expect(html, 'no palette mark without a choice').not.toHaveAttribute('data-theme');
    await expectMarked(page, 'Automatic');

    await page.goto('/setup');
    await expect(html, 'no palette mark without a choice').not.toHaveAttribute('data-theme');
    await expectPainted(page, 'light', "the closed set-up page follows the device's setting: light");
    await page.emulateMedia({ colorScheme: 'dark' });
    await expectPainted(page, 'dark', "the closed set-up page follows the device's setting: dark");
});
