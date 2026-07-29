# LoopServe

**Open-source looping media server for stage, stream, and house-of-worship displays.**

LoopServe is a standalone desktop app with a built-in HTTP media server. Upload videos or images, set one live, and play it fullscreen (looping, muted) on a chosen monitor. Control it from the UI, Bitfocus Companion, or any HTTP client.

Originally built for church production workflows; useful anywhere you need a simple “set live → fullscreen loop” box.

## Features

- **Embedded media server** — no separate Node/Pi project required for desktop use
- **Fullscreen output** — pick a monitor; black idle screen until something is live
- **Media library** — upload, replace (same ID), delete, set live / clear
- **REST + SSE API** — Companion-friendly endpoints and OpenAPI schema
- **Opt-in auto-updates** — checks GitHub Releases; you choose Update / Later / Skip
- **Cross-platform** — macOS, Windows, and Linux (via Tauri)

## Download

Installers are published on [GitHub Releases](https://github.com/crownemmanuel/LoopServe/releases). Prefer a signed release build for auto-update support.

## Quick start

1. Install and launch **LoopServe**
2. Choose the monitor for fullscreen output → **Show fullscreen output**
3. Open **Media library** → upload a video or image → **Set live**
4. Point Companion (or another client) at `http://THIS_PC_IP:8787`

Media files live in the app data folder (shown in the Control tab). Use **Open media folder** to browse them.

## Develop

### Prerequisites

- [Node.js](https://nodejs.org/) 18+
- [Rust](https://rustup.rs/) (stable)
- Platform deps for [Tauri 2](https://v2.tauri.app/start/prerequisites/)

### Run from source

```bash
git clone https://github.com/crownemmanuel/LoopServe.git
cd LoopServe
npm install
npm run tauri dev
```

Dev UI: **http://localhost:1430** (avoids clashing with other Tauri apps on 1420).  
Default media API port: **8787**.

### Build

```bash
npm run tauri build
```

Outputs under `src-tauri/target/release/bundle/` (e.g. `.app` / `.dmg` on macOS).

## API (overview)

Default base URL: `http://127.0.0.1:8787`

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/` | Fullscreen display page |
| `GET` | `/admin` | Media library UI |
| `GET` | `/api/assets` | List assets |
| `POST` | `/api/live/:id` | Set live asset |
| `GET` | `/api/live` | Current live state |
| `GET` | `/api/events` | SSE live updates |
| `GET` | `/api/docs` | Swagger UI |
| `GET` | `/openapi.json` | OpenAPI document |

See the in-app schema (**Open API schema** button) for full details.

## Companion

A Bitfocus Companion module can target this host and port to set live media by asset ID or name. If you maintain a separate Companion package, point it at LoopServe’s API (`/api/assets`, `/api/live/:id`).

## Releases & updates

Push a `v*` tag to run `.github/workflows/release.yml` (multi-platform Tauri build + updater artifacts). Details: [RELEASE.md](./RELEASE.md).

In-app updates are **opt-in**: on a new release the app can show **Update now**, **Later**, or **Skip this version**.

## Contributing

Issues and pull requests are welcome. For larger changes, open an issue first so we can align on direction.

1. Fork and create a feature branch
2. Keep changes focused and match existing style
3. Test with `npm run tauri dev` on your platform
4. Open a PR with a short description of the why

## License

MIT — see [LICENSE](./LICENSE).

## Acknowledgments

Built for real production use (including church AV). Product name **LoopServe**; not affiliated with any particular congregation or brand beyond this repository.
