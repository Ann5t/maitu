(() => {
  'use strict';
  const $ = (selector, root = document) => root.querySelector(selector);
  const mode = document.body.dataset.maituMode;
  const projectId = document.body.dataset.projectId;
  const statusNames = {draft:'待启动',queued:'等待执行',running:'执行中',produced:'已产出',failed:'执行失败',interrupted:'执行中断'};
  let snapshot;
  let selectedTask;
  let selectedAttempt;
  let taskRequestId;
  let refreshing = false;
  let initialMapLayout = true;
  const outputCache = new Map();
  const startRequests = new Map();

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
    const response = await fetch(path, {method, headers, body:data === undefined ? undefined : JSON.stringify(data), credentials:'same-origin'});
    let value;
    try {value = await response.json();} catch {throw new Error('服务响应无法读取，请检查连接后重试。');}
    if (!response.ok) throw new Error(value.error || '请求未完成，请稍后重试。');
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
    const form = $('#maitu-provider-form');
    function modeHint() {
      $('#maitu-generation-defaults').textContent = form.elements.thinkingEnabled.value === 'true'
        ? '思考模式：参考默认输出上限 65,536 Token。此额度包含思考与最终正文。'
        : '非思考模式：参考默认输出上限 8,192 Token。切换模式会保留你填写的输出上限。';
    }
    function populate(config) {
      for (const name of ['baseUrl','model','concurrency','contextTokens','maxTokens']) form.elements[name].value = config[name];
      form.elements.contextTokens.max = config.maxContextTokens;
      form.elements.maxTokens.max = config.maxOutputTokens;
      form.elements.thinkingEnabled.value = String(config.thinkingEnabled);
      modeHint();
      $('#maitu-model-limits').textContent = `DeepSeek：上下文容量 ${config.maxContextTokens.toLocaleString('en-US')} Token，最大输出 ${config.maxOutputTokens.toLocaleString('en-US')} Token。`;
      form.elements.apiKey.value = '';
      $('#maitu-provider-state').textContent = config.configured ? '已配置 · 用实际任务验证连接' : '等待填写密钥';
    }
    api('/api/maitu/provider').then(populate).catch(error => feedback(error.message,true));
    form.elements.thinkingEnabled.addEventListener('change', modeHint);
    form.addEventListener('submit', async event => {
      event.preventDefault();
      const submit = $('button[type="submit"]',form);
      submit.disabled = true;
      const data = Object.fromEntries(new FormData(form));
      for (const key of ['concurrency','contextTokens','maxTokens']) data[key] = Number(data[key]);
      data.thinkingEnabled = data.thinkingEnabled === 'true';
      if (data.maxTokens >= data.contextTokens) {
        feedback('最大输出长度须小于上下文预算，为任务要求和资料保留输入空间。', true);
        submit.disabled = false;
        return;
      }
      try {
        populate(await api('/api/maitu/provider','PUT',data));
        feedback('连接已保存，新启动的模型请求使用此配置。');
      } catch(error) {feedback(error.message,true);} finally {submit.disabled=false;}
    });
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
    const height = Math.max(380, rows*210+70);
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
    for (const task of snapshot.tasks) {
      const position = locations.get(task.id);
      const parents = snapshot.dependencies.filter(item => item.taskId === task.id);
      if (!parents.length) edge({x:272,y:height/2},{x:position.x,y:position.y+80});
      for (const parent of parents) {
        const previous = locations.get(parent.parentTaskId);
        edge({x:previous.x+248,y:previous.y+80},{x:position.x,y:position.y+80});
      }
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
      select.append(element('span',`maitu-status maitu-status--${task.status}`,statusNames[task.status]||task.status),element('strong','',task.title),element('p','',task.waitReason||task.instruction));
      select.addEventListener('click',()=>selectTask(task.id));
      node.append(select);
      const actions=element('div','maitu-node-actions');
      if (!['running','queued'].includes(task.status)) actions.append(button(task.status==='draft'?'执行':task.status==='produced'?'再执行一次':'重试',()=>startTask(task.id)));
      actions.append(button('记录与成果',()=>selectTask(task.id)));
      if (task.acceptedAttemptId) actions.append(element('span','maitu-adopted','已采用成果'));
      node.append(actions); map.append(node);
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
    if (detail.task.waitReason) panel.append(element('p','maitu-wait',detail.task.waitReason));
    if (!['running','queued'].includes(detail.task.status)) panel.append(button(detail.task.status==='draft'?'执行任务':'再执行一次',()=>startTask(id),true));
    panel.append(element('h3','','历次尝试'));
    if (!detail.attempts.length) panel.append(element('p','maitu-note','尚未启动。执行后会保存输入、请求时段、过程记录和成果。'));
    for (const attempt of detail.attempts) {
      const section=element('details','maitu-attempt');
      section.open=selectedAttempt ? selectedAttempt===attempt.id : attempt.id===detail.attempts[0].id;
      section.addEventListener('toggle',()=> {if(section.open) selectedAttempt=attempt.id;});
      const summary=element('summary','',`第 ${attempt.number} 次 · ${statusNames[attempt.status]||attempt.status}${detail.task.acceptedAttemptId===attempt.id?' · 已采用':''}`);
      section.append(summary,element('p','maitu-note',`创建：${time(attempt.createdAt)}`));
      if (attempt.model) section.append(element('p','maitu-note',`模型：${attempt.model}`));
      if (attempt.requestStartedAt) section.append(element('p','maitu-note',`请求开始：${time(attempt.requestStartedAt)}`));
      if (attempt.responseReceivedAt) section.append(element('p','maitu-note',`结果收到：${time(attempt.responseReceivedAt)}`));
      if (attempt.errorMessage) section.append(element('p','maitu-error',attempt.errorMessage));
      const events=element('ol','maitu-timeline');
      for (const event of detail.events[attempt.id]||[]) {
        const item=element('li'); item.append(element('span','',event.message),element('time','',new Date(event.createdAt).toLocaleTimeString())); events.append(item);
      }
      section.append(events);
      if (attempt.inputSnapshot) section.append(button('查看本次输入',()=>showContent(`第 ${attempt.number} 次输入`,JSON.stringify(attempt.inputSnapshot,null,2))));
      if (attempt.artifactId) {
        const actions=element('div','maitu-result-actions');
        actions.append(button('打开成果',()=>showOutput(attempt,detail.task.outputFilename)));
        const download=element('a','maitu-button maitu-button--small','下载'); download.href=`/artifacts/${attempt.artifactId}`; download.download=detail.task.outputFilename; actions.append(download);
        if (detail.task.acceptedAttemptId!==attempt.id) actions.append(button('采用这次成果',async()=> {
          await api(`/api/maitu/tasks/${id}/accept`,'POST',{attemptId:attempt.id});
          feedback('已采用这次成果；等待它的后续任务可以继续。'); await refresh();
        },true));
        section.append(actions);
      }
      if (attempt.usage?.total_tokens) section.append(element('p','maitu-note',`本次用量：${attempt.usage.total_tokens} Token`));
      panel.append(section);
    }
    panel.scrollTop=oldScroll;
  }

  async function refresh() {
    if (refreshing) return;
    refreshing=true;
    try {
      snapshot=await api(`/api/maitu/projects/${projectId}`);
      const produced=snapshot.tasks.filter(task=>task.status==='produced').length;
      $('#maitu-capacity').textContent=snapshot.provider.configured?`全局 ${snapshot.activeTasks} / ${snapshot.provider.concurrency} 执行中`:'先连接 DeepSeek';
      $('#maitu-project-status').textContent=`${snapshot.tasks.length} 个任务 · ${produced} 个已产出`;
      renderMap(); renderSources(); await renderDetail();
    } finally {refreshing=false;}
  }

  if (mode==='project') {
    function checkList(target,items,name,label) {
      target.replaceChildren();
      if (!items.length) target.append(element('span','maitu-note','暂无可选项'));
      for (const item of items) {
        const row=element('label'); const input=element('input'); input.type='checkbox'; input.name=name; input.value=item.id;
        row.append(input,element('span','',label(item))); target.append(row);
      }
    }
    $('#maitu-new-task').addEventListener('click',()=> {
      if (!snapshot) return;
      const form=$('#maitu-task-create'); form.reset(); taskRequestId=requestId();
      checkList($('#maitu-task-sources'),snapshot.sources,'sourceIds',source=>source.filename);
      checkList($('#maitu-task-dependencies'),snapshot.tasks,'dependencyIds',task=>task.title);
      $('#maitu-task-dialog').showModal();
    });
    $('#maitu-add-source').addEventListener('click',()=>$('#maitu-source-dialog').showModal());
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
          requestId:taskRequestId,title:data.get('title'),instruction:data.get('instruction'),outputFilename:data.get('outputFilename'),sourceIds:data.getAll('sourceIds'),dependencyIds:data.getAll('dependencyIds')
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
