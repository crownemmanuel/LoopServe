import { invoke } from "@tauri-apps/api/core";
import {
  checkForUpdates,
  checkForUpdatesOnStartup,
  clearSkippedVersion,
  downloadAndInstallUpdate,
  getCurrentAppVersion,
  saveSkippedVersion,
  type UpdateInfo,
} from "./updater";

type MonitorInfo = {
  index: number;
  name: string;
  width: number;
  height: number;
  x: number;
  y: number;
  isPrimary: boolean;
};

type StatusInfo = {
  port: number;
  mediaDir: string;
  dataDir: string;
  serverUrl: string;
  adminUrl: string;
  displayUrl: string;
  displayMonitorIndex: number;
  autoStartDisplay: boolean;
};

type Asset = {
  id: string;
  name: string;
  type: "video" | "image";
  url: string;
};

type CompanionLocation = {
  page: number;
  row: number;
  column: number;
  pushStyle?: boolean;
};

type Bank = {
  id: string;
  label: string | null;
  customLabel: string | null;
  assetId: string | null;
  asset: Asset | null;
  companion: CompanionLocation | null;
  live: boolean;
  missing: boolean;
};

type BanksResponse = {
  banks: Bank[];
  grid: { rows: number; columns: number };
};

const serverUrlEl = document.querySelector<HTMLParagraphElement>("#server-url")!;
const mediaPathEl = document.querySelector<HTMLParagraphElement>("#media-path")!;
const serverPill = document.querySelector<HTMLSpanElement>("#server-pill")!;
const monitorSelect = document.querySelector<HTMLSelectElement>("#monitor-select")!;
const portInput = document.querySelector<HTMLInputElement>("#port-input")!;
const autoStart = document.querySelector<HTMLInputElement>("#auto-start")!;
const settingsStatus = document.querySelector<HTMLParagraphElement>("#settings-status")!;
const updateStatus = document.querySelector<HTMLParagraphElement>("#update-status")!;
const appVersionEl = document.querySelector<HTMLSpanElement>("#app-version")!;
const adminFrame = document.querySelector<HTMLIFrameElement>("#admin-frame")!;
const updateModal = document.querySelector<HTMLDivElement>("#update-modal")!;
const updateVersionEl = document.querySelector<HTMLParagraphElement>("#update-version")!;
const updateBodyEl = document.querySelector<HTMLParagraphElement>("#update-body")!;
const updateProgress = document.querySelector<HTMLDivElement>("#update-progress")!;
const updateProgressFill = document.querySelector<HTMLDivElement>("#update-progress-fill")!;
const updateProgressText = document.querySelector<HTMLSpanElement>("#update-progress-text")!;
const updateActions = document.querySelector<HTMLDivElement>("#update-actions")!;

const deckEl = document.querySelector<HTMLDivElement>("#deck")!;
const banksStatus = document.querySelector<HTMLParagraphElement>("#banks-status")!;
const gridRows = document.querySelector<HTMLInputElement>("#grid-rows")!;
const gridColumns = document.querySelector<HTMLInputElement>("#grid-columns")!;
const bankSelectedEl = document.querySelector<HTMLSpanElement>("#bank-selected")!;
const bankEndpointEl = document.querySelector<HTMLSpanElement>("#bank-endpoint")!;
const bankAsset = document.querySelector<HTMLSelectElement>("#bank-asset")!;
const bankLabel = document.querySelector<HTMLInputElement>("#bank-label")!;
const companionHost = document.querySelector<HTMLInputElement>("#companion-host")!;
const companionPort = document.querySelector<HTMLInputElement>("#companion-port")!;
const companionPage = document.querySelector<HTMLInputElement>("#companion-page")!;
const companionRow = document.querySelector<HTMLInputElement>("#companion-row")!;
const companionColumn = document.querySelector<HTMLInputElement>("#companion-column")!;
const companionStatus = document.querySelector<HTMLParagraphElement>("#companion-status")!;

let currentStatus: StatusInfo | null = null;
let pendingUpdate: UpdateInfo | null = null;
let banks: Bank[] = [];
let assets: Asset[] = [];
let grid = { rows: 4, columns: 8 };
let selectedBankId = "1";
let banksLoaded = false;

function setUpdateStatus(message: string, type: "" | "ok" | "error" = "") {
  updateStatus.hidden = !message;
  updateStatus.textContent = message;
  updateStatus.className = `status ${type}`.trim();
}

