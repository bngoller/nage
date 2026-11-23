// The account pages in both chains (PLAN M2 slice 7, decisions 128 to 137 and 139), with the state
// the set-up project saved: the menu's Accounts entry, the list, creating an account and a category,
// changing one, the clash of two saves, archiving and unarchiving with their confirmations and a
// refusal, and a built-in account's page; a test whose title begins "on a computer" runs in the
// desktop chain alone and proves the computer's order (a section's New line beside its heading, a
// confirmation's main button first). Every test uses names of its own, so that no test depends on
// another's data on the shared server, and none expects a fixed count beside Show archived: only
// that the link appears and that the count rises by one. No account here is given an owner, so the
// home page's lines keep no group heading (tests/home.spec.ts).
import {
    accountPath,
    archivedCount,
    create,
    holdingArchives,
    line,
    notice,
    sectionPanel,
    send,
    trail,
} from '../lib/accounts';
import { openMenu } from '../lib/menu';
import { expect, test } from '../lib/test';

/** The sentence under the Archive button of an account that is neither built in nor archived (decision 133). */
const NEVER_DELETES =
    'Nage never deletes an account. To fix a typo, change its name; to stop using it, archive it. An archived account can be unarchived at any time.';

/**
 * The menu lists Home, Accounts and Change password, and its Accounts entry leads to the list,
 * which the menu then marks as the page in view. The list's one yellow button reads New account or
 * category; the balances line names the day; the three groups are Accounts (Assets, Liabilities),
 * Categories (Income, Expenses) and Built in (Equity); + New account stands with Assets and
 * Liabilities, + New category with Income and Expenses, nothing with Equity; the starter accounts
 * under Assets show 0.00 and the section its total; categories and Equity show no figure; and the
 * built-in accounts Uncategorized, Opening Balances and Valuation Changes are marked built in, each
 * line a link to its account's page.
 * Proves: Decisions 129, 131 and 137 in Chromium and in WebKit: the menu's Accounts entry and the
 * list's groups.
 */
test('the menu leads to Accounts, whose list shows the sections and the built-in accounts', async ({ page }) => {
    await page.goto('/');
    const menu = await openMenu(page);
    await expect(menu.getByRole('link')).toHaveText(['Home', 'Accounts', 'Change password']);
    await menu.getByRole('link', { name: 'Accounts', exact: true }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'Accounts', exact: true })).toBeVisible();
    expect(new URL(page.url()).pathname).toBe('/accounts');
    const marked = await openMenu(page);
    await expect(marked.getByRole('link', { name: 'Accounts', exact: true })).toHaveAttribute('aria-current', 'page');
    await expect(marked.getByRole('link', { name: 'Home', exact: true })).not.toHaveAttribute('aria-current');

    const main = page.locator('main');
    await expect(main.locator('a.button')).toHaveText(['New account or category']);
    await expect(main.locator('a.button')).toHaveAttribute('href', '/accounts/new');
    await expect(main.getByText(/^Balances at the end of \d{4}-\d{2}-\d{2}\.$/)).toBeVisible();
    await expect(main.locator('section.group > h2')).toHaveText(['Accounts', 'Categories', 'Built in']);
    await expect(main.locator('section.kind h3')).toHaveText(['Assets', 'Liabilities', 'Income', 'Expenses', 'Equity']);
    for (const [name, add] of [
        ['Assets', '+ New account'],
        ['Liabilities', '+ New account'],
        ['Income', '+ New category'],
        ['Expenses', '+ New category'],
    ] as const) {
        await expect(sectionPanel(page, name).locator('a.add'), `the New line of ${name}`).toHaveText([add]);
    }
    await expect(sectionPanel(page, 'Equity').locator('a.add'), 'no New line under Equity').toHaveCount(0);

    const assets = sectionPanel(page, 'Assets');
    await expect(assets.locator('.panel-head .amount'), "the section's total, whatever the chain's other tests funded").toHaveText(
        /^-?\d{1,3}(,\d{3})*\.\d{2}$/,
    );
    for (const name of ['Cash', 'Checking', 'Savings']) {
        await expect(line(assets, name).locator('.amount'), `the balance of ${name}`).toHaveText('0.00');
        await expect(line(assets, name)).toHaveAttribute('href', /^\/accounts\/[1-9][0-9]*$/);
    }
    await expect(line(sectionPanel(page, 'Liabilities'), 'Credit Card').locator('.amount')).toHaveText('0.00');
    const expenses = sectionPanel(page, 'Expenses');
    await expect(expenses.locator('.amount'), 'no figure under Expenses').toHaveCount(0);
    await expect(sectionPanel(page, 'Income').locator('.amount'), 'no figure under Income').toHaveCount(0);
    await expect(line(expenses, 'Uncategorized').locator('.tag')).toHaveText(['built in']);
    await expect(line(expenses, 'Groceries').locator('.tag')).toHaveCount(0);
    const equity = sectionPanel(page, 'Equity');
    await expect(equity.locator('ul.rows span.name')).toHaveText(['Opening Balances', 'Valuation Changes']);
    await expect(equity.locator('ul.rows .tag')).toHaveText(['built in', 'built in']);
    await expect(equity.locator('.amount'), 'no figure under Equity').toHaveCount(0);
});

