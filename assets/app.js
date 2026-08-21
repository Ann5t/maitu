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

  document.querySelectorAll("form").forEach((form) => {
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
})();