function showUpdateModal(info: UpdateInfo) {
  pendingUpdate = info;
  updateVersionEl.textContent = `Version ${info.version} is ready to install.`;
  if (info.body) {
    updateBodyEl.hidden = false;
    updateBodyEl.textContent = info.body;
  } else {
    updateBodyEl.hidden = true;
    updateBodyEl.textContent = "";
  }
  updateProgress.hidden = true;
  updateActions.hidden = false;
  updateProgressFill.style.width = "0%";
  updateProgressText.textContent = "0%";
  updateModal.hidden = false;
}

function hideUpdateModal() {
  updateModal.hidden = true;
  pendingUpdate = null;
}

function setSettingsStatus(message: string, type: "" | "ok" | "error" = "") {
  settingsStatus.hidden = !message;
  settingsStatus.textContent = message;
  settingsStatus.className = `status ${type}`.trim();
}

function renderStatus(status: StatusInfo) {
  currentStatus = status;
  serverUrlEl.textContent = status.serverUrl;
  mediaPathEl.textContent = status.mediaDir;
  portInput.value = String(status.port);
  autoStart.checked = status.autoStartDisplay;
}

function renderMonitors(monitors: MonitorInfo[], selectedIndex: number) {
  monitorSelect.innerHTML = monitors
    .map((m) => {
      const label = `${m.name} · ${m.width}×${m.height}${m.isPrimary ? " (primary)" : ""}`;
      return `<option value="${m.index}">${label}</option>`;
    })
    .join("");
  const fallback = monitors[0]?.index ?? 0;
  monitorSelect.value = String(
    monitors.some((m) => m.index === selectedIndex) ? selectedIndex : fallback
  );
}

function showTab(name: "control" | "banks" | "admin") {
  document.querySelectorAll(".tab").forEach((el) => {
    el.classList.toggle("active", (el as HTMLElement).dataset.tab === name);
  });
  document.querySelectorAll(".tab-panel").forEach((el) => {
    el.classList.toggle("active", el.id === `panel-${name}`);
  });
  if (name === "admin" && currentStatus) {
    const target = `${currentStatus.adminUrl}?desktop=1`;
    if (adminFrame.src !== target) {
      adminFrame.src = target;
    }
  }
  if (name === "banks" && !banksLoaded) {
    refreshBanks().catch((err) => setBanksStatus(String(err), "error"));
  }
}

// ---------------------------------------------------------------------------
// Trigger banks
// ---------------------------------------------------------------------------

function setBanksStatus(message: string, type: "" | "ok" | "error" = "") {
  banksStatus.hidden = !message;
  banksStatus.textContent = message;
  banksStatus.className = `status ${type}`.trim();
}

function setCompanionStatus(message: string, type: "" | "ok" | "error" = "") {
  companionStatus.hidden = !message;
  companionStatus.textContent = message;
  companionStatus.className = `status ${type}`.trim();
}

async function api<T>(path: string, init?: RequestInit): Promise<T> {
  if (!currentStatus) throw new Error("Server is still starting.");
  const res = await fetch(`${currentStatus.serverUrl}${path}`, {
    cache: "no-store",
    ...init,
    headers: {
      Accept: "application/json",
      ...(init?.body ? { "Content-Type": "application/json" } : {}),
      ...(init?.headers ?? {}),
    },
  });
  const text = await res.text();
  const data = text ? JSON.parse(text) : null;
  if (!res.ok) throw new Error(data?.error || `HTTP ${res.status}`);
  return data as T;
}

function assetUrl(asset: Asset) {
  return `${currentStatus?.serverUrl ?? ""}${asset.url}`;
}

function renderDeck() {
  // Always the applied layout, never a half-typed value in the rows/columns inputs.
  deckEl.style.setProperty("--deck-columns", String(grid.columns));
  deckEl.replaceChildren(
    ...banks.map((bank) => {
      const key = document.createElement("button");
      key.type = "button";
      key.className = "deck-key";
      key.dataset.bankId = bank.id;
      key.classList.toggle("selected", bank.id === selectedBankId);
      key.classList.toggle("empty", !bank.asset);
      key.classList.toggle("live", bank.live);
      key.classList.toggle("missing", bank.missing);
      key.title = bank.missing
        ? `Bank ${bank.id} — assigned media was deleted`
        : `Bank ${bank.id}${bank.label ? ` — ${bank.label}` : " — empty"}`;

      if (bank.asset) {
        if (bank.asset.type === "video") {
          const video = document.createElement("video");
          video.src = `${assetUrl(bank.asset)}#t=0.1`;
          video.muted = true;
          video.preload = "metadata";
          video.playsInline = true;
          key.append(video);
        } else {
          const img = document.createElement("img");
          img.src = assetUrl(bank.asset);
          img.alt = "";
          key.append(img);
        }
      }

      const id = document.createElement("span");
      id.className = "deck-id";
      id.textContent = bank.id;

      const label = document.createElement("span");
      label.className = "deck-label";
      label.textContent = bank.missing ? "Media missing" : (bank.label ?? "Empty");

      key.append(id, label);
      return key;
    })
  );
}

