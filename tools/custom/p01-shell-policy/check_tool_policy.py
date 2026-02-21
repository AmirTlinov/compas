#!/usr/bin/env python3
"""P01 tool-policy checker.

Scans plugin and imported tool definitions in the repo and reports forbidden
shell command usage in strict mode.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import sys
import tomllib
from typing import Any, Iterable, Optional


FORBIDDEN_COMMANDS = {
    "bash",
    "sh",
    "zsh",
    "dash",
    "fish",
    "csh",
    "tcsh",
    "ksh",
    "pwsh",
    "powershell",
    "cmd",
    "cmd.exe",
}

PLUGIN_DIR = pathlib.Path(".agents/mcp/compas/plugins")


class CheckerError(RuntimeError):
    """Raised for fatal checker failures."""


def _repo_root(path: str | None) -> pathlib.Path:
    root = pathlib.Path(path or ".").expanduser().resolve()
    if not root.is_dir():
        raise CheckerError(f"repo_root does not exist: {root}")
    return root


def _load_toml_file(path: pathlib.Path) -> dict[str, Any]:
    try:
        return tomllib.loads(path.read_text(encoding="utf-8"))
    except OSError as exc:
        raise CheckerError(f"unable to read toml {path}: {exc}") from exc
    except Exception as exc:
        raise CheckerError(f"unable to parse toml {path}: {exc}") from exc


def _command_basename(command: str) -> str:
    if not command:
        return ""
    base = pathlib.Path(command.split()[0]).name
    return base.lower()


def _line_for_command(path: pathlib.Path, command: str) -> Optional[int]:
    needle = f'command = "{command}"'
    for idx, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        if needle in line:
            return idx
    return None


def _iter_tool_defs(repo_root: pathlib.Path, plugin_path: pathlib.Path) -> Iterable[dict[str, Any]]:
    plugin_toml = _load_toml_file(plugin_path)
    tools = plugin_toml.get("tools")
    if isinstance(tools, list):
        for t in tools:
            if isinstance(t, dict):
                yield {"tool": t, "source": plugin_path}

    import_globs = plugin_toml.get("plugin", {}).get("tool_import_globs", [])
    if isinstance(import_globs, str):
        import_globs = [import_globs]
    for pattern in import_globs or []:
        for tool_path in repo_root.glob(pattern):
            if not tool_path.is_file() or tool_path.suffix.lower() != ".toml":
                continue
            data = _load_toml_file(tool_path)
            tool = data.get("tool")
            if isinstance(tool, dict):
                yield {"tool": tool, "source": tool_path}


def _collect_violations(repo_root: pathlib.Path) -> tuple[list[dict[str, Any]], list[str]]:
    seen = set()
    findings: list[dict[str, Any]] = []
    unsupported: list[str] = []
    plugin_root = repo_root / PLUGIN_DIR
    if not plugin_root.is_dir():
        return findings, unsupported

    for plugin_dir in sorted(p for p in plugin_root.iterdir() if p.is_dir()):
        plugin_file = plugin_dir / "plugin.toml"
        if not plugin_file.is_file():
            continue
        plugin_data = _load_toml_file(plugin_file)
        plugin_policy = plugin_data.get("tool_policy", {}) or {}
        mode = plugin_policy.get("mode", "allowlist")
        if str(mode) != "allowlist":
            continue

        for row in _iter_tool_defs(repo_root, plugin_file):
            tool = row["tool"]
            source = row["source"]
            tool_id = str(tool.get("id", "<unknown>")).strip()
            key = (str(source), tool_id)
            if key in seen:
                continue
            seen.add(key)

            command = str(tool.get("command", "")).strip()
            basename = _command_basename(command)
            if not basename:
                unsupported.append(
                    f"plugin file {plugin_file}: tool {tool_id} has missing command"
                )
                continue
            if basename in FORBIDDEN_COMMANDS:
                findings.append(
                    {
                        "code": "tool_policy.forbidden_shell",
                        "severity": "high",
                        "category": "tool-policy",
                        "message": (
                            f"Forbidden command '{basename}' in tool '{tool_id}' "
                            f"({source.relative_to(repo_root)})"
                        ),
                        "path": str(source.relative_to(repo_root)),
                        "line": _line_for_command(source, command),
                        "evidence_ref": "",
                    }
                )
    return findings, unsupported


def _make_report(
    status: str,
    findings: list[dict[str, Any]],
    errors: list[str],
    commit: str,
) -> dict[str, Any]:
    payload = {
        "status": status,
        "command": "p01_tool_policy_checker",
        "findings": findings,
        "errors": errors,
        "meta": {"plugin": "p01", "commit_sha": commit},
    }
    payload["payload_hash"] = hashlib.sha256(
        json.dumps(payload, sort_keys=True).encode("utf-8")
    ).hexdigest()
    return payload


def _git_sha(repo_root: pathlib.Path) -> str:
    try:
        import subprocess

        r = subprocess.run(
            ["git", "rev-parse", "HEAD"],
            cwd=repo_root,
            capture_output=True,
            text=True,
            check=False,
        )
        if r.returncode == 0:
            return r.stdout.strip()
        return ""
    except Exception:
        return ""


def _main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo-root", default=".")
    args = parser.parse_args(argv)

    try:
        repo = _repo_root(args.repo_root)
        findings, errors = _collect_violations(repo)
        status = "pass" if not findings and not errors else "fail"
        if errors and not findings:
            status = "error"
        report = _make_report(status, findings, errors, _git_sha(repo))
        print(json.dumps(report, ensure_ascii=False, sort_keys=True))

        if status == "pass":
            return 0
        if status == "error":
            return 1
        return 2
    except CheckerError as exc:
        report = _make_report("error", [], [str(exc)], "")
        print(json.dumps(report, ensure_ascii=False, sort_keys=True))
        return 1


if __name__ == "__main__":
    raise SystemExit(_main(sys.argv[1:]))
