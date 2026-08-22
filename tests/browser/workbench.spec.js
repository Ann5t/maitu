const { test, expect } = require('@playwright/test');

const baseURL = process.env.BASE_URL;
const screenshotDir = process.env.SCREENSHOT_DIR || '/work/docs/screenshots';

async function renderedFontSize(locator) {
  return locator.first().evaluate((element) => Number.parseFloat(getComputedStyle(element).fontSize));
}

async function renderedContrast(locator) {
  return locator.first().evaluate((element) => {
    const channels = (value) => value.match(/[\d.]+/g).slice(0, 3).map(Number);
    const luminance = (value) => {
      const normalized = channels(value).map((channel) => {
        const ratio = channel / 255;
        return ratio <= 0.04045 ? ratio / 12.92 : ((ratio + 0.055) / 1.055) ** 2.4;
      });
      return 0.2126 * normalized[0] + 0.7152 * normalized[1] + 0.0722 * normalized[2];
    };
    let backgroundElement = element;
    let background = getComputedStyle(backgroundElement).backgroundColor;
    while (backgroundElement.parentElement && background.endsWith(', 0)')) {
      backgroundElement = backgroundElement.parentElement;
      background = getComputedStyle(backgroundElement).backgroundColor;
    }
    const foregroundLuminance = luminance(getComputedStyle(element).color);
    const backgroundLuminance = luminance(background);
    const lighter = Math.max(foregroundLuminance, backgroundLuminance);
    const darker = Math.min(foregroundLuminance, backgroundLuminance);
    return (lighter + 0.05) / (darker + 0.05);
  });
}

test.use({
  baseURL,
  viewport: { width: 1440, height: 1000 },
  colorScheme: 'light',
});

test('goal branch workbench is operable on desktop and mobile', async ({ page }) => {
  test.setTimeout(60_000);
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

  const contractRevisionPanel = page.locator('details').filter({
    has: page.locator('input[value="contract.propose_revision"]'),
  });
  await contractRevisionPanel.locator(':scope > summary').click();
  const contractRevision = contractRevisionPanel.locator('form');
  await contractRevision.locator('[name="reason"]').fill('把结构化 Evidence 纳入浏览器验收');
  await contractRevision.locator('[name="validation_plan"]').fill(
    'Playwright 运行桌面与移动端闭环\n记录并冻结结构化 Evidence',
  );
  const contractAdvanced = contractRevision.locator('details.goal-form-advanced');
  await contractAdvanced.locator(':scope > summary').click();
  await contractRevision.locator('[name="exploration_mode"]').selectOption('exploration');
  await contractRevision.locator('[name="exploration_budgets"]').fill('最多验证两个候选后请用户判断');
  await contractRevision.locator('[name="exploration_candidates"]').fill('桌面与手机可操作候选');
  await contractRevision.locator('[name="uncertainty_reduction"]').fill('用户能排除至少一种不合适的交互');
  await contractRevision.getByRole('button', { name: '生成差异，等待决定' }).click();
  const contractReview = page.locator('[data-contract-revision-id]');
  await expect(contractReview).toContainText('验证计划');
  await expect(contractReview).toContainText('探索契约');
  const contractAccept = contractReview.locator('form:has(input[value="contract.accept_revision"])');
  await contractAccept.locator('[name="rationale"]').fill('差异明确且没有降低原验收');
  await contractAccept.getByRole('button', { name: '接受并安全暂停' }).click();
  const resume = page.locator('form:has(input[value="session.resume"])');
  await resume.locator('[name="resolution"]').fill('已阅读契约差异，继续浏览器验收');
  await resume.getByRole('button', { name: '恢复 Session' }).click();
  await expect(page.locator('.worksite-contract')).toContainText('探索模式');

  const structuredEvidencePanel = page.locator('details').filter({
    has: page.locator('input[value="session.add_evidence"]'),
  });
  await structuredEvidencePanel.locator(':scope > summary').click();
  const structuredEvidence = structuredEvidencePanel.locator('form');
  await structuredEvidence.locator('[name="evidence_kind"]').selectOption('browser');
  await structuredEvidence.locator('[name="evidence_stance"]').selectOption('supports');
  await structuredEvidence.locator('[name="claim"]').fill('工作台在真实 Chromium 可操作');
  await structuredEvidence.locator('[name="observation"]').fill('契约差异批准、暂停和恢复全部由页面完成');
  await structuredEvidence.locator('[name="verification_status"]').selectOption('verified');
  await structuredEvidence.getByRole('button', { name: '保存 Evidence' }).click();
  await expect(page.locator('.test-evidence-record')).toContainText('工作台在真实 Chromium 可操作');

  const contribution = page.locator('form:has(input[value="session.add_contribution"])');
  await contribution.locator('[name="contribution_kind"]').selectOption('evidence');
  await contribution.locator('[name="title"]').fill('Chromium 端到端证据');
  await contribution.locator('[name="body"]').fill('桌面与移动端工作台可操作，文件已安全导入');
  await contribution.getByRole('button', { name: '保存可回流产出' }).click();
  await expect(page.locator('.contribution-stack')).toContainText('Chromium 端到端证据');

  expect(await renderedFontSize(page.locator('body'))).toBeGreaterThanOrEqual(16);
  expect(await renderedFontSize(page.locator('.goal-toolbar p'))).toBeGreaterThanOrEqual(14);
  expect(await renderedFontSize(page.locator('.goal-session-node__copy strong'))).toBeGreaterThanOrEqual(14);
  expect(await renderedFontSize(page.locator('.worksite-assignment'))).toBeGreaterThanOrEqual(16);
  expect(await renderedFontSize(page.locator('.worksite-section > h4'))).toBeGreaterThanOrEqual(16);
  expect(await renderedFontSize(page.locator('.contribution-stack p'))).toBeGreaterThanOrEqual(14);
  expect(await renderedFontSize(page.locator('.worksite-meta span'))).toBeGreaterThanOrEqual(12);
  expect(await renderedContrast(page.locator('.contribution-stack p'))).toBeGreaterThanOrEqual(4.5);

  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1)).toBeTruthy();
  await page.screenshot({ path: `${screenshotDir}/goal-workbench-desktop.png`, fullPage: true });

  const merge = page.locator('form:has(input[value="merge.propose"])');
  await merge.locator('[name="test_evidence"]').fill('Chromium 桌面布局无溢出\n文件导入成功');
  await merge.locator('[name="self_check"]').fill('已核对目标、验证、未知与停止条件');
  await merge.getByRole('button', { name: '冻结现场并进入拟合并审核' }).click();
  await expect(page.locator('.review-evidence-grid')).toContainText('结构化 Evidence');
  await expect(page.locator('.review-evidence-grid')).toContainText('工作台在真实 Chromium 可操作');

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
  expect(await renderedFontSize(page.locator('.goal-session-node__copy strong'))).toBeGreaterThanOrEqual(14);
  expect(await renderedFontSize(page.locator('.worksite-assignment'))).toBeGreaterThanOrEqual(16);
  expect(await renderedFontSize(page.locator('.mobile-nav b'))).toBeGreaterThanOrEqual(12);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1)).toBeTruthy();
  const workbenchBox = await page.locator('#goal-workbench').boundingBox();
  expect(workbenchBox.x).toBeGreaterThanOrEqual(0);
  expect(workbenchBox.x + workbenchBox.width).toBeLessThanOrEqual(390);
  await page.screenshot({ path: `${screenshotDir}/goal-workbench-mobile.png`, fullPage: true });
});