/**
 * New account or category opens What do you want to add?, whose Something you have leads to the
 * form New account under Assets, under the trail Accounts, New account; the account created there
 * opens its own page, which says This account was created. once: the page loaded again says
 * nothing; its facts are Owner Joint, Balance 0.00 and Opening balance None yet (decision 141). Its
 * New account inside opens the same form
 * with the trail Accounts, the account, New account, its Inside chosen, and Back to the account,
 * which leads back to its page; a name with a colon sent from it comes back with the same trail,
 * way back and place; the account created there sits inside it, its page under the trail Accounts,
 * the one that holds it, itself, says This account was created. once, and the page of the one that
 * holds it lists it under Inside it. A message left waiting by a create whose page was not opened
 * is not shown on the page of the one that holds it, which clears it unseen.
 * Proves: Decisions 132 and 137 with P10 in Chromium and in WebKit: create through the choice page
 * and through New account inside, its trail and way back, the message once.
 */
test('an account is created inside another and its page says so once', async ({ page }) => {
    await page.goto('/accounts');
    await page.getByRole('link', { name: 'New account or category', exact: true }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'New account or category' })).toBeVisible();
    await expect(page.getByRole('heading', { level: 2, name: 'What do you want to add?' })).toBeVisible();
    await expect(page.locator('ul.choose strong')).toHaveText([
        'Something you have',
        'Something you owe',
        'A kind of income',
        'A kind of spending',
    ]);
    await page.getByRole('link', { name: /^Something you have/ }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'New account under Assets' })).toBeVisible();
    await expect(trail(page).locator('li')).toHaveText(['Accounts', 'New account']);
    await expect(page.getByLabel('Inside', { exact: true }).locator('option:checked')).toHaveText('Directly under Assets');
    await expect(page.getByLabel('Owner', { exact: true }).locator('option:checked')).toHaveText('Joint');
    await expect(page.getByRole('link', { name: 'Back to Accounts', exact: true })).toHaveAttribute('href', '/accounts');
    await page.getByLabel('Name', { exact: true }).fill('Rainy day');
    expect(await send(page, page.getByRole('button', { name: 'Create account', exact: true }))).toBe(303);
    await expect(page.getByRole('heading', { level: 1, name: 'Rainy day', exact: true })).toBeVisible();
    const parent = accountPath(page);
    await expect(page.locator('p.place')).toHaveText('Assets:Rainy day');
    await expect(notice(page)).toHaveText('This account was created.');
    await expect(notice(page)).toHaveAttribute('role', 'status');
    await page.reload();
    await expect(page.getByRole('heading', { level: 1, name: 'Rainy day', exact: true })).toBeVisible();
    await expect(notice(page), 'the message shown once').toHaveCount(0);
    await expect(page.locator('dl.facts dt')).toHaveText(['Owner', 'Balance', 'Opening balance']);
    await expect(page.locator('dl.facts dd')).toHaveText(['Joint', '0.00', 'None yet']);

    await page.getByRole('link', { name: 'New account inside Rainy day', exact: true }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'New account under Assets' })).toBeVisible();
    await expect(trail(page).locator('li')).toHaveText(['Accounts', 'Rainy day', 'New account']);
    await expect(trail(page).getByRole('link')).toHaveText(['Accounts', 'Rainy day']);
    await expect(page.getByLabel('Inside', { exact: true }).locator('option:checked')).toHaveText('Assets:Rainy day');
    await page.getByRole('link', { name: 'Back to Rainy day', exact: true }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'Rainy day', exact: true })).toBeVisible();
    expect(new URL(page.url()).pathname).toBe(parent);
    await expect(notice(page)).toHaveCount(0);

    await page.getByRole('link', { name: 'New account inside Rainy day', exact: true }).click();
    await page.getByLabel('Name', { exact: true }).fill('Rain: coat');
    expect(await send(page, page.getByRole('button', { name: 'Create account', exact: true }))).toBe(422);
    await expect(trail(page).locator('li'), 'the refused form keeps its trail').toHaveText(['Accounts', 'Rainy day', 'New account']);
    await expect(page.getByRole('link', { name: 'Back to Rainy day', exact: true })).toHaveAttribute('href', parent);
    await expect(page.getByLabel('Inside', { exact: true }).locator('option:checked')).toHaveText('Assets:Rainy day');
    await page.getByLabel('Name', { exact: true }).fill('Umbrella');
    expect(await send(page, page.getByRole('button', { name: 'Create account', exact: true }))).toBe(303);
    await expect(page.getByRole('heading', { level: 1, name: 'Umbrella', exact: true })).toBeVisible();
    await expect(page.locator('p.place')).toHaveText('Assets:Rainy day:Umbrella');
    await expect(trail(page).locator('li')).toHaveText(['Accounts', 'Rainy day', 'Umbrella']);
    await expect(notice(page)).toHaveText('This account was created.');
    await page.reload();
    await expect(page.getByRole('heading', { level: 1, name: 'Umbrella', exact: true })).toBeVisible();
    await expect(notice(page), 'the message shown once').toHaveCount(0);

    await trail(page).getByRole('link', { name: 'Rainy day', exact: true }).click();
    expect(new URL(page.url()).pathname).toBe(parent);
    await expect(notice(page), "the message shown on no other account's page").toHaveCount(0);
    const inside = page.getByRole('region', { name: 'Inside it', exact: true });
    await expect(inside.locator('span.name')).toHaveText(['Umbrella']);
    await expect(line(inside, 'Umbrella').locator('.amount')).toHaveText('0.00');

    const rainy = parent.slice('/accounts/'.length);
    const waiting = await page.request.post(new URL(`/accounts/new?under=assets&inside=${rainy}`, page.url()).toString(), {
        form: { name: 'Galoshes', inside: rainy },
        headers: { origin: new URL(page.url()).origin },
        maxRedirects: 0,
    });
    expect(waiting.status(), 'the create answers with the way to its page, which is not opened').toBe(303);
    await page.goto(parent);
    await expect(page.getByRole('heading', { level: 1, name: 'Rainy day', exact: true })).toBeVisible();
    await expect(notice(page), "the waiting message shown on another account's page").toHaveCount(0);
    await line(page.getByRole('region', { name: 'Inside it', exact: true }), 'Galoshes').click();
    await expect(page.getByRole('heading', { level: 1, name: 'Galoshes', exact: true })).toBeVisible();
    await expect(notice(page), "the message cleared unseen by another account's page").toHaveCount(0);
});

