(() => {
  'use strict';
  // 目标分支工作台：契约 / 会话 / 贡献 / 证据 / 合入门禁。
  // 数据来自 /api/v1/projects/{id}/goal-graph，命令走 goal-commands。
  const $ = (selector, root = document) => root.querySelector(selector);
  const projectId = document.body.dataset.projectId;
  const branchLabels = {active:'探索中', waiting:'等待条件', review_pending:'待复核', integrated:'已带回主线', completed:'已完成', stopped:'已停止', archived:'已归档'};
  const sessionLabels = {running:'进行中', waiting_branch_review:'等分支复核', waiting_dependency:'等依赖', waiting_judgment:'等判断', exception_paused:'异常暂停', manual_paused:'手动暂停', awaiting_merge_review:'待合入复核', review_rejected:'复核驳回', accepted:'已通过', stopped:'已停止'};
  const gateLabels = {pending_ai_review:'待 AI 复核', pending_human_review:'待人工决策', accepted:'已通过', partially_accepted:'部分通过', rejected:'已驳回', abandoned:'已放弃', withdrawn:'已撤回'};
  const proposalLabels = {draft:'草稿', awaiting_approval:'待决策', submitted:'待决策', approved:'已通过', rejected:'已驳回', cancelled:'已取消'};
  const kindLabels = {finding:'结论 / 经验', artifact:'文件 / 代码 / 内容', evidence:'证据 / 反馈', decision:'决定', condition:'条件变化', other:'其他产出'};
  const evidenceKinds = {observation:'观察', measurement:'度量', reference:'引用', reasoning:'推理'};
  let snapshot;
  let selectedBranch;

  function element(tag, className, text) {
    const node = document.createElement(tag);
    if (className) node.className = className;
    if (text !== undefined) node.textContent = text;
    return node;
  }
  function feedback(message, error = false) {
    const node = $('#maitu-feedback');
    node.textContent = message;
    node.classList.toggle('is-error', error);
    node.hidden = false;
    clearTimeout(feedback.timer);
    feedback.timer = setTimeout(() => {node.hidden = true;}, error ? 14000 : 5000);
  }
  function requestId() { return crypto.randomUUID(); }
  async function api(path, method = 'GET', data) {
    const headers = {};
    if (data !== undefined) headers['Content-Type'] = 'application/json';
    if (method !== 'GET') {
      const status = await fetch('/auth/status').then(r => r.ok ? r.json() : {}).catch(() => ({}));
      if (status.csrfToken) headers['x-csrf-token'] = status.csrfToken;
    }
    let response;
    try {
      response = await fetch(path, {method, headers, body: data === undefined ? undefined : JSON.stringify(data), credentials: 'same-origin'});
    } catch { throw new Error('无法连接本机服务，请确认服务正在运行后重试。'); }
    let value;
    try {value = await response.json();} catch {throw new Error(`服务回复未能读取（HTTP ${response.status}）。`);}
    if (!response.ok) throw new Error(typeof value?.error === 'string' ? value.error : `请求失败（HTTP ${response.status}）。`);
    return value;
  }
  function goalCommand(action, payload) {
    return api(`/api/v1/projects/${projectId}/goal-commands`, 'POST', {clientRequestId: requestId(), action, payload});
  }
  function handle(action) {
    return async event => {
      const button = event.currentTarget;
      button.disabled = true;
      try {await action();} catch (error) {feedback(error.message, true);} finally {button.disabled = false;}
    };
  }
  function button(text, action, variant = '') {
    const node = element('button', `maitu-button maitu-button--small${variant ? ' ' + variant : ''}`, text);
    node.type = 'button';
    node.addEventListener('click', handle(action));
    return node;
  }
  function chip(text, kind = '') { return element('span', `maitu-chip${kind ? ' maitu-chip--' + kind : ''}`, text); }
  function statusChip(labels, status) {
    const kind = ['running', 'active', 'accepted', 'completed', 'integrated', 'approved'].includes(status) ? 'produced'
      : ['waiting_branch_review', 'waiting_judgment', 'waiting', 'waiting_dependency', 'exception_paused', 'manual_paused', 'awaiting_merge_review', 'review_pending', 'pending_ai_review', 'pending_human_review', 'awaiting_approval', 'submitted'].includes(status) ? 'queued'
      : ['rejected', 'cancelled', 'stopped', 'failed', 'review_rejected', 'abandoned'].includes(status) ? 'failed' : '';
    return chip(labels[status] || status, kind);
  }
  function time(value) {return value ? new Date(value).toLocaleString() : '—';}
  function lines(value) {return Array.isArray(value) ? value : [];}
  function showOutput(title, content) {
    $('#maitu-output-title').textContent = title;
    $('#maitu-output-content').textContent = content;
    $('#maitu-output-dialog').showModal();
  }

  function formRow(labelText, control) {
    const label = element('label', '', labelText);
    label.append(control);
    return label;
  }
  function field(name, {tag = 'input', value = '', placeholder = '', rows = 3, required = true, options} = {}) {
    const control = element(tag);
    control.name = name;
    if (tag === 'textarea') {control.rows = rows; control.textContent = value;}
    else control.value = value;
    if (placeholder) control.placeholder = placeholder;
    control.required = required;
    if (options) for (const [item, title] of options) {const option = element('option', '', title); option.value = item; control.append(option);}
    return control;
  }
  function openGoalDialog(title, form) {
    const dialog = $('#maitu-goal-dialog');
    dialog.replaceChildren();
    const head = element('div', 'maitu-section-head');
    head.append(element('h2', '', title));
    const close = element('button', 'maitu-close', '×');
    close.type = 'button';
    close.setAttribute('aria-label', '关闭' + title);
    close.addEventListener('click', () => dialog.close());
    head.append(close);
    dialog.append(head, form);
    dialog.showModal();
  }
  function bindDialog(form, submitLabel, onSubmit) {
    const submit = element('button', 'maitu-button maitu-button--primary', submitLabel);
    submit.type = 'submit';
    form.append(submit);
    form.addEventListener('submit', async event => {
      event.preventDefault();
      submit.disabled = true;
      try { await onSubmit(Object.fromEntries(new FormData(form)), form); }
      catch (error) { feedback(error.message, true); submit.disabled = false; }
    });
  }
  function refreshAfter() { return load().then(render); }

  // ── 视图切换 ──
  const viewTasks = $('#maitu-view-tasks');
  const viewGoals = $('#maitu-view-goals');
  function setView(goals) {
    viewTasks.classList.toggle('is-active', !goals);
    viewTasks.setAttribute('aria-selected', String(!goals));
    viewGoals.classList.toggle('is-active', goals);
    viewGoals.setAttribute('aria-selected', String(goals));
    $('#maitu-workspace-tasks').hidden = goals;
    $('#maitu-workspace-goals').hidden = !goals;
    if (goals) load().then(render).catch(error => feedback(error.message, true));
  }
  viewTasks.addEventListener('click', () => setView(false));
  viewGoals.addEventListener('click', () => setView(true));

  // ── 目标提案 ──
  function renderProposals() {
    const host = $('#maitu-goals-proposals');
    host.replaceChildren();
    const pending = snapshot.proposals.filter(item => ['draft', 'submitted', 'awaiting_approval'].includes(item.status));
    if (!pending.length) return;
    const panel = element('div', 'maitu-attention');
    const head = element('div', 'maitu-section-head');
    head.append(element('h2', '', '目标提案'), element('span', '', pending.length + ' 项待处理'));
    panel.append(head);
    for (const proposal of pending) {
      const revision = snapshot.proposalRevisions.filter(item => item.proposalId === proposal.id)
        .find(item => item.revision === proposal.currentRevision);
      const label = revision?.whyNeeded || '目标提案';
      const row = element('div', 'maitu-attention-row');
      row.append(element('strong', '', label), statusChip(proposalLabels, proposal.status));
      const actions = element('div', 'maitu-attention-actions');
      if (proposal.status === 'draft') {
        actions.append(button('提交评审', () => goalCommand('proposal.submit', {proposalId: proposal.id, expectedRevision: proposal.currentRevision}).then(refreshAfter)));
      }
      if (['submitted', 'awaiting_approval'].includes(proposal.status)) {
        actions.append(button('批准建枝', () => goalCommand('proposal.approve', {
          proposalId: proposal.id, expectedRevision: proposal.currentRevision,
          branchName: label.slice(0, 24), assignment: '推进：' + label, agentIdentity: null
        }).then(() => {feedback('提案已批准，目标分支已建立。'); return refreshAfter();})));
        actions.append(button('驳回', () => goalCommand('proposal.cancel', {proposalId: proposal.id, reason: '手动取消'}).then(refreshAfter), 'maitu-button--danger'));
      }
      row.append(actions);
      panel.append(row);
    }
    host.append(panel);
  }

  function goalForm(prefix = '') {
    const form = element('form', '');
    form.append(formRow('目标', field('goal', {tag: 'textarea', rows: 2, placeholder: '要达成什么、怎样算完成'})));
    const advanced = element('details', 'maitu-advanced');
    advanced.append(element('summary', '', '高级'));
    advanced.append(
      formRow('硬性约束（每行一条）', field('hardConstraints', {tag: 'textarea', rows: 2, required: false})),
      formRow('未知（每行一条）', field('unknowns', {tag: 'textarea', rows: 2, required: false})),
      formRow('验证计划（每行一条）', field('validationPlan', {tag: 'textarea', rows: 2, required: false})),
      formRow('停止/完成条件（每行一条）', field('stopConditions', {tag: 'textarea', rows: 2, required: false})));
    form.append(advanced);
    return form;
  }
  function contractFrom(data) {
    const toLines = value => value.split('\n').map(item => item.trim()).filter(Boolean);
    const goalText = (data.goal || '').trim();
    const [firstLine, ...rest] = goalText.split('\n');
    return {
      whyNeeded: firstLine || '',
      contract: {
        desiredOutcome: rest.join('\n').trim() || firstLine || '',
        hardConstraints: toLines(data.hardConstraints || ''),
        subjectivePreferences: [], unknowns: toLines(data.unknowns || ''), nonGoals: [],
        validationPlan: toLines(data.validationPlan || '').length ? toLines(data.validationPlan || '') : ['使用中按真实任务验证'],
        judgmentTriggers: [],
        stopConditions: toLines(data.stopConditions || '').length ? toLines(data.stopConditions || '') : ['目标达成或明确放弃'],
        expectedContributions: [],
        exploration: {mode: 'delivery', budgets: [], candidateOutputs: [], uncertaintyReduction: []}
      },
      expectedContributions: [], explorationPlan: [], contextInheritance: {}, toolRequirements: [], inferences: [], revisionReason: null
    };
  }
  $('#maitu-goal-proposal-new').addEventListener('click', () => {
    const form = goalForm();
    bindDialog(form, '创建提案', async data => {
      const contract = contractFrom(data);
      const title = contract.contract.desiredOutcome.slice(0, 24) || '目标提案';
      await goalCommand('proposal.create', {revision: {...contract, title: data.goal.split('\n')[0].slice(0, 24) || title}});
      $('#maitu-goal-dialog').close();
      feedback('提案已创建，提交后批准建枝。');
      await refreshAfter();
    });
    openGoalDialog('提出目标', form);
  });

  // ── 分支列表 ──
  function renderBranches() {
    const host = $('#maitu-goals-branches');
    host.replaceChildren();
    const branches = [...snapshot.branches].sort((a, b) => new Date(b.updatedAt) - new Date(a.updatedAt));
    if (!branches.length) {
      host.append(element('p', 'maitu-note', '还没有目标分支。从「提出目标提案」开始：描述要达成的结果与约束，批准后建立分支。'));
      return;
    }
    for (const branch of branches) {
      const sessions = snapshot.sessions.filter(item => item.goalBranchId === branch.id);
      const card = element('button', 'maitu-goal-branch' + (selectedBranch === branch.id ? ' is-selected' : ''));
      card.type = 'button';
      const head = element('div', 'maitu-card-top');
      head.append(statusChip(branchLabels, branch.status), element('span', '', `${sessions.length} 个会话`));
      card.append(head, element('strong', '', branch.name));
      const contract = snapshot.contracts.find(item => item.id === branch.currentContractVersionId);
      card.append(element('p', '', contract?.desiredOutcome || ''));
      card.addEventListener('click', () => {selectedBranch = branch.id; render();});
      host.append(card);
    }
  }

  // ── 分支工作台 ──
  function contractList(title, items) {
    if (!items.length) return null;
    const block = element('div', 'maitu-contract-block');
    block.append(element('h4', '', title));
    const list = element('ul', 'maitu-contract-list');
    for (const item of items) list.append(element('li', '', item));
    block.append(list);
    return block;
  }
  function renderContract(workspace, branch) {
    const contract = snapshot.contracts.find(item => item.id === branch.currentContractVersionId);
    if (!contract) return;
    const panel = element('section', 'maitu-goal-card');
    const head = element('div', 'maitu-section-head');
    head.append(element('h3', '', `契约 · 第 ${contract.version} 版`));
    const actions = element('div', 'maitu-attention-actions');
    actions.append(button('提议契约修订', () => openContractRevision(branch, contract)));
    head.append(actions);
    panel.append(head, element('p', 'maitu-detail-instruction', contract.desiredOutcome));
    for (const [title, value] of [['硬性约束', lines(contract.hardConstraints)], ['未知', lines(contract.unknowns)], ['验证计划', lines(contract.validationPlan)], ['预期贡献', lines(contract.expectedContributions)]]) {
      const block = contractList(title, value);
      if (block) panel.append(block);
    }
    const pending = snapshot.contractRevisionRequests.find(item => item.goalBranchId === branch.id && item.status === 'awaiting_approval');
    if (pending) {
      const notice = element('div', 'maitu-wait');
      notice.append(document.createTextNode('有一份契约修订待决定：' + (pending.reason || '')));
      const decide = element('div', 'maitu-attention-actions');
      decide.append(button('接受修订', () => goalCommand('contract.accept_revision', {revisionRequestId: pending.id, rationale: '接受修订'}).then(() => {feedback('契约修订已接受。'); return refreshAfter();})));
      decide.append(button('驳回', () => goalCommand('contract.reject_revision', {revisionRequestId: pending.id, rationale: '驳回修订'}).then(refreshAfter), 'maitu-button--danger'));
      notice.append(decide);
      panel.append(notice);
    }
    workspace.append(panel);
  }
  function openContractRevision(branch, contract) {
    const form = goalForm();
    form.elements.goal.value = contract.desiredOutcome;
    const advanced = form.querySelector('.maitu-advanced');
    advanced.querySelector('[name="hardConstraints"]').value = lines(contract.hardConstraints).join('\n');
    advanced.querySelector('[name="unknowns"]').value = lines(contract.unknowns).join('\n');
    advanced.querySelector('[name="validationPlan"]').value = lines(contract.validationPlan).join('\n');
    form.append(formRow('修订理由', field('reason', {placeholder: '为什么调整'})));
    bindDialog(form, '提交修订', async data => {
      await goalCommand('contract.propose_revision', {
        goalBranchId: branch.id, expectedContractVersionId: contract.id, proposedBySessionId: branch.headSessionId,
        ...contractFrom(data), reason: data.reason,
        sourceAnnotations: [{fieldPath: '/', sourceKind: 'human_input', sourceRef: null, note: data.reason || '用户提出契约调整'}]
      });
      $('#maitu-goal-dialog').close();
      feedback('契约修订请求已提交。');
      await refreshAfter();
    });
    openGoalDialog('契约修订', form);
  }

  function renderSession(workspace, branch, session) {
    const card = element('section', 'maitu-goal-card maitu-session');
    const head = element('div', 'maitu-section-head');
    head.append(element('h3', '', `会话 ${session.sessionNumber}`), statusChip(sessionLabels, session.status));
    card.append(head, element('p', 'maitu-note', `${session.assignment} · 开始于 ${time(session.startedAt)}${session.agentIdentity ? ' · ' + session.agentIdentity : ''}`));
    const contributions = snapshot.contributions.filter(item => item.sessionId === session.id);
    if (contributions.length) {
      const list = element('div', '');
      list.append(element('h4', '', '贡献'));
      for (const item of contributions) {
        const row = element('div', 'maitu-contribution');
        row.append(element('strong', '', `${kindLabels[item.kind] || item.kind} · ${item.title}`), element('p', 'maitu-note', item.body));
        list.append(row);
      }
      card.append(list);
    }
    const evidence = snapshot.evidence.filter(item => item.sessionId === session.id);
    if (evidence.length) {
      const list = element('div', '');
      list.append(element('h4', '', '证据'));
      for (const item of evidence) {
        const row = element('div', 'maitu-link-row');
        row.append(chip(evidenceKinds[item.kind] || item.kind), element('strong', '', item.claim));
        row.append(element('span', 'maitu-note', item.observation + (item.sourceUri ? ' · ' + item.sourceUri : '')));
        list.append(row);
      }
      card.append(list);
    }
    const gates = snapshot.reviewGates.filter(item => item.sessionId === session.id);
    for (const gate of gates) {
      const block = element('div', 'maitu-goal-gate');
      block.append(element('h4', '', '合入门禁'), statusChip(gateLabels, gate.status));
      if (gate.status === 'pending_ai_review') {
        block.append(element('p', 'maitu-note', '等待独立 Review Worker 复核，完成后出现决策按钮。'));
        const decide = element('div', 'maitu-attention-actions');
        decide.append(button('撤回合入', () => goalCommand('merge.withdraw', {reviewGateId: gate.id, reason: '撤回本次合入', newEvidence: []}).then(refreshAfter)));
        block.append(decide);
      }
      if (gate.status === 'pending_human_review') {
        const decide = element('div', 'maitu-attention-actions');
        decide.append(button('通过并合入', () => goalCommand('review.human_decide', {
          reviewGateId: gate.id, decision: 'accept', rationale: '人工审核通过',
          selectedContributionIds: snapshot.contributions.filter(item => item.sessionId === session.id).map(item => item.id)
        }).then(() => {feedback('门禁已通过，分支进入合入。'); return refreshAfter();})));
        decide.append(button('部分通过', () => goalCommand('review.human_decide', {
          reviewGateId: gate.id, decision: 'partial_accept', rationale: '部分贡献合入',
          selectedContributionIds: snapshot.contributions.filter(item => item.sessionId === session.id).map(item => item.id)
        }).then(refreshAfter)));
        decide.append(button('驳回', () => goalCommand('review.human_decide', {reviewGateId: gate.id, decision: 'reject', rationale: '人工驳回'}).then(refreshAfter), 'maitu-button--danger'));
        decide.append(button('撤回合入', () => goalCommand('merge.withdraw', {reviewGateId: gate.id, reason: '撤回本次合入', newEvidence: []}).then(refreshAfter)));
        block.append(decide);
      }
      const show = button('查看候选快照', () => showOutput('合入候选快照', JSON.stringify(gate.candidateSnapshot, null, 2)));
      block.append(show);
      card.append(block);
    }
    const actions = element('div', 'maitu-goal-actions');
    if (session.status === 'running') {
      actions.append(button('添加贡献', () => openContribution(session)));
      actions.append(button('添加证据', () => openEvidence(session)));
      actions.append(button('请求判断', () => openJudgment(session)));
      actions.append(button('提议合入', () => openMerge(branch, session)));
      actions.append(button('暂停', () => goalCommand('session.pause_manual', {sessionId: session.id, reason: '手动暂停'}).then(refreshAfter)));
      actions.append(button('停止', () => { if (confirm('停止该会话？结束后可从分支开启新会话。')) return goalCommand('session.stop', {sessionId: session.id, reason: '手动停止'}).then(refreshAfter); }, 'maitu-button--danger'));
    }
    if (['paused_exception', 'paused_manual', 'waiting', 'waiting_dependency'].includes(session.status)) {
      actions.append(button('恢复会话', () => goalCommand('session.resume', {sessionId: session.id, resolution: '手动恢复'}).then(refreshAfter)));
    }
    if (session.status !== 'running') {
      actions.append(button('开启下一会话', () => openNextSession(branch, session)));
    }
    card.append(actions);
    workspace.append(card);
  }

  function openContribution(session) {
    const form = element('form', '');
    form.append(
      formRow('标题', field('title', {placeholder: '一句话'})),
      formRow('内容', field('body', {tag: 'textarea', rows: 2})));
    bindDialog(form, '添加', async data => {
      await goalCommand('session.add_contribution', {sessionId: session.id, kind: 'finding', title: data.title, body: data.body, artifactId: null, evidenceRefs: [], evidenceIds: [], supersedesId: null});
      $('#maitu-goal-dialog').close();
      feedback('贡献已记录。'); await refreshAfter();
    });
    openGoalDialog('贡献', form);
  }
  function openEvidence(session) {
    const form = element('form', '');
    form.append(
      formRow('说明什么', field('claim', {placeholder: '一句话'})),
      formRow('观察', field('observation', {tag: 'textarea', rows: 2})));
    bindDialog(form, '添加', async data => {
      await goalCommand('session.add_evidence', {sessionId: session.id, kind: 'observation', stance: 'supports', claim: data.claim, observation: data.observation, sourceUri: null, artifactId: null, toolCallId: null, verificationStatus: 'unverified'});
      $('#maitu-goal-dialog').close();
      feedback('证据已记录。'); await refreshAfter();
    });
    openGoalDialog('证据', form);
  }
  function openJudgment(session) {
    const form = element('form', '');
    form.append(formRow('问题', field('question', {placeholder: '需要人判断什么'})));
    bindDialog(form, '提交', async data => {
      await goalCommand('session.request_judgment', {sessionId: session.id, question: data.question, candidates: [], evidence: null, recommendation: null});
      $('#maitu-goal-dialog').close();
      feedback('判断请求已提交。'); await refreshAfter();
    });
    openGoalDialog('请求判断', form);
  }
  function openMerge(branch, session) {
    const contributions = snapshot.contributions.filter(item => item.sessionId === session.id);
    if (!contributions.length) {feedback('先添加至少一条贡献。', true); return;}
    const form = element('form', '');
    const checks = element('div', 'maitu-checks');
    for (const item of contributions) {
      const row = element('label');
      const input = element('input');
      input.type = 'checkbox'; input.name = 'contributionIds'; input.value = item.id; input.checked = true;
      row.append(input, element('span', '', `${kindLabels[item.kind] || item.kind} · ${item.title}`));
      checks.append(row);
    }
    const wrap = element('fieldset', '');
    wrap.append(element('legend', '', '合入贡献'), checks);
    form.append(wrap, formRow('自检（可选）', field('selfCheck', {tag: 'textarea', rows: 2, required: false, placeholder: '为什么可以合入'})));
    bindDialog(form, '提议合入', async (data, form) => {
      await goalCommand('merge.propose', {sessionId: session.id, candidate: {
        contributionIds: new FormData(form).getAll('contributionIds'), evidenceIds: [],
        contractVersionId: session.contractVersionId, gitBaseCommit: null, gitHeadCommit: null, gitDirty: false,
        environmentFingerprint: null,
        testEvidence: [], risks: [],
        selfCheck: data.selfCheck || ''
      }});
      $('#maitu-goal-dialog').close();
      feedback('合入已提议，等待门禁决定。'); await refreshAfter();
    });
    openGoalDialog('提议合入', form);
  }
  function openNextSession(branch, previous) {
    const form = element('form', '');
    form.append(formRow('任务', field('assignment', {placeholder: '接下来推进什么'})));
    bindDialog(form, '开启', async data => {
      await goalCommand('session.start_next', {goalBranchId: branch.id, previousSessionId: previous.id, assignment: data.assignment, agentIdentity: null});
      $('#maitu-goal-dialog').close();
      feedback('新会话已开启。'); await refreshAfter();
    });
    openGoalDialog('下一会话', form);
  }

  function render() {
    renderProposals();
    renderBranches();
    const summary = $('#maitu-goals-summary');
    summary.textContent = `${snapshot.branches.filter(item => ['active', 'waiting', 'review_pending'].includes(item.status)).length} 个活跃分支 · ${snapshot.sessions.filter(item => item.status === 'running').length} 个会话进行中`;
    const workspace = $('#maitu-goals-workspace');
    workspace.replaceChildren();
    const branch = snapshot.branches.find(item => item.id === selectedBranch);
    if (!branch) {
      workspace.append(element('div', 'maitu-empty', ''));
      workspace.firstChild.append(element('strong', '', '选择一个目标分支'), element('p', '', '查看契约、会话记录、证据与合入门禁；也可以从这里推进下一步。'));
      return;
    }
    renderContract(workspace, branch);
    const sessions = snapshot.sessions.filter(item => item.goalBranchId === branch.id).sort((a, b) => b.sessionNumber - a.sessionNumber);
    for (const session of sessions) renderSession(workspace, branch, session);
    const archive = element('div', 'maitu-goal-actions');
    archive.append(button('归档分支', () => { if (confirm(`归档分支「${branch.name}」？`)) return goalCommand('goal_branch.archive', {goalBranchId: branch.id, reason: '手动归档'}).then(() => {selectedBranch = null; return refreshAfter();}); }, 'maitu-button--danger'));
    workspace.append(archive);
  }
  async function load() {
    snapshot = await api(`/api/v1/projects/${projectId}/goal-graph`);
    if (!selectedBranch && snapshot.branches.length) selectedBranch = snapshot.branches[0].id;
  }
})();