function renderInspector() {
  const bank = banks.find((b) => b.id === selectedBankId);
  bankSelectedEl.textContent = selectedBankId;
  bankEndpointEl.textContent = selectedBankId;
  if (document.activeElement !== bankAsset) bankAsset.value = bank?.assetId ?? "";
  if (document.activeElement !== bankLabel) bankLabel.value = bank?.customLabel ?? "";
  // Reset rather than inherit the last bank's coordinates — a stale page/row/column would
  // push this bank's thumbnail onto someone else's button.
  companionPage.value = String(bank?.companion?.page ?? 1);
  companionRow.value = String(bank?.companion?.row ?? 0);
  companionColumn.value = String(bank?.companion?.column ?? 0);
}

function renderAssetOptions() {
  bankAsset.replaceChildren(
    new Option("— Empty —", ""),
    ...assets.map((asset) => new Option(`${asset.name} (${asset.type})`, asset.id))
  );
}

async function refreshBanks() {
  const [banksRes, assetsRes] = await Promise.all([
    api<BanksResponse>("/api/trigger"),
    api<{ assets: Asset[] }>("/api/assets"),
  ]);
  banks = banksRes.banks;
  assets = assetsRes.assets;
  grid = banksRes.grid;
  // A live SSE refresh must not overwrite a size the user is in the middle of typing.
  if (document.activeElement !== gridRows) gridRows.value = String(grid.rows);
  if (document.activeElement !== gridColumns) gridColumns.value = String(grid.columns);
  if (!banks.some((b) => b.id === selectedBankId)) {
    selectedBankId = banks[0]?.id ?? "1";
  }
  renderAssetOptions();
  renderDeck();
  renderInspector();
  banksLoaded = true;
}

/** Draw an asset into a 72×72 Companion button PNG. Videos use their first frame. */
async function makeThumbnail(asset: Asset): Promise<string> {
  const size = 72;
  const canvas = document.createElement("canvas");
  canvas.width = size;
  canvas.height = size;
  const ctx = canvas.getContext("2d")!;
  ctx.fillStyle = "#000000";
  ctx.fillRect(0, 0, size, size);

  const source = await new Promise<HTMLImageElement | HTMLVideoElement>((resolve, reject) => {
    const fail = () => reject(new Error(`Could not read ${asset.name} for a thumbnail.`));
    if (asset.type === "video") {
      const video = document.createElement("video");
      video.crossOrigin = "anonymous";
      video.muted = true;
      video.preload = "auto";
      video.addEventListener("seeked", () => resolve(video), { once: true });
      video.addEventListener("error", fail, { once: true });
      video.addEventListener(
        "loadeddata",
        () => {
          video.currentTime = Math.min(0.1, video.duration || 0.1);
        },
        { once: true }
      );
      video.src = assetUrl(asset);
    } else {
      const img = new Image();
      img.crossOrigin = "anonymous";
      img.addEventListener("load", () => resolve(img), { once: true });
      img.addEventListener("error", fail, { once: true });
      img.src = assetUrl(asset);
    }
  });

  const sw = source instanceof HTMLVideoElement ? source.videoWidth : source.naturalWidth;
  const sh = source instanceof HTMLVideoElement ? source.videoHeight : source.naturalHeight;
  if (sw && sh) {
    // Cover-fit so the button never shows letterboxing.
    const scale = Math.max(size / sw, size / sh);
    const w = sw * scale;
    const h = sh * scale;
    ctx.drawImage(source, (size - w) / 2, (size - h) / 2, w, h);
  }
  return canvas.toDataURL("image/png");
}

function selectBank(id: string) {
  selectedBankId = id;
  renderDeck();
  renderInspector();
  setBanksStatus("");
}

async function saveBank() {
  const assetId = bankAsset.value || null;
  const label = bankLabel.value.trim() || null;
  await api(`/api/trigger/${encodeURIComponent(selectedBankId)}`, {
    method: "PUT",
    body: JSON.stringify({ assetId, label }),
  });
  await refreshBanks();
  setBanksStatus(
    assetId
      ? `Bank ${selectedBankId} now fires “${bankAsset.selectedOptions[0]?.text ?? assetId}”.`
      : `Bank ${selectedBankId} unassigned.`,
    "ok"
  );
}

