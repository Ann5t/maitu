const { test, expect } = require('@playwright/test');
const { randomUUID } = require('node:crypto');

const baseURL = process.env.BASE_URL;
const screenshotDir = process.env.SCREENSHOT_DIR || '/work/docs/assets/screenshots';
const workerBootstrap = process.env.WORKER_BOOTSTRAP;

async function completeIndependentReview(page, reviewGateId) {
  const projectId = new URL(page.url()).pathname.match(/\/projects\/([0-9a-f-]+)/)[1];
  const workerId = randomUUID();
  const workerToken = `browser_review_worker_${workerId.replaceAll('-', '')}`;
  const leaseToken = `browser_review_lease_${workerId.replaceAll('-', '')}`;
  const registration = await page.request.post('/api/v1/scheduler/workers', {
    headers: { 'x-fudian-worker-bootstrap': workerBootstrap },
    data: {
      clientRequestId: randomUUID(),
      workerId,
      workerToken,
      displayName: `browser-independent-reviewer-${workerId}`,
      capabilities: ['review.goal_candidate.v1'],
    },
  });
  expect(registration.ok()).toBeTruthy();
  const claimResponse = await page.request.post('/api/v1/scheduler/claim', {
    data: {
      workerId,
      workerToken,
      clientRequestId: randomUUID(),
      leaseToken,
      softTtlSeconds: 120,
      hardTtlSeconds: 300,
    },
  });
  expect(claimResponse.ok()).toBeTruthy();
  const claim = await claimResponse.json();
  expect(claim.action.projectId).toBe(projectId);
  expect(claim.action.subjectId).toBe(reviewGateId);
  const payload = claim.action.payload;
  const completion = await page.request.post(
    `/api/v1/scheduler/action-runs/${claim.action.id}/complete`,
    {
      data: {
        workerId,
        workerToken,
        leaseId: claim.lease.id,
        leaseToken,
        fencingToken: claim.lease.fencingToken,
        result: {
          schemaVersion: 1,
          candidateDigest: payload.candidateDigest,
          contractVersionId: payload.contractVersionId,
          observedHeadCommit: payload.headCommit,
          observedTreeId: payload.treeId,
          observedWorkspaceSnapshot: payload.workspaceSnapshot,
          environmentFingerprint: payload.environmentFingerprint,
          decision: 'recommend_accept',
          rationale: '独立浏览器 Worker 复验契约与冻结证据后建议接受',
          contractCheck: { browserFlow: 'passed', frozenCandidate: 'passed' },
          counterexamples: [],
          retestEvidence: ['重跑 Chromium 关键流程'],
          isolation: {
            candidateReadOnly: true,
            noWorkspaceWrites: true,
            noNewPrivileges: true,
            dockerSocketAbsent: true,
            hostSecretsAbsent: true,
            effectiveCapabilitiesHex: '0000000000000000',
          },
        },
      },
    },
  );
  expect(completion.ok()).toBeTruthy();
}

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