/**
 * + New category under Expenses opens New category under Expenses, with no Owner; a name with a
 * colon is refused with 422: the message at the top says the category was not created, the problem
 * under the name says why in the words of decision 136, and the name typed is kept. The corrected
 * name creates the category, whose page says This category was created. and, under its actions,
 * that Nage never deletes a category; Change category opens its form under the trail Accounts, the
 * category, Change, with the help line of its section; a new name with a colon is refused with 422,
 * the message at the top saying the category was not saved and the name box alone marked; a new
 * name and a place inside another category are saved, and the category's page, under its new place,
 * says This category was changed. once.
 * Proves: Decisions 132, 134 and 136 in Chromium and in WebKit: the refused form keeps what was
 * typed, the change saves and says so.
 */
test('a category is created, refused for a colon, changed, and its page says so once', async ({ page }) => {
    await page.goto('/accounts');
    await sectionPanel(page, 'Expenses').getByRole('link', { name: '+ New category', exact: true }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'New category under Expenses' })).toBeVisible();
    await expect(trail(page).locator('li')).toHaveText(['Accounts', 'New category']);
    await expect(page.getByLabel('Owner', { exact: true }), 'no owner under Expenses').toHaveCount(0);
    const name = page.getByLabel('Name', { exact: true });
    await name.fill('Pets: supplies');
    expect(await send(page, page.getByRole('button', { name: 'Create category', exact: true }))).toBe(422);
    await expect(page.getByRole('heading', { level: 1, name: 'New category under Expenses' })).toBeVisible();
    await expect(notice(page)).toHaveText(
        'The category was not created, because some of the boxes below need a change. Each one says what to change; then send the form again.',
    );
    await expect(notice(page)).toHaveAttribute('role', 'alert');
    await expect(page.locator('.field small.problem')).toHaveText([
        'The category was not created because its name contains a colon (:). Nage uses the colon to write where a thing sits, as in Expenses:Groceries. Use another sign, such as a dash, then create it again.',
    ]);
    await expect(name).toHaveValue('Pets: supplies');
    await expect(name).toHaveAttribute('aria-invalid', 'true');

    await name.fill('Pet supplies');
    expect(await send(page, page.getByRole('button', { name: 'Create category', exact: true }))).toBe(303);
    await expect(page.getByRole('heading', { level: 1, name: 'Pet supplies', exact: true })).toBeVisible();
    const path = accountPath(page);
    await expect(page.locator('p.place')).toHaveText('Expenses:Pet supplies');
    await expect(notice(page)).toHaveText('This category was created.');
    await expect(page.locator('.doing .actions > a')).toHaveText([
        'Change category',
        'New category inside Pet supplies',
        'Archive',
    ]);
    await expect(page.locator('.doing p.note')).toHaveText(
        'Nage never deletes a category. To fix a typo, change its name; to stop using it, archive it. An archived category can be unarchived at any time.',
    );

    await page.getByRole('link', { name: 'Change category', exact: true }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'Change category' })).toBeVisible();
    await expect(trail(page).locator('li')).toHaveText(['Accounts', 'Pet supplies', 'Change']);
    await expect(page.locator('.field small.help')).toHaveText([
        'Only places under Expenses: a category stays in its section. To move it to Income, make a new one there and archive this one.',
    ]);
    await expect(page.getByLabel('Name', { exact: true })).toHaveValue('Pet supplies');
    await expect(page.getByLabel('Inside', { exact: true }).locator('option:checked')).toHaveText('Directly under Expenses');
    await expect(page.getByLabel('Owner', { exact: true })).toHaveCount(0);
    await page.getByLabel('Name', { exact: true }).fill('Pet: care');
    expect(await send(page, page.getByRole('button', { name: 'Save changes', exact: true }))).toBe(422);
    await expect(notice(page)).toHaveText(
        'The category was not saved, because some of the boxes below need a change. Each one says what to change; then send the form again.',
    );
    await expect(notice(page)).toHaveAttribute('role', 'alert');
    await expect(page.getByLabel('Name', { exact: true })).toHaveValue('Pet: care');
    await expect(page.getByLabel('Name', { exact: true })).toHaveAttribute('aria-invalid', 'true');
    await expect(page.getByLabel('Inside', { exact: true })).not.toHaveAttribute('aria-invalid', 'true');
    await page.getByLabel('Name', { exact: true }).fill('Pet care');
    await page.getByLabel('Inside', { exact: true }).selectOption({ label: 'Expenses:Health' });
    expect(await send(page, page.getByRole('button', { name: 'Save changes', exact: true }))).toBe(303);
    await expect(page.getByRole('heading', { level: 1, name: 'Pet care', exact: true })).toBeVisible();
    expect(new URL(page.url()).pathname).toBe(path);
    await expect(page.locator('p.place')).toHaveText('Expenses:Health:Pet care');
    await expect(trail(page).locator('li')).toHaveText(['Accounts', 'Health', 'Pet care']);
    await expect(notice(page)).toHaveText('This category was changed.');
    await expect(notice(page)).toHaveAttribute('role', 'status');
    await page.reload();
    await expect(page.getByRole('heading', { level: 1, name: 'Pet care', exact: true })).toBeVisible();
    await expect(notice(page), 'the message shown once').toHaveCount(0);
});

