#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import sys

SKIP_DIRS = {".git", ".idea", ".pytest_cache", ".ruff_cache", "node_modules", "target"}

MANIFEST_BASENAMES = {
    "Cargo.toml",
    "Cargo.lock",
    "package.json",
    "package-lock.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "npm-shrinkwrap.json",
    "requirements.txt",
    "requirements-dev.txt",
    "pyproject.toml",
    "poetry.lock",
    "Pipfile",
    "Pipfile.lock",
    "go.mod",
    "go.sum",
    "gomod.lock",
    "gradle.properties",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "go.work",
    "go.work.sum",
    "Gemfile",
    "Gemfile.lock",
    "composer.json",
    "composer.lock",
    "mix.exs",
    "mix.lock",
    "packages.config",
    "project.assets.json",
}


def should_skip(path: Path) -> bool:
    for part in path.parents:
        if part.name in SKIP_DIRS:
            return True
    return False


def file_sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as fh:
        for chunk in iter(lambda: fh.read(8192), b""):
            h.update(chunk)
    return h.hexdigest()


def collect_manifests(repo_root: Path) -> list[tuple[str, str]]:
    records: list[tuple[str, str]] = []

    for name in sorted(MANIFEST_BASENAMES):
        for path in repo_root.rglob(name):
            if path.is_file() and not should_skip(path):
                rel = path.relative_to(repo_root).as_posix()
                digest = file_sha256(path)
                records.append((rel, digest))

    records.sort(key=lambda item: item[0])
    return records


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run lightweight repository SBOM check")
    parser.add_argument("--repo-root", default=".", dest="repo_root")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    root = Path(args.repo_root).resolve()

    if not root.is_dir():
        print(f"SBOM_ERROR: repo root not found: {args.repo_root}", file=sys.stderr)
        return 2

    manifests = collect_manifests(root)
    report = {
        "plugin": "p10",
        "status": "pass",
        "manifests": [
            {"path": path, "sha256": digest}
            for path, digest in manifests
        ],
    }

    out_path = root / "artifacts" / "sbom" / "report.json"
    try:
        out_path.parent.mkdir(parents=True, exist_ok=True)
        out_path.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    except OSError as exc:
        print(f"SBOM_ERROR: cannot write report: {exc}", file=sys.stderr)
        return 3

    print(f"SBOM_OK manifests={len(manifests)} report={out_path.relative_to(root)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