async function openDetails(locator) {
  if (!(await locator.evaluate((element) => element.open))) {
    await locator.locator(':scope > summary').click();
  }
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
  await expect(page.locator('#goal-workbench')).toHaveAttribute('data-projection-version', 'goal-worksite-v2');

  const rootContract = page.locator('[data-goal-contract-form="root"]');
  await rootContract.locator('[name="why_needed"]').fill('证明工作台可在浏览器中完整推进');
  await rootContract.locator('[name="desired_outcome"]').fill('用表单、文件和审核闭环完成根目标');
  await rootContract.locator('[name="validation_plan"]').fill('Playwright 运行桌面与移动端闭环');
  await rootContract.locator('[name="stop_conditions"]').fill('用户在页面接受候选');
  await rootContract.locator('[name="unknowns"]').fill('最终布局仍需用户凭感觉调整');
  await rootContract.getByRole('button', { name: '建立 BranchProposal 草案' }).click();

  await openDetails(page.locator('.proposal-queue'));
  const proposal = page.locator('.proposal-card');
  await expect(proposal).toContainText('用表单、文件和审核闭环');
  await proposal.getByRole('button', { name: '提交审核' }).click();
  await openDetails(page.locator('.proposal-queue'));
  const approval = page.locator('form:has(input[value="proposal.approve"])');
  await approval.locator('[name="branch_name"]').fill('Chromium 根目标');
  await approval.locator('[name="assignment"]').fill('在真实页面操作文件、产出与审核');
  await approval.locator('[name="agent_identity"]').fill('browser-worker');
  await approval.getByRole('button', { name: '批准 BranchProposal' }).click();

  await expect(page.locator('.goal-session-node.is-selected')).toBeVisible();
  await expect(page.locator('[data-worksite-view="scene"]')).toHaveAttribute('aria-pressed', 'true');
  await expect(page.locator('[data-worksite-view="result"]')).toHaveAttribute('aria-pressed', 'false');
  await expect(page.locator('[data-worksite-group="detail"]').first()).toBeHidden();
  await expect(page.locator('.session-worksite')).toContainText('文件与代码');
  await expect(page.locator('.session-worksite')).toContainText('工具与浏览器');
  await expect(page.locator('.session-worksite')).toContainText('行动');
  await page.locator('[data-worksite-view="detail"]').click();
  await expect(page.locator('.plugin-worksite')).toBeVisible();
  await expect(page.locator('[data-worksite-view="detail"]')).toHaveAttribute('aria-pressed', 'true');
  await expect(page.locator('.worksite-context')).toBeVisible();
  await expect(page.locator('.worksite-context')).toContainText('永不折叠');
  await expect(page.locator('.worksite-context')).toContainText('按需披露');
  await expect(page.locator('.context-api-link')).toHaveAttribute('href', /\/context$/);

  const commandBar = page.locator('[data-goal-command-bar]');
  await expect(commandBar).toBeVisible();
  await commandBar.locator('[data-goal-search]').fill('Chromium 根目标');
  await expect(page.locator('[data-goal-lane]')).toBeVisible();
  await commandBar.locator('[data-goal-search]').fill('完全不存在的目标');
  await expect(page.locator('[data-goal-lane]')).toBeHidden();
  await expect(commandBar.locator('[data-goal-filter-result]')).toHaveText('0 / 1');
  await commandBar.locator('[data-goal-focus-current]').click();
  await expect(page.locator('.goal-session-node.is-selected')).toBeFocused();
  await page.keyboard.press('End');
  await expect(page.locator('.goal-session-node.is-selected')).toBeFocused();
  expect(await commandBar.locator('[data-goal-focus-current]').evaluate((element) => element.getBoundingClientRect().height)).toBeGreaterThanOrEqual(44);

  await openDetails(page.locator('.plugin-worksite'));
  const pluginRequest = page.locator('.plugin-request-form');
  await pluginRequest.locator(':scope > summary').click();
  await pluginRequest.locator('[name="plugin_id"]').fill('fudian.tools.pptmaster');
  await pluginRequest.locator('[name="version_requirement"]').fill('1.0.0');
  await pluginRequest.locator('[name="capability"]').fill('presentation.build');
  await pluginRequest.locator('[name="reason"]').fill('当前目标需要生成并验证演示文稿');
  await pluginRequest.getByRole('button', { name: '记录安装请求' }).click();
  await page.locator('[data-worksite-view="detail"]').click();
  await expect(page.locator('.plugin-request-list')).toContainText('fudian.tools.pptmaster');
  await expect(page.locator('.plugin-request-list')).toContainText('当前目标需要生成并验证演示文稿');

  await page.locator('[data-worksite-view="scene"]').click();
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

  await openDetails(page.locator('[data-session-actions]'));
  const contractRevisionPanel = page.locator('details.session-action').filter({
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
  await page.locator('[data-worksite-view="detail"]').click();
  await expect(page.locator('.worksite-contract')).toContainText('探索模式');

  await openDetails(page.locator('[data-session-actions]'));
  const structuredEvidencePanel = page.locator('details.session-action').filter({
    has: page.locator('input[value="session.add_evidence"]'),
  });
  await structuredEvidencePanel.locator(':scope > summary').click();
  const structuredEvidence = structuredEvidencePanel.locator('form');
  await structuredEvidence.locator('[name="evidence_kind"]').selectOption('browser');
  await structuredEvidence.locator('[name="evidence_stance"]').selectOption('supports');
  await structuredEvidence.locator('[name="claim"]').fill('工作台在真实 Chromium 可操作');
  await structuredEvidence.locator('[name="observation"]').fill('契约差异批准、暂停和恢复全部由页面完成');
  await structuredEvidence.locator('[name="verification_status"]').selectOption('verified');
  await structuredEvidence.getByRole('button', { name: '保存证据' }).click();
  await page.locator('[data-worksite-view="result"]').click();
  await expect(page.locator('.test-evidence-record')).toContainText('工作台在真实 Chromium 可操作');

  await openDetails(page.locator('[data-session-actions]'));
  const contributionPanel = page.locator('details.session-action').filter({
    has: page.locator('input[value="session.add_contribution"]'),
  });
  await openDetails(contributionPanel);
  const contribution = contributionPanel.locator('form');
  await contribution.locator('[name="contribution_kind"]').selectOption('evidence');
  await contribution.locator('[name="title"]').fill('Chromium 端到端证据');
  await contribution.locator('[name="body"]').fill('桌面与移动端工作台可操作，文件已安全导入');
  await contribution.getByRole('button', { name: '保存可回流产出' }).click();
  await page.locator('[data-worksite-view="result"]').click();
  await expect(page.locator('.contribution-stack')).toContainText('Chromium 端到端证据');

  expect(await renderedFontSize(page.locator('body'))).toBeGreaterThanOrEqual(16);
  expect(await renderedFontSize(page.locator('.goal-session-node__copy strong'))).toBeGreaterThanOrEqual(14);
  expect(await renderedFontSize(page.locator('.worksite-head h3'))).toBeGreaterThanOrEqual(16);
  expect(await renderedFontSize(page.locator('.worksite-disclosure__summary strong'))).toBeGreaterThanOrEqual(14);
  expect(await renderedFontSize(page.locator('.contribution-stack p'))).toBeGreaterThanOrEqual(14);
  expect(await renderedFontSize(page.locator('.worksite-meta span'))).toBeGreaterThanOrEqual(12);
  expect(await renderedFontSize(page.locator('.worksite-context > summary'))).toBeGreaterThanOrEqual(14);
  expect(await renderedFontSize(page.locator('.context-catalog-preview strong'))).toBeGreaterThanOrEqual(14);
  expect(await renderedContrast(page.locator('.contribution-stack p'))).toBeGreaterThanOrEqual(4.5);
  const undersizedText = await page.locator('#goal-workbench').evaluate((root) => [...root.querySelectorAll('*')]
    .filter((element) => element.getClientRects().length > 0 && !element.closest('[aria-hidden="true"]'))
    .filter((element) => [...element.childNodes].some((node) => node.nodeType === Node.TEXT_NODE && node.textContent.trim()))
    .map((element) => ({ text: element.textContent.trim().slice(0, 80), size: Number.parseFloat(getComputedStyle(element).fontSize) }))
    .filter((item) => item.size < 12));
  expect(undersizedText).toEqual([]);
  const undersizedCoreTargets = await page.locator('#goal-workbench').evaluate((root) => [
    ...root.querySelectorAll('button, summary, .worksite-head__controls a, .context-api-link, .worksite-record > a'),
  ].filter((element) => element.getClientRects().length > 0)
    .map((element) => ({ text: element.textContent.trim().slice(0, 50), height: element.getBoundingClientRect().height }))
    .filter((item) => item.height < 44));
  expect(undersizedCoreTargets).toEqual([]);
  await page.emulateMedia({ reducedMotion: 'reduce' });
  const reducedTransitionSeconds = await page.locator('.goal-session-node').first()
    .evaluate((element) => Number.parseFloat(getComputedStyle(element).transitionDuration));
  expect(reducedTransitionSeconds).toBeLessThanOrEqual(0.001);

  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1)).toBeTruthy();
  await page.evaluate(() => window.scrollTo(0, 0));
  await page.screenshot({ path: `${screenshotDir}/goal-workbench-desktop.png`, fullPage: true });

  await openDetails(page.locator('[data-session-actions]'));
  const mergePanel = page.locator('details.session-action').filter({
    has: page.locator('input[value="merge.propose"]'),
  });
  await openDetails(mergePanel);
  const merge = mergePanel.locator('form');
  await merge.locator('[name="test_evidence"]').fill('Chromium 桌面布局无溢出\n文件导入成功');
  await merge.locator('[name="self_check"]').fill('已核对目标、验证、未知与停止条件');
  await merge.getByRole('button', { name: '冻结现场并进入拟合并审核' }).click();
  await expect(page.locator('.review-evidence-grid')).toContainText('结构化证据');
  await expect(page.locator('.review-evidence-grid')).toContainText('工作台在真实 Chromium 可操作');

  const reviewGateId = await page.locator('[data-review-gate-id]').getAttribute('data-review-gate-id');
  await completeIndependentReview(page, reviewGateId);
  await page.reload({ waitUntil: 'networkidle' });

  const humanAccept = page.locator('form:has(input[value="accept"])');
  await humanAccept.locator('[name="rationale"]').fill('用户确认这条目标枝干已达成');
  await humanAccept.getByRole('button', { name: '接受整条枝干产出' }).click();
  await expect(page.locator('.review-decision-record').filter({ hasText: '你的最终决定' })).toBeVisible();

  await page.setViewportSize({ width: 820, height: 1180 });
  await page.reload({ waitUntil: 'networkidle' });
  await expect(page.locator('.goal-session-node.is-selected')).toBeVisible();
  await expect(page.locator('.session-worksite')).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1)).toBeTruthy();
  await page.screenshot({ path: `${screenshotDir}/goal-workbench-tablet.png`, fullPage: true });

  await page.setViewportSize({ width: 390, height: 844 });
  await page.reload({ waitUntil: 'networkidle' });
  await expect(page.locator('.goal-session-node.is-selected')).toBeVisible();
  await expect(page.locator('.session-worksite')).toBeVisible();
  await expect(page.locator('.worksite-context')).toContainText('继承上下文');
  expect(await renderedFontSize(page.locator('.goal-session-node__copy strong'))).toBeGreaterThanOrEqual(14);
  expect(await renderedFontSize(page.locator('.worksite-head h3'))).toBeGreaterThanOrEqual(16);
  expect(await renderedFontSize(page.locator('.mobile-nav b'))).toBeGreaterThanOrEqual(12);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1)).toBeTruthy();
  const workbenchBox = await page.locator('#goal-workbench').boundingBox();
  expect(workbenchBox.x).toBeGreaterThanOrEqual(0);
  expect(workbenchBox.x + workbenchBox.width).toBeLessThanOrEqual(390);
  expect(await page.evaluate(() => document.documentElement.scrollHeight)).toBeLessThan(3200);
  await page.screenshot({ path: `${screenshotDir}/goal-workbench-mobile.png`, fullPage: true });
});

