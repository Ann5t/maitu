const { test, expect } = require('@playwright/test');

const baseURL = process.env.BASE_URL;
const screenshotDir = process.env.SCREENSHOT_DIR || '/work/docs/screenshots';

test.use({
  baseURL,
  viewport: { width: 1440, height: 1000 },
  colorScheme: 'light',
});

async function noHorizontalOverflow(page) {
  return page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1);
}

test('idea revisions, relationships and ProjectProposal promotion work on three sizes', async ({ page }) => {
  await page.goto('/ideas/new');
  await page.getByLabel('标题（可留空）').fill('科研目标枝干');
  await page.getByLabel('现在想到什么？').fill('把科研目标展开成可审查的独立枝干，并保留未知与反例。');
  await page.getByRole('button', { name: '保留这个想法' }).click();
  const firstIdeaURL = page.url();
  await expect(page.getByRole('heading', { name: '科研目标枝干' })).toBeVisible();

  const revision = page.locator('.idea-revision-form');
  await revision.getByLabel('内容').fill('把科研目标展开成可审查的独立枝干，并在移动设备查看工作现场。');
  await revision.getByLabel('这次为什么改变？').fill('补充移动工作现场');
  await revision.getByRole('button', { name: '保存为 v2' }).click();
  await expect(page.locator('.idea-version-list')).toContainText('v2');

  await page.goto('/ideas/new');
  await page.getByLabel('标题（可留空）').fill('证据驱动审核');
  await page.getByLabel('现在想到什么？').fill('每次拟合并都冻结证据并由独立审核检查反例。');
  await page.getByRole('button', { name: '保留这个想法' }).click();
  await expect(page.getByRole('heading', { name: '证据驱动审核' })).toBeVisible();

  await page.goto(firstIdeaURL);
  const relatedIdeaValue = await page
    .getByLabel('关联到')
    .locator('option')
    .filter({ hasText: '证据驱动审核' })
    .getAttribute('value');
  await page.getByLabel('关联到').selectOption(relatedIdeaValue);
  await page.getByLabel('关系').selectOption('supports');
  await page.getByLabel('为什么这样关联？').fill('独立审核为目标枝干提供完成证据');
  await page.getByRole('button', { name: '保存关系' }).click();
  await expect(page.locator('.idea-link-card')).toContainText('证据驱动审核');

  const proposal = page.locator('.project-proposal-form').first();
  await proposal.getByLabel('项目名称').fill('目标枝干科研工作台');
  await proposal.getByLabel('项目意图').fill('开发可在电脑、手机和平板使用的目标枝干科研工作台');
  await proposal.getByLabel('为什么现在值得立项？').fill('核心语义已经足以做一个可运行闭环');
  await proposal.getByLabel('第一条根目标想得到什么结果？').fill('完成可运行并可审查的目标枝干工作台');
  await proposal.getByLabel('怎样验证（每行一项）').fill('运行隔离 HTTP 与 Chromium 测试');
  await proposal.getByLabel('何时完成或停止（每行一项）').fill('用户接受候选或明确停止');
  await proposal.getByLabel('仍然不知道什么？').fill('最终图布局手感');
  await proposal.getByLabel('何时回来请你凭感觉判断？').fill('完成三尺寸可操作候选后');
  await proposal.getByRole('button', { name: '建立 ProjectProposal 草案' }).click();

  const proposalCard = page.locator('.project-proposal-card');
  await expect(proposalCard).toContainText('目标枝干科研工作台');
  await proposalCard.getByRole('button', { name: '提交立项审核' }).click();
  await expect(proposalCard).toContainText('等待你批准');
  await proposalCard.getByRole('button', { name: '批准并创建项目' }).click();

  await expect(page.locator('#goal-workbench')).toBeVisible();
  await expect(page.locator('.proposal-card')).toContainText('完成可运行并可审查的目标枝干工作台');
  expect(await noHorizontalOverflow(page)).toBeTruthy();

  await page.goto('/ideas?view=map');
  await expect(page.locator('.idea-map')).toBeVisible();
  await expect(page.locator('.idea-map__relations')).toContainText('独立审核为目标枝干提供完成证据');
  expect(await noHorizontalOverflow(page)).toBeTruthy();
  await page.screenshot({ path: `${screenshotDir}/ideas-map-desktop.png`, fullPage: true });

  await page.setViewportSize({ width: 820, height: 1180 });
  await page.reload({ waitUntil: 'networkidle' });
  await expect(page.locator('.idea-map')).toBeVisible();
  expect(await noHorizontalOverflow(page)).toBeTruthy();
  await page.screenshot({ path: `${screenshotDir}/ideas-map-tablet.png`, fullPage: true });

  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto(firstIdeaURL, { waitUntil: 'networkidle' });
  await expect(page.locator('.idea-detail-head')).toBeVisible();
  await expect(page.locator('.mobile-nav')).toContainText('想法');
  expect(await noHorizontalOverflow(page)).toBeTruthy();
  await page.screenshot({ path: `${screenshotDir}/idea-detail-mobile.png`, fullPage: true });
});
