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

let currentStatus: StatusInfo | null = null;
let pendingUpdate: UpdateInfo | null = null;

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

function showTab(name: "control" | "admin") {
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
  const online = await waitForServer(status.serverUrl);
  if (online) {
    // Prefetch admin so the Media library tab is ready.
    adminFrame.src = `${status.adminUrl}?desktop=1`;
  }

  checkForUpdatesOnStartup().then((result) => {
    if (result.available && result.update) {
      showUpdateModal(result.update);
    }
  });
}

document.querySelectorAll(".tab").forEach((tab) => {
  tab.addEventListener("click", () => {
    const name = (tab as HTMLElement).dataset.tab as "control" | "admin";
    showTab(name);
  });
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
