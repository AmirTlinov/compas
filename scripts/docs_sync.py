#!/usr/bin/env python3
from __future__ import annotations

import argparse
import glob
import hashlib
import json
from pathlib import Path
import re
import sys
import tomllib

BEGIN = "<!-- COMPAS_AUTO_ARCH:BEGIN -->"
END = "<!-- COMPAS_AUTO_ARCH:END -->"


def sha256_of(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()[:12]


def discover_mcp_tools(server_rs: Path) -> list[str]:
    text = server_rs.read_text(encoding="utf-8")
    blocks = re.findall(r"#\[tool\((.*?)\)\]", text, flags=re.S)
    tools: list[str] = []
    for block in blocks:
        m = re.search(r'name\s*=\s*"([^"]+)"', block)
        if m:
            tools.append(m.group(1))
    return sorted(dict.fromkeys(tools))


def parse_tool_manifest(path: Path) -> dict:
    data = tomllib.loads(path.read_text(encoding="utf-8"))
    tool = data.get("tool", {})
    return {
        "id": str(tool.get("id", "")).strip(),
        "description": str(tool.get("description", "")).strip(),
        "command": str(tool.get("command", "")).strip(),
        "args": tool.get("args", []),
    }


def discover_plugins(repo_root: Path) -> tuple[list[dict], dict[str, dict]]:
    plugins_root = repo_root / ".agents" / "mcp" / "compas" / "plugins"
    plugins: list[dict] = []
    tools: dict[str, dict] = {}

    for plugin_dir in sorted([p for p in plugins_root.iterdir() if p.is_dir()]):
        plugin_toml = plugin_dir / "plugin.toml"
        if not plugin_toml.is_file():
            continue
        data = tomllib.loads(plugin_toml.read_text(encoding="utf-8"))
        meta = data.get("plugin", {})
        gate = data.get("gate", {})
        plugin_id = str(meta.get("id", "")).strip()
        plugin_description = str(meta.get("description", "")).strip()
        tool_import_globs = meta.get("tool_import_globs", []) or []

        plugin_tools: list[str] = []

        for inline_tool in data.get("tools", []) or []:
            tool_id = str(inline_tool.get("id", "")).strip()
            if not tool_id:
                continue
            plugin_tools.append(tool_id)
            tools[tool_id] = {
                "plugin_id": plugin_id,
                "description": str(inline_tool.get("description", "")).strip(),
                "command": str(inline_tool.get("command", "")).strip(),
            }

        for pattern in tool_import_globs:
            abs_pattern = str((repo_root / pattern).resolve())
            for manifest in sorted(glob.glob(abs_pattern, recursive=True)):
                manifest_path = Path(manifest)
                parsed = parse_tool_manifest(manifest_path)
                if not parsed["id"]:
                    continue
                plugin_tools.append(parsed["id"])
                tools[parsed["id"]] = {
                    "plugin_id": plugin_id,
                    "description": parsed["description"],
                    "command": parsed["command"],
                }

        plugins.append(
            {
                "id": plugin_id,
                "description": plugin_description,
                "tools": sorted(dict.fromkeys(plugin_tools)),
                "gate_ci_fast": list(gate.get("ci_fast", []) or []),
                "gate_ci": list(gate.get("ci", []) or []),
                "gate_flagship": list(gate.get("flagship", []) or []),
                "plugin_toml": plugin_toml,
            }
        )

    plugins.sort(key=lambda p: p["id"])
    return plugins, tools


def generate_mermaid(plugins: list[dict], tools: dict[str, dict]) -> str:
    lines = [
        "flowchart LR",
        "  Agent[AI Agent] --> DX[./dx]",
        "  DX --> MCP[compas MCP]",
        "  MCP --> V[validate]",
        "  MCP --> G[gate]",
        "  MCP --> PL[plugins.*]",
        "  MCP --> TL[tools.*]",
        "  V --> C1[loc/env/boundary/public-surface]",
    ]

    for plugin in plugins:
        p_node = f"P_{plugin['id'].replace('-', '_')}"
        lines.append(f'  PL --> {p_node}["plugin:{plugin["id"]}"]')
        for tool_id in plugin["tools"]:
            t_node = f"T_{tool_id.replace('-', '_').replace('.', '_')}"
            lines.append(f'  {p_node} --> {t_node}["tool:{tool_id}"]')
            lines.append(f"  G --> {t_node}")

    if not plugins:
        lines.append("  PL --> PNONE[no plugins]")
    if not tools:
        lines.append("  TL --> TNONE[no tools]")
    else:
        for tool_id in sorted(tools.keys()):
            t_node = f"T_{tool_id.replace('-', '_').replace('.', '_')}"
            lines.append(f"  TL --> {t_node}")

    return "\n".join(lines)


def render_arch_block(repo_root: Path) -> str:
    server_rs = repo_root / "crates" / "ai-dx-mcp" / "src" / "server.rs"
    tools_surface = discover_mcp_tools(server_rs)
    plugins, tools = discover_plugins(repo_root)
    fingerprint_plugins = [
        {k: v for (k, v) in p.items() if k != "plugin_toml"} for p in plugins
    ]
    fingerprint = hashlib.sha256(
        json.dumps(
            {
                "mcp_tools": tools_surface,
                "plugins": fingerprint_plugins,
                "tools": tools,
            },
            ensure_ascii=False,
            sort_keys=True,
        ).encode("utf-8")
    ).hexdigest()[:16]

    plugin_rows = []
    for p in plugins:
        plugin_rows.append(
            f"| `{p['id']}` | {p['description']} | "
            f"{', '.join(f'`{t}`' for t in p['tools']) or '—'} | "
            f"{', '.join(f'`{x}`' for x in p['gate_ci_fast']) or '—'} / "
            f"{', '.join(f'`{x}`' for x in p['gate_ci']) or '—'} / "
            f"{', '.join(f'`{x}`' for x in p['gate_flagship']) or '—'} |"
        )

    tool_rows = []
    for tool_id in sorted(tools.keys()):
        t = tools[tool_id]
        tool_rows.append(
            f"| `{tool_id}` | `{t['plugin_id']}` | {t['description']} | `{t['command']}` |"
        )

    mermaid = generate_mermaid(plugins, tools)
    plugin_table_rows = plugin_rows if plugin_rows else ["| — | — | — | — |"]
    tool_table_rows = tool_rows if tool_rows else ["| — | — | — | — |"]
    core_paths = [
        ("MCP server", "crates/ai-dx-mcp/src/server.rs"),
        ("Runtime pipeline", "crates/ai-dx-mcp/src/{app,repo,runner}.rs"),
        ("Plugin configs", ".agents/mcp/compas/plugins/*/plugin.toml"),
        ("Tool manifests", "tools/custom/**/tool.toml"),
        ("Docs sync script", "scripts/docs_sync.py"),
        ("DX wrapper", "dx"),
    ]
    path_rows = "\n".join(
        f"| {name} | `{path}` |" for (name, path) in core_paths
    )

    return "\n".join(
        [
            BEGIN,
            f"_fingerprint: {fingerprint}_",
            "",
            "## Runtime Map (auto)",
            "",
            "### Core paths",
            "| Segment | Path |",
            "|---|---|",
            path_rows,
            "",
            "### Installed plugins",
            "| Plugin | Purpose | Tools | Gates (ci-fast / ci / flagship) |",
            "|---|---|---|---|",
            *plugin_table_rows,
            "",
            "### Installed tools",
            "| Tool | Owner plugin | Purpose | Command |",
            "|---|---|---|---|",
            *tool_table_rows,
            "",
            "### MCP surface",
            ", ".join(f"`{name}`" for name in tools_surface) if tools_surface else "—",
            "",
            "### Mermaid",
            "```mermaid",
            mermaid,
            "```",
            END,
            "",
        ]
    )


def replace_managed_block(path: Path, block: str) -> bool:
    text = path.read_text(encoding="utf-8") if path.exists() else ""
    if BEGIN in text and END in text:
        start = text.index(BEGIN)
        end = text.index(END) + len(END)
        new_text = text[:start] + block.strip() + text[end:]
    else:
        prefix = text.rstrip()
        if prefix:
            prefix += "\n\n"
        new_text = prefix + block

    if text == new_text:
        return False
    path.write_text(new_text, encoding="utf-8")
    return True


def ensure_file(path: Path, heading: str, intro_lines: list[str]) -> None:
    if path.exists():
        return
    payload = [f"# {heading}", "", *intro_lines, ""]
    path.write_text("\n".join(payload), encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser(description="Sync auto-managed architecture docs.")
    parser.add_argument("--check", action="store_true", help="Fail if files are out of date.")
    args = parser.parse_args()

    repo_root = Path(__file__).resolve().parent.parent
    architecture = repo_root / "ARCHITECTURE.md"
    agents = repo_root / "AGENTS.md"

    ensure_file(
        architecture,
        "ARCHITECTURE",
        [
            "SSOT архитектуры проекта compas.",
            "Ниже только auto-managed блок; обновление через `./dx docs-sync`.",
        ],
    )
    ensure_file(
        agents,
        "AGENTS",
        [
            "## Rules (max 10 lines)",
            "1) Всегда сначала `cargo test -p ai-dx-mcp`.",
            "2) Потом `cargo run -p ai-dx-mcp -- validate ratchet`.",
            "3) Финально `./dx ci-fast --dry-run`.",
            "4) Любая новая ручка/инструмент — через plugin/tool manifest.",
            "5) Манивесты только strict schema, unknown fields запрещены.",
            "6) Fail-closed: при ошибке конфига не продолжать.",
            "7) Не держать ручные дубликаты архитектуры.",
            "8) Архитектуру и диаграммы обновлять только `./dx docs-sync`.",
            "9) Изменения держать малыми, до зелёного Verify.",
            "10) Истина в коде + автосинке, а не в произвольных заметках.",
            "",
            "## Architecture quick map",
            "Ниже — auto-managed блок из `scripts/docs_sync.py`.",
        ],
    )

    block = render_arch_block(repo_root)
    changed_arch = replace_managed_block(architecture, block)
    changed_agents = replace_managed_block(agents, block)

    summary = {
        "architecture_changed": changed_arch,
        "agents_changed": changed_agents,
        "architecture_sha": sha256_of(architecture),
        "agents_sha": sha256_of(agents),
    }
    print(json.dumps(summary, ensure_ascii=False, indent=2))

    if args.check and (changed_arch or changed_agents):
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
