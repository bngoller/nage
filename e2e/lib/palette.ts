// The two palettes of Nage's look (decisions 123 and 124) as a browser paints them: the page's
// ground and its text, read from what the page really paints, never from `data-theme` alone, so
// that a palette block missing from the style file cannot pass for a chosen appearance.
import type { Page } from '@playwright/test';

/** What a page paints: the ground behind it and its text, as `getComputedStyle` gives them. */
export interface Painted {
    ground: string;
    text: string;
}

/**
 * The page's ground and text of each palette as SPEC decision 123 lists them, written as a
 * browser reports a computed colour: dark `#0b1023` and `#f2f1ea`, light `#f7f4ea` and `#10162f`.
 */
export const PALETTES = {
    dark: { ground: 'rgb(11, 16, 35)', text: 'rgb(242, 241, 234)' },
    light: { ground: 'rgb(247, 244, 234)', text: 'rgb(16, 22, 47)' },
} as const satisfies Record<string, Painted>;

/** The palettes by name: `dark` or `light`. */
export type Palette = keyof typeof PALETTES;

/**
 * The colours the page in view paints on its body, or `null` while no page can be read, as
 * between a form's post and the page that answers it; for `expect.poll`, which then asks again.
 */
export async function painted(page: Page): Promise<Painted | null> {
    try {
        return await page.evaluate(() => {
            const style = getComputedStyle(document.body);
            return { ground: style.backgroundColor, text: style.color };
        });
    } catch {
        return null;
    }
}
