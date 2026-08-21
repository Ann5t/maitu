const { test, expect } = require('@playwright/test');

const baseURL = process.env.BASE_URL;
const screenshotDir = process.env.SCREENSHOT_DIR || '/work/docs/screenshots';

test.use({
  baseURL,
  viewport: { width: 1440, height: 1000 },
  colorScheme: 'light',
});

test('goal branch workbench is operable on desktop and mobile', async ({ page }) => {
  await page.goto('/new');
  await page.getByLabel('项目意图').fill('用 Chromium 验证目标枝干工作台');
  await page.getByRole('button', { name: '形成项目起点' }).click();
  await expect(page.locator('#goal-workbench')).toBeVisible();
  await expect(page.locator('#goal-workbench')).toHaveAttribute('data-projection-version', 'goal-lanes-v1');

  const rootContract = page.locator('[data-goal-contract-form="root"]');
  await rootContract.locator('[name="why_needed"]').fill('证明工作台可在浏览器中完整推进');
  await rootContract.locator('[name="desired_outcome"]').fill('用表单、文件和审核闭环完成根目标');
  await rootContract.locator('[name="validation_plan"]').fill('Playwright 运行桌面与移动端闭环');
  await rootContract.locator('[name="stop_conditions"]').fill('用户在页面接受候选');
  await rootContract.locator('[name="unknowns"]').fill('最终布局仍需用户凭感觉调整');
  await rootContract.getByRole('button', { name: '建立 BranchProposal 草案' }).click();

  const proposal = page.locator('.proposal-card');
  await expect(proposal).toContainText('用表单、文件和审核闭环');
  await proposal.getByRole('button', { name: '提交审核' }).click();
  const approval = page.locator('form:has(input[value="proposal.approve"])');
  await approval.locator('[name="branch_name"]').fill('Chromium 根目标');
  await approval.locator('[name="assignment"]').fill('在真实页面操作文件、产出与审核');
  await approval.locator('[name="agent_identity"]').fill('browser-worker');
  await approval.getByRole('button', { name: '批准 BranchProposal' }).click();

  await expect(page.locator('.goal-session-node.is-selected')).toBeVisible();
  await expect(page.locator('.session-worksite')).toContainText('FILES / ARTIFACTS');
  await expect(page.locator('.session-worksite')).toContainText('TOOLS / BROWSER / TESTS');

  const upload = page.locator('[data-input-upload]');
  await upload.locator('input[type="file"]').setInputFiles({
    name: 'browser-evidence.md',
    mimeType: 'text/markdown',
    buffer: Buffer.from('# Chromium evidence\nmobile and desktop verified\n'),
  });
  await upload.locator('[name="inbox_relative_path"]').fill('evidence/browser-evidence.md');
  await upload.getByRole('button', { name: '验证并导入' }).click();
  await expect(upload.locator('[data-upload-status]')).toHaveAttribute('data-state', 'success', { timeout: 15_000 });
  await page.waitForLoadState('networkidle');
  await expect(page.locator('.worksite-record')).toContainText('browser-evidence.md');

  const contribution = page.locator('form:has(input[value="session.add_contribution"])');
  await contribution.locator('[name="contribution_kind"]').selectOption('evidence');
  await contribution.locator('[name="title"]').fill('Chromium 端到端证据');
  await contribution.locator('[name="body"]').fill('桌面与移动端工作台可操作，文件已安全导入');
  await contribution.getByRole('button', { name: '保存可回流产出' }).click();
  await expect(page.locator('.contribution-stack')).toContainText('Chromium 端到端证据');

  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1)).toBeTruthy();
  await page.screenshot({ path: `${screenshotDir}/goal-workbench-desktop.png`, fullPage: true });

  const merge = page.locator('form:has(input[value="merge.propose"])');
  await merge.locator('[name="test_evidence"]').fill('Chromium 桌面布局无溢出\n文件导入成功');
  await merge.locator('[name="self_check"]').fill('已核对目标、验证、未知与停止条件');
  await merge.getByRole('button', { name: '冻结现场并进入拟合并审核' }).click();

  const aiReview = page.locator('form:has(input[value="review.ai_record"])');
  await aiReview.locator('[name="reviewer_identity"]').fill('browser-reviewer');
  await aiReview.locator('[name="rationale"]').fill('契约和浏览器证据完整，建议接受');
  await aiReview.locator('[name="test_evidence"]').fill('重跑 Chromium 关键流程');
  await aiReview.getByRole('button', { name: '保存独立审核' }).click();

  const humanAccept = page.locator('form:has(input[value="accept"])');
  await humanAccept.locator('[name="rationale"]').fill('用户确认这条目标枝干已达成');
  await humanAccept.getByRole('button', { name: '接受整条枝干产出' }).click();
  await expect(page.locator('.review-decision-record').filter({ hasText: '你的最终决定' })).toBeVisible();

  await page.setViewportSize({ width: 390, height: 844 });
  await page.reload({ waitUntil: 'networkidle' });
  await expect(page.locator('.goal-session-node.is-selected')).toBeVisible();
  await expect(page.locator('.session-worksite')).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1)).toBeTruthy();
  const workbenchBox = await page.locator('#goal-workbench').boundingBox();
  expect(workbenchBox.x).toBeGreaterThanOrEqual(0);
  expect(workbenchBox.x + workbenchBox.width).toBeLessThanOrEqual(390);
  await page.screenshot({ path: `${screenshotDir}/goal-workbench-mobile.png`, fullPage: true });
});
