// The self-test of the phone-check helper (decision 74): synthetic pages prove that each rule
// fails on bad markup and that the running-text exemption holds. The pages are loaded with
// `page.setContent`, so they carry no server CSP and may use inline styles; each carries the
// viewport meta tag the real layout has (base.html), except the case that tests its absence.
import { expect, test, type Page } from '@playwright/test';
import { expectNoHorizontalScroll, expectPhoneLayout, expectTapTargets, smallTapTargets } from '../lib/phone';

const VIEWPORT_META = '<meta name="viewport" content="width=device-width, initial-scale=1">';
/** A text link far below 44 px: exempt only inside running text. */
const SMALL_TEXT = 'font-size: 10px; line-height: 12px;';
/** A 20×20 inline-block control: never exempt. */
const SMALL =
    'display: inline-block; width: 20px; height: 20px; padding: 0; border: 0; font-size: 10px; line-height: 20px; overflow: hidden;';
/** An inline-block control whose width and height each case sets. */
const BOX = 'display: inline-block; padding: 0; border: 0; font-size: 10px; line-height: 20px; overflow: hidden;';
/** A 20×20 block-level control, for every kind of the interactive set. */
const BLOCK =
    'display: block; width: 20px; height: 20px; padding: 0; border: 0; margin: 0; font-size: 10px; overflow: hidden;';
const LARGE = 'display: inline-block; width: 48px; height: 48px; padding: 0; border: 0;';

function html(body: string, head = VIEWPORT_META): string {
    return `<!doctype html><html lang="en"><head><meta charset="utf-8">${head}<title>Self-test</title></head><body>${body}</body></html>`;
}

/** The offenders' descriptions, the part of a result that does not depend on fonts. */
async function offenders(page: Page): Promise<string[]> {
    return (await smallTapTargets(page)).map((target) => target.description);
}

/**
 * A page that fits the viewport, whose only small interactive element is a text link in a
 * sentence and whose button is large enough, passes both rules.
 * Proves: decision 74 (the rules accept conforming markup; the running-text exemption).
 */
test('a conforming page passes', async ({ page }) => {
    await page.setContent(
        html(
            `<main><h1>Fits</h1><p>Read the <a href="#" style="${SMALL_TEXT}">terms</a> before you continue.</p>` +
                `<p><button style="${LARGE}">Go</button></p></main>`,
        ),
    );
    await expectPhoneLayout(page);
});

/**
 * An element wider than the viewport fails rule 1, alone and in the full check the named pages call.
 * Proves: decision 74 rule 1 (page-level horizontal scroll is detected).
 */
test('an element wider than the viewport fails the scroll rule', async ({ page }) => {
    await page.setContent(html('<main><div style="width: 2000px; height: 10px;"></div></main>'));
    await expect(expectNoHorizontalScroll(page)).rejects.toThrow('page-level horizontal scroll');
    await expect(expectPhoneLayout(page)).rejects.toThrow('page-level horizontal scroll');
});

/**
 * Without the viewport meta tag, mobile emulation lays the page out at 980 CSS px, which rule 1's
 * width clause rejects; desktop browsers ignore the tag, so the case runs in the phone project only.
 * Proves: decision 74 rule 1 (the width clause catches a missing viewport meta).
 */
test('a page without the viewport meta fails the width clause', async ({ page, isMobile }) => {
    test.skip(!isMobile, 'the 980 px fallback exists only in mobile emulation');
    await page.setContent(html('<main><p>No viewport meta.</p></main>', ''));
    await expect(expectNoHorizontalScroll(page)).rejects.toThrow('layout viewport width');
});

/**
 * A small button in its own block fails rule 2, alone and in the full check the named pages call,
 * and is named with its size; a control short on one side only (200×20, 20×60, 43×44) is an
 * offender too, while a 44×44 one is not.
 * Proves: decision 74 rule 2 (a small standalone control is an offender, both sides measured against 44 CSS px, through expectTapTargets and the full check).
 */
