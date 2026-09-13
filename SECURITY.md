# Astra 实验版安全边界

Astra 工作台仅供个人在回环地址使用，不提供公网多用户认证。自动研究会执行模型生成的代码，请在独立工作目录或隔离环境运行。
Codex 后端使用官方权限控制；Pi 后端的执行边界见下文。任务日志可能包含研究内容和本机路径，不应随问题报告整份公开。

Astra 的安全问题请使用本仓库的 GitHub 私密漏洞报告，不要将细节发到公开问题区。下文保留上游 Pi 安全策略，提到的邮箱和基础设施属于上游。

# Security Policy (upstream reference)

This document should guide you about understanding the security concept behind
Pi and also where the boundaries are.

In general Pi is a coding agent that runs locally within the security boundary
of the user that is running it.  It's the responsibility of the user to monitor
its operations or to contain it within a container, virtual machine or other
Sandbox solution.

Pi treats the local user account and files writable by that account as inside
the same trust boundary as the Pi process itself.  If an attacker can modify files
under the user's home directory, workspace, shell startup files, environment, or
Pi configuration, they can generally influence Pi or other local developer tools.
Reports that depend on such prior local write access are not security
vulnerabilities unless they demonstrate how Pi grants that write access or crosses
an operating-system privilege boundary.

Pi relies on users installing trustworthy extensions and loading trustworthy
skills and only to use pi within trusted repositories.  This is because files
like `AGENTS.md` or instructions in comments can be used to prompt inject the
coding agent trivially and this cannot be protected against.

## Reporting a Vulnerability

If you believe you found a security vulnerability in pi or another package in
this repository, please report it privately by either:

- Emailing `security@earendil.com`, or
- Opening a private report through GitHub Security Advisories for this repository

Please include:

- A description of the issue and its impact
- Steps to reproduce, proof of concept, or relevant logs
- Affected package, version, commit, or configuration
- Any known mitigations

Do not open a public issue for security-sensitive reports.  We will review
reports and coordinate disclosure as appropriate.

## Scope

Security issues in the distributed packages, command-line tools, APIs, and
repository code are in scope as well as earendil operated infrastructure
on `pi.dev`.

## Out Of Scope

- Local code execution or sandboxing behavior (the Pi coding agent intentionally does not have a sandbox)
- Behavior of pi extensions or skills installed by the user
- Risks from working in untrusted repositories
- Risks from installing untrusted extensions, skills, packages, or tools
- Isuses caused by non trustworthy MITM proxies
- Public internet exposure of a Pi installation
- Prompt injection attacks
- Exposed secrets that are third-party/user-controlled credentials
- Reports requiring the ability to create, modify, delete, or replace files,
  directories, symlinks, environment variables, shell configuration, or other
  user-controlled local state on the target machine. This includes `~/.pi`,
  `~/.pi/agent/models.json`, workspace files, `AGENTS.md`, skills, extensions,
  extension configuration, dotfiles, and files synchronized through NFS, roaming
  profiles, or dotfile managers, unless the report shows how Pi itself grants
  that access.
- Issues caused by intentionally weakened user configuration.
- Resource/DOS claims that require trusted local input/config against the pi coding agent.
- Reports about malicious model output.
- User-approved or user-initiated local actions presented as vulnerabilities.

## Notes for Reporters

The most useful reports show a current, reproducible security boundary bypass
with demonstrated impact.  Reports that only show expected local-agent behavior,
prompt injection, or a malicious trusted extension/skill are not security
vulnerabilities under this model.

For example, a report showing that malicious contents written to a trusted Pi
configuration file cause Pi to execute commands, load attacker-controlled tools,
send credentials to an attacker-controlled endpoint, or otherwise change behavior
is out of scope.

When possible, include the exact affected path, package version or commit SHA,
configuration, and a proof of concept against the latest release or latest
`main`.  For dependency reports, include evidence that the shipped dependency is
affected and that the issue is reachable through Pi.  For exposed-secret reports,
include evidence that the credential is owned by Earendil or grants access to
Earendil-operated infrastructure or services.
