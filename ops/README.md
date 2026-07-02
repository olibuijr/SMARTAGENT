# SMARTAGENT ops

Runtime dependencies and their reboot-survival wiring. Without these, the
`browser`, `search`, and embedding-backed tools (`semdb`/`memory`/`rag`) fail
at call time after a reboot.

## Services

| Dep | Managed by | Endpoint |
|-----|-----------|----------|
| Headless Chromium (CDP) | `systemd --user` unit `smartagent-chromium.service` | `http://127.0.0.1:9222` |
| SearXNG | docker container `smartagent-searxng` (`--restart unless-stopped`) | `http://127.0.0.1:8888` |
| Embeddings (titan) | external host, see `config/smartagent.conf` | `embeddings_endpoint` |
| Nightly backup | `systemd --user` timer `smartagent-backup.timer` (03:30) | `~/.smartagent-backups/` |

## Install (once)

```sh
mkdir -p ~/.config/systemd/user
cp ops/systemd/*.service ops/systemd/*.timer ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now smartagent-chromium.service smartagent-backup.timer
loginctl enable-linger "$USER"   # so user units run without an active login (survives reboot)
```

## Operate

- `ops/preflight.sh` — start anything down + probe every configured endpoint (run after reboot / before a session).
- `ops/backup.sh` — one-shot backup of `data/` (memory, secrets, schedule, semdb); keeps 7. Fired nightly by the timer.
- Self-update pi: `./pi --self-update` (pinned version in `.pi/runtime/package.json`; smoke-tested with rollback — never automatic).

## Restore

```sh
tar -xzf ~/.smartagent-backups/smartagent-data-<STAMP>.tar.gz -C /path/to/SMARTAGENT
```