/**
 * Two pages show the change form of one category. The first renames it and moves it inside
 * another; the second, which still shows the old values, renames it too and is refused with 409:
 * the page names the category by its saved place and says its name was changed while the person
 * was working, Saved now shows the name saved and nothing of the place, which the second page did
 * not change, and Your change holds the name typed and the place saved now. Save mine then saves the
 * second name, and the place the first save chose stays.
 * Proves: Decision 134 with P2 in Chromium and in WebKit: two pages of one form, the later save
 * refused with what is saved, Save mine then saved, the other save's untouched box kept.
 */
test('two saves of one account clash and the later one is shown what is saved', async ({ page }) => {
    const path = await create(page, 'income', 'Side job');
    await page.goto(`${path}/edit`);
    await expect(page.getByRole('heading', { level: 1, name: 'Change category' })).toBeVisible();

    const other = await page.context().newPage();
    await other.goto(`${path}/edit`);
    await other.getByLabel('Name', { exact: true }).fill('Side work');
    await other.getByLabel('Inside', { exact: true }).selectOption({ label: 'Income:Salary' });
    expect(await send(other, other.getByRole('button', { name: 'Save changes', exact: true }))).toBe(303);
    await expect(notice(other)).toHaveText('This category was changed.');
    await expect(other.locator('p.place')).toHaveText('Income:Salary:Side work');
    await other.close();

    await page.getByLabel('Name', { exact: true }).fill('Gig income');
    expect(await send(page, page.getByRole('button', { name: 'Save changes', exact: true }))).toBe(409);
    await expect(notice(page)).toHaveText(
        'Your change to Income:Salary:Side work was not saved because its name was changed while you were working, by another user or in another window.',
    );
    await expect(notice(page)).toHaveAttribute('role', 'alert');
    const saved = page.getByRole('region', { name: 'Saved now', exact: true });
    await expect(saved.locator('dt')).toHaveText(['Name']);
    await expect(saved.locator('dd')).toHaveText(['Side work']);
    const yours = page.getByRole('region', { name: 'Your change', exact: true });
    await expect(yours.getByLabel('Name', { exact: true })).toHaveValue('Gig income');
    await expect(yours.getByLabel('Inside', { exact: true }).locator('option:checked')).toHaveText('Income:Salary');
    await expect(page.getByRole('link', { name: 'Keep what is saved', exact: true })).toHaveAttribute('href', path);

    expect(await send(page, page.getByRole('button', { name: 'Save mine', exact: true }))).toBe(303);
    await expect(page.getByRole('heading', { level: 1, name: 'Gig income', exact: true })).toBeVisible();
    expect(new URL(page.url()).pathname).toBe(path);
    await expect(page.locator('p.place'), "the other save's place kept").toHaveText('Income:Salary:Gig income');
    await expect(notice(page)).toHaveText('This category was changed.');
});