async function fireBank(id: string) {
  const res = await api<{ fired: boolean; reason?: string }>(
    `/api/trigger/${encodeURIComponent(id)}`,
    { method: "POST" }
  );
  await refreshBanks();
  if (res.fired) {
    setBanksStatus(`Bank ${id} fired — it is now live.`, "ok");
  } else {
    setBanksStatus(res.reason ?? `Bank ${id} is empty.`);
  }
}

async function pushCompanionStyle() {
  const bank = banks.find((b) => b.id === selectedBankId);
  const host = companionHost.value.trim() || "127.0.0.1";
  const port = Number(companionPort.value) || 8000;
  const location = {
    page: Number(companionPage.value) || 1,
    row: Number(companionRow.value) || 0,
    column: Number(companionColumn.value) || 0,
  };

  setCompanionStatus("Pushing to Companion…");
  const png64 = bank?.asset ? await makeThumbnail(bank.asset) : undefined;
  await api("/api/companion/style", {
    method: "POST",
    body: JSON.stringify({
      host,
      port,
      ...location,
      text: bank?.label ?? "",
      png64,
    }),
  });

  // Remember the location so the next push to this bank is one click.
  await api(`/api/trigger/${encodeURIComponent(selectedBankId)}`, {
    method: "PUT",
    body: JSON.stringify({ companion: { ...location, pushStyle: true } }),
  });
  localStorage.setItem("loopserve.companion", JSON.stringify({ host, port }));
  await refreshBanks();
  setCompanionStatus(
    `Pushed to Companion ${host}:${port} page ${location.page}, row ${location.row}, column ${location.column}.`,
    "ok"
  );
}

function restoreCompanionSettings() {
  companionHost.value = "127.0.0.1";
  companionPort.value = "8000";
  companionPage.value = "1";
  companionRow.value = "0";
  companionColumn.value = "0";
  try {
    const saved = JSON.parse(localStorage.getItem("loopserve.companion") ?? "null");
    if (saved?.host) companionHost.value = saved.host;
    if (saved?.port) companionPort.value = String(saved.port);
  } catch {
    // fall back to defaults
  }
}

/** Keep the grid in step with presses from Stream Deck or edits in the media library. */
function connectBankEvents(serverUrl: string) {
  const events = new EventSource(`${serverUrl}/api/events`);
  events.onmessage = () => {
    if (!banksLoaded) return;
    refreshBanks().catch(() => {
      // a blip here just means the grid redraws on the next event
    });
  };
  events.onerror = () => {
    events.close();
    setTimeout(() => connectBankEvents(serverUrl), 3000);
  };
}

async function waitForServer(url: string) {
  for (let i = 0; i < 40; i++) {
    try {
      const res = await fetch(`${url}/api/live`, { cache: "no-store" });
      if (res.ok) {
        serverPill.textContent = "Online";
        serverPill.classList.add("ok");
        return true;
      }
    } catch {
      // retry
    }
    await new Promise((r) => setTimeout(r, 250));
  }
  serverPill.textContent = "API unreachable";
  return false;
}

async function boot() {
  const status = await invoke<StatusInfo>("get_status");
  const monitors = await invoke<MonitorInfo[]>("list_monitors");
  renderStatus(status);
  renderMonitors(monitors, status.displayMonitorIndex);
  appVersionEl.textContent = await getCurrentAppVersion();
  restoreCompanionSettings();
  const online = await waitForServer(status.serverUrl);
  if (online) {
    // Prefetch admin so the Media library tab is ready.
    adminFrame.src = `${status.adminUrl}?desktop=1`;
    await refreshBanks().catch((err) => setBanksStatus(String(err), "error"));
    connectBankEvents(status.serverUrl);
  }

  checkForUpdatesOnStartup().then((result) => {
    if (result.available && result.update) {
      showUpdateModal(result.update);
    }
  });
}

document.querySelectorAll(".tab").forEach((tab) => {
  tab.addEventListener("click", () => {
    const name = (tab as HTMLElement).dataset.tab as "control" | "banks" | "admin";
    showTab(name);
  });
});

deckEl.addEventListener("click", (event) => {
  const key = (event.target as HTMLElement).closest<HTMLElement>(".deck-key");
  const id = key?.dataset.bankId;
  if (!id) return;
  // Plain click selects; Cmd/Ctrl-click fires, matching a real Stream Deck press.
  if (event.metaKey || event.ctrlKey) {
    fireBank(id).catch((err) => setBanksStatus(String(err), "error"));
    return;
  }
  selectBank(id);
});

document.querySelector("#banks-refresh")!.addEventListener("click", () => {
  refreshBanks()
    .then(() => setBanksStatus("Banks reloaded.", "ok"))
    .catch((err) => setBanksStatus(String(err), "error"));
});

