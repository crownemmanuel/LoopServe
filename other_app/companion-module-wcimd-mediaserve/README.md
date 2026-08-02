# companion-module-wcimd-mediaserve

Bitfocus Companion module for **LoopServe** (and the older WCIMD Media Serve Pi build).

Lets you fire loop-stinger assets from Companion buttons and set them live on the fullscreen output.

## Features

- **Trigger Bank** — press a stable bank number; LoopServe decides what it plays
- Asset dropdown populated from `GET /api/assets`
- Set live by asset / id / name
- Clear live
- Feedbacks for live state
- Variables for live asset info

## Trigger banks (recommended for Stream Deck)

Companion's API cannot rewrite a button's action from outside, so assigning a new asset to a button
normally means editing Companion. Banks remove that step:

```text
Stream Deck press
  → Companion action "Trigger Bank", bankId = 3
  → POST http://LOOPSERVE_HOST:8787/api/trigger/3
  → LoopServe looks up bank 3 → asset → sets it live
```

Remapping happens entirely in LoopServe's **Banks** tab. The Companion action stays `3` forever.

Firing an empty bank is not an error — the module logs a warning and the button stays green.

## Install in Companion (upload package)

Build the installable package:

```bash
cd other_app/companion-module-wcimd-mediaserve
npm install
npm run package
```

That creates:

`wcimd-mediaserve-1.0.0.tgz`

In Companion:

1. Open **Settings → Modules** (or **Import module / Custom module**)
2. Upload / import `wcimd-mediaserve-1.0.0.tgz`
3. Add a connection:
   - Module: **WCIMD Media Serve**
   - Host: the LoopServe computer's IP (or `wcimdmediaserver.local` on the Pi build)
   - Port: `8787`
4. Create buttons with action **Trigger Bank** and bank ids `1`, `2`, `3`, …
5. In LoopServe → **Banks**, assign an asset to each of those bank ids

> Companion uses a `.tgz` package (not a plain `.zip`). That is the file you upload.

## Pair with the media server

- LoopServe desktop app: the repository this module now lives in (`../..`)
- Admin UI: `http://HOST:8787/admin`
- API schema: `http://HOST:8787/api/docs`
- Older Pi build: `/Users/emmanuelcrown/Documents/Projects/wcimdmediaServe`

## Actions / API mapping

| Companion action | API |
|---|---|
| Trigger Bank | `POST /api/trigger/:bankId` |
| Set Live (Asset / ID) | `POST /api/live/:id` |
| Set Live (Name) | `PUT /api/live` `{ "name": "..." }` |
| Clear Live | `POST /api/live/clear` |
| Refresh Assets | `GET /api/assets` |
