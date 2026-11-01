// The phone-layout check of SPEC 10.3 ("Phone layout (G6)") as decision 74 defines it.
// Both rules measure CSS pixels inside the page, so the device scale factor never enters.
//
// 1. No page-level horizontal scroll: the root element's scroll width does not exceed its client
//    width, and the client width does not exceed the project's viewport width (a page without the
//    `width=device-width` viewport meta lays out at 980 CSS px in mobile emulation and fails here).
// 2. Tap targets: every interactive element outside running text is at least 44×44 CSS px
//    (WCAG 2.5.5). Interactive: a[href], button, input other than hidden, select, textarea,
//    summary, [role=button], [role=link], [tabindex] other than -1. Skipped: an element that is not
//    rendered (a box without a positive width or height, or `visibility: hidden`). Exempt as
//    running text (WCAG's "in a sentence or block of text"): a link laid out as text (display
//    exactly `inline`) whose own block holds, in a rendered element outside any interactive
//    element, a letter or digit; text inside a nested block (a heading, an inline-block element),
//    punctuation-only separators and hidden text do not count, and a button or form control is a
//    target whatever its display.
import { expect, type Page } from '@playwright/test';

/** The minimum tap-target side in CSS px (SPEC 10.3). */
export const MIN_TAP_TARGET_PX = 44;

/** The interactive elements of decision 74. */
export const INTERACTIVE_SELECTOR = [
    'a[href]',
    'button',
    'input:not([type="hidden"])',
    'select',
    'textarea',
    'summary',
    '[role="button"]',
    '[role="link"]',
    '[tabindex]:not([tabindex="-1"])',
].join(', ');

/** The root element's widths in CSS px. */
export interface RootWidths {
    scrollWidth: number;
    clientWidth: number;
}

/** An interactive element outside running text that is smaller than the minimum. */
export interface SmallTapTarget {
    /** The tag, the id when there is one, and the element's text, for the failure message. */
    description: string;
    width: number;
    height: number;
}

/** Reads the root element's scroll and client width. */
export async function rootWidths(page: Page): Promise<RootWidths> {
    return page.evaluate(() => {
        const root = document.documentElement;
        return { scrollWidth: root.scrollWidth, clientWidth: root.clientWidth };
    });
}

/** Lists every rendered interactive element outside running text smaller than the minimum. */
export async function smallTapTargets(page: Page): Promise<SmallTapTarget[]> {
    return page.evaluate(
        ({ selector, min }) => {
            const isInline = (element: Element): boolean =>
                getComputedStyle(element).display === 'inline';
            // The element itself when it is not laid out as text, else its nearest ancestor that is
            // not; an inline-block, inline-flex or inline-grid element is its own block, like a heading.
            const blockOf = (element: Element): Element => {
                let current: Element | null = element;
                while (current !== null && current !== document.body && isInline(current)) {
                    current = current.parentElement;
                }
                return current ?? document.body;
            };
            // Rendered: a box with a positive width or height, and not `visibility: hidden`.
            const isRendered = (element: Element): boolean => {
                const box = element.getBoundingClientRect();
                return (box.width > 0 || box.height > 0) && getComputedStyle(element).visibility !== 'hidden';
            };
            // Running text: a link laid out as text (display: inline) whose own block holds a rendered
            // letter or digit outside every interactive element; text in a nested block,
            // punctuation-only separators and hidden text do not count. A button or form control is
            // never running text, whatever its display.
            const inRunningText = (element: Element): boolean => {
                const parent = element.parentElement;
                if (parent === null || !element.matches('a[href], [role="link"]') || !isInline(element)) {
                    return false;
                }
                const block = blockOf(parent);
                const walker = document.createTreeWalker(block, NodeFilter.SHOW_TEXT);
                for (let node = walker.nextNode(); node !== null; node = walker.nextNode()) {
                    const owner = node.parentElement;
                    if (
                        owner === null ||
                        owner.closest(selector) !== null ||
                        blockOf(owner) !== block ||
                        !isRendered(owner)
                    ) {
                        continue;
                    }
                    if (/[\p{L}\p{N}]/u.test(node.textContent ?? '')) {
                        return true;
                    }
                }
                return false;
            };
            const describe = (element: Element): string => {
                const text = (element.textContent ?? '').trim().replace(/\s+/g, ' ').slice(0, 40);
                const id = element.id === '' ? '' : `#${element.id}`;
                return `${element.tagName.toLowerCase()}${id} "${text}"`;
            };
            const small: { description: string; width: number; height: number }[] = [];
            for (const element of document.querySelectorAll(selector)) {
                if (!isRendered(element) || inRunningText(element)) {
                    continue;
                }
                const box = element.getBoundingClientRect();
                if (box.width < min || box.height < min) {
                    small.push({ description: describe(element), width: box.width, height: box.height });
                }
            }
            return small;
        },
        { selector: INTERACTIVE_SELECTOR, min: MIN_TAP_TARGET_PX },
    );
}

/** Rule 1 of decision 74: no page-level horizontal scroll at the project's viewport width. */
export async function expectNoHorizontalScroll(page: Page): Promise<void> {
    const widths = await rootWidths(page);
    expect(widths.scrollWidth, 'page-level horizontal scroll (scrollWidth over clientWidth)')
        .toBeLessThanOrEqual(widths.clientWidth);
    // Not equality: a rendered scroll bar makes the client width narrower than the viewport.
    expect(widths.clientWidth, 'layout viewport width (a missing viewport meta lays out at 980)')
        .toBeLessThanOrEqual(page.viewportSize()?.width ?? 0);
}

/** Rule 2 of decision 74: every interactive element outside running text is at least 44×44 CSS px. */
export async function expectTapTargets(page: Page): Promise<void> {
    const small = await smallTapTargets(page);
    expect(small, `interactive elements outside running text under ${MIN_TAP_TARGET_PX}×${MIN_TAP_TARGET_PX} CSS px`)
        .toEqual([]);
}

/** The full phone check of SPEC 10.3 for a named page: both rules. */
export async function expectPhoneLayout(page: Page): Promise<void> {
    await expectNoHorizontalScroll(page);
    await expectTapTargets(page);
}
