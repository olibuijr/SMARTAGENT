---
name: vpn-upgrade
description: Upgrade or repair AkurAI-VPN nodes and embedding reachability. Use when VPN nodes need the latest version, Arch-built binaries are being deployed to Ubuntu EC2, akurai-vpn on EC2/titan/midget is unhealthy, or SMARTAGENT embeddings fail against titan over 100.88.0.2:8081.
---

# VPN Upgrade

Use this skill for AkurAI-VPN node upgrades and for embedding failures caused by
the VPN overlay path to titan.

## Operating rule

- Work from `workspaces/AkurAI-VPN` and read that repo's `AGENTS.md` first.
- Use the repo deploy path: `./deploy.sh ec2` for republish, or
  `./deploy.sh patch|minor|major` for a release. Do not manually copy a local
  Arch GNU binary to Ubuntu EC2.
- The deploy target for EC2 is `x86_64-unknown-linux-musl`. Arch-to-Ubuntu
  deploys must be portable musl artifacts unless the deploy is an intentional
  same-distro operation.
- Before deploy, `deploy.sh` must run the regression gate: deploy-script
  regression, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets`,
  `cargo test --workspace`, `cargo build -p akurai-node -p akurai-relay`,
  `sudo -n tests/netns/direct.sh`, and `sudo -n tests/netns/symmetric.sh`.
- If a deployed binary errors with `GLIBC_2.39 not found` or similar, stop and
  rebuild/deploy through `./deploy.sh`; do not patch around it with ad hoc
  copies.

## Nodes and paths

- midget local service: `akurai-node.service`, binary
  `/usr/local/bin/akurai-node`, user binary `~/.akurai-vpn/bin/akurai-node`,
  overlay `100.88.0.5`.
- titan service: `akurai-node-tunnel.service`, binaries
  `/usr/local/bin/akurai-node` and `~/.akurai-vpn/bin/akurai-node`, overlay
  `100.88.0.2`.
- EC2 akurai-mail services: `akurai-vpn-control.service`,
  `akurai-vpn-relay.service`, and `akurai-node.service`. Control binary is
  `/opt/akurai-vpn-control/bin/akurai-vpn-control`; relay is
  `/opt/akurai-vpn-relay/bin/akurai-relay`; EC2 node is
  `/usr/local/bin/akurai-node` and `/opt/akurai-peer/bin/akurai-node`.

Always verify every installed binary path with `version`. Also verify `file`
and `ldd`; static or musl-linked artifacts are expected for EC2 control/node,
and no artifact should require a glibc newer than the Ubuntu host provides.

## Verification checklist

1. `curl -fsS https://vpn.olibuijr.com/api/health` returns status ok and the
   expected version.
2. EC2, titan, and midget services are active.
3. midget default route is unchanged; overlay route is only `100.88.0.0/16`
   via `akurai0`.
4. midget reaches titan overlay:
   `ping -c 3 -W 2 100.88.0.2` and `nc -vz -w 3 100.88.0.2 8081`.
5. The titan embedding endpoint works:
   POST `http://100.88.0.2:8081/v1/embeddings` with model `embeddinggemma` and
   confirm a 768-dimensional vector.
6. SMARTAGENT semdb works end to end from the repo:
   create a table under `target/test-scratch/`, then run
   `./target/release/semdb embed <db> --id <id> --text <text>`.

When reading node tokens for peermap checks, use sudo and pass the token without
printing it. Never echo bearer tokens into logs, notes, or task comments.

## Common failure modes

- `connect 100.88.0.2:8081: unreachable within 10s`: check overlay service
  state, node versions, peermap refresh logs, and midget-to-titan ping/nc.
- `~/.akurai-vpn/bin/akurai-node` stale while `/usr/local/bin/akurai-node` is
  current: sync the user binary too, or status/debug commands will lie.
- `status` shows `overlay: unassigned`: ensure the node has `network.conf`
  `overlay_ip=...` or a fresh `state/overlay.ip` written by the running tunnel.
- `GLIBC_2.39 not found` on Ubuntu: an Arch GNU artifact was deployed. Rebuild
  and deploy with `./deploy.sh`, which defaults to musl and validates artifacts.

## Close-out

- Update AkurAI Notes note 30 with durable live-state/deploy facts.
- Store a semantic memory fact describing the successful upgrade path and
  verification evidence.
- Update the SMARTAGENT task board through `./pi`.
