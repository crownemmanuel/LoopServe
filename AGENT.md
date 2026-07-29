# Agent / maintainer notes — LoopServe

Instructions for releasing and for the in-app auto-updater.

## Where the updater keys go

Tauri signs release artifacts with a **minisign** keypair.

| Key | Where it lives | Commit? |
| --- | --- | --- |
| **Public key** | `src-tauri/tauri.conf.json` → `plugins.updater.pubkey` | **Yes** (safe; apps use it to verify downloads) |
| **Private key** | Local file only, e.g. `~/.tauri/wcimd-media.key` or `~/.tauri/loopserve.key` | **Never** |
| **Private key (CI)** | GitHub → Settings → Secrets → Actions → `TAURI_SIGNING_PRIVATE_KEY` | N/A (secret) |
| Optional password | Secret `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | N/A |

Current public key is already set in `tauri.conf.json`. The matching private key on this machine was generated at:

```text
~/.tauri/wcimd-media.key
```

Put the **full contents** of that file into the GitHub secret `TAURI_SIGNING_PRIVATE_KEY` before the first tagged release. Without it, CI cannot produce signed `latest.json` updater artifacts.

### Regenerate keys (only if lost)

```bash
npx tauri signer generate -w ~/.tauri/loopserve.key
# Paste ~/.tauri/loopserve.key.pub into tauri.conf.json plugins.updater.pubkey
# Update GitHub secret TAURI_SIGNING_PRIVATE_KEY with the new private key
```

Changing the public key breaks update verification for installs that still have the old pubkey baked in — ship a manual install once after a key rotation.

## Update check URL

Apps check:

```text
https://github.com/crownemmanuel/LoopServe/releases/latest/download/latest.json
```

Configured in `src-tauri/tauri.conf.json` → `plugins.updater.endpoints`. CI (`.github/workflows/release.yml`) uploads `latest.json` when a version tag is pushed.

## Push a new release (tags)

1. **Bump version** in all three places (keep them equal):
   - `package.json` → `version`
   - `src-tauri/tauri.conf.json` → `version`
   - `src-tauri/Cargo.toml` → `version`
2. **Update** `CHANGELOG.md`.
3. **Commit** on `main`:

```bash
git add -A
git commit -m "Bump version to 0.1.1"
git push origin main
```

4. **Tag and push the tag** (this triggers the Release workflow):

```bash
git tag v0.1.1
git push origin v0.1.1
```

Tag must match `v*` (e.g. `v0.1.1`). The workflow:

- Creates a GitHub Release
- Builds macOS (ARM + Intel), Windows, Linux
- Signs updater artifacts with `TAURI_SIGNING_PRIVATE_KEY`
- Uploads installers + `latest.json`

5. Confirm under **Actions** that the workflow succeeded, then under **Releases** that assets include `latest.json`.

Manual run: Actions → **Release** → **Run workflow**.

## First-time remote setup

```bash
git remote add origin git@github.com:crownemmanuel/LoopServe.git
git branch -M main
git push -u origin main
```

Then add secrets (especially `TAURI_SIGNING_PRIVATE_KEY`) before tagging `v0.1.0`.

## In-app updater UX

On launch (~2s delay) the app calls the updater plugin. Users get **Update now** / **Later** / **Skip this version**. Manual check: Settings → **Check for updates**.
