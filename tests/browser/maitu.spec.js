const { test, expect } = require('@playwright/test');

test.use({ baseURL: process.env.BASE_URL || 'http://127.0.0.1:3000' });
const screenshotDir = process.env.SCREENSHOT_DIR || '/screenshots';
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');

test('Maitu graph persists real tasks, explains waiting and retains failed attempts', async ({ page }) => {
  test.setTimeout(150_000);
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
  await expect(page.locator('#maitu-map')).toContainText('请先在设置中配置可用的模型连接');
  const snapshot = await (await page.request.get(`/api/maitu/projects/${projectId}`)).json();
  expect(snapshot.tasks).toHaveLength(3);
  expect(snapshot.sources).toHaveLength(1);
  expect(snapshot.connections.every(connection => !connection.configured)).toBe(true);
  await page.reload();
  await expect(page.locator('.maitu-node--queued')).toHaveCount(3);
  await page.screenshot({ path: `${screenshotDir}/maitu-graph-desktop.png`, fullPage: true });

  // A local closed port verifies the real network-failure path without calling a paid API.
  await page.goto('/maitu/settings');
  const addForm = page.locator('#maitu-connection-form');
  await expect(addForm.locator('[name="timeoutSeconds"]')).toHaveCount(0);
  await expect(addForm.locator('[name="contextTokens"]')).toHaveValue('1048576');
  await expect(addForm.locator('[name="maxTokens"]')).toHaveValue('65536');
  await expect(addForm.locator('[name="thinkingEnabled"]')).toHaveValue('true');
  await expect(page.locator('#maitu-model-limits')).toContainText('1,048,576');
  await expect(page.locator('#maitu-model-limits')).toContainText('393,216');
  await addForm.locator('[name="key"]').fill('browser-fixture');
  await addForm.locator('[name="label"]').fill('浏览器隔离连接');
  await addForm.locator('[name="baseUrl"]').fill('http://127.0.0.1:1');
  await addForm.locator('[name="apiKey"]').fill('isolated-browser-fixture-key');
  await addForm.locator('[name="maxTokens"]').fill('393216');
  const plainTextRejection = async route => {
    if (route.request().method() === 'POST') {
      await route.fulfill({status:422, contentType:'text/plain', body:'plain-text fixture rejection'});
    } else { await route.continue(); }
  };
  await page.route('**/api/maitu/connections', plainTextRejection);
  await addForm.getByRole('button', { name: '保存连接' }).click();
  await expect(page.locator('#maitu-feedback')).toContainText('填写内容格式不正确（HTTP 422）');
  await expect(addForm.locator('[name="apiKey"]')).toHaveValue('isolated-browser-fixture-key');
  await expect(page.locator('#maitu-feedback')).not.toContainText('plain-text fixture rejection');
  await page.unroute('**/api/maitu/connections', plainTextRejection);
  await addForm.getByRole('button', { name: '保存连接' }).click();
  const fixtureCard = page.locator('.maitu-connection-card').filter({ hasText: '浏览器隔离连接' });
  await expect(fixtureCard).toContainText('已启用');
  await expect(addForm.locator('[name="apiKey"]')).toHaveValue('');
  const connectionsList = await (await page.request.get('/api/maitu/connections')).json();
  const fixtureConnection = connectionsList.find(connection => connection.key === 'browser-fixture');
  expect(JSON.stringify(connectionsList)).not.toContain('isolated-browser-fixture-key');
  expect(fixtureConnection).not.toHaveProperty('apiKey');
  expect(fixtureConnection).not.toHaveProperty('timeoutSeconds');
  expect(fixtureConnection.maxTokens).toBe(393216);
  expect(fixtureConnection.contextTokens).toBe(1048576);
  expect(fixtureConnection.thinkingEnabled).toBe(true);
  const invalidResponse = await page.request.post('/api/maitu/connections', {
    data:{...fixtureConnection, apiKey:'isolated-browser-fixture-key', maxTokens:'not-an-integer'},
  });
  expect(invalidResponse.status()).toBe(422);
  expect(invalidResponse.headers()['content-type']).toContain('application/json');
  const invalidBody = await invalidResponse.json();
  expect(invalidBody.error).toContain('须为整数');
  expect(JSON.stringify(invalidBody)).not.toContain('isolated-browser-fixture-key');
  const unchangedConnection = connectionsList.find(connection => connection.key === 'browser-fixture');
  expect(unchangedConnection.maxTokens).toBe(393216);
  expect(unchangedConnection.configured).toBe(true);
  await page.reload();
  await expect(page.locator('.maitu-connection-card').filter({ hasText: '浏览器隔离连接' })).toBeVisible();
  const editCard = page.locator('.maitu-connection-card').filter({ hasText: '浏览器隔离连接' });
  await editCard.getByRole('button', { name: '编辑' }).click();
  await expect(addForm.locator('[name="key"]')).toHaveValue('browser-fixture');
  await expect(addForm.locator('[name="key"]')).toBeDisabled();
  await addForm.locator('[name="thinkingEnabled"]').selectOption('false');
  await expect(addForm.locator('[name="maxTokens"]')).toHaveValue('393216');
  await addForm.locator('[name="contextTokens"]').fill('393216');
  await addForm.getByRole('button', { name: '保存连接' }).click();
  await expect(page.locator('#maitu-feedback')).toContainText('为任务要求和资料保留输入空间');
  await addForm.locator('[name="contextTokens"]').fill('1048576');
  await addForm.locator('[name="maxTokens"]').fill('65536');
  const [saveResponse] = await Promise.all([
    page.waitForResponse(response => response.url().endsWith('/api/maitu/connections') && response.request().method() === 'POST'),
    addForm.getByRole('button', { name: '保存连接' }).click(),
  ]);
  expect(saveResponse.status()).toBe(200);
  await expect(addForm.locator('[name="apiKey"]')).toHaveValue('');
  const updatedConnections = await (await page.request.get('/api/maitu/connections')).json();
  const updated = updatedConnections.find(connection => connection.key === 'browser-fixture');
  expect(updated.configured).toBe(true);
  expect(updated.maxTokens).toBe(65536);
  expect(updated.thinkingEnabled).toBe(false);
  await page.reload();
  const editAgain = page.locator('.maitu-connection-card').filter({ hasText: '浏览器隔离连接' });
  await editAgain.getByRole('button', { name: '编辑' }).click();
  await expect(addForm.locator('[name="thinkingEnabled"]')).toHaveValue('false');
  await expect(addForm.locator('[name="maxTokens"]')).toHaveValue('65536');
  await addForm.locator('[name="thinkingEnabled"]').selectOption('true');
  const [thinkingResponse] = await Promise.all([
    page.waitForResponse(response => response.url().endsWith('/api/maitu/connections') && response.request().method() === 'POST'),
    addForm.getByRole('button', { name: '保存连接' }).click(),
  ]);
  expect(thinkingResponse.status()).toBe(200);
  const thinkingConnection = await thinkingResponse.json();
  expect(thinkingConnection.thinkingEnabled).toBe(true);
  expect(thinkingConnection.maxTokens).toBe(65536);
  expect(thinkingConnection.configured).toBe(true);
  await page.screenshot({ path: `${screenshotDir}/maitu-settings-desktop.png`, fullPage: true });
  await page.goto(graphUrl);
  // the closed-port connection fails fast; bounded automatic retries (1+3) then leave the task failed
  await expect(page.locator('.maitu-node--failed')).toHaveCount(3, { timeout: 60_000 });
  const firstNode = page.locator('.maitu-node').filter({ hasText: '整理需求' });
  await firstNode.getByRole('button', { name: '记录与成果' }).click();
  await expect(page.locator('#maitu-detail')).toContainText('无法连接模型服务');
  await expect(page.locator('#maitu-detail .maitu-attempt')).toHaveCount(4);
  await firstNode.getByRole('button', { name: '重试', exact: true }).click();
  await expect(page.locator('#maitu-detail .maitu-attempt')).toHaveCount(5);
  await expect(page.locator('#maitu-detail .maitu-attempt').first()).toContainText('执行失败');
  await page.locator('#maitu-detail').getByRole('button', { name: '查看本次输入' }).first().click();
  await expect(page.locator('#maitu-output-content')).toContainText('实际输入');
  await page.getByRole('button', { name: '关闭内容' }).click();

  await page.locator('#maitu-new-task').click();
  const childForm = page.locator('#maitu-task-create');
  await childForm.locator('[name="title"]').fill('汇总后续工作');
  await childForm.locator('[name="instruction"]').fill('基于已经采用的整理需求成果继续。');
  await childForm.locator('details.maitu-advanced summary').click();
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
  await page.goto('/maitu/settings');
  await expect(addForm.locator('[name="contextTokens"]')).toHaveValue('1048576');
  await expect(addForm.locator('[name="maxTokens"]')).toHaveValue('65536');
  await expect(addForm.locator('[name="thinkingEnabled"]')).toHaveValue('true');
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1)).toBe(true);
  await page.screenshot({ path: `${screenshotDir}/maitu-settings-mobile.png`, fullPage: true });
  expect(errors).toEqual([]);
  expect(serverErrors).toEqual([]);
});

