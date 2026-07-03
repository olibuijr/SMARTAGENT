---
name: akurai-ec2-vm-ops
description: Operate AkurAI EC2/VM safely with akurai-ec2 and SSH aliases
metadata:
  smartagent:
    role: Ops
    task: ec2-vm-ops
---
# AkurAI EC2/VM Ops

## When to Use
Use this skill when a task asks for EC2, VM, AWS platform box, `mail.olibuijr.com`, `akurai-mail`, remote systemd/nginx/TLS/logs, or AkurAI app deploy/status work.

The approved local tool is `akurai-ec2` (installed at `~/.local/bin/akurai-ec2`; source workspace `workspaces/akurai-ec2`). The default SSH alias is `akurai-mail` via the user's SSH config.

## Procedure
1. Route and pull a board task first, then work in that task worktree for any repo edits.
2. Discover command help without touching secrets:
   - `akurai-ec2 --help`
   - `akurai-ec2 ports list`
   - `akurai-ec2 status`
3. For read-only remote probes, prefer the CLI wrappers:
   - `akurai-ec2 status` for services, ports, nginx, disk.
   - `akurai-ec2 ps [pattern]` / `akurai-ec2 processes [pattern]` for systemd/process status.
   - `akurai-ec2 logs <service>` for recent journal output.
   - `akurai-ec2 ports check <port>` or `akurai-ec2 ports free [n]` before deploying.
4. For deployments, use the repo's own release/deploy path when present. If using the EC2 helper directly:
   - build the artifact locally in the appropriate repo/worktree;
   - claim/check a port with `akurai-ec2 ports ...`;
   - use `akurai-ec2 deploy-binary <name> <bin> <port> [appdir]`;
   - use `akurai-ec2 nginx-proxy <domain> <port>` and `akurai-ec2 tls <domain>` only when the task explicitly requires public routing/TLS.
5. For source repos with `akurai-deploy.toml`, prefer `akurai-ec2 release --mode update|publish|release [-C <dir>]` rather than re-deriving release steps.
6. For SSH one-offs, use `akurai-ec2 ssh '<command>'`; avoid raw `ssh` unless diagnosing the helper itself.

## Pitfalls
- Never print or copy SSH private keys, password files, AWS credentials, MCP tokens, or remote env secret values.
- Do not read `~/.ssh`, `~/.aws`, SMARTAGENT `data/secrets`, or remote `/etc/...env` secret files unless a credential-handling task explicitly authorizes a policy-safe path.
- `akurai-ec2 allow-ssh` may modify AWS security group ingress for the current public IP; use only when SSH timeout indicates IP drift or the task asks for EC2 access repair.
- `secret-sync` is a guide for passvault-to-EC2 env sync; it must not be used to expose secret values in logs.
- Copy/install of `akurai-ec2` must keep `libexec/akurai-ec2-manifest.py` at `../libexec/` relative to the binary.

## Verification
- Skill/tool discovery: `skills search ec2` or `skills match --role Ops '<ec2 vm task>'` should find this skill.
- CLI discovery: `command -v akurai-ec2` and `akurai-ec2 --help` should show the approved command list.
- For remote changes, verify by using the real surface: `akurai-ec2 status`, `akurai-ec2 ps <service>`, `akurai-ec2 logs <service>`, `curl`/browser against the public URL, or the repo's release verification.

Required secret/credential names (names only, never values): SSH config alias `akurai-mail`; AWS CLI credentials/profile sufficient for EC2 security group changes when using `allow-ssh`; akurai-passvault entries for any app env secret synced with `akurai-ec2 secret-sync`.