/**
 * An account at zero: Archive opens its confirmation, under the trail Accounts, the account,
 * Archive, whose Keep it leads back with nothing done; its Archive archives it, and its page says
 * This account was archived., marks it archived, says what that means and offers Unarchive and
 * Change name, which opens a form that holds the name alone; its opening-balance form leads back
 * to its page (decision 140). The list then hides it, and Show
 * archived, with its count risen by one, shows it in its place, marked archived, with Hide archived.
 * Unarchive asks the same way, with Keep it archived, and its Unarchive brings it back to the list,
 * the count back to where it was.
 * Proves: Decisions 131 and 135 with P4 and P12 in Chromium and in WebKit: the confirmations, the
 * archived mark and Change name, Show archived appearing with its count risen by one.
 */
test('an account at zero is archived and unarchived after confirming', async ({ page, baseURL }) => {
    const path = await create(page, 'assets', 'Old jar');
    await holdingArchives(baseURL, async () => {
        const before = await archivedCount(page);
        await expect(line(sectionPanel(page, 'Assets'), 'Old jar')).toBeVisible();

        await page.goto(path);
        await expect(page.locator('.doing p.note')).toHaveText(NEVER_DELETES);
        await page.getByRole('link', { name: 'Archive', exact: true }).click();
        await expect(page.getByRole('heading', { level: 1, name: 'Archive Old jar?' })).toBeVisible();
        expect(new URL(page.url()).pathname).toBe(`${path}/archive`);
        await expect(trail(page).locator('li')).toHaveText(['Accounts', 'Old jar', 'Archive']);
        await expect(page.locator('p.place')).toHaveText('Assets:Old jar');
        await expect(page.getByText('It will be hidden from the lists and take no new entries. Nothing is deleted, and you can unarchive it at any time.')).toBeVisible();
        await page.getByRole('link', { name: 'Keep it', exact: true }).click();
        await expect(page.getByRole('heading', { level: 1, name: 'Old jar', exact: true })).toBeVisible();
        expect(new URL(page.url()).pathname).toBe(path);
        await expect(page.locator('p.place .tag')).toHaveCount(0);

        await page.getByRole('link', { name: 'Archive', exact: true }).click();
        expect(await send(page, page.getByRole('button', { name: 'Archive', exact: true }))).toBe(303);
        await expect(page.getByRole('heading', { level: 1, name: 'Old jar', exact: true })).toBeVisible();
        expect(new URL(page.url()).pathname).toBe(path);
        await expect(notice(page)).toHaveText('This account was archived.');
        await expect(page.locator('p.place .tag')).toHaveText(['archived']);
        await expect(page.getByText('This account is archived: it is hidden from the lists and takes no new entries.')).toBeVisible();
        await expect(page.locator('.doing .actions > a')).toHaveText(['Unarchive', 'Change name']);
        await expect(page.locator('.doing p.note')).toHaveCount(0);
        await page.getByRole('link', { name: 'Change name', exact: true }).click();
        await expect(page.getByRole('heading', { level: 1, name: 'Change name' })).toBeVisible();
        await expect(page.getByLabel('Name', { exact: true })).toHaveValue('Old jar');
        await expect(page.getByLabel('Inside', { exact: true })).toHaveCount(0);
        await expect(page.getByLabel('Owner', { exact: true })).toHaveCount(0);
        // An archived account's opening-balance form leads to its page (decision 140).
        await page.goto(`${path}/opening-balance`);
        await expect(page.getByRole('heading', { level: 1, name: 'Old jar', exact: true })).toBeVisible();
        expect(new URL(page.url()).pathname, "the archived account's opening-balance form leads to its page").toBe(path);

        expect(await archivedCount(page), 'the count beside Show archived risen by one').toBe(before + 1);
        await expect(line(sectionPanel(page, 'Assets'), 'Old jar'), 'an archived account hidden').toHaveCount(0);
        await page.getByRole('link', { name: `Show archived (${before + 1})`, exact: true }).click();
        expect(new URL(page.url()).search).toBe('?archived=shown');
        const shown = line(sectionPanel(page, 'Assets'), 'Old jar');
        await expect(shown.locator('.tag')).toHaveText(['archived']);
        await expect(shown.locator('.amount')).toHaveText('0.00');
        await expect(page.getByRole('link', { name: 'Hide archived', exact: true })).toHaveAttribute('href', '/accounts');

        await shown.click();
        await page.getByRole('link', { name: 'Unarchive', exact: true }).click();
        await expect(page.getByRole('heading', { level: 1, name: 'Unarchive Old jar?' })).toBeVisible();
        await expect(trail(page).locator('li')).toHaveText(['Accounts', 'Old jar', 'Unarchive']);
        await expect(page.getByText('It will show in the lists again and can take new entries.')).toBeVisible();
        await expect(page.getByRole('link', { name: 'Keep it archived', exact: true })).toHaveAttribute('href', path);
        expect(await send(page, page.getByRole('button', { name: 'Unarchive', exact: true }))).toBe(303);
        await expect(notice(page)).toHaveText('This account was unarchived.');
        await expect(page.locator('p.place .tag')).toHaveCount(0);
        await expect(page.locator('.doing .actions > a')).toHaveText([
            'Enter opening balance',
            'Change account',
            'New account inside Old jar',
            'Archive',
        ]);

        expect(await archivedCount(page), 'the count back where it was').toBe(before);
        await expect(line(sectionPanel(page, 'Assets'), 'Old jar')).toBeVisible();
    });
});

