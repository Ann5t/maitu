const { test, expect } = require('@playwright/test');

test.use({ baseURL: process.env.BASE_URL || 'http://127.0.0.1:3000' });
const screenshotDir = process.env.SCREENSHOT_DIR || '/screenshots';

test('Maitu graph persists real tasks, explains waiting and retains failed attempts', async ({ page }) => {
  test.setTimeout(90_000);
  const errors = [];
  const serverErrors = [];
  page.on('pageerror', error => errors.push(error.message));
  page.on('response', response => { if (response.status() >= 500) serverErrors.push(response.url()); });
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.goto('/');
  await expect(page.locator('.maitu-brand')).toContainText('脉图');
  await page.locator('#maitu-project-create [name="intent"]').fill('浏览器验收：从资料到并行任务，查看进度和失败记录');
  await page.locator('#maitu-project-create button[type="submit"]').click();
  await expect(page).toHaveURL(/\/maitu\/projects\/[a-f0-9-]+$/);
  const graphUrl = page.url();
  const projectId = graphUrl.split('/').pop();
  await expect(page.locator('#maitu-project-status')).toContainText('0 个任务');

  await page.locator('#maitu-add-source').click();
  await page.locator('#maitu-source-files').setInputFiles({
    name: '需求.md', mimeType: 'text/markdown', buffer: Buffer.from('# 实际输入\n希望多个任务并行执行，并保留历史。'),
  });
  await expect(page.locator('#maitu-source-list')).toContainText('需求.md');
  await page.locator('#maitu-source-list').getByRole('button', { name: '查看', exact: true }).click();
  await expect(page.locator('#maitu-output-content')).toContainText('# 实际输入');
  await page.getByRole('button', { name: '关闭内容' }).click();

  for (const title of ['整理需求', '分析风险', '拟定计划']) {
    await page.locator('#maitu-new-task').click();
    await expect(page.locator('#maitu-task-dialog')).toBeVisible();
    const form = page.locator('#maitu-task-create');
    await form.locator('[name="title"]').fill(title);
    await form.locator('[name="instruction"]').fill(`依据资料${title}，保存成文件。`);
    await form.locator('[name="outputFilename"]').fill(`${title}.md`);
    await form.getByRole('button', { name: '加入任务图' }).click();
    await expect(page.locator('.maitu-node').filter({ hasText: title })).toBeVisible();
  }
  await page.locator('#maitu-start-ready').click();
  await expect(page.locator('.maitu-node--queued')).toHaveCount(3);
  await expect(page.locator('#maitu-map')).toContainText('请先配置 DeepSeek 连接');
  const snapshot = await (await page.request.get(`/api/maitu/projects/${projectId}`)).json();
  expect(snapshot.tasks).toHaveLength(3);
  expect(snapshot.sources).toHaveLength(1);
  expect(snapshot.provider.configured).toBe(false);
  await page.reload();
  await expect(page.locator('.maitu-node--queued')).toHaveCount(3);
  await page.screenshot({ path: `${screenshotDir}/maitu-graph-desktop.png`, fullPage: true });

  // A local closed port verifies the real network-failure path without calling a paid API.
  await page.goto('/maitu/settings');
  const configForm = page.locator('#maitu-provider-form');
  await expect(page.locator('#maitu-provider-state')).toContainText('等待填写密钥');
  await configForm.locator('[name="baseUrl"]').fill('http://127.0.0.1:1');
  await configForm.locator('[name="apiKey"]').fill('isolated-browser-fixture-key');
  await configForm.getByRole('button', { name: '保存连接' }).click();
  await expect(page.locator('#maitu-provider-state')).toContainText('已配置');
  await expect(configForm.locator('[name="apiKey"]')).toHaveValue('');
  const providerView = await (await page.request.get('/api/maitu/provider')).json();
  expect(JSON.stringify(providerView)).not.toContain('isolated-browser-fixture-key');
  expect(providerView).not.toHaveProperty('apiKey');
  await page.goto(graphUrl);
  await expect(page.locator('.maitu-node--failed')).toHaveCount(3);
  const firstNode = page.locator('.maitu-node').filter({ hasText: '整理需求' });
  await firstNode.getByRole('button', { name: '记录与成果' }).click();
  await expect(page.locator('#maitu-detail')).toContainText('无法连接模型服务');
  await firstNode.getByRole('button', { name: '重试', exact: true }).click();
  await expect(page.locator('#maitu-detail .maitu-attempt')).toHaveCount(2);
  await expect(page.locator('#maitu-detail .maitu-attempt').first()).toContainText('执行失败');
  await page.locator('#maitu-detail').getByRole('button', { name: '查看本次输入' }).first().click();
  await expect(page.locator('#maitu-output-content')).toContainText('实际输入');
  await page.getByRole('button', { name: '关闭内容' }).click();

  await page.locator('#maitu-new-task').click();
  const childForm = page.locator('#maitu-task-create');
  await childForm.locator('[name="title"]').fill('汇总后续工作');
  await childForm.locator('[name="instruction"]').fill('基于已经采用的整理需求成果继续。');
  await page.locator('#maitu-task-dependencies label').filter({ hasText: '整理需求' }).locator('input').check();
  await childForm.getByRole('button', { name: '加入任务图' }).click();
  const child = page.locator('.maitu-node').filter({ hasText: '汇总后续工作' });
  await child.getByRole('button', { name: '执行', exact: true }).click();
  await expect(child).toContainText('等待前序任务的成果被采用');
  await expect(page.locator('#maitu-map svg path')).toHaveCount(4);
  await page.setViewportSize({ width: 390, height: 844 });
  await page.reload();
  await expect(page.locator('.maitu-node')).toHaveCount(4);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1)).toBe(true);
  await page.screenshot({ path: `${screenshotDir}/maitu-graph-mobile.png`, fullPage: true });
  expect(errors).toEqual([]);
  expect(serverErrors).toEqual([]);
});
