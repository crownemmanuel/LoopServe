# Releasing LoopServe

Tag-triggered GitHub Actions builds (macOS ARM/Intel, Windows, Linux), signs updater artifacts, and publishes a GitHub Release with `latest.json` for the in-app opt-in updater.

## One-time setup

### 1. Updater signing keys

Keys live locally (never commit the private key):

- Private: `~/.tauri/wcimd-media.key` (or regenerate as `~/.tauri/loopserve.key`)
- Public key is embedded in `src-tauri/tauri.conf.json` → `plugins.updater.pubkey`

```bash
npx tauri signer generate -w ~/.tauri/loopserve.key
```

If you regenerate, paste the new `.pub` contents into `tauri.conf.json` and update the GitHub secret. Existing installs cannot verify updates signed with a different private key.

### 2. GitHub repository secrets

Repo → Settings → Secrets and variables → Actions:

| Secret | Value |
| --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | Full contents of the private key file |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Password if you set one (empty OK) |

Optional macOS Developer ID / notarization:

| Secret | Purpose |
| --- | --- |
| `APPLE_CERTIFICATE` | Base64 `.p12` |
| `APPLE_CERTIFICATE_PASSWORD` | Certificate password |
| `KEYCHAIN_PASSWORD` | Temporary CI keychain password |
| `APPLE_ID` / `APPLE_PASSWORD` / `APPLE_TEAM_ID` | Notarization |

If Apple secrets are missing, macOS builds still run but are unsigned.

### 3. Updater endpoint

`tauri.conf.json` points at:

```text
https://github.com/crownemmanuel/LoopServe/releases/latest/download/latest.json
```

Change the owner/repo if the GitHub remote differs.

## Ship a release

1. Bump `version` in `package.json`, `src-tauri/tauri.conf.json`, and `src-tauri/Cargo.toml`.
2. Update [CHANGELOG.md](./CHANGELOG.md).
3. Commit, then:

```bash
git tag v0.1.0
git push origin main
git push origin v0.1.0
```

4. Actions → **Release** builds and uploads installers + updater artifacts.

## In-app updater UX

On launch (after ~2s) the app checks GitHub Releases. If a newer signed build exists and was not skipped, a modal offers:

- **Update now** — download, install, relaunch
- **Later** — dismiss until next launch
- **Skip this version** — remember and hide until a newer version

**Check for updates** in Settings clears a skipped version and checks immediately.
