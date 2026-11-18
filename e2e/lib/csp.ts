// The policy check of decision 40 as decision 121 carries it out: every page of Nage that an e2e
// test opens is watched, in both browsers, for any report of a Content Security Policy violation,
// and on the page a test ends on htmx must have loaded with the configuration of Nage's layout; a
// page that a test opens and then leaves, the busy page among them, is watched but not asked for
// htmx. lib/test.ts applies both to the `page` of every test; tests/csp-check.spec.ts is this
// helper's own test.
//
// A report is taken from three sources, so that the check depends neither on one browser's wording
// nor on one kind of violation: the page's `securitypolicyviolation` events, which both browsers
// send for every violation, each with its directive, what was refused and the element it was raised
// on; console messages that name the Content Security Policy; and uncaught errors that name it (a
// refused evaluation of a string, for which Chromium writes no console message). A bare check for
// console errors would not do: any request a test has refused on purpose writes one.
//
// One report is known and set aside, never counted. WebKit raises an inline-style report of its own
// for every `select` element on a page whose policy refuses inline styles (seen with Playwright's
// WebKit 27.2, in the phone profile and in a desktop one): the element carries no `style` attribute,
// nothing of the page is refused, and Chromium raises nothing. Its event tells it apart: the
// directive is `style-src-attr` and the element it is raised on is a `select` without a `style`
// attribute. A `select` that does carry one is reported like any other element. WebKit's console
// sentence for a refused inline style does not say which element it was, so that one sentence is
// left to the events, which judge every refused inline style; every other console message that
// names the policy counts. The self-test pins both sides, and that WebKit still raises the report,
// so that the exception does not outlive its reason.
import { expect, type Page } from '@playwright/test';

/** The response header that carries Nage's policy (SPEC 5.3). */
export const POLICY_HEADER = 'content-security-policy';

/** What a watched page has reported. */
export interface PolicyWatch {
    /**
     * One line per report that counts, in the order they arrived, each naming its source (`event`,
     * `console` or `error`) and what it said. Waits for the reports of the page in view to arrive.
     */
    reports(): Promise<string[]>;
    /** One line per report that was set aside: WebKit's own report for a `select` (see above). */
    setAside(): Promise<string[]>;
}

/**
 * Starts watching `page` for reports of a Content Security Policy violation, on every document it
 * loads from now on. Call it before the page's first navigation. The watch itself breaks no policy:
 * it adds no inline script or style to a page.
 */
export async function watchPolicy(page: Page): Promise<PolicyWatch> {
    const counted: string[] = [];
    const setAside: string[] = [];
    page.on('console', (message) => {
        const text = message.text();
        if (NAMES_THE_POLICY.test(text) && text !== WEBKIT_REFUSED_INLINE_STYLE) {
            counted.push(`console: ${text}`);
        }
    });
    page.on('pageerror', (error) => {
        if (NAMES_THE_POLICY.test(error.message)) {
            counted.push(`error: ${error.message}`);
        }
    });
    await page.exposeBinding(REPORT_BINDING, (_source, report: ViolationEvent) => {
        (isWebKitSelectReport(report) ? setAside : counted).push(describeEvent(report));
    });
    await page.addInitScript(listenForViolations, REPORT_BINDING);
    return {
        reports: async () => {
            await reportsArrived(page);
            return [...counted];
        },
        setAside: async () => {
            await reportsArrived(page);
            return [...setAside];
        },
    };
}

/** Fails, listing every report that counts, when the watch holds one. */
export async function expectNoPolicyViolation(watch: PolicyWatch): Promise<void> {
    expect(await watch.reports(), 'reports of a Content Security Policy violation on the watched page').toEqual([]);
}

/**
 * Asserts that htmx has loaded on the page in view with the configuration of Nage's layout
 * (decision 40): `allowEval` off and `includeIndicatorStyles` off, the two settings without which
 * htmx would evaluate strings and add an inline style, against the policy. Waits for the page to
 * finish loading first, since the layout loads htmx with `defer`.
 */
export async function expectHtmxLoaded(page: Page): Promise<void> {
    await page.waitForLoadState('load');
    const config = await page.evaluate(() => {
        const htmx = (window as unknown as { htmx?: { config?: { allowEval?: unknown; includeIndicatorStyles?: unknown } } }).htmx;
        if (htmx?.config === undefined) {
            return null;
        }
        return { allowEval: htmx.config.allowEval, includeIndicatorStyles: htmx.config.includeIndicatorStyles };
    });
    expect(config, 'the configuration of htmx on the page in view (null: htmx has not loaded)').toEqual({
        allowEval: false,
        includeIndicatorStyles: false,
    });
}

/** What the page sends for one `securitypolicyviolation` event. */
interface ViolationEvent {
    /** The directive that refused it (`effectiveDirective`). */
    directive: string;
    /** What was refused: an address, or `inline` for inline code. */
    refused: string;
    /** The tag of the element the event was raised on, or `document`. */
    element: string;
    /** Whether that element carries a `style` attribute. */
    styled: boolean;
    /** The source file and line the browser names, when it names one. */
    source: string;
}

/** The name of the function the watch exposes to every page, which the page calls for each event. */
const REPORT_BINDING = '__nagePolicyViolation';

/** A console message or an error that names the Content Security Policy, in either browser's words. */
const NAMES_THE_POLICY = /Content Security Policy/i;

/**
 * WebKit's console sentence for a refused inline style, which does not name the element: an event
 * for every such refusal judges it instead (see the module comment).
 */
const WEBKIT_REFUSED_INLINE_STYLE =
    "Refused to apply a stylesheet because its hash, its nonce, or 'unsafe-inline' does not appear in the style-src directive of the Content Security Policy.";

/** WebKit's own report for a `select`, which is set aside: an inline style refused on a `select` that has none. */
function isWebKitSelectReport(report: ViolationEvent): boolean {
    return report.directive === 'style-src-attr' && report.element === 'select' && !report.styled;
}

function describeEvent(report: ViolationEvent): string {
    const styled = report.styled ? ' with a style attribute' : '';
    const source = report.source === '' ? '' : ` (${report.source})`;
    return `event: ${report.directive} refused ${report.refused} on ${report.element}${styled}${source}`;
}

/**
 * Runs in every document the page loads, before the document's own scripts, as an init script of
 * the browser's protocol (not an element of the page, so the policy does not judge it): sends each
 * `securitypolicyviolation` event, which reaches the document from the element it was raised on, to
 * the watch. It may use nothing from outside its own body but its argument.
 */
function listenForViolations(binding: string): void {
    document.addEventListener(
        'securitypolicyviolation',
        (event) => {
            const element = event.target instanceof Element ? event.target : null;
            const send = (window as unknown as Record<string, (report: unknown) => void>)[binding];
            send({
                directive: event.effectiveDirective,
                refused: event.blockedURI === '' ? 'inline' : event.blockedURI,
                element: element === null ? 'document' : element.tagName.toLowerCase(),
                styled: element !== null && element.hasAttribute('style'),
                source: event.sourceFile === '' ? '' : `${event.sourceFile}:${event.lineNumber}`,
            });
        },
        true,
    );
}

/**
 * Waits until the reports the page in view has raised so far have reached this process: a task of
 * the page that runs after the tasks that sent the events, then one more round trip. A page that is
 * closed or between two documents has nothing more to send.
 */
async function reportsArrived(page: Page): Promise<void> {
    if (page.isClosed()) {
        return;
    }
    await page.evaluate(() => new Promise<void>((done) => setTimeout(done, 0))).catch(() => undefined);
    await page.evaluate(() => undefined).catch(() => undefined);
}
