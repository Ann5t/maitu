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

  const attentionKinds = {session_failed:'会话失败', judgment_requested:'等待判断', gate_decision:'门禁待决策', contract_revision:'契约待决', merge_conflict:'合入冲突'};
  function attentionRow(item) {
    const row = element('div', 'maitu-attention-row');
    row.append(element('strong', '', item.title), chip(attentionKinds[item.kind] || item.kind, item.status === 'open' ? 'maitu-chip--queued' : ''));
    if (item.reason) row.append(element('span', 'maitu-note', item.reason));
    const actions = element('div', 'maitu-attention-actions');
    if (item.goalBranchId) actions.append(button('查看分支', () => {selectedBranch = item.goalBranchId; render();}));
    row.append(actions);
    return row;
  }
  function renderAttention() {
    const host = document.createElement('div');
    const open = (snapshot.attentionItems || []).filter(item => item.status === 'open');
    if (!open.length) return null;
    const panel = element('div', 'maitu-attention');
    const head = element('div', 'maitu-section-head');
    head.append(element('h2', '', '需要处理'), element('span', '', open.length + ' 项'));
    panel.append(head);
    for (const item of open) panel.append(attentionRow(item));
    host.append(panel);
    return host;
  }

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
        actions.append(button('编辑', () => openProposalRevise(proposal, revision)));
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
  function openProposalRevise(proposal, revision) {
    const form = goalForm();
    if (revision) form.elements.goal.value = (revision.contract?.desiredOutcome || revision.whyNeeded || '') + '\n' + (revision.whyNeeded || '');
    bindDialog(form, '保存修订', async data => {
      const contract = contractFrom(data);
      await goalCommand('proposal.revise', {proposalId: proposal.id, expectedRevision: proposal.currentRevision,
        revision: {...contract, title: data.goal.split('\n')[0].slice(0, 24) || '目标提案'}});
      $('#maitu-goal-dialog').close();
      feedback('提案已修订。'); await refreshAfter();
    });
    openGoalDialog('编辑提案', form);
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
    const versions = snapshot.contracts.filter(item => item.goalBranchId === branch.id).sort((a, b) => b.version - a.version);
    const contract = versions.find(item => item.id === branch.currentContractVersionId);
    if (!contract) return;
    const panel = element('section', 'maitu-goal-card');
    const head = element('div', 'maitu-section-head');
    head.append(element('h3', '', `契约 · 第 ${contract.version} 版`));
    const actions = element('div', 'maitu-attention-actions');
    if (versions.length > 1) actions.append(button(`历史 ${versions.length} 版`, () => showContractHistory(branch, versions)));
    actions.append(button('提议修订', () => openContractRevision(branch, contract)));
    head.append(actions);
    panel.append(head, element('p', 'maitu-detail-instruction', contract.desiredOutcome));
    for (const [title, value] of [['硬性约束', lines(contract.hardConstraints)], ['未知', lines(contract.unknowns)], ['验证计划', lines(contract.validationPlan)], ['停止条件', lines(contract.stopConditions)], ['预期贡献', lines(contract.expectedContributions)]]) {
      const block = contractList(title, value);
      if (block) panel.append(block);
    }
    const pending = snapshot.contractRevisionRequests.find(item => item.goalBranchId === branch.id && item.status === 'awaiting_approval');
    if (pending) {
      const notice = element('div', 'maitu-wait');
      notice.append(document.createTextNode('契约修订待决定：' + (pending.reason || '')));
      const decide = element('div', 'maitu-attention-actions');
      decide.append(button('接受修订', () => goalCommand('contract.accept_revision', {revisionRequestId: pending.id, rationale: '接受修订'}).then(() => {feedback('契约修订已接受。'); return refreshAfter();})));
      decide.append(button('驳回', () => goalCommand('contract.reject_revision', {revisionRequestId: pending.id, rationale: '驳回修订'}).then(refreshAfter), 'maitu-button--danger'));
      notice.append(decide);
      panel.append(notice);
    }
    const decisions = snapshot.contractRevisionDecisions.filter(item => {
      const request = snapshot.contractRevisionRequests.find(r => r.id === item.revisionRequestId);
      return request && request.goalBranchId === branch.id;
    }).slice(-3).reverse();
    for (const decision of decisions) {
      panel.append(element('p', 'maitu-note', `修订${decision.decision === 'accepted' ? '已接受' : '已驳回'} · ${decision.actorRole} · ${decision.rationale || ''}`));
    }
    workspace.append(panel);
  }
  function showContractHistory(branch, versions) {
    const list = element('div', '');
    for (const version of versions) {
      const row = element('details', 'maitu-operation');
      row.append(element('summary', '', `第 ${version.version} 版${version.id === branch.currentContractVersionId ? ' · 当前' : ''} · ${time(version.createdAt)}`));
      row.append(element('p', 'maitu-note', version.desiredOutcome));
      for (const [title, value] of [['硬性约束', lines(version.hardConstraints)], ['验证计划', lines(version.validationPlan)], ['停止条件', lines(version.stopConditions)]]) {
        const block = contractList(title, value);
        if (block) row.append(block);
      }
      list.append(row);
    }
    openGoalDialog('契约版本', list);
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
      const meta = [];
      if (gate.gitHeadCommit) meta.push('提交 ' + gate.gitHeadCommit.slice(0, 8));
      if (gate.candidateDigest) meta.push('摘要 ' + gate.candidateDigest.slice(0, 8));
      const risks = lines(gate.risks), tests = lines(gate.testEvidence);
      if (meta.length || risks.length || tests.length || gate.selfCheck) {
        const info = element('div', '');
        if (meta.length) info.append(element('p', 'maitu-note', meta.join(' · ')));
        if (tests.length) info.append(contractList('复测证据', tests));
        if (risks.length) info.append(contractList('风险', risks));
        const selfCheckText = typeof gate.selfCheck === 'string' ? gate.selfCheck : (gate.selfCheck?.selfCheck || gate.selfCheck?.summary || '');
        if (selfCheckText) info.append(element('p', 'maitu-note', '自检：' + selfCheckText));
        block.append(info);
      }
      const gateEvidence = (snapshot.reviewGateEvidence || []).filter(item => item.reviewGateId === gate.id)
        .map(item => snapshot.evidence.find(e => e.id === item.evidenceId)).filter(Boolean);
      if (gateEvidence.length) block.append(contractList('关联证据', gateEvidence.map(item => item.claim)));
      const decisions = (snapshot.reviewDecisions || []).filter(item => item.reviewGateId === gate.id);
      for (const decision of decisions) {
        const row = element('div', 'maitu-link-row');
        row.append(chip(decision.actorRole === 'ai_reviewer' ? 'AI 复核' : decision.actorRole === 'human' ? '人工' : decision.actorRole,
          ['accept', 'accepted', 'recommend_accept'].includes(decision.decision) ? 'maitu-chip--produced' : ['reject', 'rejected', 'recommend_reject'].includes(decision.decision) ? 'maitu-chip--failed' : ''));
        row.append(element('span', 'maitu-note', decision.rationale || decision.decision), element('time', '', time(decision.createdAt)));
        block.append(row);
      }
      if (gate.status === 'pending_ai_review') {
        block.append(element('p', 'maitu-note', '等待独立 Review Worker 复核，完成后出现决策按钮。'));
        const decide = element('div', 'maitu-attention-actions');
        decide.append(button('撤回合入', () => goalCommand('merge.withdraw', {reviewGateId: gate.id, reason: '撤回本次合入', newEvidence: []}).then(refreshAfter)));
        block.append(decide);
      }
      if (gate.status === 'pending_human_review') {
        const contributions = snapshot.contributions.filter(item => item.sessionId === session.id);
        const checks = element('div', 'maitu-checks');
        for (const item of contributions) {
          const rowE = element('label');
          const input = element('input');
          input.type = 'checkbox'; input.name = 'gate-contributions'; input.value = item.id; input.checked = true;
          rowE.append(input, element('span', '', `${kindLabels[item.kind] || item.kind} · ${item.title}`));
          checks.append(rowE);
        }
        const wrap = element('fieldset', '');
        wrap.append(element('legend', '', '选择合入的贡献'), checks);
        block.append(wrap);
        const decide = element('div', 'maitu-attention-actions');
        const selectedIds = () => [...block.querySelectorAll('[name="gate-contributions"]:checked')].map(input => input.value);
        decide.append(button('通过并合入', () => goalCommand('review.human_decide', {
          reviewGateId: gate.id, decision: 'accept', rationale: '人工审核通过', selectedContributionIds: selectedIds()
        }).then(() => {feedback('门禁已通过，分支进入合入。'); return refreshAfter();})));
        decide.append(button('部分通过', () => goalCommand('review.human_decide', {
          reviewGateId: gate.id, decision: 'partial_accept', rationale: '部分贡献合入', selectedContributionIds: selectedIds()
        }).then(refreshAfter)));
        decide.append(button('驳回', () => goalCommand('review.human_decide', {reviewGateId: gate.id, decision: 'reject', rationale: '人工驳回', selectedContributionIds: []}).then(refreshAfter), 'maitu-button--danger'));
        decide.append(button('撤回合入', () => goalCommand('merge.withdraw', {reviewGateId: gate.id, reason: '撤回本次合入', newEvidence: []}).then(refreshAfter)));
        block.append(decide);
      }
      const show = button('候选快照', () => showOutput('合入候选快照', JSON.stringify(gate.candidateSnapshot, null, 2)));
      block.append(show);
      card.append(block);
    }
    const actions = element('div', 'maitu-goal-actions');
    actions.append(button('上下文', () => openContextCatalog(session)));
    actions.append(button('执行记录', () => openActionRuns(session)));
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

  async function openContextCatalog(session) {
    const page = await api(`/api/v1/projects/${projectId}/sessions/${session.id}/context/entries?limit=50`);
    const host = element('div', '');
    const entries = page.entries || [];
    if (!entries.length) host.append(element('p', 'maitu-note', '该会话还没有上下文目录。'));
    for (const entry of entries) {
      const row = element('button', 'maitu-link-row maitu-context-entry');
      row.type = 'button';
      row.append(chip(entry.sourceKind), element('strong', '', entry.title), element('span', 'maitu-note', entry.inclusionReason || ''));
      row.addEventListener('click', () => readContextEntry(session, page.snapshotId, entry).catch(error => feedback(error.message, true)));
      host.append(row);
    }
    openGoalDialog(`上下文 · ${entries.length} 条`, host);
  }
  async function readContextEntry(session, snapshotId, entry) {
    const result = await api(`/api/v1/projects/${projectId}/sessions/${session.id}/context/read`, 'POST', {
      clientRequestId: requestId(), snapshotId, entryId: entry.id, level: 'full', purpose: '用户查看上下文', query: null, actorType: 'human'
    });
    $('#maitu-goal-dialog').close();
    const content = result?.content ?? result?.text ?? JSON.stringify(result, null, 2);
    showOutput(entry.title, typeof content === 'string' ? content : JSON.stringify(content, null, 2));
  }
  async function openActionRuns(session) {
    const data = await api(`/api/v1/projects/${projectId}/sessions/${session.id}/action-runs`);
    const host = element('div', '');
    const runs = data.actions || [];
    if (!runs.length) host.append(element('p', 'maitu-note', '该会话还没有执行记录。'));
    for (const run of runs.slice(0, 30)) {
      const row = element('details', 'maitu-operation');
      row.append(element('summary', '', `${run.kind} · ${run.status} · 第 ${run.attemptCount} 次`));
      if (run.lastErrorSummary) row.append(element('p', 'maitu-error', run.lastErrorSummary));
      if (run.result) row.append(button('查看结果', () => showOutput(run.kind, JSON.stringify(run.result, null, 2))));
      host.append(row);
    }
    openGoalDialog(`执行记录 · ${runs.length} 条`, host);
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
    const attention = renderAttention();
    renderProposals();
    if (attention) $('#maitu-goals-proposals').prepend(attention);
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
    const integrations = (snapshot.integrations || []).filter(item => item.sourceGoalBranchId === branch.id);
    if (integrations.length) {
      const panel = element('section', 'maitu-goal-card');
      panel.append(element('h3', '', `合入记录 · ${integrations.length} 次`));
      for (const integration of integrations.slice().reverse()) {
        const row = element('div', 'maitu-link-row');
        const included = (snapshot.integrationContributions || []).filter(item => item.integrationId === integration.id);
        row.append(chip(integration.gitIntegrationStatus === 'succeeded' ? 'maitu-chip--produced' : integration.gitIntegrationStatus === 'failed' ? 'maitu-chip--failed' : ''));
        row.lastChild.textContent = integration.gitIntegrationStatus;
        row.append(element('strong', '', integration.summary || integration.kind), element('span', 'maitu-note', `${included.length} 条贡献 · ${time(integration.createdAt)}`));
        panel.append(row);
      }
      workspace.append(panel);
    }
    const archive = element('div', 'maitu-goal-actions');
    archive.append(button('归档分支', () => { if (confirm(`归档分支「${branch.name}」？`)) return goalCommand('goal_branch.archive', {goalBranchId: branch.id, reason: '手动归档'}).then(() => {selectedBranch = null; return refreshAfter();}); }, 'maitu-button--danger'));
    workspace.append(archive);
  }
  async function load() {
    snapshot = await api(`/api/v1/projects/${projectId}/goal-graph`);
    if (!selectedBranch && snapshot.branches.length) selectedBranch = snapshot.branches[0].id;
  }
})();