document.querySelector("#grid-apply")!.addEventListener("click", async () => {
  try {
    await api("/api/trigger", {
      method: "PUT",
      body: JSON.stringify({
        grid: { rows: Number(gridRows.value), columns: Number(gridColumns.value) },
      }),
    });
    await refreshBanks();
    setBanksStatus("Grid layout updated. Existing mappings were kept.", "ok");
  } catch (err) {
    setBanksStatus(String(err), "error");
  }
});

document.querySelector("#bank-save")!.addEventListener("click", () => {
  saveBank().catch((err) => setBanksStatus(String(err), "error"));
});

document.querySelector("#bank-fire")!.addEventListener("click", () => {
  fireBank(selectedBankId).catch((err) => setBanksStatus(String(err), "error"));
});

document.querySelector("#bank-clear")!.addEventListener("click", async () => {
  try {
    await api(`/api/trigger/${encodeURIComponent(selectedBankId)}`, { method: "DELETE" });
    await refreshBanks();
    setBanksStatus(`Bank ${selectedBankId} unassigned.`, "ok");
  } catch (err) {
    setBanksStatus(String(err), "error");
  }
});

document.querySelector("#companion-push")!.addEventListener("click", () => {
  pushCompanionStyle().catch((err) => setCompanionStatus(String(err), "error"));
});

document.querySelector("#goto-admin")!.addEventListener("click", () => showTab("admin"));

document.querySelector("#reload-admin")!.addEventListener("click", () => {
  if (!currentStatus) return;
  adminFrame.src = `${currentStatus.adminUrl}?desktop=1&t=${Date.now()}`;
});

document.querySelector("#save-settings")!.addEventListener("click", async () => {
  try {
    const status = await invoke<StatusInfo>("save_settings", {
      port: Number(portInput.value),
      displayMonitorIndex: Number(monitorSelect.value),
      autoStartDisplay: autoStart.checked,
    });
    renderStatus(status);
    setSettingsStatus("Settings saved. Restart the app if you changed the port.", "ok");
  } catch (err) {
    setSettingsStatus(String(err), "error");
  }
});

document.querySelector("#open-media")!.addEventListener("click", async () => {
  try {
    await invoke("open_media_folder");
  } catch (err) {
    setSettingsStatus(String(err), "error");
  }
});

document.querySelector("#open-docs")!.addEventListener("click", async () => {
  await invoke("open_docs");
});

document.querySelector("#show-display")!.addEventListener("click", async () => {
  try {
    await invoke("save_settings", {
      port: Number(portInput.value),
      displayMonitorIndex: Number(monitorSelect.value),
      autoStartDisplay: autoStart.checked,
    });
    await invoke("show_display");
    setSettingsStatus(
      "Fullscreen output opened. Use Media library → Set live to show content.",
      "ok"
    );
  } catch (err) {
    setSettingsStatus(String(err), "error");
  }
});

document.querySelector("#hide-display")!.addEventListener("click", async () => {
  await invoke("hide_display");
});

document.querySelector("#check-updates")!.addEventListener("click", async () => {
  const btn = document.querySelector<HTMLButtonElement>("#check-updates")!;
  btn.disabled = true;
  setUpdateStatus("Checking for updates…");
  try {
    clearSkippedVersion();
    const result = await checkForUpdates();
    if (result.available && result.update) {
      setUpdateStatus(`Update ${result.update.version} available.`, "ok");
      showUpdateModal(result.update);
    } else {
      setUpdateStatus("You’re on the latest version.", "ok");
    }
  } catch (err) {
    setUpdateStatus(String(err), "error");
  } finally {
    btn.disabled = false;
  }
});

document.querySelector("#update-now")!.addEventListener("click", async () => {
  updateActions.hidden = true;
  updateProgress.hidden = false;
  const ok = await downloadAndInstallUpdate((downloaded, total) => {
    const percent = Math.round((downloaded / total) * 100);
    updateProgressFill.style.width = `${percent}%`;
    updateProgressText.textContent = `${percent}%`;
  });
  if (!ok) {
    updateActions.hidden = false;
    updateProgress.hidden = true;
    setUpdateStatus("Update failed. Try again or download from GitHub Releases.", "error");
  }
});

document.querySelector("#update-later")!.addEventListener("click", () => {
  hideUpdateModal();
});

document.querySelector("#update-skip")!.addEventListener("click", () => {
  if (pendingUpdate?.version) {
    saveSkippedVersion(pendingUpdate.version);
  }
  hideUpdateModal();
});

boot().catch((err) => {
  serverPill.textContent = "Error";
  setSettingsStatus(String(err), "error");
});