/**
 * An account that holds another one that is not archived is not archived: the confirmation's
 * Archive answers 422 with the account's page, which says at the top what was refused, why and
 * what to do, and still offers Archive, the account not marked archived.
 * Proves: Decision 136 with INV-12 in Chromium and in WebKit: an account holding another live one
 * is not archived and the page says why.
 */
test("an archive that is refused says why on the account's page", async ({ page }) => {
    const path = await create(page, 'assets', 'Tool box');
    await create(page, 'assets', 'Spanner', 'Assets:Tool box');
    await page.goto(`${path}/archive`);
    await expect(page.getByRole('heading', { level: 1, name: 'Archive Tool box?' })).toBeVisible();
    expect(await send(page, page.getByRole('button', { name: 'Archive', exact: true }))).toBe(422);
    await expect(page.getByRole('heading', { level: 1, name: 'Tool box', exact: true })).toBeVisible();
    await expect(notice(page)).toHaveText(
        'Assets:Tool box was not archived because Assets:Tool box:Spanner inside it is not archived. Archive that one first.',
    );
    await expect(notice(page)).toHaveAttribute('role', 'alert');
    await expect(page.locator('p.place')).toHaveText('Assets:Tool box');
    await expect(page.locator('p.place .tag')).toHaveCount(0);
    await expect(page.locator('.doing .actions > a')).toHaveText([
        'Enter opening balance',
        'Change account',
        'New account inside Tool box',
        'Archive',
    ]);
    await page.goto(path);
    await expect(page.getByRole('heading', { level: 1, name: 'Tool box', exact: true })).toBeVisible();
    await expect(notice(page), 'the refusal belongs to the answer that refused').toHaveCount(0);
});

