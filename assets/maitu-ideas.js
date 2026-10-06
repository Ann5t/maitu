(() => {
  'use strict';
  const $ = (selector, root = document) => root.querySelector(selector);
  const mode = document.body.dataset.maituMode;
  const ideaId = document.body.dataset.projectId;
  const relationNames = {related:'有关联', supports:'支持', contradicts:'矛盾', depends_on:'依赖', duplicates:'重复'};
  const proposalStates = {draft:'草稿', submitted:'待决策', awaiting_approval:'待决策', approved:'已立项', rejected:'已驳回', cancelled:'已取消'};
  let snapshot;
  let ideaList = [];

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
  function showDiff(title, oldText, newText) {
    const a = oldText.split('\n'), b = newText.split('\n');
    const table = Array.from({length: a.length + 1}, () => new Array(b.length + 1).fill(0));
    for (let i = a.length - 1; i >= 0; i--) for (let j = b.length - 1; j >= 0; j--)
      table[i][j] = a[i] === b[j] ? table[i + 1][j + 1] + 1 : Math.max(table[i + 1][j], table[i][j + 1]);
    const rows = [];
    let i = 0, j = 0;
    while (i < a.length && j < b.length) {
      if (a[i] === b[j]) {rows.push('  ' + a[i]); i++; j++;}
      else if (table[i + 1][j] >= table[i][j + 1]) {rows.push('- ' + a[i]); i++;}
      else {rows.push('+ ' + b[j]); j++;}
    }
    while (i < a.length) {rows.push('- ' + a[i]); i++;}
    while (j < b.length) {rows.push('+ ' + b[j]); j++;}
    showContent(title + '（- 旧 / + 新）', rows.join('\n'));
  }
  function showContent(title, content) {
    const dialog = $('#maitu-idea-dialog');
    dialog.replaceChildren();
    const head = element('div', 'maitu-section-head');
    head.append(element('h2', '', title));
    const close = element('button', 'maitu-close', '×');
    close.type = 'button';
    close.addEventListener('click', () => dialog.close());
    head.append(close);
    const pre = element('pre', 'maitu-diff');
    pre.textContent = content || '没有差异。';
    dialog.append(head, pre);
    dialog.showModal();
  }
  async function api(path, method = 'GET', data, raw) {
    const headers = {};
    if (raw) headers['Content-Type'] = 'text/plain; charset=utf-8';
    else if (data !== undefined) headers['Content-Type'] = 'application/json';
    if (method !== 'GET') {
      const status = await fetch('/auth/status').then(r => r.ok ? r.json() : {}).catch(() => ({}));
      if (status.csrfToken) headers['x-csrf-token'] = status.csrfToken;
    }
    let response;
    try {
      response = await fetch(path, {method, headers, body: raw ?? (data === undefined ? undefined : JSON.stringify(data)), credentials: 'same-origin'});
    } catch { throw new Error('无法连接本机服务，请确认服务正在运行后重试。'); }
    let value;
    try {value = await response.json();} catch {throw new Error(`服务回复未能读取（HTTP ${response.status}），请刷新页面核对操作是否已经生效。`);}
    if (!response.ok) throw new Error(typeof value?.error === 'string' ? value.error : `请求失败（HTTP ${response.status}），请稍后重试。`);
    return value;
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
  function time(value) {return value ? new Date(value).toLocaleString() : '—';}
  function openDialog(title, body) {
    const dialog = $('#maitu-idea-dialog') || $('#maitu-idea-create-dialog');
    dialog.replaceChildren();
    const head = element('div', 'maitu-section-head');
    head.append(element('h2', '', title));
    const close = element('button', 'maitu-close', '×');
    close.type = 'button';
    close.setAttribute('data-close-dialog', dialog.id);
    close.setAttribute('aria-label', '关闭' + title);
    close.addEventListener('click', () => dialog.close());
    head.append(close);
    dialog.append(head, body);
    dialog.showModal();
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
    if (options) for (const [value, title] of options) {const option = element('option', '', title); option.value = value; control.append(option);}
    return control;
  }

  document.querySelectorAll('[data-close-dialog]').forEach(button => button.addEventListener('click', () => document.getElementById(button.dataset.closeDialog).close()));

  if (mode === 'ideas') {
    async function load() {
      const ideas = (await api('/api/v1/ideas')).ideas ?? [];
      ideaList = ideas;
      $('#maitu-ideas-count').textContent = ideas.length ? ideas.length + ' 条' : '';
      const list = $('#maitu-idea-list');
      list.replaceChildren();
      if (!ideas.length) {
        const empty = element('div', 'maitu-empty');
        empty.append(element('strong', '', '还没有记录任何想法'), element('p', '', '从右上角记下第一条，之后可以关联、修订，成熟时立项。'));
        list.append(empty);
        return;
      }
      for (const idea of ideas) {
        const card = element('a', 'maitu-project-card maitu-idea-card');
        card.href = '/maitu/ideas/' + idea.id;
        const top = element('div', 'maitu-card-top');
        top.append(chip(idea.state === 'archived' ? '已归档' : '进行中', idea.state === 'archived' ? '' : 'maitu-chip--produced'));
        if (idea.promotedProjectId) top.append(chip('已立项', 'maitu-chip--produced'));
        card.append(top, element('h3', '', idea.title), element('p', '', idea.body));
        const bottom = element('div', 'maitu-card-bottom');
        const bits = [`第 ${idea.currentRevision} 版`];
        if (idea.linkCount) bits.push(`${idea.linkCount} 条关联`);
        if (idea.proposalCount) bits.push(`${idea.proposalCount} 次立项`);
        bottom.append(element('span', '', bits.join(' · ')), element('span', '', new Date(idea.updatedAt).toLocaleDateString()));
        card.append(bottom);
        list.append(card);
      }
    }
    $('#maitu-idea-new').addEventListener('click', () => $('#maitu-idea-create-dialog').showModal());
    $('#maitu-idea-create').addEventListener('submit', async event => {
      event.preventDefault();
      const form = event.currentTarget;
      const submit = $('button[type="submit"]', form);
      submit.disabled = true;
      try {
        const data = Object.fromEntries(new FormData(form));
        const response = await api('/api/v1/ideas', 'POST', {
          clientRequestId: requestId(), action: 'idea.create',
          payload: {revision: {title: data.title, body: data.body || data.title, sourceKind: 'text', sourceRef: null}}
        });
        const id = response.result?.ideaId || response.result?.id;
        if (!id) throw new Error('想法已提交，请刷新列表查看。');
        location.assign('/maitu/ideas/' + id);
      } catch (error) {feedback(error.message, true); submit.disabled = false;}
    });
    load().catch(error => feedback(error.message, true));
  }

  if (mode === 'idea') {
    function command(action, payload, extraPath = '/commands') {
      return api(`/api/v1/ideas/${ideaId}${extraPath}`, 'POST', {clientRequestId: requestId(), action, payload});
    }
    function proposalCommand(proposalId, action, payload) {
      return api(`/api/v1/project-proposals/${proposalId}/commands`, 'POST', {clientRequestId: requestId(), action, payload});
    }
    function currentRevision() {
      return snapshot.revisions.find(item => item.revision === snapshot.idea.currentRevision) || snapshot.revisions[0];
    }
    function renderHeader() {
      const revision = currentRevision();
      const idea = {...snapshot.idea, title: revision?.title || '未命名', body: revision?.body || ''};
      document.title = idea.title + ' · 脉图';
      $('#maitu-idea-title').textContent = idea.title;
      $('#maitu-idea-meta').textContent = `第 ${idea.currentRevision} 版 · 更新于 ${time(idea.updatedAt)}`;
      const state = $('#maitu-idea-state');
      state.replaceChildren(chip(idea.state === 'archived' ? '已归档' : '进行中', idea.state === 'archived' ? '' : 'maitu-chip--produced'));
      const actions = $('#maitu-idea-actions');
      actions.replaceChildren();
      const revise = button('修订内容', () => openReviseDialog(), 'maitu-button--primary');
      revise.classList.remove('maitu-button--small');
      actions.append(revise, button('关联想法', openLinkDialog), button('立项提案', openProposalDialog));
      if (idea.state !== 'archived') actions.append(button('归档', async () => {
        if (!confirm(`归档想法「${idea.title}」？历史修订保留，可随时在需要时查阅。`)) return;
        await command('idea.archive', {reason: '手动归档'});
        feedback('想法已归档。'); await load();
      }, 'maitu-button--danger'));
    }
    function renderBody() {
      const revision = snapshot.revisions.find(item => item.revision === snapshot.idea.currentRevision) || snapshot.revisions.at(-1);
      const body = $('#maitu-idea-body');
      body.textContent = revision?.body || snapshot.idea.body || '';
      const sources = $('#maitu-idea-sources');
      sources.replaceChildren();
      if (snapshot.sources.length) {
        const head = element('div', 'maitu-section-head');
        head.append(element('h3', '', '已附来源'), button('添加文本来源', openSourceDialog));
        const rows = element('div', '');
        for (const source of snapshot.sources) {
          const row = element('div', 'maitu-source-row');
          row.append(element('strong', '', source.displayName || source.originalFilename),
            element('span', '', (source.sizeBytes / 1024).toFixed(1) + ' KiB'));
          rows.append(row);
        }
        sources.append(head, rows);
      } else {
        const head = element('div', 'maitu-section-head');
        head.append(element('h3', '', '来源'), button('添加文本来源', openSourceDialog));
        sources.append(head, element('p', 'maitu-note', '附加资料会随立项提案进入项目目标。'));
      }
    }
    function renderHistory() {
      const history = $('#maitu-idea-history');
      history.replaceChildren();
      const count = $('#maitu-idea-history-count');
      count.textContent = `${snapshot.revisions.length} 次修订 · ${snapshot.links.length} 条关联 · ${snapshot.proposals.length} 次立项`;
      const revisions = element('ol', 'maitu-revision-list');
      for (const revision of [...snapshot.revisions].reverse()) {
        const item = element('li', 'maitu-revision');
        const head = element('div', 'maitu-revision-head');
        head.append(chip(`第 ${revision.revision} 版`, revision.revision === snapshot.idea.currentRevision ? 'maitu-chip--produced' : ''), element('time', '', time(revision.createdAt)));
        if (revision.revision > 1) {
          const diffBtn = button('对比上一版', () => {
            const previous = snapshot.revisions.find(r => r.revision === revision.revision - 1);
            showDiff(revision.title, previous?.body || '', revision.body || '');
          });
          head.append(diffBtn);
        }
        item.append(head, element('strong', '', revision.title));
        if (revision.revisionReason) item.append(element('p', 'maitu-note', revision.revisionReason));
        revisions.append(item);
      }
      history.append(revisions);
      if (snapshot.links.length) {
        const head = element('h3', '', '关联想法');
        const list = element('div', '');
        for (const link of snapshot.links) {
          const row = element('div', 'maitu-link-row');
          const other = link.sourceIdeaId === ideaId ? {id: link.targetIdeaId, title: link.targetTitle, revision: link.targetRevision} : {id: link.sourceIdeaId, title: link.sourceTitle, revision: link.sourceRevision};
          row.append(chip(relationNames[link.relation] || link.relation),
            element('a', '', `${other.title}（第 ${other.revision} 版）`));
          row.lastChild.href = '/maitu/ideas/' + other.id;
          if (link.rationale) row.append(element('span', 'maitu-note', link.rationale));
          list.append(row);
        }
        history.append(head, list);
      }
      if (snapshot.proposals.length) {
        history.append(element('h3', '', '立项提案'));
        const list = element('div', '');
        for (const proposal of snapshot.proposals) {
          const revision = snapshot.proposalRevisions.filter(item => item.proposalId === proposal.id)
            .find(item => item.revision === proposal.currentRevision);
          const card = element('div', 'maitu-connection-card');
          const head = element('div', 'maitu-connection-head');
          head.append(element('strong', '', revision?.title || '立项提案'), chip(proposalStates[proposal.status] || proposal.status,
            proposal.status === 'approved' ? 'maitu-chip--produced' : ['submitted', 'awaiting_approval'].includes(proposal.status) ? 'maitu-chip--queued' : proposal.status === 'rejected' ? 'maitu-chip--failed' : ''));
          card.append(head, element('p', 'maitu-note', `${revision?.projectIntent || ''}${revision?.whyNow ? ' · ' + revision.whyNow : ''}`));
          const actions = element('div', 'maitu-connection-actions');
          if (proposal.status === 'draft') {
            actions.append(button('编辑', () => openIdeaProposalRevise(proposal, revision)));
            actions.append(button('提交评审', () => proposalCommand(proposal.id, 'project_proposal.submit', {expectedRevision: proposal.currentRevision}).then(refreshAfter)));
            actions.append(button('取消提案', () => proposalCommand(proposal.id, 'project_proposal.cancel', {reason: '手动取消'}).then(refreshAfter), 'maitu-button--danger'));
          }
          if (['submitted', 'awaiting_approval'].includes(proposal.status)) {
            actions.append(button('批准立项', () => proposalCommand(proposal.id, 'project_proposal.approve', {expectedRevision: proposal.currentRevision}).then(response => {
              const projectId = response.result?.projectId;
              feedback(projectId ? '提案已批准，正在打开新项目…' : '提案已批准。');
              if (projectId) setTimeout(() => location.assign('/maitu/projects/' + projectId), 600);
              return refreshAfter();
            }), 'maitu-button--primary'));
            actions.append(button('驳回', () => proposalCommand(proposal.id, 'project_proposal.reject', {rationale: '手动驳回'}).then(refreshAfter), 'maitu-button--danger'));
          }
          if (proposal.status === 'approved' && proposal.approvedProjectId) {
            const open = element('a', 'maitu-button maitu-button--small', '打开项目 ↗');
            open.href = '/maitu/projects/' + proposal.approvedProjectId;
            actions.append(open);
          }
          card.append(actions);
          list.append(card);
        }
        history.append(list);
      }
    }
    function refreshAfter() { return load(); }

    function openReviseDialog() {
      const current = currentRevision();
      const form = element('form', '');
      form.append(
        formRow('标题', field('title', {value: current?.title || ''})),
        formRow('内容', field('body', {tag: 'textarea', value: current?.body || '', rows: 4})));
      const submit = element('button', 'maitu-button maitu-button--primary', '保存');
      submit.type = 'submit';
      form.append(submit);
      form.addEventListener('submit', async event => {
        event.preventDefault();
        submit.disabled = true;
        try {
          const data = Object.fromEntries(new FormData(form));
          await command('idea.revise', {expectedRevision: snapshot.idea.currentRevision, revision: {
            title: data.title, body: data.body, sourceKind: 'text', sourceRef: null, revisionReason: null
          }});
          $('#maitu-idea-dialog').close();
          feedback('修订已保存。'); await load();
        } catch (error) {feedback(error.message, true); submit.disabled = false;}
      });
      openDialog('修订', form);
    }
    function openLinkDialog() {
      const form = element('form', '');
      const others = ideaList.filter(item => item.id !== ideaId);
      if (!others.length) {feedback('还没有其他想法可以关联。'); return;}
      form.append(
        formRow('关联到', field('targetIdeaId', {options: others.map(item => [item.id, item.title])})),
        formRow('关系', field('relation', {options: Object.entries(relationNames)})),
        formRow('说明（可选）', field('rationale', {required: false, placeholder: ''})));
      const submit = element('button', 'maitu-button maitu-button--primary', '保存');
      submit.type = 'submit';
      form.append(submit);
      form.addEventListener('submit', async event => {
        event.preventDefault();
        submit.disabled = true;
        try {
          const data = Object.fromEntries(new FormData(form));
          const target = others.find(item => item.id === data.targetIdeaId);
          await command('idea.link', {
            expectedSourceRevision: snapshot.idea.currentRevision,
            targetIdeaId: data.targetIdeaId, expectedTargetRevision: target.currentRevision,
            relation: data.relation, rationale: data.rationale || ''
          });
          $('#maitu-idea-dialog').close();
          feedback('关联已保存。'); await load();
        } catch (error) {feedback(error.message, true); submit.disabled = false;}
      });
      openDialog('关联', form);
    }
    function openProposalDialog() {
      const current = currentRevision();
      const form = element('form', '');
      form.append(formRow('目标', field('goal', {tag: 'textarea', rows: 2, placeholder: '这个项目要达成什么、怎样算完成', value: current?.title || ''})));
      const advanced = element('details', 'maitu-advanced');
      advanced.append(element('summary', '', '高级'));
      const reasons = formRow('为什么是现在（可选）', field('whyNow', {required: false}));
      advanced.append(reasons);
      form.append(advanced);
      const submit = element('button', 'maitu-button maitu-button--primary', '创建提案');
      submit.type = 'submit';
      form.append(submit);
      form.addEventListener('submit', async event => {
        event.preventDefault();
        submit.disabled = true;
        try {
          const data = Object.fromEntries(new FormData(form));
          const goalText = data.goal.trim();
          const [firstLine, ...rest] = goalText.split('\n');
          const title = firstLine.slice(0, 60) || current?.title || '立项';
          const desiredOutcome = rest.join('\n').trim() || firstLine;
          await command('project_proposal.create', {revision: {
            title, projectIntent: goalText, whyNow: data.whyNow || '',
            rootGoal: {
              whyNeeded: data.whyNow || firstLine,
              contract: {
                desiredOutcome, hardConstraints: [],
                subjectivePreferences: [], unknowns: [], nonGoals: [],
                validationPlan: ['使用中按真实任务验证'], judgmentTriggers: [], stopConditions: ['目标达成或明确放弃'],
                expectedContributions: [],
                exploration: {mode: 'delivery', budgets: [], candidateOutputs: [], uncertaintyReduction: []}
              },
              expectedContributions: [], explorationPlan: [], contextInheritance: {}, toolRequirements: [], inferences: [], revisionReason: null
            },
            retainedNotes: [], omittedNotes: [],
            sources: [{ideaId: ideaId, ideaRevision: snapshot.idea.currentRevision, role: 'source', rationale: '当前想法是立项来源'}],
            revisionReason: null
          }});
          $('#maitu-idea-dialog').close();
          feedback('提案已创建，提交后可批准立项。'); await load();
        } catch (error) {feedback(error.message, true); submit.disabled = false;}
      });
      openDialog('立项', form);
    }
    function openIdeaProposalRevise(proposal, revision) {
      const form = element('form', '');
      form.append(formRow('目标', field('goal', {tag: 'textarea', rows: 2, value: (revision?.projectIntent || revision?.title || '')})));
      const submit = element('button', 'maitu-button maitu-button--primary', '保存');
      submit.type = 'submit';
      form.append(submit);
      form.addEventListener('submit', async event => {
        event.preventDefault();
        submit.disabled = true;
        try {
          const goalText = Object.fromEntries(new FormData(form)).goal.trim();
          const [firstLine, ...rest] = goalText.split('\n');
          const base = revision ? JSON.parse(JSON.stringify(revision.rootGoal)) : null;
          const contract = base?.contract || {desiredOutcome: '', hardConstraints: [], subjectivePreferences: [], unknowns: [], nonGoals: [], validationPlan: [], judgmentTriggers: [], stopConditions: [], expectedContributions: [], exploration: {mode: 'delivery', budgets: [], candidateOutputs: [], uncertaintyReduction: []}};
          contract.desiredOutcome = rest.join('\n').trim() || firstLine;
          await proposalCommand(proposal.id, 'project_proposal.revise', {expectedRevision: proposal.currentRevision, revision: {
            title: firstLine.slice(0, 60) || revision?.title || '立项',
            projectIntent: goalText, whyNow: revision?.whyNow || '',
            rootGoal: base || {whyNeeded: firstLine, contract, expectedContributions: [], explorationPlan: [], contextInheritance: {}, toolRequirements: [], inferences: [], revisionReason: null},
            retainedNotes: [], omittedNotes: [],
            sources: [{ideaId: ideaId, ideaRevision: snapshot.idea.currentRevision, role: 'source', rationale: '当前想法是立项来源'}],
            revisionReason: '界面编辑'
          }});
          $('#maitu-idea-dialog').close();
          feedback('提案已修订。'); await load();
        } catch (error) {feedback(error.message, true); submit.disabled = false;}
      });
      openDialog('编辑提案', form);
    }

    function openSourceDialog() {
      const form = element('form', '');
      const file = element('input');
      file.type = 'file';
      file.accept = '.txt,.md,.json,.csv,text/*';
      file.multiple = true;
      form.append(formRow('上传文件', file));
      form.append(formRow('或粘贴', field('content', {tag: 'textarea', rows: 4})));
      const submit = element('button', 'maitu-button maitu-button--primary', '保存来源');
      submit.type = 'submit';
      form.append(submit);
      file.addEventListener('change', async () => {
        if (!file.files.length) return;
        submit.disabled = true;
        try {
          for (const item of file.files) {
            if (item.size > 256 * 1024) throw new Error(item.name + ' 超过 256 KiB');
            const content = new TextDecoder('utf-8', {fatal: true}).decode(await item.arrayBuffer());
            const query = new URLSearchParams({client_request_id: requestId(), expected_revision: String(snapshot.idea.currentRevision), filename: item.name});
            await api(`/api/v1/ideas/${ideaId}/sources?${query}`, 'POST', undefined, content);
          }
          $('#maitu-idea-dialog').close();
          feedback('来源已附加。'); await load();
        } catch (error) {feedback(error.message, true); submit.disabled = false;}
      });
      form.addEventListener('submit', async event => {
        event.preventDefault();
        if (file.files.length) return;
        submit.disabled = true;
        try {
          const data = Object.fromEntries(new FormData(form));
          if (!data.content.trim()) throw new Error('粘贴内容或选择文件。');
          const query = new URLSearchParams({client_request_id: requestId(), expected_revision: String(snapshot.idea.currentRevision), filename: '想法资料.txt'});
          await api(`/api/v1/ideas/${ideaId}/sources?${query}`, 'POST', undefined, data.content);
          $('#maitu-idea-dialog').close();
          feedback('来源已附加。'); await load();
        } catch (error) {feedback(error.message, true); submit.disabled = false;}
      });
      openDialog('添加来源', form);
    }
    async function load() {
      snapshot = await api(`/api/v1/ideas/${ideaId}`);
      ideaList = (await api('/api/v1/ideas')).ideas ?? [];
      renderHeader(); renderBody(); renderHistory();
    }
    load().catch(error => feedback(error.message, true));
  }
})();
