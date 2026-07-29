(() => {
  const liveLabel = document.getElementById("live-label");
  const assetList = document.getElementById("asset-list");
  const uploadForm = document.getElementById("upload-form");
  const uploadStatus = document.getElementById("upload-status");
  const clearLiveBtn = document.getElementById("clear-live");
  const refreshBtn = document.getElementById("refresh");
  const replaceInput = document.getElementById("replace-file");
  const rebootBtn = document.getElementById("reboot-device");
  const shutdownBtn = document.getElementById("shutdown-device");
  const powerStatus = document.getElementById("power-status");
  const powerPanel = document.getElementById("power-panel");

  let replaceTargetId = null;

  // Desktop app embeds admin with ?desktop=1 — keep Pi power controls hidden.
  const isDesktop = new URLSearchParams(location.search).has("desktop");
  if (powerPanel && !isDesktop) {
    // Only relevant on Raspberry Pi builds that opt in.
    powerPanel.hidden = true;
  }

  function setStatus(message, type = "") {
    uploadStatus.hidden = !message;
    uploadStatus.textContent = message || "";
    uploadStatus.className = `status ${type}`.trim();
  }

  function setPowerStatus(message, type = "") {
    if (!powerStatus) return;
    powerStatus.hidden = !message;
    powerStatus.textContent = message || "";
    powerStatus.className = `status ${type}`.trim();
  }

  async function requestPower(path, confirmText) {
    if (!rebootBtn || !shutdownBtn) return;
    if (!confirm(confirmText)) return;
    rebootBtn.disabled = true;
    shutdownBtn.disabled = true;
    setPowerStatus("Sending request…");
    try {
      const res = await fetch(path, { method: "POST" });
      const data = await res.json().catch(() => ({}));
      if (!res.ok) throw new Error(data.error || "Request failed");
      setPowerStatus(data.message || "Requested.", "ok");
    } catch (err) {
      setPowerStatus(err.message || "Request failed", "error");
      rebootBtn.disabled = false;
      shutdownBtn.disabled = false;
    }
  }

  function escapeHtml(value) {
    return String(value)
      .replaceAll("&", "&amp;")
      .replaceAll("<", "&lt;")
      .replaceAll(">", "&gt;")
      .replaceAll('"', "&quot;");
  }

  function renderLive(assets, liveId) {
    const live = assets.find((a) => a.id === liveId);
    if (!live) {
      liveLabel.textContent = "Nothing live";
      return;
    }
    liveLabel.textContent = `${live.name} (${live.type})`;
  }

  function thumbHtml(asset) {
    const cacheBust = asset.updatedAt ? `?v=${encodeURIComponent(asset.updatedAt)}` : "";
    if (asset.type === "image") {
      return `<img class="thumb" src="${escapeHtml(asset.url)}${cacheBust}" alt="" />`;
    }
    return `<video class="thumb" src="${escapeHtml(asset.url)}${cacheBust}" muted playsinline preload="metadata"></video>`;
  }

  function renderAssets(assets, liveId) {
    if (!assets.length) {
      assetList.innerHTML = '<p class="empty">No assets yet. Upload a video or image above.</p>';
      return;
    }

    assetList.innerHTML = assets
      .map((asset) => {
        const isLive = asset.id === liveId;
        return `
          <article class="asset-row ${isLive ? "is-live" : ""}" data-id="${escapeHtml(asset.id)}" data-name="${escapeHtml(asset.name)}">
            ${thumbHtml(asset)}
            <div class="asset-meta">
              <h3>
                ${escapeHtml(asset.name)}
                ${isLive ? '<span class="badge">LIVE</span>' : ""}
              </h3>
              <p>${escapeHtml(asset.type)} · ${escapeHtml(asset.originalName || asset.filename)}</p>
              <p class="asset-id">id: ${escapeHtml(asset.id)}</p>
            </div>
            <div class="asset-actions">
              <button class="btn ${isLive ? "ghost" : "live"}" data-action="live" type="button" ${
                isLive ? "disabled" : ""
              }>
                ${isLive ? "On air" : "Set live"}
              </button>
              <button class="btn ghost" data-action="rename" type="button">Rename</button>
              <button class="btn ghost" data-action="replace" type="button">Replace</button>
              <button class="btn danger" data-action="delete" type="button">Delete</button>
            </div>
          </article>
        `;
      })
      .join("");
  }

  async function loadAssets() {
    const res = await fetch("/api/assets", { cache: "no-store" });
    if (!res.ok) throw new Error("Failed to load assets");
    const data = await res.json();
    renderLive(data.assets, data.liveId);
    renderAssets(data.assets, data.liveId);
  }

  uploadForm.addEventListener("submit", async (event) => {
    event.preventDefault();
    const fileInput = document.getElementById("asset-file");
    const nameInput = document.getElementById("asset-name");
    if (!fileInput.files?.length) {
      setStatus("Choose a file first.", "error");
      return;
    }

    const formData = new FormData();
    formData.append("file", fileInput.files[0]);
    if (nameInput.value.trim()) {
      formData.append("name", nameInput.value.trim());
    }

    setStatus("Uploading…");
    try {
      const res = await fetch("/api/assets", { method: "POST", body: formData });
      const data = await res.json();
      if (!res.ok) throw new Error(data.error || "Upload failed");
      uploadForm.reset();
      setStatus(`Uploaded “${data.asset.name}”.`, "ok");
      await loadAssets();
    } catch (err) {
      setStatus(err.message || "Upload failed", "error");
    }
  });

  replaceInput.addEventListener("change", async () => {
    const file = replaceInput.files?.[0];
    const id = replaceTargetId;
    replaceInput.value = "";
    replaceTargetId = null;
    if (!file || !id) return;

    const formData = new FormData();
    formData.append("file", file);
    setStatus(`Replacing asset ${id}…`);

    try {
      const res = await fetch(`/api/assets/${encodeURIComponent(id)}`, {
        method: "PUT",
        body: formData,
      });
      const data = await res.json();
      if (!res.ok) throw new Error(data.error || "Replace failed");
      setStatus(`Updated “${data.asset.name}” (id unchanged).`, "ok");
      await loadAssets();
    } catch (err) {
      setStatus(err.message || "Replace failed", "error");
    }
  });

  assetList.addEventListener("click", async (event) => {
    const button = event.target.closest("button[data-action]");
    if (!button) return;
    const row = button.closest(".asset-row");
    const id = row?.dataset.id;
    if (!id) return;

    const action = button.dataset.action;
    button.disabled = true;

    try {
      if (action === "live") {
        const res = await fetch(`/api/live/${encodeURIComponent(id)}`, { method: "POST" });
        const data = await res.json();
        if (!res.ok) throw new Error(data.error || "Failed to set live");
      }

      if (action === "rename") {
        const nextName = prompt("New display name", row.dataset.name || "");
        if (nextName === null) {
          button.disabled = false;
          return;
        }
        const trimmed = nextName.trim();
        if (!trimmed) {
          alert("Name cannot be empty.");
          button.disabled = false;
          return;
        }
        const res = await fetch(`/api/assets/${encodeURIComponent(id)}`, {
          method: "PUT",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ name: trimmed }),
        });
        const data = await res.json();
        if (!res.ok) throw new Error(data.error || "Rename failed");
        setStatus(`Renamed to “${data.asset.name}”.`, "ok");
      }

      if (action === "replace") {
        replaceTargetId = id;
        replaceInput.click();
        button.disabled = false;
        return;
      }

      if (action === "delete") {
        if (!confirm("Delete this asset?")) {
          button.disabled = false;
          return;
        }
        const res = await fetch(`/api/assets/${encodeURIComponent(id)}`, { method: "DELETE" });
        const data = await res.json();
        if (!res.ok) throw new Error(data.error || "Delete failed");
      }

      await loadAssets();
    } catch (err) {
      alert(err.message || "Action failed");
      button.disabled = false;
    }
  });

  clearLiveBtn.addEventListener("click", async () => {
    clearLiveBtn.disabled = true;
    try {
      const res = await fetch("/api/live/clear", { method: "POST" });
      const data = await res.json();
      if (!res.ok) throw new Error(data.error || "Failed to clear live");
      await loadAssets();
    } catch (err) {
      alert(err.message || "Failed to clear live");
    } finally {
      clearLiveBtn.disabled = false;
    }
  });

  refreshBtn.addEventListener("click", () => {
    loadAssets().catch((err) => alert(err.message));
  });

  rebootBtn?.addEventListener("click", () => {
    requestPower(
      "/api/system/reboot",
      "Reboot the Raspberry Pi now? The display will go offline briefly."
    );
  });

  shutdownBtn?.addEventListener("click", () => {
    requestPower(
      "/api/system/shutdown",
      "Shut down the Raspberry Pi now?\n\nWait until activity lights stop before unplugging power."
    );
  });

  loadAssets().catch((err) => {
    assetList.innerHTML = `<p class="empty">${escapeHtml(err.message)}</p>`;
  });

  setInterval(() => {
    loadAssets().catch(() => {});
  }, 4000);
})();
