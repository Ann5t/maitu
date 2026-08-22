const { test, expect } = require('@playwright/test');
const path = require('path');

const baseURL = process.env.BASE_URL;
const username = process.env.FUDIAN_TEST_USERNAME;
const password = process.env.FUDIAN_TEST_PASSWORD;
const projectId = process.env.FUDIAN_TEST_PROJECT_ID;
const screenshotDir = process.env.SCREENSHOT_DIR;

const sizes = [
  ['desktop', { width: 1440, height: 1000 }],
  ['tablet', { width: 820, height: 1180 }],
  ['mobile', { width: 390, height: 844 }],
];

for (const [name, viewport] of sizes) {
  test(`private HTTPS login and workbench at ${name}`, async ({ browser }) => {
    const context = await browser.newContext({
      baseURL,
      viewport,
      ignoreHTTPSErrors: true,
    });
    const page = await context.newPage();
    const insecureRequests = [];
    page.on('request', request => {
      const url = request.url();
      if (url.startsWith('http://')) insecureRequests.push(url);
    });
    await page.goto('/auth/login');
    await expect(page.locator('h1')).toContainText('登录 Fudian');
    await page.locator('input[name="username"]').fill(username);
    await page.locator('input[name="password"]').fill(password);
    await Promise.all([
      page.waitForURL(url => url.pathname === '/'),
      page.locator('button[type="submit"]').click(),
    ]);
    await expect(page.locator('body')).toContainText('最近项目');
    const cookies = await context.cookies();
    const session = cookies.find(cookie => cookie.name === '__Host-fudian_session');
    const csrf = cookies.find(cookie => cookie.name === '__Host-fudian_csrf');
    expect(session).toMatchObject({ secure: true, httpOnly: true, sameSite: 'Strict', path: '/' });
    expect(csrf).toMatchObject({ secure: true, httpOnly: false, sameSite: 'Strict', path: '/' });
    await page.goto(`/projects/${projectId}?view=graph`);
    await expect(page.locator('#goal-workbench')).toBeVisible();
    const overflow = await page.evaluate(() => ({
      scroll: document.documentElement.scrollWidth,
      viewport: window.innerWidth,
    }));
    expect(overflow.scroll).toBeLessThanOrEqual(overflow.viewport + 1);
    expect(insecureRequests).toEqual([]);
    await page.screenshot({
      path: path.join(screenshotDir, `private-${name}.png`),
      fullPage: true,
    });
    await context.close();
  });
}
