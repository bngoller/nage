// The look in a browser (milestone 2 slice 6a, SPEC decisions 123, 125 and 126), in both chains
// with the state the set-up project saved: the letters Nage serves and the mark beside its name;
// and, on a computer only, the rail that holds the menu beside the centred content. The rail's
// test has a title that begins "on a computer", which the phone server's chain leaves out
// (playwright.config.ts).
import type { Page } from '@playwright/test';
import { expect, test } from '../lib/test';

/** The address of a font Nage serves (decisions 46 and 126): `/static/<stem>.<hash>.woff2`. */
const FONT_PATH = /^\/static\/IBMPlex(Sans|Mono)-(Regular|SemiBold)-Latin1\.[0-9a-f]{16}\.woff2$/;

/**
 * The four faces Nage serves, by the stem of their files: the home page uses every one of them
 * (text, headings, figures, and the labels and totals in semibold mono).
 */
const FACES = ['IBMPlexSans-Regular', 'IBMPlexSans-SemiBold', 'IBMPlexMono-Regular', 'IBMPlexMono-SemiBold'] as const;

/** The yellow of the mark's tile, the accent of both palettes (`#ffd300`, decision 123). */
const ACCENT = 'rgb(255, 211, 0)';

/** The font families the page in view has loaded, without quotes, once its fonts are ready. */
async function loadedFamilies(page: Page): Promise<string[]> {
    try {
        return await page.evaluate(async () => {
            await document.fonts.ready;
            return [...document.fonts]
                .filter((face) => face.status === 'loaded')
                .map((face) => face.family.replace(/^["']|["']$/g, ''));
        });
    } catch {
        return [];
    }
}

/**
 * The home page is set in IBM Plex Sans, its figures and its small capital labels (the section
 * names and the Net worth name) in IBM Plex Mono, each of the four faces (both families, regular
 * and semibold) loaded from Nage's own hashed addresses as woff2; a figure stands at the right of
 * its row; the bar shows the mark, hidden from screen readers and its tile drawn in the accent,
 * beside the name Nage, shown, together one link to the home page, and on a phone they stand left
 * of the Menu button in one bar. The sign-in page, which knows nobody (the test's own cookies are
 * cleared, which ends no session), shows the mark and the name too, but not as a link, and its
 * masthead and panel stand centred on the screen.
 * Proves: decision 126 and decision 125: Plex Sans and Plex Mono loaded from Nage and the mark
 * shown in the bar and on the sign-in page
 */
test('the pages are set in IBM Plex and carry the mark', async ({ page }) => {
    const fonts: string[] = [];
    page.on('response', (response) => {
        const address = new URL(response.url());
        const type = response.headers()['content-type'] ?? '';
        if (response.status() === 200 && type === 'font/woff2' && FONT_PATH.test(address.pathname)) {
            fonts.push(`${address.origin}${address.pathname}`);
        }
    });

    await page.goto('/');
    await expect(page.getByRole('heading', { level: 1, name: 'Home' })).toBeVisible();
    await expect.poll(() => loadedFamilies(page), { message: 'the page is set in IBM Plex Sans' }).toContain('IBM Plex Sans');
    await expect.poll(() => loadedFamilies(page), { message: 'the figures are set in IBM Plex Mono' }).toContain('IBM Plex Mono');
    const heading = await page.locator('h1').evaluate((element) => getComputedStyle(element).fontFamily);
    expect(heading, 'the first font of the heading').toMatch(/^"?IBM Plex Sans"?,/);
    const amount = await page.locator('section.balances li .amount').first().evaluate((element) => getComputedStyle(element).fontFamily);
    expect(amount, 'the first font of a figure').toMatch(/^"?IBM Plex Mono"?,/);
    for (const label of ['section.balances h2 .name', 'p.net-worth .name']) {
        const family = await page.locator(label).first().evaluate((element) => getComputedStyle(element).fontFamily);
        expect(family, `the first font of the small capital label ${label}`).toMatch(/^"?IBM Plex Mono"?,/);
    }
    const row = page.locator('section.balances li').first();
    const rowBox = (await row.boundingBox()) ?? { x: 0, y: 0, width: 0, height: 0 };
    const figureBox = (await row.locator('.amount').boundingBox()) ?? { x: 0, y: 0, width: 0, height: 0 };
    const rowPadding = await row.evaluate((element) => Number.parseFloat(getComputedStyle(element).paddingRight));
    expect(
        figureBox.x + figureBox.width,
        "a figure stands at the right of its row, in a column, not after the account's name",
    ).toBeGreaterThanOrEqual(rowBox.x + rowBox.width - rowPadding - 1);
    const origin = new URL(page.url()).origin;
    const sans = fonts.filter((font) => font.startsWith(`${origin}/static/IBMPlexSans-`));
    const mono = fonts.filter((font) => font.startsWith(`${origin}/static/IBMPlexMono-`));
    expect(sans.length, 'IBM Plex Sans loaded from Nage').toBeGreaterThan(0);
    expect(mono.length, 'IBM Plex Mono loaded from Nage').toBeGreaterThan(0);
    for (const face of FACES) {
        await expect
            .poll(() => fonts.some((font) => font.startsWith(`${origin}/static/${face}-Latin1.`)), {
                message: `${face} loaded from Nage`,
            })
            .toBe(true);
    }

    const brand = page.locator('header.bar a.brand');
    await expect(brand, 'the mark and the name, one link').toHaveCount(1);
    await expect(brand).toBeVisible();
    await expect(brand).toHaveText('Nage');
    await expect(brand.locator('.brand-name'), 'the name Nage shown beside the mark in the bar').toBeVisible();
    await expect(brand).toHaveAttribute('href', '/');
    const menuButton = page.locator('header.bar details.menu-narrow > summary');
    if (await menuButton.isVisible()) {
        const brandBox = (await brand.boundingBox()) ?? { x: 0, y: 0, width: 0, height: 0 };
        const buttonBox = (await menuButton.boundingBox()) ?? { x: 0, y: 0, width: 0, height: 0 };
        expect(brandBox.x + brandBox.width, 'on a phone the mark and the name stand left of the Menu button').toBeLessThanOrEqual(buttonBox.x);
        expect(
            brandBox.y < buttonBox.y + buttonBox.height && buttonBox.y < brandBox.y + brandBox.height,
            'on a phone the mark and the name stand in one bar with the Menu button, not in a row of their own',
        ).toBe(true);
    }
    const mark = brand.locator('svg.mark');
    await expect(mark).toBeVisible();
    await expect(mark).toHaveAttribute('aria-hidden', 'true');
    expect(await mark.evaluate((element) => getComputedStyle(element).color), 'the tile of the mark').toBe(ACCENT);
    expect(await mark.locator('rect').evaluate((element) => getComputedStyle(element).fill), 'the tile drawn in the accent').toBe(ACCENT);

    await page.context().clearCookies();
    await page.goto('/login');
    await expect(page.getByRole('heading', { level: 1, name: 'Sign in to Nage' })).toBeVisible();
    const masthead = page.locator('header.masthead .brand');
    await expect(masthead).toBeVisible();
    await expect(masthead).toHaveText('Nage');
    await expect(masthead.locator('.brand-name'), 'the name Nage shown beside the mark on the sign-in page').toBeVisible();
    await expect(page.locator('header.masthead a'), 'the mark and the name, no link').toHaveCount(0);
    const pageWidth = await page.evaluate(() => document.documentElement.clientWidth);
    for (const [what, locator] of [
        ['the mark and the name', masthead],
        ['the panel', page.locator('main.panel')],
    ] as const) {
        const box = (await locator.boundingBox()) ?? { x: 0, y: 0, width: 0, height: 0 };
        const left = box.x;
        const right = pageWidth - (box.x + box.width);
        expect(Math.abs(left - right), `${what} of the sign-in page centred on the screen`).toBeLessThanOrEqual(1);
    }
    const loneMark = masthead.locator('svg.mark');
    await expect(loneMark).toBeVisible();
    await expect(loneMark).toHaveAttribute('aria-hidden', 'true');
    expect(await loneMark.evaluate((element) => getComputedStyle(element).color), 'the tile of the mark').toBe(ACCENT);
    expect(await loneMark.locator('rect').evaluate((element) => getComputedStyle(element).fill), 'the tile drawn in the accent').toBe(ACCENT);
});

/**
 * At 1280 px the bar is a rail at the left of the page as tall as the screen, its panel running
 * from the top of the screen to its foot within a small gap, the mark and the name above its menu
 * entries, which stand one under the other, every one of them shown with no Menu button, and Sign
 * out at the panel's foot; the content stands right of the rail, at most 62rem wide, with equal
 * space on its two sides.
 * Proves: decision 123 and decision 116: at 1280 px the menu stands in a rail left of the
 * content, no Menu button shows, and the content column is at most 62rem wide and centred in the
 * space beside the rail
 */
test('on a computer the menu stands in a rail beside the centred content', async ({ page }) => {
    expect(page.viewportSize(), "the computer's screen").toEqual({ width: 1280, height: 800 });
    await page.goto('/');
    await expect(page.getByRole('heading', { level: 1, name: 'Home' })).toBeVisible();

    const rail = page.locator('header.bar');
    const menu = page.getByRole('navigation', { name: 'Menu' });
    const content = page.locator('main');
    await expect(menu, 'the menu does not stand in a rail: it is not shown').toBeVisible();
    const railBox = (await rail.boundingBox()) ?? { x: 0, y: 0, width: 0, height: 0 };
    const menuBox = (await menu.boundingBox()) ?? { x: 0, y: 0, width: 0, height: 0 };
    const contentBox = (await content.boundingBox()) ?? { x: 0, y: 0, width: 0, height: 0 };
    expect(railBox.x + railBox.width, 'the menu does not stand in a rail left of the content').toBeLessThanOrEqual(contentBox.x);
    expect(railBox.x, 'the rail at the left edge of the page').toBe(0);
    expect(railBox.height, 'the rail as tall as the screen').toBeGreaterThanOrEqual(800);
    expect(menuBox.x + menuBox.width, 'the menu inside the rail').toBeLessThanOrEqual(railBox.x + railBox.width);
    const rootPx = await page.evaluate(() => Number.parseFloat(getComputedStyle(document.documentElement).fontSize));
    const panel = (await rail.locator(':scope > .bar-inner').boundingBox()) ?? { x: 0, y: 0, width: 0, height: 0 };
    expect(panel.y, "the rail's panel starts at the top of the screen").toBeLessThanOrEqual(rootPx);
    expect(panel.y + panel.height, "the rail's panel runs to the foot of the screen").toBeGreaterThanOrEqual(800 - rootPx);

    const home = (await menu.getByRole('link', { name: 'Home' }).boundingBox()) ?? { x: 0, y: 0, width: 0, height: 0 };
    const password = (await menu.getByRole('link', { name: 'Change password' }).boundingBox()) ?? { x: 1, y: 0, width: 0, height: 0 };
    expect(home.y + home.height, 'Change password stands under Home').toBeLessThanOrEqual(password.y);
    expect(password.x, 'Change password stands in line with Home').toBe(home.x);
    const brand = (await rail.locator('a.brand').boundingBox()) ?? { x: 0, y: 801, width: 0, height: 0 };
    expect(brand.y + brand.height, 'the mark and the name stand above the menu, not over its entries').toBeLessThanOrEqual(home.y);
    const signOut = menu.getByRole('button', { name: 'Sign out' });
    await expect(signOut).toBeVisible();
    const signOutBox = (await signOut.boundingBox()) ?? { x: 0, y: 0, width: 0, height: 0 };
    expect(
        signOutBox.y + signOutBox.height,
        "Sign out stands at the foot of the rail's panel, under the switch and who is signed in",
    ).toBeGreaterThanOrEqual(panel.y + panel.height - 3 * rootPx);
    await expect(page.getByRole('button', { name: 'Menu' }), 'no Menu button on a computer').toBeHidden();

    const pageWidth = await page.evaluate(() => document.documentElement.clientWidth);
    expect(contentBox.width, 'the content column at most 62rem wide').toBeLessThanOrEqual(62 * rootPx + 0.5);
    const before = contentBox.x - (railBox.x + railBox.width);
    const after = pageWidth - (contentBox.x + contentBox.width);
    expect(before, 'space between the rail and the content').toBeGreaterThan(0);
    expect(Math.abs(before - after), 'the content centred in the space beside the rail').toBeLessThanOrEqual(1);
});