/**
 * Opening Balances, a line of Equity on the list, opens a page that marks it built in, says what
 * Nage uses it for and that it always stays and may be renamed, and offers one action, Change name,
 * which opens a form that holds the name alone; nothing there archives it, and its archive address
 * is the page for an unknown address.
 * Proves: Decision 133 with INV-11 in Chromium and in WebKit: the built-in mark, its sentence,
 * Change name and no Archive.
 */
test('a built-in account says what Nage uses it for and offers only a change of name', async ({ page }) => {
    await page.goto('/accounts');
    await line(sectionPanel(page, 'Equity'), 'Opening Balances').click();
    await expect(page.getByRole('heading', { level: 1, name: 'Opening Balances', exact: true })).toBeVisible();
    const path = accountPath(page);
    await expect(page.locator('p.place > span')).toHaveText('Equity:Opening Balances');
    await expect(page.locator('p.place .tag')).toHaveText(['built in']);
    await expect(page.locator('p.about')).toHaveText([
        'Nage uses this account itself: every opening balance is entered against it. It always stays here and cannot be archived. You may rename it.',
    ]);
    await expect(page.locator('.doing .actions > a')).toHaveText(['Change name']);
    await expect(page.getByRole('link', { name: 'Archive', exact: true })).toHaveCount(0);
    await expect(page.locator('.doing p.note')).toHaveCount(0);
    await expect(page.locator('dl.facts')).toHaveCount(0);

    await page.getByRole('link', { name: 'Change name', exact: true }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'Change name' })).toBeVisible();
    await expect(trail(page).locator('li')).toHaveText(['Accounts', 'Opening Balances', 'Change']);
    await expect(page.getByLabel('Name', { exact: true })).toHaveValue('Opening Balances');
    await expect(page.getByLabel('Inside', { exact: true })).toHaveCount(0);
    await expect(page.getByRole('link', { name: 'Back to Opening Balances', exact: true })).toHaveAttribute('href', path);

    const response = await page.goto(`${path}/archive`);
    expect(response?.status()).toBe(404);
    await expect(page.getByRole('heading', { level: 1, name: 'Page not found' })).toBeVisible();
});