test('Maitu code import, editable plan adoption and additional attempts persist', async ({page}) => {
  test.setTimeout(90_000);
  const errors=[];page.on('pageerror',error=>errors.push(error.message));
  const projectId='51000000-0000-0000-0000-000000000001';
  const fixtureDirectory=fs.mkdtempSync(path.join(os.tmpdir(),'maitu-source-'));
  fs.writeFileSync(path.join(fixtureDirectory,'a.js'),'module.exports=()=>1;\n');
  fs.writeFileSync(path.join(fixtureDirectory,'.env'),'isolated fixture, not a credential\n');
  fs.mkdirSync(path.join(fixtureDirectory,'node_modules'));
  fs.writeFileSync(path.join(fixtureDirectory,'node_modules','cache.js'),'ignored cache\n');
  await page.setViewportSize({width:1440,height:1000});
  await page.goto('/maitu/projects/'+projectId);
  await page.locator('#maitu-import-code').click();
  await page.locator('#maitu-code-files').setInputFiles(fixtureDirectory);
  await expect(page.locator('#maitu-code-import-count')).toContainText('将导入 1 个文本文件');
  await expect(page.locator('#maitu-code-import-count')).toContainText('跳过 2 件');
  await page.locator('#maitu-code-import-form button[type="submit"]').click();
  await expect(page.locator('#maitu-code-state')).toContainText('1 个文件');
  await expect(page.locator('#maitu-export-code')).toBeVisible();
  const planner=page.locator('.maitu-node').filter({hasText:'界面测试计划'});
  await planner.getByRole('button',{name:'记录与成果'}).click();
  await page.getByRole('button',{name:'调整并加入任务图'}).click();
  const edit=page.locator('#maitu-plan-review-form');
  await edit.locator('[data-plan-key="a"] [name="title"]').fill('真正修改代码 A');
  await edit.locator('[data-plan-key="a"] [name="kind"]').selectOption('code');
  await edit.locator('[data-plan-key="a"] [name="instruction"]').fill('读取 a.js 后修改，并实际运行项目测试。');
  await edit.locator('[data-plan-key="a"] [name="acceptanceCriteria"]').fill('实际检查通过，差异可打开');
  await edit.getByRole('button',{name:'加入任务图',exact:true}).click();
  await expect(page.locator('.maitu-node')).toHaveCount(4);
  const state=await (await page.request.get('/api/maitu/projects/'+projectId)).json();
  const added=state.tasks.find(task=>task.title==='真正修改代码 A');
  expect(added.taskKind).toBe('code');expect(added.status).toBe('draft');
  expect(state.tasks.filter(task=>task.status==='queued'||task.status==='running')).toHaveLength(0);
  await page.reload();
  await expect(page.locator('.maitu-node')).toHaveCount(4);
  await page.locator('.maitu-node').filter({hasText:'界面测试计划'}).getByRole('button',{name:'记录与成果'}).click();
  await page.getByRole('button',{name:'查看采用的计划'}).click();
  await expect(edit.locator('[data-plan-key="a"] [name="title"]')).toHaveValue('真正修改代码 A');
  await expect(edit.locator('[data-plan-key="a"] [name="title"]')).toBeDisabled();
  await page.getByRole('button',{name:'关闭计划编辑'}).click();
  await page.locator('#maitu-new-task').click();
  const form=page.locator('#maitu-task-create');
  await form.locator('[name="title"]').fill('编码入口检查');
  await form.locator('details.maitu-advanced summary').click();
  await form.locator('[name="taskKind"]').selectOption('code');
  await form.locator('[name="instruction"]').fill('测试没有检查服务时不能调用模型');
  await form.locator('[name="acceptanceCriteria"]').fill('先验证执行环境');
  await form.getByRole('button',{name:'加入任务图',exact:true}).click();
  await page.locator('.maitu-node').filter({hasText:'编码入口检查'}).getByRole('button',{name:'执行',exact:true}).click();
  await expect(page.locator('#maitu-detail')).toContainText('代码检查服务尚未启动');
  const current=(await (await page.request.get('/api/maitu/projects/'+projectId)).json()).tasks.find(task=>task.title==='编码入口检查');
  const first=await (await page.request.get('/api/maitu/tasks/'+current.id)).json();
  expect(first.attempts[0].requestStartedAt).toBeNull();
  await page.getByRole('button',{name:'补充要求再执行'}).click();
  await page.locator('#maitu-retry-form [name="additionalInstruction"]').fill('环境就绪后再处理；原尝试保留');
  await page.locator('#maitu-retry-form button[type="submit"]').click();
  await expect(page.locator('#maitu-detail .maitu-attempt')).toHaveCount(2);
  await expect(page.locator('#maitu-detail')).toContainText('本次补充：环境就绪后再处理；原尝试保留');
  await page.setViewportSize({width:390,height:844});
  await page.reload();
  await expect(page.locator('.maitu-node')).toHaveCount(5);
  const overlapping=await page.locator('.maitu-node').evaluateAll(nodes=> {
    const bounds=nodes.map(node=>node.getBoundingClientRect());
    return bounds.some((a,index)=>bounds.slice(index+1).some(b=>a.left<b.right&&b.left<a.right&&a.top<b.bottom&&b.top<a.bottom));
  });
  expect(overlapping).toBe(false);
  expect(await page.evaluate(()=>document.documentElement.scrollWidth<=window.innerWidth+1)).toBe(true);
  await page.screenshot({path:screenshotDir+'/maitu-plan-code-mobile.png',fullPage:true});
  expect(errors).toEqual([]);
});