test('a small standalone control fails the tap rule on either side', async ({ page }) => {
    await page.setContent(html(`<main><p><button id="tiny" style="${SMALL}">x</button></p></main>`));
    expect(await smallTapTargets(page)).toEqual([{ description: 'button#tiny "x"', width: 20, height: 20 }]);
    await expect(expectTapTargets(page)).rejects.toThrow('button#tiny');
    await expect(expectPhoneLayout(page)).rejects.toThrow('button#tiny');
    await page.setContent(html(`<main><p><button id="wide" style="${BOX} width: 200px; height: 20px;">Wide</button></p></main>`));
    expect(await smallTapTargets(page)).toEqual([{ description: 'button#wide "Wide"', width: 200, height: 20 }]);
    await page.setContent(html(`<main><p><button id="tall" style="${BOX} width: 20px; height: 60px;">T</button></p></main>`));
    expect(await smallTapTargets(page)).toEqual([{ description: 'button#tall "T"', width: 20, height: 60 }]);
    await page.setContent(
        html(
            `<main><p><button id="under" style="${BOX} width: 43px; height: 44px;">U</button></p>` +
                `<p><button id="exact" style="${BOX} width: 44px; height: 44px;">E</button></p></main>`,
        ),
    );
    expect(await smallTapTargets(page)).toEqual([{ description: 'button#under "U"', width: 43, height: 44 }]);
});

/**
 * Every kind of interactive element decision 74 lists is measured when it stands outside running
 * text (select, textarea, summary, button and link roles, a tab stop), and each exclusion is not (a
 * hidden input, a tabindex of -1, a link without href).
 * Proves: decision 74 rule 2 (the interactive set and its exclusions).
 */
test('every interactive kind is measured and each exclusion is not', async ({ page }) => {
    await page.setContent(
        html(
            `<main><select id="sel" style="${BLOCK}"><option>s</option></select>` +
                `<textarea id="txt" style="${BLOCK}"></textarea>` +
                `<details><summary id="sum" style="${BLOCK}">d</summary></details>` +
                `<div id="rb" role="button" style="${BLOCK}">b</div>` +
                `<div id="rl" role="link" style="${BLOCK}">l</div>` +
                `<div id="tab" tabindex="0" style="${BLOCK}">t</div>` +
                `<input id="hid" type="hidden" value="h">` +
                `<div id="neg" tabindex="-1" style="${BLOCK}">n</div>` +
                `<a id="nohref" style="${BLOCK}">a</a></main>`,
        ),
    );
    expect(await offenders(page)).toEqual(['select#sel "s"', 'textarea#txt ""', 'summary#sum "d"', 'div#rb "b"', 'div#rl "l"', 'div#tab "t"']);
});

/**
 * A small text link in a sentence is running text and exempt, beside letters or beside digits alone,
 * and so is a link inside an inline-block label with its own text. The same link alone in a block,
 * two links separated by a space or by punctuation, a link beside a heading (the heading's text
 * belongs to a nested block), a link beside an inline-block label (its text is a nested block too),
 * a link whose text sits in a nested element, an inline-block link in a sentence (not laid out as
 * text), a button or a checkbox laid out inline in a sentence (a control is a target whatever its
 * display), a checkbox in a text label and a link beside hidden or zero-size text are all targets.
 * Proves: decision 74 (the running-text exemption and each of its limits).
 */
