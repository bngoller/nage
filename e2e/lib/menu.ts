// The menu of a page that knows who is signed in (decisions 116 and 123), as the screen's width
// shows it: the rail under the mark and Nage's name on a wide screen, a list under the Menu button
// on a narrow one. The page carries the entries twice and shows one of the two.
import { expect, type Locator, type Page } from '@playwright/test';

/**
 * Returns the menu's list as it is on screen: on a wide screen the rail's list, which needs no
 * tap; on a narrow one the list under the Menu button, after a tap on the button when the list is
 * closed.
 */
export async function openMenu(page: Page): Promise<Locator> {
    const rail = page.locator('header.bar nav.menu-wide');
    const narrow = page.locator('header.bar details.menu-narrow');
    const button = narrow.locator(':scope > summary');
    await expect(
        rail.or(button).filter({ visible: true }),
        'the menu shown either as the rail or as the Menu button, never both',
    ).toHaveCount(1);
    let list: Locator;
    if (await rail.isVisible()) {
        list = rail.locator(':scope > ul');
    } else {
        if ((await narrow.getAttribute('open')) === null) {
            await button.click();
        }
        list = narrow.locator(':scope > ul');
    }
    await expect(list, "the menu's list").toBeVisible();
    return list;
}