test('settings reports real AI status and persists explicit theme choice', async ({ page }) => {
  await page.goto('/settings');
  await expect(page.getByRole('heading', { name: '设置', level: 1 })).toBeVisible();
  await expect(page.getByText('AI 服务')).toBeVisible();
  await expect(page.getByText('未连接')).toBeVisible();
  await expect(page.locator('body')).not.toContainText('Rust edition');
  await expect(page.locator('body')).not.toContainText('可恢复单体');
  await expect(page.locator('body')).not.toContainText('当前没有真实 AI 在运行');
  expect(await renderedFontSize(page.locator('.settings-state'))).toBeGreaterThanOrEqual(12);
  expect(await renderedFontSize(page.locator('.settings-row strong'))).toBeGreaterThanOrEqual(14);

  const appearance = page.locator('#appearance');
  await appearance.locator('[data-theme-set="dark"]').click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  await expect(appearance.locator('[data-theme-set="dark"]')).toHaveAttribute('aria-pressed', 'true');
  await expect(page.locator('.theme-switch [data-theme-set="dark"]')).toHaveAttribute('aria-pressed', 'true');

  await page.reload();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  expect(await page.evaluate(() => localStorage.getItem('fudian-theme'))).toBe('dark');
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1)).toBeTruthy();
  await page.screenshot({ path: `${screenshotDir}/settings-desktop-dark.png`, fullPage: true });

  await page.setViewportSize({ width: 390, height: 844 });
  await page.reload();
  await expect(page.locator('.mobile-nav a[href="/settings"]')).toBeVisible();
  await expect(page.locator('.mobile-nav a[href="/settings"]')).toHaveClass(/is-active/);
  await appearance.locator('[data-theme-set="light"]').click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
  await page.waitForTimeout(220);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1)).toBeTruthy();
  expect(await page.evaluate(() => document.documentElement.scrollHeight)).toBeLessThan(1200);
  await page.screenshot({ path: `${screenshotDir}/settings-mobile-light.png`, fullPage: true });
});

test('project dashboard keeps the current work visually primary', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByRole('heading', { name: '项目', level: 1 })).toBeVisible();
  await expect(page.locator('.side-nav a[href="/?view=artifacts"]')).toHaveCount(0);
  await expect(page.locator('body')).not.toContainText('WORKSPACE');
  await expect(page.locator('body')).not.toContainText('RECENT');
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1)).toBeTruthy();
  await page.screenshot({ path: `${screenshotDir}/projects-dashboard-desktop.png`, fullPage: true });

  await page.setViewportSize({ width: 390, height: 844 });
  await page.reload({ waitUntil: 'networkidle' });
  await expect(page.locator('.mobile-nav')).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1)).toBeTruthy();
  await page.screenshot({ path: `${screenshotDir}/projects-dashboard-mobile.png`, fullPage: true });
});
