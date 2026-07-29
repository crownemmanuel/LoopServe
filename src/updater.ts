import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

export interface UpdateInfo {
  version: string;
  date?: string;
  body?: string;
}

export interface UpdateResult {
  available: boolean;
  update?: UpdateInfo;
}

export const FALLBACK_APP_VERSION = "0.1.0";

const STORAGE_KEY_SKIPPED_VERSION = "loopserve-skipped-version";

export function saveSkippedVersion(version: string): void {
  try {
    localStorage.setItem(STORAGE_KEY_SKIPPED_VERSION, version);
  } catch {
    // ignore
  }
}

export function getSkippedVersion(): string | null {
  try {
    return localStorage.getItem(STORAGE_KEY_SKIPPED_VERSION);
  } catch {
    return null;
  }
}

export function clearSkippedVersion(): void {
  try {
    localStorage.removeItem(STORAGE_KEY_SKIPPED_VERSION);
  } catch {
    // ignore
  }
}

export async function getCurrentAppVersion(): Promise<string> {
  try {
    const appApi = await import("@tauri-apps/api/app");
    if (typeof appApi.getVersion === "function") {
      return await appApi.getVersion();
    }
  } catch {
    // fall through
  }
  return FALLBACK_APP_VERSION;
}

export async function checkForUpdates(): Promise<UpdateResult> {
  try {
    const update = await check();
    if (update) {
      return {
        available: true,
        update: {
          version: update.version,
          date: update.date,
          body: update.body,
        },
      };
    }
    return { available: false };
  } catch (error) {
    console.error("[Updater] Failed to check for updates:", error);
    return { available: false };
  }
}

export async function downloadAndInstallUpdate(
  onProgress?: (progress: number, total: number) => void
): Promise<boolean> {
  try {
    const update = await check();
    if (!update) return false;

    let downloaded = 0;
    let contentLength = 0;

    await update.downloadAndInstall((event) => {
      switch (event.event) {
        case "Started":
          contentLength = event.data.contentLength || 0;
          break;
        case "Progress":
          downloaded += event.data.chunkLength;
          if (onProgress && contentLength > 0) {
            onProgress(downloaded, contentLength);
          }
          break;
        case "Finished":
          break;
      }
    });

    await relaunch();
    return true;
  } catch (error) {
    console.error("[Updater] Failed to download and install update:", error);
    return false;
  }
}

/** Silent startup check; respects skipped versions. */
export async function checkForUpdatesOnStartup(): Promise<UpdateResult> {
  await new Promise((resolve) => setTimeout(resolve, 2000));
  const result = await checkForUpdates();
  if (result.available && result.update) {
    if (getSkippedVersion() === result.update.version) {
      return { available: false };
    }
  }
  return result;
}