/**
 * At 1280 px each section's + New line stands on the heading's row, right of the heading and above
 * the section's lines, and a confirmation's Archive, the yellow main button, stands first: left of
 * Keep it, on the same row.
 * Proves: Decision 130 with P10 and P12 in Chromium: the computer's seen order of the list's New
 * lines and of a confirmation's buttons.
 */
test("on a computer a section's New line stands beside its heading and a confirmation's main button comes first", async ({ page }) => {
    expect(page.viewportSize(), "the computer's screen").toEqual({ width: 1280, height: 800 });
    const path = await create(page, 'expenses', 'Desk order');
    await page.goto('/accounts');
    for (const name of ['Assets', 'Liabilities', 'Income', 'Expenses']) {
        const panel = sectionPanel(page, name);
        const heading = (await panel.locator('h3').boundingBox()) ?? { x: 0, y: 0, width: 0, height: 0 };
        const add = (await panel.locator('a.add').boundingBox()) ?? { x: 0, y: 900, width: 0, height: 0 };
        const first = (await panel.locator('ul.rows > li').first().boundingBox()) ?? { x: 0, y: 0, width: 0, height: 0 };
        expect(add.x, `the New line of ${name} right of its heading`).toBeGreaterThanOrEqual(heading.x + heading.width);
        expect(
            add.y < heading.y + heading.height && heading.y < add.y + add.height,
            `the New line of ${name} on its heading's row`,
        ).toBe(true);
        expect(add.y + add.height, `the New line of ${name} above its lines`).toBeLessThanOrEqual(first.y + 1);
    }

    await page.goto(`${path}/archive`);
    await expect(page.getByRole('heading', { level: 1, name: 'Archive Desk order?' })).toBeVisible();
    const main = (await page.getByRole('button', { name: 'Archive', exact: true }).boundingBox()) ?? { x: 900, y: 0, width: 0, height: 0 };
    const keep = (await page.getByRole('link', { name: 'Keep it', exact: true }).boundingBox()) ?? { x: 0, y: 900, width: 0, height: 0 };
    expect(main.x + main.width, 'Archive left of Keep it').toBeLessThanOrEqual(keep.x);
    expect(Math.abs(main.y - keep.y), 'Archive and Keep it on one row').toBeLessThanOrEqual(2);
});
