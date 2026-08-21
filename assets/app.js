(() => {
  const root = document.documentElement;
  const applyTheme = (theme) => {
    root.dataset.theme = theme;
    root.style.colorScheme = theme;
    try { localStorage.setItem("fudian-theme", theme); } catch (_) {}
  };

  document.querySelectorAll("[data-theme-toggle]").forEach((button) => {
    button.addEventListener("click", () => {
      applyTheme(root.dataset.theme === "dark" ? "light" : "dark");
    });
  });

  document.querySelectorAll("form:not([data-input-upload])").forEach((form) => {
    form.addEventListener("submit", () => {
      const button = form.querySelector("button[type='submit']");
      if (!button || button.disabled) return;
      button.disabled = true;
      button.dataset.originalLabel = button.textContent;
      button.textContent = "处理中…";
    });
  });

  document.querySelectorAll("textarea").forEach((textarea) => {
    const resize = () => {
      if (!textarea.hasAttribute("data-autogrow") && textarea.scrollHeight < 190) return;
      textarea.style.height = "auto";
      textarea.style.height = `${Math.min(360, textarea.scrollHeight)}px`;
    };
    textarea.addEventListener("input", resize);
  });

  const graph = document.querySelector("[data-graph-scroll]");
  const selected = graph?.querySelector("[data-selected='true']");
  if (graph && selected && window.innerWidth > 620) {
    requestAnimationFrame(() => {
      graph.scrollTo({
        left: Math.max(0, selected.offsetLeft - graph.clientWidth / 2 + selected.clientWidth / 2),
        top: Math.max(0, selected.offsetTop - graph.clientHeight / 2 + selected.clientHeight / 2),
        behavior: "auto",
      });
    });
  }

  const goalMap = document.querySelector("[data-goal-map-scroll]");
  const selectedSession = goalMap?.querySelector("[data-selected='true']");
  if (goalMap && selectedSession && window.innerWidth > 620) {
    requestAnimationFrame(() => {
      goalMap.scrollTo({
        left: Math.max(0, selectedSession.offsetLeft - goalMap.clientWidth / 2 + selectedSession.clientWidth / 2),
        top: Math.max(0, selectedSession.offsetTop - goalMap.clientHeight / 2 + selectedSession.clientHeight / 2),
        behavior: "auto",
      });
    });
  }

  const requestId = () => {
    if (globalThis.crypto?.randomUUID) return globalThis.crypto.randomUUID();
    return "10000000-1000-4000-8000-100000000000".replace(/[018]/g, (value) =>
      (Number(value) ^ (globalThis.crypto.getRandomValues(new Uint8Array(1))[0] & (15 >> (Number(value) / 4)))).toString(16));
  };
  const sha256 = async (buffer) => {
    if (!globalThis.crypto?.subtle) return null;
    const digest = await globalThis.crypto.subtle.digest("SHA-256", buffer);
    return [...new Uint8Array(digest)].map((value) => value.toString(16).padStart(2, "0")).join("");
  };
  const jsonRequest = async (url, options) => {
    const response = await fetch(url, options);
    const payload = await response.json().catch(() => ({}));
    if (!response.ok) throw new Error(payload.error || `请求失败（${response.status}）`);
    return payload;
  };

  document.querySelectorAll("[data-input-upload]").forEach((form) => {
    form.addEventListener("submit", async (event) => {
      event.preventDefault();
      const file = form.elements.file?.files?.[0];
      if (!file) return;
      const button = form.querySelector("button[type='submit']");
      const status = form.querySelector("[data-upload-status]");
      const projectId = form.dataset.projectId;
      const sessionId = form.dataset.sessionId;
      const base = `/api/v1/projects/${projectId}/sessions/${sessionId}/inputs`;
      const setStatus = (message, state = "working") => {
        status.textContent = message;
        status.dataset.state = state;
      };
      button.disabled = true;
      try {
        setStatus("建立受限上传…");
        const started = await jsonRequest(base, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({
            clientRequestId: requestId(),
            filename: file.name,
            declaredMediaType: file.type || null,
            declaredSize: file.size,
          }),
        });
        const inputId = started.input.id;
        const chunkSize = 1024 * 1024;
        for (let offset = 0; offset < file.size; offset += chunkSize) {
          const chunk = file.slice(offset, Math.min(file.size, offset + chunkSize));
          const chunkBuffer = await chunk.arrayBuffer();
          const chunkHash = await sha256(chunkBuffer);
          const query = new URLSearchParams({
            clientRequestId: requestId(),
            offset: String(offset),
          });
          if (chunkHash) query.set("sha256", chunkHash);
          await jsonRequest(`${base}/${inputId}/chunks?${query}`, {
            method: "PUT",
            headers: { "content-type": "application/octet-stream" },
            body: chunkBuffer,
          });
          setStatus(`上传与验证 ${Math.min(file.size, offset + chunk.size)} / ${file.size} B…`);
        }
        const fullHash = await sha256(await file.arrayBuffer());
        await jsonRequest(`${base}/${inputId}/finish`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ clientRequestId: requestId(), expectedSha256: fullHash }),
        });
        const requestedPath = form.elements.inbox_relative_path?.value?.trim();
        const imported = await jsonRequest(`${base}/${inputId}/import`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({
            clientRequestId: requestId(),
            inboxRelativePath: requestedPath || null,
          }),
        });
        const mode = imported.input.importMode === "worktree_copy" ? "已复制到 Session inbox" : "已作为产物引用";
        setStatus(`完成：${mode}`, "success");
        const location = new URL(window.location.href);
        location.searchParams.set("notice", "文件已验证并导入");
        location.hash = "goal-workbench";
        window.setTimeout(() => window.location.assign(location), 450);
      } catch (error) {
        setStatus(error instanceof Error ? error.message : "文件导入失败", "error");
        button.disabled = false;
      }
    });
  });
})();
