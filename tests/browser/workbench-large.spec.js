const { test, expect } = require('@playwright/test');

const baseURL = process.env.BASE_URL;
const projectId = process.env.PROJECT_ID;
const screenshotDir = process.env.SCREENSHOT_DIR || '/tmp';

test.use({
  baseURL,
  viewport: { width: 1440, height: 1000 },
  colorScheme: 'light',
});

test('100 branches and 300 sessions remain searchable, keyboard operable and responsive', async ({ page }) => {
  test.setTimeout(60_000);
  await page.goto(`/projects/${projectId}?tab=goals`, { waitUntil: 'domcontentloaded' });

  const navigation = await page.evaluate(() => {
    const entry = performance.getEntriesByType('navigation')[0];
    return {
      domContentLoadedMs: entry.domContentLoadedEventEnd - entry.startTime,
      responseStartMs: entry.responseStart - entry.startTime,
    };
  });
  expect(navigation.domContentLoadedMs).toBeLessThanOrEqual(2500);
  await expect(page.locator('[data-goal-lane]')).toHaveCount(100);
  await expect(page.locator('.goal-session-node')).toHaveCount(300);
  await expect(page.locator('.proposal-card')).toHaveCount(20);

  const commandBar = page.locator('[data-goal-command-bar]');
  await commandBar.locator('[data-goal-filter-menu] > summary').click();
  await commandBar.locator('[data-goal-filter="attention"]').click();
  const attentionCount = await page.locator('[data-goal-lane]:visible').count();
  expect(attentionCount).toBeGreaterThan(0);
  expect(attentionCount).toBeLessThan(100);

  await commandBar.locator('[data-goal-filter-menu] > summary').click();
  await commandBar.locator('[data-goal-filter="notification"]').click();
  const notificationCount = await page.locator('[data-goal-lane]:visible').count();
  expect(notificationCount).toBe(7);

  await commandBar.locator('[data-goal-filter-menu] > summary').click();
  await commandBar.locator('[data-goal-filter="all"]').click();
  const filterMs = await page.evaluate(async () => {
    const input = document.querySelector('[data-goal-search]');
    const started = performance.now();
    input.value = '目标 099';
    input.dispatchEvent(new Event('input', { bubbles: true }));
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    return performance.now() - started;
  });
  expect(filterMs).toBeLessThanOrEqual(100);
  await expect(page.locator('[data-goal-lane]:visible')).toHaveCount(1);
  await expect(commandBar.locator('[data-goal-filter-result]')).toHaveText('1 / 100');

  await commandBar.locator('[data-goal-focus-current]').click();
  await expect(page.locator('.goal-session-node.is-selected')).toBeFocused();
  await page.keyboard.press('End');
  const focusedSessionId = await page.evaluate(() => document.activeElement?.dataset.sessionId);
  expect(focusedSessionId).toBeTruthy();
  await Promise.all([
    page.waitForURL(new RegExp(`session=${focusedSessionId}`)),
    page.keyboard.press('Enter'),
  ]);
  await expect(page.locator(`.goal-session-node[data-session-id="${focusedSessionId}"]`)).toHaveAttribute('aria-current', 'page');

  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1)).toBeTruthy();
  await page.screenshot({ path: `${screenshotDir}/goal-workbench-large-desktop.png` });

  await page.setViewportSize({ width: 390, height: 844 });
  await page.reload({ waitUntil: 'domcontentloaded' });
  await expect(page.locator('[data-goal-lane]')).toHaveCount(100);
  await expect(page.locator('.goal-session-node')).toHaveCount(300);
  const mobileMapHeight = await page.locator('.goal-map').evaluate((element) => element.getBoundingClientRect().height);
  expect(mobileMapHeight).toBeLessThanOrEqual(480);
  await page.locator('.session-worksite').scrollIntoViewIfNeeded();
  await expect(page.locator('.session-worksite')).toBeInViewport();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1)).toBeTruthy();
  const touchHeight = await page.locator('[data-goal-focus-current]').evaluate((element) => element.getBoundingClientRect().height);
  expect(touchHeight).toBeGreaterThanOrEqual(44);
  await page.screenshot({ path: `${screenshotDir}/goal-workbench-large-mobile.png` });

  console.log(`BP08_LARGE_METRICS ${JSON.stringify({ ...navigation, filterMs, attentionCount, notificationCount })}`);
});
