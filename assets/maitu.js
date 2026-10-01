(() => {
  'use strict';
  const $ = (selector, root = document) => root.querySelector(selector);
  const mode = document.body.dataset.maituMode;
  const projectId = document.body.dataset.projectId;
  const statusNames = {draft:'待启动',queued:'等待执行',running:'执行中',produced:'已产出',failed:'执行失败',interrupted:'执行中断',cancelled:'已取消'};
  let snapshot;
  let selectedTask;
  let selectedAttempt;
  let taskRequestId;
  let planGenerateRequestId;
  let planReviewRecord;
  let codeImportRequestId;
  let codeImportFiles=[];
  let codeImportSource;
  let retryTaskId;
  let retryRequestId;
  let refreshing = false;
  let initialMapLayout = true;
  let mapZoom = 1;
  let adoptTaskId;
  let adoptAttemptId;

  function layoutMemory() {
    try {
      const raw = localStorage.getItem('maitu-map-' + projectId);
      return raw ? JSON.parse(raw) : {};
    } catch { return {}; }
  }
  function saveLayout(patch) {
    try {
      const merged = Object.assign(layoutMemory(), patch);
      localStorage.setItem('maitu-map-' + projectId, JSON.stringify(merged));
    } catch { /* layout stays for this page only */ }
  }
  function applyZoom() {
    const map = $('#maitu-map');
    if (!map) return;
    map.style.transformOrigin = '0 0';
    map.style.transform = `scale(${mapZoom})`;
    const level = $('#maitu-zoom-level');
    if (level) level.textContent = Math.round(mapZoom * 100) + '%';
  }
  function setZoom(value) {
    mapZoom = Math.min(2.5, Math.max(0.4, value));
    applyZoom();
    saveLayout({zoom: mapZoom});
  }
  function focusSelected() {
    if (!selectedTask) {feedback('先选择一个节点，再聚焦。', true); return;}
    const node = document.querySelector(`[data-select-task="${selectedTask}"]`);
    if (!node) return;
    node.scrollIntoView({behavior:'smooth', inline:'center', block:'center'});
    saveLayout({focus: selectedTask});
  }
  const outputCache = new Map();
  const startRequests = new Map();
  const openOperations = new Set();

  function requestId() {
    if (typeof crypto.randomUUID === 'function') return crypto.randomUUID();
    const bytes = crypto.getRandomValues(new Uint8Array(16));
    bytes[6] = (bytes[6] & 15) | 64;
    bytes[8] = (bytes[8] & 63) | 128;
    const hex = Array.from(bytes, byte => byte.toString(16).padStart(2, '0')).join('');
    return `${hex.slice(0,8)}-${hex.slice(8,12)}-${hex.slice(12,16)}-${hex.slice(16,20)}-${hex.slice(20)}`;
  }

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
  async function api(path, method = 'GET', data) {
    const headers = {};
    if (data !== undefined) headers['Content-Type'] = 'application/json';
    if (method !== 'GET') {
      const status = await fetch('/auth/status').then(r => r.ok ? r.json() : {}).catch(() => ({}));
      if (status.csrfToken) headers['x-csrf-token'] = status.csrfToken;
    }
    let response;
    try {
      response = await fetch(path, {method, headers, body:data === undefined ? undefined : JSON.stringify(data), credentials:'same-origin'});
    } catch {
      throw new Error('无法连接本机服务，请确认服务正在运行后重试。');
    }
    function unreadableResponse() {
      const status = response.status;
      if ([400,415,422].includes(status)) return `填写内容格式不正确（HTTP ${status}）。请刷新页面后检查设置，再保存。`;
      if ([401,403].includes(status)) return `访问被拒绝（HTTP ${status}）。请重新打开页面并确认登录状态。`;
      if (status === 404) return '接口不存在（HTTP 404），请刷新页面以加载当前版本。';
      if (status === 413) return '提交内容过大（HTTP 413），请减少内容后重试。';
      if (status === 429) return '请求过于频繁（HTTP 429），请稍后重试。';
      if (status >= 500) return `本机服务暂时无法完成请求（HTTP ${status}），请稍后重试。`;
      return `服务回复未能读取（HTTP ${status}），请刷新页面核对操作是否已经生效。`;
    }
    let value;
    try {value = await response.json();} catch {throw new Error(unreadableResponse());}
    if (!response.ok) throw new Error(typeof value?.error === 'string' ? value.error : unreadableResponse());
    return value;
  }
  function handle(action) {
    return async event => {
      const button = event.currentTarget;
      button.disabled = true;
      try {await action();} catch (error) {feedback(error.message, true);} finally {button.disabled = false;}
    };
  }
  function button(text, action, primary = false) {
    const node = element('button', `maitu-button maitu-button--small${primary ? ' maitu-button--primary' : ''}`, text);
    node.type = 'button';
    node.addEventListener('click', handle(action));
    return node;
  }
  function time(value) {return value ? new Date(value).toLocaleString() : '—';}

  document.querySelectorAll('[data-close-dialog]').forEach(button => button.addEventListener('click', () => document.getElementById(button.dataset.closeDialog).close()));
  document.querySelectorAll('[data-theme-set]').forEach(button => button.addEventListener('click', () => {
    const theme=button.dataset.themeSet;
    document.documentElement.dataset.theme=theme;
    document.documentElement.style.colorScheme=theme;
    try {localStorage.setItem('fudian-theme',theme);} catch { /* Theme remains available for this page. */ }
  }));

  if (mode === 'dashboard') {
    $('#maitu-project-create').addEventListener('submit', async event => {
      event.preventDefault();
      const form = event.currentTarget;
      const submit = $('button[type="submit"]', form);
      submit.disabled = true;
      try {
        const project = await api('/api/projects', 'POST', {intent:form.elements.intent.value});
        const id = project.projectId || project.id || project.project?.id;
        if (!id) throw new Error('项目已提交，请刷新项目列表查看。');
        location.assign(`/maitu/projects/${id}`);
      } catch (error) {feedback(error.message, true); submit.disabled = false;}
    });
  }

  if (mode === 'settings') {
    const form = $('#maitu-connection-form');
    const list = $('#maitu-connection-list');
    for (const name of ['contextTokens','maxTokens']) {
      form.elements[name].max = name === 'contextTokens' ? 1048576 : 393216;
    }
    function modeHint() {
      $('#maitu-model-limits').textContent = form.elements.thinkingEnabled.value === 'true'
        ? 'DeepSeek：上下文容量 1,048,576 Token，最大输出 393,216 Token。思考模式下参考默认输出上限 65,536 Token。'
        : 'DeepSeek：上下文容量 1,048,576 Token，最大输出 393,216 Token。非思考模式参考默认输出上限 8,192 Token。';
    }
    form.elements.thinkingEnabled.addEventListener('change', modeHint);
    function setEditing(connection) {
      form.elements.originalKey.value = connection ? connection.key : '';
      form.elements.key.value = connection ? connection.key : '';
      form.elements.key.readOnly = Boolean(connection);
      form.elements.label.value = connection ? connection.label : '';
      form.elements.baseUrl.value = connection ? connection.baseUrl : 'https://api.deepseek.com';
      form.elements.model.value = connection ? connection.model : 'deepseek-flash';
      form.elements.thinkingEnabled.value = String(connection ? connection.thinkingEnabled : true);
      form.elements.concurrency.value = connection ? connection.concurrency : 3;
      form.elements.contextTokens.value = connection ? connection.contextTokens : 1048576;
      form.elements.maxTokens.value = connection ? connection.maxTokens : 65536;
      form.elements.enabled.checked = connection ? connection.enabled : true;
      form.elements.apiKey.value = '';
      form.elements.apiKey.placeholder = connection ? '留空保留当前密钥' : '首次配置时填写';
      $('#maitu-connection-form-title').textContent = connection ? `编辑连接 ${connection.label}` : '添加连接';
      $('#maitu-connection-cancel').hidden = !connection;
      modeHint();
    }
    $('#maitu-connection-cancel').addEventListener('click', () => setEditing(null));
    function render(connections) {
      list.replaceChildren();
      if (!connections.length) list.append(element('p','maitu-note','还没有连接。请先添加一个。'));
      for (const connection of connections) {
        const card = element('div','maitu-connection-card');
        if (!connection.enabled) card.classList.add('is-disabled');
        const head = element('div','maitu-connection-head');
        head.append(
          element('strong','',connection.label + ' '),
          element('span','maitu-note',connection.key + ' · ' + connection.model),
          element('span','maitu-status ' + (connection.configured && connection.enabled ? 'maitu-status--produced' : 'maitu-status--failed'),
            connection.configured ? (connection.enabled ? '已启用' : '已停用') : '待填写密钥')
        );
        card.append(head, element('p','maitu-note',
          `${connection.baseUrl} · 并发 ${connection.concurrency} · 上下文 ${connection.contextTokens.toLocaleString('en-US')} · 输出上限 ${connection.maxTokens.toLocaleString('en-US')} Token`));
        const actions = element('div','maitu-connection-actions');
        actions.append(button('编辑',() => setEditing(connection)));
        actions.append(button(connection.enabled ? '停用' : '启用', async () => {
          await api('/api/maitu/connections','POST',{...connection,enabled:!connection.enabled,apiKey:''});
          await load(); feedback(connection.enabled ? '连接已停用，其排队任务会等待其他连接。' : '连接已启用。');
        }));
        actions.append(button('删除', async () => {
          if (!confirm(`删除连接「${connection.label}」？历史记录保留，但排队中的任务将等待其他连接。`)) return;
          await api(`/api/maitu/connections/${encodeURIComponent(connection.key)}`,'DELETE');
          await load(); feedback('连接已删除。');
        }));
        card.append(actions);
        list.append(card);
      }
    }
    async function load() {
      render(await api('/api/maitu/connections'));
      $('#maitu-connections-state').textContent = '以实际任务结果验证连接是否可用。';
    }
    form.addEventListener('submit', async event => {
      event.preventDefault();
      const submit = $('button[type="submit"]',form);
      submit.disabled = true;
      const data = Object.fromEntries(new FormData(form));
      const payload = {
        key: data.key, label: data.label, baseUrl: data.baseUrl, model: data.model,
        apiKey: data.apiKey, concurrency: Number(data.concurrency),
        contextTokens: Number(data.contextTokens), maxTokens: Number(data.maxTokens),
        thinkingEnabled: data.thinkingEnabled === 'true', enabled: form.elements.enabled.checked
      };
      if (payload.maxTokens >= payload.contextTokens) {
        feedback('最大输出长度须小于上下文预算，为任务要求和资料保留输入空间。', true);
        submit.disabled = false;
        return;
      }
      try {
        await api('/api/maitu/connections','POST',payload);
        setEditing(null);
        await load();
        feedback('连接已保存，新启动的模型请求使用此配置。');
      } catch(error) {feedback(error.message,true);} finally {submit.disabled=false;}
    });
    setEditing(null);
    load().catch(error => feedback(error.message,true));
  }

  async function startTask(id) {
    if (!startRequests.has(id)) startRequests.set(id, requestId());
    await api(`/api/maitu/tasks/${id}/start`,'POST',{requestId:startRequests.get(id)});
    startRequests.delete(id);
    await refresh();
  }
  function selectTask(id) {
    selectedTask = id;
    selectedAttempt = undefined;
    renderMap();
    renderDetail().catch(error => feedback(error.message,true));
  }
  function depthOf(taskId, memo) {
    if (memo.has(taskId)) return memo.get(taskId);
    const parents = snapshot.dependencies.filter(edge => edge.taskId === taskId);
    const depth = parents.length ? 1 + Math.max(...parents.map(edge => depthOf(edge.parentTaskId,memo))) : 0;
    memo.set(taskId, depth);
    return depth;
  }
  function renderMap() {
    const map = $('#maitu-map');
    const previousFocus = document.activeElement?.dataset.selectTask;
    map.replaceChildren();
    const depths = new Map();
    const columns = new Map();
    for (const task of snapshot.tasks) {
      const depth = depthOf(task.id, depths);
      if (!columns.has(depth)) columns.set(depth,[]);
      columns.get(depth).push(task);
    }
    const rows = Math.max(1,...Array.from(columns.values(), tasks => tasks.length));
    let height = Math.max(380, rows*210+70);
    const width = Math.max(650,(Math.max(0,...depths.values())+2)*304+30);
    map.style.width = `${width}px`;
    map.style.height = `${height}px`;
    const locations = new Map();
    const origin = element('div','maitu-origin');
    origin.style.left = '24px'; origin.style.top = `${height/2-75}px`;
    origin.append(element('span','maitu-eyebrow','项目目标'),element('strong','',snapshot.project.title),element('p','',`${snapshot.sources.length} 份资料 · ${snapshot.tasks.length} 个任务`));
    map.append(origin);
    for (const [depth,tasks] of columns) {
      tasks.forEach((task,index) => locations.set(task.id,{x:(depth+1)*304+24,y:35+index*210}));
    }
    const svg = document.createElementNS('http://www.w3.org/2000/svg','svg');
    svg.setAttribute('width',width); svg.setAttribute('height',height); svg.setAttribute('aria-hidden','true');
    svg.classList.add('maitu-edges');
    function edge(from,to) {
      const path = document.createElementNS('http://www.w3.org/2000/svg','path');
      const mid = (from.x+to.x)/2;
      path.setAttribute('d',`M${from.x},${from.y} C${mid},${from.y} ${mid},${to.y} ${to.x},${to.y}`);
      path.setAttribute('class','maitu-edge'); svg.append(path);
    }
    map.prepend(svg);
    for (const task of snapshot.tasks) {
      const position = locations.get(task.id);
      const node = element('article',`maitu-node maitu-node--${task.status}${task.id===selectedTask?' is-selected':''}`);
      node.dataset.taskId = task.id;
      node.style.left=`${position.x}px`; node.style.top=`${position.y}px`;
      const select = element('button','maitu-node-select');
      select.type='button'; select.dataset.selectTask=task.id;
      select.setAttribute('aria-pressed',String(task.id===selectedTask));
      const kindName={file:'资料',plan:'计划',code:'编码'}[task.taskKind]||'资料';
      select.append(element('span',`maitu-status maitu-status--${task.status}`,`${kindName} · ${statusNames[task.status]||task.status}`),element('strong','',task.title),element('p','',task.waitReason||task.instruction));
      select.addEventListener('click',()=>selectTask(task.id));
      node.append(select);
      const actions=element('div','maitu-node-actions');
      if (!['running','queued'].includes(task.status)) actions.append(button(task.status==='draft'?'执行':task.status==='produced'?'再执行一次':'重试',()=>startTask(task.id)));
      actions.append(button('记录与成果',()=>selectTask(task.id)));
      if (task.acceptedAttemptId) actions.append(element('span','maitu-adopted','已采用成果'));
      node.append(actions); map.append(node); position.node=node;
    }
    for(const position of locations.values()) position.height=position.node.getBoundingClientRect().height;
    height=380;
    for(const tasks of columns.values()) {
      let y=35;
      for(const task of tasks) {
        const position=locations.get(task.id);position.y=y;position.node.style.top=y+'px';y+=position.height+26;
      }
      height=Math.max(height,y+30);
    }
    map.style.height=height+'px';origin.style.top=(height/2-origin.getBoundingClientRect().height/2)+'px';
    svg.setAttribute('height',height);
    for(const task of snapshot.tasks) {
      const position=locations.get(task.id);
      const target={x:position.x,y:position.y+position.height/2};
      const parents=snapshot.dependencies.filter(item=>item.taskId===task.id);
      if(!parents.length) edge({x:272,y:height/2},target);
      for(const parent of parents) {
        const previous=locations.get(parent.parentTaskId);
        edge({x:previous.x+248,y:previous.y+previous.height/2},target);
      }
    }
    if (!snapshot.tasks.length) {
      const empty=element('div','maitu-map-empty');
      empty.style.left='328px';
      empty.append(element('strong','','把目标拆成独立任务'),element('p','','例如：整理资料、检查风险、拟定推进计划。每个任务都能独立执行并留下文件。'));
      map.append(empty);
    }
    if (previousFocus) map.querySelector(`[data-select-task="${previousFocus}"]`)?.focus({preventScroll:true});
    if (initialMapLayout && snapshot.tasks.length && window.innerWidth <= 720) {
      $('.maitu-map-scroll').scrollLeft = locations.get(snapshot.tasks[0].id).x - 16;
    }
    initialMapLayout = false;
  }

  function renderSources() {
    const list=$('#maitu-source-list'); list.replaceChildren();
    if (!snapshot.sources.length) list.append(element('p','maitu-note','还没有资料。添加文件或粘贴内容，供任务读取。'));
    for (const source of snapshot.sources) {
      const row=element('div','maitu-source-row');
      row.append(element('strong','',source.filename),element('span','',`${(source.sizeBytes/1024).toFixed(1)} KiB`),button('查看',async()=> {
        const record=await api(`/api/maitu/sources/${source.id}`);
        showContent(record.filename,record.content);
      }));
      list.append(row);
    }
  }
  function showContent(title,content) {
    $('#maitu-output-title').textContent=title;
    $('#maitu-output-content').textContent=content;
    $('#maitu-output-dialog').showModal();
  }
  async function showOutput(attempt,filename) {
    if (!outputCache.has(attempt.artifactId)) {
      const response=await fetch(`/artifacts/${attempt.artifactId}`);
      if (!response.ok) throw new Error('成果文件暂时无法打开，请检查执行记录。');
      outputCache.set(attempt.artifactId,await response.text());
    }
    showContent(filename,outputCache.get(attempt.artifactId));
  }
  async function renderDetail() {
    if (!selectedTask) return;
    const id=selectedTask;
    const detail=await api(`/api/maitu/tasks/${id}`);
    if (selectedTask!==id) return;
    const panel=$('#maitu-detail');
    const oldScroll=panel.scrollTop;
    panel.replaceChildren();
    panel.append(element('p','maitu-eyebrow','任务详情'),element('h2','',detail.task.title),element('p','maitu-detail-instruction',detail.task.instruction));
    if (detail.task.acceptanceCriteria) panel.append(element('p','maitu-note',`验收要求：${detail.task.acceptanceCriteria}`));
    if (detail.task.waitReason) panel.append(element('p','maitu-wait',detail.task.waitReason));
    if (!['running','queued'].includes(detail.task.status)) {
      panel.append(button(detail.task.status==='draft'?'执行任务':'再执行一次',()=>startTask(id),true));
      panel.append(button('补充要求再执行',()=> {
        retryTaskId=id;retryRequestId=requestId();$('#maitu-retry-form').reset();$('#maitu-retry-dialog').showModal();
      }));
    }
    panel.append(element('h3','','历次尝试'));
    if (!detail.attempts.length) panel.append(element('p','maitu-note','尚未启动。执行后会保存输入、请求时段、过程记录和成果。'));
    for (const attempt of detail.attempts) {
      const section=element('details','maitu-attempt');
      section.open=selectedAttempt ? selectedAttempt===attempt.id : attempt.id===detail.attempts[0].id;
      section.addEventListener('toggle',()=> {if(section.open) selectedAttempt=attempt.id;});
      const summary=element('summary','',`第 ${attempt.number} 次 · ${statusNames[attempt.status]||attempt.status}${detail.task.acceptedAttemptId===attempt.id?' · 已采用':''}`);
      section.append(summary,element('p','maitu-note',`创建：${time(attempt.createdAt)}`));
      if (attempt.model) section.append(element('p','maitu-note',`模型：${attempt.model}${attempt.connectionKey?` · 连接：${attempt.connectionKey}`:''}`));
      if (attempt.requestStartedAt) section.append(element('p','maitu-note',`请求开始：${time(attempt.requestStartedAt)}`));
      if (attempt.responseReceivedAt) section.append(element('p','maitu-note',`结果收到：${time(attempt.responseReceivedAt)}`));
      if (attempt.errorMessage) section.append(element('p','maitu-error',attempt.errorMessage));
      if (detail.staleInputs?.[attempt.id]) section.append(element('p','maitu-wait','输入已过期：'+detail.staleInputs[attempt.id]));
      if(attempt.inputSnapshot?.additionalInstruction) section.append(element('p','maitu-note','本次补充：'+attempt.inputSnapshot.additionalInstruction));
      const events=element('ol','maitu-timeline');
      for (const event of detail.events[attempt.id]||[]) {
        const item=element('li'); item.append(element('span','',event.message),element('time','',new Date(event.createdAt).toLocaleTimeString())); events.append(item);
      }
      section.append(events);
      const codeAttempt=(detail.codeAttempts||[]).find(record=>record.attemptId===attempt.id);
      if(codeAttempt) {
        section.append(element('p','maitu-note','代码基线：'+codeAttempt.baseCommit.slice(0,12)));
        if(codeAttempt.candidateCommit) section.append(element('p','maitu-note','成果版本：'+codeAttempt.candidateCommit.slice(0,12)));
        if(codeAttempt.adoptedAt) section.append(element('p','maitu-note','曾采用：'+time(codeAttempt.adoptedAt)));
        if(codeAttempt.patchArtifactId) section.append(button('查看代码差异',()=>showOutput({artifactId:codeAttempt.patchArtifactId},'代码差异')));
        else section.append(button('查看当前改动',async()=> {
          const result=await api('/api/maitu/tasks/'+id+'/attempts/'+attempt.id+'/diff');
          showContent('本次工作区的当前改动',result.content||'当前没有代码差异。');
        }));
      }
      const operations=(detail.operations||[]).filter(record=>record.attemptId===attempt.id);
      if(operations.length) {
        section.append(element('h4','','实际操作'));
        const list=element('div','maitu-operations');
        const operationNames={running:'执行中',succeeded:'成功',failed:'失败',interrupted:'中断'};
        for(const operation of operations) {
          const row=element('details','maitu-operation');
          row.open=openOperations.has(operation.id);
          row.addEventListener('toggle',()=> {if(row.open) openOperations.add(operation.id); else openOperations.delete(operation.id);});
          const result=operation.output;
          row.append(element('summary','',operation.label+' · '+(operationNames[operation.status]||operation.status)));
          row.append(element('p','maitu-note',time(operation.startedAt)+' → '+time(operation.completedAt)));
          if(operation.kind==='check' && result?.command) {
            row.append(element('p','maitu-note','退出码：'+(result.exitCode??'未确定')+' · '+result.durationMs+' ms'));
            row.append(element('pre','',result.stdout||'没有标准输出'));
            if(result.stderr) row.append(element('pre','maitu-error',result.stderr));
            if(result.error) row.append(element('p','maitu-error',result.error));
          } else if(result?.error) row.append(element('p','maitu-error',result.error));
          row.append(button('查看操作记录',()=>showContent(operation.label,JSON.stringify({input:operation.input,output:result},null,2))));
          list.append(row);
        }
        section.append(list);
      }
      if (attempt.inputSnapshot) section.append(button('查看本次输入',()=>showContent(`第 ${attempt.number} 次输入`,JSON.stringify(attempt.inputSnapshot,null,2))));
      if (attempt.artifactId) {
        const actions=element('div','maitu-result-actions');
        actions.append(button('打开成果',()=>showOutput(attempt,detail.task.outputFilename)));
        const download=element('a','maitu-button maitu-button--small','下载'); download.href=`/artifacts/${attempt.artifactId}`; download.download=detail.task.outputFilename; actions.append(download);
        const plan=(detail.plans||[]).find(record=>record.attemptId===attempt.id);
        if (plan) actions.append(button(plan.adoptedAt?'查看采用的计划':'调整并加入任务图',()=>openPlanReview(plan),!plan.adoptedAt));
        if (attempt.status==='produced' && detail.task.taskKind!=='plan' && detail.task.acceptedAttemptId!==attempt.id && !codeAttempt?.adoptedAt) actions.append(button(detail.task.taskKind==='code'?'合并检查并采用':'采用这次成果',()=> {
          adoptTaskId=id; adoptAttemptId=attempt.id;
          $('#maitu-adopt-form').elements.reason.value='';
          $('#maitu-adopt-dialog').showModal();
        },true));
        section.append(actions);
      }
      if (attempt.usage?.total_tokens) section.append(element('p','maitu-note',`本次用量：${attempt.usage.total_tokens} Token`));
      panel.append(section);
    }
    const withOutput=detail.attempts.filter(attempt=>attempt.artifactId);
    if (withOutput.length>=2 && detail.task.taskKind==='file') {
      panel.append(button('比较两次成果',async()=> {
        const contents=await Promise.all(withOutput.slice(0,2).map(async attempt=> {
          if (!outputCache.has(attempt.artifactId)) {
            const response=await fetch(`/artifacts/${attempt.artifactId}`);
            if (!response.ok) throw new Error('成果文件暂时无法打开。');
            outputCache.set(attempt.artifactId,await response.text());
          }
          return {attempt,content:outputCache.get(attempt.artifactId)};
        }));
        showContent(`成果比较：第 ${contents[0].attempt.number} 次 ↔ 第 ${contents[1].attempt.number} 次`,lineDiff(contents[0].content,contents[1].content));
      }));
    }
    panel.scrollTop=oldScroll;
  }

  function lineDiff(oldText,newText) {
    const a=oldText.split('\n'), b=newText.split('\n');
    const m=a.length, n=b.length;
    const table=Array.from({length:m+1},()=>new Array(n+1).fill(0));
    for (let i=m-1;i>=0;i--) for (let j=n-1;j>=0;j--)
      table[i][j]=a[i]===b[j]?table[i+1][j+1]+1:Math.max(table[i+1][j],table[i][j+1]);
    const rows=[];
    let i=0,j=0;
    while (i<m&&j<n) {
      if (a[i]===b[j]) {rows.push('  '+a[i]);i++;j++;}
      else if (table[i+1][j]>=table[i][j+1]) {rows.push('- '+a[i]);i++;}
      else {rows.push('+ '+b[j]);j++;}
    }
    while (i<m) {rows.push('- '+a[i]);i++;}
    while (j<n) {rows.push('+ '+b[j]);j++;}
    return rows.length?('第 1 次与第 2 次成果按行比较（- 旧 / + 新）：\n'+rows.join('\n')):'两次成果没有文本。';
  }

  function renderAttention() {
    const section=$('#maitu-attention');
    const list=$('#maitu-attention-list');
    if (!section||!list) return;
    const pinnedUnusable=snapshot.tasks.filter(t=>t.status==='queued'&&t.waitReason&&t.waitReason.includes('指定的模型连接当前不可用'));
    const items=snapshot.tasks.filter(t=>['failed','interrupted'].includes(t.status))
      .concat(pinnedUnusable)
      .concat(snapshot.tasks.filter(t=>t.status==='produced'&&!t.acceptedAttemptId&&t.taskKind!=='plan'));
    const seen=new Set(); const unique=items.filter(t=>!seen.has(t.id)&&seen.add(t.id));
    section.hidden=!unique.length;
    $('#maitu-attention-count').textContent=unique.length?unique.length+' 项':'';
    list.replaceChildren();
    for (const task of unique) {
      const row=element('div','maitu-attention-row');
      const kindName={file:'资料',plan:'计划',code:'编码'}[task.taskKind]||'资料';
      row.append(element('strong','',task.title),element('span','maitu-note',
        (kindName+' · '+(statusNames[task.status]||task.status)+(task.waitReason?' · '+task.waitReason:' · 等待你查看并采用'))));
      const actions=element('div','maitu-attention-actions');
      actions.append(button('查看任务',()=>selectTask(task.id)));
      if (['queued','running'].includes(task.status)) actions.append(button('取消',async()=> {
        if (!confirm(`取消任务「${task.title}」？排队中的尝试不会发出请求；执行中的请求不会中断但不会保存成果。`)) return;
        await api(`/api/maitu/tasks/${task.id}/cancel`,'POST',{});
        feedback('取消已处理。'); await refresh();
      }));
      row.append(actions);
      list.append(row);
    }
  }

  function planField(parent,title,name,value,multiline=false) {
    const label=element('label','maitu-plan-field',title);
    const input=element(multiline?'textarea':'input'); input.name=name; input.value=value; input.required=true;
    if(multiline) input.rows=name==='instruction'?4:2;
    label.append(input); parent.append(label); return input;
  }
  function openPlanReview(record) {
    planReviewRecord=record;
    const plan=record.adoptedProposal||record.proposal;
    const form=$('#maitu-plan-review-form');
    form.elements.summary.value=plan.summary;
    const questions=$('#maitu-plan-questions'); questions.replaceChildren();
    if(plan.questions.length) {
      questions.append(element('h3','','仍需明确的信息'));
      const list=element('ul','maitu-note');
      for(const question of plan.questions) list.append(element('li','',question));
      questions.append(list);
    }
    const target=$('#maitu-plan-edit-tasks'); target.replaceChildren();
    for(const task of plan.tasks) {
      const card=element('fieldset','maitu-plan-task'); card.dataset.planKey=task.key;
      card.append(element('legend','',task.title));
      planField(card,'任务名称','title',task.title);
      const label=element('label','maitu-plan-field','任务类型');
      const kind=element('select'); kind.name='kind';
      for(const [value,title] of [['file','读取资料并产出文件'],['code','修改代码并运行检查']]) {
        const option=element('option','',title); option.value=value; kind.append(option);
      }
      kind.value=task.kind; label.append(kind); card.append(label);
      planField(card,'具体要求','instruction',task.instruction,true);
      planField(card,task.kind==='code'?'成果说明文件名（不含路径）':'成果文件名（不含路径）','outputFilename',task.outputFilename);
      planField(card,'验收要求','acceptanceCriteria',task.acceptanceCriteria,true);
      card.append(element('p','maitu-note','等待哪些前序任务？没有真实依赖时可以独立执行。'));
      const dependencies=element('div','maitu-checks');
      for(const parent of plan.tasks.filter(item=>item.key!==task.key)) {
        const row=element('label'); const check=element('input'); check.type='checkbox'; check.name='dependsOn'; check.value=parent.key; check.checked=task.dependsOn.includes(parent.key);
        row.append(check,element('span','',parent.title)); dependencies.append(row);
      }
      card.append(dependencies); target.append(card);
    }
    for(const control of form.querySelectorAll('input,textarea,select,button[type="submit"]')) control.disabled=Boolean(record.adoptedAt);
    $('button[type="submit"]',form).textContent=record.adoptedAt?'这份计划已经加入图中':'加入任务图';
    $('#maitu-plan-review-dialog').showModal();
  }

  async function refresh() {
    if (refreshing) return;
    refreshing=true;
    try {
      snapshot=await api(`/api/maitu/projects/${projectId}`);
      const produced=snapshot.tasks.filter(task=>task.status==='produced').length;
      const usable=(snapshot.connections||[]).filter(c=>c.enabled&&c.configured);
      const capacity=usable.reduce((sum,c)=>sum+c.concurrency,0);
      const load=new Map((snapshot.connectionLoad||[]).map(item=>[item.key,item.running]));
      $('#maitu-capacity').textContent=usable.length
        ?`${snapshot.activeTasks} 项执行中 · 容量 ${capacity}（`+usable.map(c=>`${c.label} ${load.get(c.key)||0}/${c.concurrency}`).join('，')+'）'
        :'先在设置中配置可用的模型连接';
      $('#maitu-project-status').textContent=`${snapshot.tasks.length} 个任务 · ${produced} 个已产出`;
      renderMap(); renderSources(); await renderDetail();
      const code=snapshot.codeProject;
      $('#maitu-code-state').textContent=code
        ? code.sourceName+' · '+code.fileCount+' 个文件 · 当前采用版本 '+code.acceptedCommit.slice(0,12)+' · '+code.checks.map(check=>check.label).join('、')
        : '导入代码副本后，可以在图上真实修改代码并运行检查。';
      $('#maitu-import-code').hidden=Boolean(code);
      $('#maitu-export-code').hidden=!code;
      renderAttention();
    } finally {refreshing=false;}
  }

  if (mode==='project') {
    mapZoom = layoutMemory().zoom || 1;
    $('#maitu-zoom-in').addEventListener('click',()=>setZoom(mapZoom*1.2));
    $('#maitu-zoom-out').addEventListener('click',()=>setZoom(mapZoom/1.2));
    $('#maitu-zoom-reset').addEventListener('click',()=> {setZoom(1); $('.maitu-map-scroll').scrollTo({left:0});});
    $('#maitu-focus-selected').addEventListener('click',focusSelected);
    $('#maitu-adopt-form').addEventListener('submit',async event=> {
      event.preventDefault(); const form=event.currentTarget; const submit=$('button[type="submit"]',form); submit.disabled=true;
      try {
        await api(`/api/maitu/tasks/${adoptTaskId}/accept`,'POST',{attemptId:adoptAttemptId,reason:form.elements.reason.value});
        $('#maitu-adopt-dialog').close();
        feedback('已采用这次成果；理由与决定一起保留。'); await refresh();
      } catch(error) {feedback(error.message,true);} finally {submit.disabled=false;}
    });
    $('#maitu-retry-form').addEventListener('submit',async event=> {
      event.preventDefault();const form=event.currentTarget;const submit=$('button[type="submit"]',form);submit.disabled=true;
      try {
        await api('/api/maitu/tasks/'+retryTaskId+'/start','POST',{requestId:retryRequestId,additionalInstruction:form.elements.additionalInstruction.value});
        $('#maitu-retry-dialog').close();selectedTask=retryTaskId;selectedAttempt=undefined;await refresh();feedback('新的尝试已排队，原记录仍可查看。');
      } catch(error) {feedback(error.message,true);} finally {submit.disabled=false;}
    });
    const codeForm=$('#maitu-code-import-form');
    $('#maitu-import-code').addEventListener('click',()=> {
      codeImportRequestId=requestId(); codeImportFiles=[]; codeForm.reset();
      $('#maitu-code-import-dialog').showModal();
    });
    codeForm.elements.checkProgram.addEventListener('change',()=> {
      codeForm.elements.checkArgs.value={node:'--test',python3:'-m\nunittest\ndiscover',cargo:'test\n--offline'}[codeForm.elements.checkProgram.value];
    });
    $('#maitu-code-files').addEventListener('change',async event=> {
      const files=Array.from(event.currentTarget.files); codeImportFiles=[];
      const submit=$('button[type="submit"]',codeForm); submit.disabled=true;
      let skipped=0; let bytes=0;
      try {
        codeImportSource=files[0]?.webkitRelativePath.split('/')[0]||'项目代码';
        const decoder=new TextDecoder('utf-8',{fatal:true});
        for(const file of files) {
          const path=(file.webkitRelativePath||file.name).split('/').slice(1).join('/');
          const parts=path.toLowerCase().split('/');
          if(!path || parts.some(part=>['.git','node_modules','target','.ssh','.aws','__pycache__','.env'].includes(part)||part.startsWith('.maitu-write-')||(part.startsWith('.env.')&&!part.endsWith('.example')))) {skipped++;continue;}
          if(file.size>1024*1024) {skipped++;continue;}
          let content;
          try {content=decoder.decode(await file.arrayBuffer());} catch {skipped++;continue;}
          if(content.includes('\0')) {skipped++;continue;}
          bytes+=new TextEncoder().encode(content).length;
          if(bytes>20*1024*1024 || codeImportFiles.length>=2000) throw new Error('代码文本超过 2,000 件或 20 MiB，请选较小的源代码目录。');
          codeImportFiles.push({path,content});
        }
        $('#maitu-code-import-count').textContent=codeImportSource+'：将导入 '+codeImportFiles.length+' 个文本文件，'+(bytes/1024).toFixed(1)+' KiB；跳过 '+skipped+' 件缓存、配置或非文本原件。';
      } catch(error) {codeImportFiles=[];feedback(error.message,true);} finally {submit.disabled=false;}
    });
    codeForm.addEventListener('submit',async event=> {
      event.preventDefault(); const submit=$('button[type="submit"]',codeForm); submit.disabled=true;
      try {
        if(!codeImportFiles.length) throw new Error('先选择包含可用文本代码的文件夹。');
        const checks=[{id:'project-check',label:codeForm.elements.checkLabel.value,program:codeForm.elements.checkProgram.value,
          args:codeForm.elements.checkArgs.value.split('\n').map(arg=>arg.trim()).filter(Boolean)}];
        await api('/api/maitu/projects/'+projectId+'/code','POST',{requestId:codeImportRequestId,sourceName:codeImportSource,files:codeImportFiles,checks});
        $('#maitu-code-import-dialog').close(); codeImportFiles=[]; await refresh();
        feedback('代码基线已保存，现在可以生成编码计划或添加编码任务。');
      } catch(error) {feedback(error.message,true);} finally {submit.disabled=false;}
    });
    function checkList(target,items,name,label) {
      target.replaceChildren();
      if (!items.length) target.append(element('span','maitu-note','暂无可选项'));
      for (const item of items) {
        const row=element('label'); const input=element('input'); input.type='checkbox'; input.name=name; input.value=item.id;
        row.append(input,element('span','',label(item))); target.append(row);
      }
    }
    function connectionOptions(select) {
      const current=select.value;
      select.replaceChildren(element('option','','自动选择可用连接'));
      select.firstChild.value='';
      for (const connection of (snapshot.connections||[]).filter(c=>c.enabled)) {
        const option=element('option','',connection.label+'（'+connection.key+'）');
        option.value=connection.key; select.append(option);
      }
      select.value=current;
      if (select.value!==current) select.value='';
    }
    $('#maitu-new-task').addEventListener('click',()=> {
      if (!snapshot) return;
      const form=$('#maitu-task-create'); form.reset(); taskRequestId=requestId();
      checkList($('#maitu-task-sources'),snapshot.sources,'sourceIds',source=>source.filename);
      checkList($('#maitu-task-dependencies'),snapshot.tasks,'dependencyIds',task=>task.title);
      connectionOptions($('#maitu-task-connection'));
      $('#maitu-task-dialog').showModal();
    });
    $('#maitu-add-source').addEventListener('click',()=>$('#maitu-source-dialog').showModal());
    $('#maitu-generate-plan').addEventListener('click',()=> {
      if(!snapshot) return;
      planGenerateRequestId=requestId();
      checkList($('#maitu-plan-sources'),snapshot.sources,'sourceIds',source=>source.filename);
      $('#maitu-plan-generate-dialog').showModal();
    });
    $('#maitu-plan-generate-form').addEventListener('submit',async event=> {
      event.preventDefault(); const form=event.currentTarget; const submit=$('button[type="submit"]',form); submit.disabled=true;
      try {
        const data=new FormData(form);
        const task=await api(`/api/maitu/projects/${projectId}/plans`,'POST',{requestId:planGenerateRequestId,instruction:data.get('instruction'),sourceIds:data.getAll('sourceIds')});
        $('#maitu-plan-generate-dialog').close(); selectedTask=task.id; selectedAttempt=undefined; await refresh(); feedback('计划已开始生成，完成后可以调整并加入任务图。');
      } catch(error) {feedback(error.message,true);} finally {submit.disabled=false;}
    });
    $('#maitu-plan-review-form').addEventListener('submit',async event=> {
      event.preventDefault(); const form=event.currentTarget; const submit=$('button[type="submit"]',form); submit.disabled=true;
      try {
        const plan=JSON.parse(JSON.stringify(planReviewRecord.proposal)); plan.summary=form.elements.summary.value;
        plan.tasks=Array.from(form.querySelectorAll('[data-plan-key]'),card=> ({
          key:card.dataset.planKey,title:$('[name="title"]',card).value,kind:$('[name="kind"]',card).value,instruction:$('[name="instruction"]',card).value,
          outputFilename:$('[name="outputFilename"]',card).value,acceptanceCriteria:$('[name="acceptanceCriteria"]',card).value,
          dependsOn:Array.from(card.querySelectorAll('[name="dependsOn"]:checked'),input=>input.value)
        }));
        const result=await api(`/api/maitu/projects/${projectId}/plans/adopt`,'POST',{attemptId:planReviewRecord.attemptId,plan});
        $('#maitu-plan-review-dialog').close(); selectedTask=result.taskIds[0]; selectedAttempt=undefined; await refresh(); feedback(`${result.taskIds.length} 个节点已加入图中，你可以决定开始执行。`);
      } catch(error) {feedback(error.message,true);} finally {submit.disabled=false;}
    });
    $('#maitu-start-ready').addEventListener('click',handle(async()=> {
      const tasks=snapshot.tasks.filter(task=>task.status==='draft');
      if (!tasks.length) {feedback('没有待启动任务。失败或已产出的任务可在节点上单独重试。'); return;}
      const results=await Promise.allSettled(tasks.map(task=>startTask(task.id)));
      const failed=results.filter(result=>result.status==='rejected');
      feedback(failed.length?`${tasks.length-failed.length} 项已提交；${failed[0].reason.message}`:`已提交 ${tasks.length} 个任务。`,failed.length>0);
    }));
    $('#maitu-task-create').addEventListener('submit',async event=> {
      event.preventDefault(); const form=event.currentTarget; const submit=$('button[type="submit"]',form); submit.disabled=true;
      const data=new FormData(form);
      try {
        const task=await api(`/api/maitu/projects/${projectId}/tasks`,'POST',{
          requestId:taskRequestId,title:data.get('title'),instruction:data.get('instruction'),outputFilename:data.get('outputFilename'),taskKind:data.get('taskKind'),acceptanceCriteria:data.get('acceptanceCriteria'),sourceIds:data.getAll('sourceIds'),dependencyIds:data.getAll('dependencyIds'),connectionKey:data.get('connectionKey')||''
        });
        $('#maitu-task-dialog').close(); selectedTask=task.id; selectedAttempt=undefined; await refresh(); feedback('任务已加入图中，可以从节点上执行。');
      } catch(error) {feedback(error.message,true);} finally {submit.disabled=false;}
    });
    $('#maitu-source-create').addEventListener('submit',async event=> {
      event.preventDefault(); const form=event.currentTarget; const submit=$('button[type="submit"]',form); submit.disabled=true;
      try {
        await api(`/api/maitu/projects/${projectId}/sources`,'POST',Object.fromEntries(new FormData(form)));
        form.reset(); $('#maitu-source-dialog').close(); await refresh(); feedback('资料已保存到项目。');
      } catch(error) {feedback(error.message,true);} finally {submit.disabled=false;}
    });
    $('#maitu-source-files').addEventListener('change',async event=> {
      const input=event.currentTarget; input.disabled=true;
      try {
        for (const file of input.files) {
          if (file.size>256*1024) throw new Error(`${file.name} 超过 256 KiB。`);
          let content;
          try {content=new TextDecoder('utf-8',{fatal:true}).decode(await file.arrayBuffer());} catch {throw new Error(`${file.name} 不是 UTF-8 文本文件。`);}
          await api(`/api/maitu/projects/${projectId}/sources`,'POST',{filename:file.name,content});
        }
        $('#maitu-source-dialog').close(); await refresh(); feedback('文件已添加到项目资料。');
      } catch(error) {feedback(error.message,true); await refresh();} finally {input.disabled=false;input.value='';}
    });
    refresh().catch(error=>feedback(error.message,true));
    setInterval(()=> {if (!document.hidden) refresh().catch(error=>feedback(error.message,true));},2000);
    document.addEventListener('visibilitychange',()=> {if(!document.hidden) refresh().catch(error=>feedback(error.message,true));});
  }
})();