test('the running-text exemption applies to text links in sentences only', async ({ page }) => {
    await page.setContent(html(`<main><p>Read the <a href="#" style="${SMALL_TEXT}">terms</a> before you continue.</p></main>`));
    expect(await offenders(page)).toEqual([]);
    await page.setContent(html(`<main><p><a id="alone" href="#" style="${SMALL_TEXT}">Back</a></p></main>`));
    expect(await offenders(page)).toEqual(['a#alone "Back"']);
    await page.setContent(
        html(`<main><p><a id="one" href="#" style="${SMALL_TEXT}">Home</a> <a id="two" href="#" style="${SMALL_TEXT}">About</a></p></main>`),
    );
    expect(await offenders(page)).toEqual(['a#one "Home"', 'a#two "About"']);
    await page.setContent(
        html(`<main><p><a id="three" href="#" style="${SMALL_TEXT}">Home</a> · <a id="four" href="#" style="${SMALL_TEXT}">About</a></p></main>`),
    );
    expect(await offenders(page)).toEqual(['a#three "Home"', 'a#four "About"']);
    await page.setContent(html(`<main><h1>Title</h1><a id="top" href="#" style="${SMALL_TEXT}">x</a></main>`));
    expect(await offenders(page)).toEqual(['a#top "x"']);
    await page.setContent(html(`<main><p>Read the <a id="chip" href="#" style="${SMALL}">terms</a> now.</p></main>`));
    expect(await offenders(page)).toEqual(['a#chip "terms"']);
    await page.setContent(
        html('<main><label><input id="agree" type="checkbox" style="appearance: none; width: 20px; height: 20px; margin: 0;"> I agree</label></main>'),
    );
    expect(await offenders(page)).toEqual(['input#agree ""']);
    await page.setContent(html(`<main><p><span hidden>words</span><a id="ghost" href="#" style="${SMALL_TEXT}">x</a></p></main>`));
    expect(await offenders(page)).toEqual(['a#ghost "x"']);
    await page.setContent(html(`<main><p>42.50 <a id="amount" href="#" style="${SMALL_TEXT}">edit</a></p></main>`));
    expect(await offenders(page)).toEqual([]);
    await page.setContent(
        html(`<main><p><span style="display: inline-block;">Total <a id="within" href="#" style="${SMALL_TEXT}">edit</a></span></p></main>`),
    );
    expect(await offenders(page)).toEqual([]);
    await page.setContent(
        html(`<main><p><span style="display: inline-block;">Total</span> <a id="beside" href="#" style="${SMALL_TEXT}">edit</a></p></main>`),
    );
    expect(await offenders(page)).toEqual(['a#beside "edit"']);
    await page.setContent(html(`<main><p><a id="wrapped" href="#" style="${SMALL_TEXT}"><span>Back</span></a></p></main>`));
    expect(await offenders(page)).toEqual(['a#wrapped "Back"']);
    await page.setContent(
        html(`<main><p>Read the <button id="textual" style="display: inline; padding: 0; border: 0; background: none; ${SMALL_TEXT}">terms</button> now.</p></main>`),
    );
    expect(await offenders(page)).toEqual(['button#textual "terms"']);
    await page.setContent(
        html('<main><label>I agree <input id="tick" type="checkbox" style="appearance: none; display: inline; width: 20px; height: 20px; margin: 0;"> to the terms</label></main>'),
    );
    expect(await offenders(page)).toEqual(['input#tick ""']);
    await page.setContent(
        html(`<main><p><span style="visibility: hidden;">words</span><a id="veiled" href="#" style="${SMALL_TEXT}">x</a></p></main>`),
    );
    expect(await offenders(page)).toEqual(['a#veiled "x"']);
    await page.setContent(html(`<main><p><span style="font-size: 0;">words</span><a id="zero" href="#" style="${SMALL_TEXT}">x</a></p></main>`));
    expect(await offenders(page)).toEqual(['a#zero "x"']);
});

/**
 * An interactive element that is not rendered is skipped.
 * Proves: decision 74 rule 2 (only rendered elements are measured).
 */
test('an unrendered element is skipped', async ({ page }) => {
    await page.setContent(
        html(
            `<main><p><button style="${SMALL} display: none;">x</button>` +
                `<button style="${SMALL} visibility: hidden;">y</button></p></main>`,
        ),
    );
    expect(await smallTapTargets(page)).toEqual([]);
});
