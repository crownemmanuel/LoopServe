# Changelog

## 0.1.2

- Add trigger banks: a Stream Deck–style grid page for mapping assets to stable bank ids
  (`/api/trigger/:bankId`), so Companion buttons never need editing when the target asset changes
- Optional helper to push a bank's label and thumbnail to a Companion button
- Companion module ("Trigger Bank" action) moved in-repo under `other_app/`

## 0.1.1

- Fix unsigned macOS release packaging when Apple signing secrets are not configured

## 0.1.0

- Initial open-source release as **LoopServe**
- Standalone Tauri app with embedded media server
- Control + Media library UI, fullscreen output on selected monitor
- GitHub Actions release workflow and opt-in in-app updater
