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

  document.querySelectorAll("form:not([data-input-upload]):not([data-idea-source-upload]):not([data-idea-capture])").forEach((form) => {
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

  const attachIdeaSource = async ({ ideaId, expectedRevision, file, note, status }) => {
    const setStatus = (message, state = "working") => {
      if (!status) return;
      status.textContent = message;
      status.dataset.state = state;
    };
    setStatus("计算摘要并验证来源…");
    const buffer = await file.arrayBuffer();
    const digest = await sha256(buffer);
    const query = new URLSearchParams({
      clientRequestId: requestId(),
      expectedRevision: String(expectedRevision),
      filename: file.name,
      declaredMediaType: file.type || "application/octet-stream",
    });
    if (digest) query.set("expectedSha256", digest);
    if (note?.trim()) query.set("note", note.trim());
    const attached = await jsonRequest(`/api/v1/ideas/${ideaId}/sources?${query}`, {
      method: "POST",
      headers: { "content-type": "application/octet-stream" },
      body: buffer,
    });
    setStatus(`完成：${attached.source.displayName} 已进入想法 v${attached.ideaRevision}`, "success");
    return attached;
  };

  document.querySelectorAll("[data-proposal-source-selector]").forEach((selector) => {
    const form = selector.closest("form");
    form?.addEventListener("submit", () => {
      const selected = [...selector.querySelectorAll("[data-proposal-source]:checked")]
        .map((input) => input.value);
      const target = selector.querySelector("[data-proposal-sources-value]");
      if (target) target.value = JSON.stringify(selected);
    });
  });

  document.querySelectorAll("[data-idea-capture]").forEach((form) => {
    form.addEventListener("submit", async (event) => {
      const file = form.elements.file?.files?.[0];
      if (!file) {
        const button = form.querySelector("button[type='submit']");
        if (button) {
          button.disabled = true;
          button.textContent = "处理中…";
        }
        return;
      }
      event.preventDefault();
      const button = form.querySelector("button[type='submit']");
      button.disabled = true;
      try {
        const title = form.elements.title?.value?.trim() || file.name;
        const body = form.elements.body?.value?.trim() || `由文件开始：${file.name}`;
        const created = await jsonRequest("/api/v1/ideas", {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({
            clientRequestId: requestId(),
            action: "idea.create",
            payload: {
              revision: {
                title,
                body,
                sourceKind: "text",
                sourceRef: null,
                revisionReason: null,
              },
            },
          }),
        });
        const ideaId = created.result.ideaId;
        await attachIdeaSource({
          ideaId,
          expectedRevision: created.result.revision,
          file,
          note: form.elements.body?.value || "",
          status: null,
        });
        window.location.assign(`/ideas/${ideaId}?notice=${encodeURIComponent("文件已验证并形成想法来源")}`);
      } catch (error) {
        button.disabled = false;
        button.textContent = "保留这个想法";
        window.alert(error instanceof Error ? error.message : "想法来源上传失败");
      }
    });
  });

  document.querySelectorAll("[data-idea-source-upload]").forEach((form) => {
    form.addEventListener("submit", async (event) => {
      event.preventDefault();
      const file = form.elements.file?.files?.[0];
      if (!file) return;
      const button = form.querySelector("button[type='submit']");
      const status = form.querySelector("[data-idea-source-status]");
      button.disabled = true;
      try {
        await attachIdeaSource({
          ideaId: form.dataset.ideaId,
          expectedRevision: form.dataset.ideaRevision,
          file,
          note: form.elements.note?.value || "",
          status,
        });
        const location = new URL(window.location.href);
        location.searchParams.set("notice", "来源已校验并附加");
        window.setTimeout(() => window.location.assign(location), 350);
      } catch (error) {
        status.textContent = error instanceof Error ? error.message : "来源附加失败";
        status.dataset.state = "error";
        button.disabled = false;
      }
    });
  });

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
