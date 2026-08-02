## WCIMD Media Serve

Control the WCIMD Media Serve loop-stinger server from Companion.

### Connection

| Field | Description |
|---|---|
| Host | IP or hostname of the LoopServe computer / Pi (e.g. `172.16.20.120` or `wcimdmediaserver.local`) |
| Port | HTTP port (default `8787`) |
| Poll Interval | How often to refresh the asset list and live state (seconds) |

Admin UI / API docs (from another computer):

- `http://HOST:PORT/admin`
- `http://HOST:PORT/api/docs`

### Actions

- **Trigger Bank** — fire a numbered LoopServe bank (`POST /api/trigger/{bankId}`). **Use this for
  Stream Deck buttons.** The bank id stays put; you change which asset it plays in LoopServe →
  **Banks**, and this button never needs editing. An empty bank does nothing and logs a warning
  rather than erroring.
- **Set Live (Asset)** — pick an asset from the dropdown (names from `GET /api/assets`)
- **Set Live (ID)** — set live by asset id
- **Set Live (Name)** — set live by display name
- **Clear Live** — clear the currently live asset
- **Refresh Assets** — force-refresh the asset list now

### Feedbacks

- **Asset Is Live** — true when the selected asset is currently live
- **Something Is Live** — true when any asset is live

### Variables

- `$(wcimd-mediaserve:live_id)`
- `$(wcimd-mediaserve:live_name)`
- `$(wcimd-mediaserve:live_type)`
- `$(wcimd-mediaserve:asset_count)`
- `$(wcimd-mediaserve:connection_ok)`
