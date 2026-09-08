#!/usr/bin/env python3
"""Offline, one-shot Astro domain-layout migration. Runtime has no legacy fallback.

Requires Python 3.11+ and PyYAML only when config.yaml exists. Default is read-only.
--apply requires no processes holding files under the selected home. Every removed
byte is retained in a private tar backup; conflicts are resolved before any writes.
"""
from __future__ import annotations

import argparse
from contextlib import closing
import datetime as dt
import hashlib
import json
import os
from pathlib import Path
import shutil
import sqlite3
import subprocess
import tarfile
import tempfile
import tomllib

FILES = {
    "providers.json": "models/providers.json",
    "active-agent.json": "agents/active.json",
    "app-icon.json": "ui/app-icon.json",
    "onboarding.json": "ui/onboarding.json",
    "skills-enabled.json": "skills/enabled.json",
    "tools-enabled.json": "tools/enabled.json",
    "skill-origins.json": "skills/origins.json",
    "skills-lock.json": "skills/lock.json",
    "dreaming.json": "memory/dreaming.json",
    "data/state.db": "sessions/state.db",
    "data/usage.db": "usage/usage.db",
    "data/subagents-v2.db": "sessions/subagents/subagents-v2.db",
    "data/artifacts.db": "artifacts/artifacts.db",
    "data/knowledge.db": "artifacts/knowledge.db",
    "data/cron_v1.db": "automation/cron/cron_v1.db",
}
PREFIXES = {
    "memory/learning/evolution/": "evolution/",
    "learning/evolution/": "evolution/",
    "memory/audit/": "security/audit/",
    "audit/": "security/audit/",
    "memory/learning/": "evolution/learning/",
    "learning/": "evolution/learning/",
    "pending/": "memory/pending/",
    "cron/": "automation/cron/",
    "workflows/": "automation/workflows/",
    "uploads/": "artifacts/uploads/",
    "data/tool_spills/": "sessions/tool_spills/",
    "skill-backups/": "skills/backups/",
    "evolution-dspy/": "evolution/dspy/",
    "cache/pending-agent-icons/": "agents/pending-icons/",
    "cache/": "models/cache/",
}
RETIRED = {"models.json", "mcp.json", "mcp.json.migrated.bak"}
RETIRED_DIRS = ["data", "cache", "cron", "workflows", "uploads", "learning", "audit", "pending",
                "delegate_async", "evolution-dspy", "skill-backups", ".agents", ".claude",
                "agents/workspace", "memory/audit", "memory/learning"]

# These are mutable resource indexes, not history. Never discover writable fields
# by SQL type or recursively replace JSON strings: prompts, outputs and rollout
# bytes are immutable evidence, even when they mention a retired path.
RESOURCE_COLUMNS = {
    "artifacts/artifacts.db": {"artifacts": "path"},
    "artifacts/knowledge.db": {"contents": "path"},
}
IN_PROGRESS = "backups/layout-in-progress.json"


def encode_toml(document: dict) -> bytes:
    def value(item):
        if isinstance(item, dict):
            return "{ " + ", ".join(json.dumps(k, ensure_ascii=False) + " = " + value(v)
                                     for k, v in item.items() if v is not None) + " }"
        if isinstance(item, list):
            if any(v is None for v in item):
                raise ValueError("null array entries cannot be represented in TOML")
            return "[" + ", ".join(value(v) for v in item) + "]"
        if item is None:
            raise ValueError("null TOML value")
        return json.dumps(item, ensure_ascii=False, allow_nan=False)
    text = "\n".join(json.dumps(k, ensure_ascii=False) + " = " + value(v)
                     for k, v in document.items() if v is not None) + "\n"
    tomllib.loads(text)
    return text.encode()


def merge(a, b):
    if a == b:
        return a
    if isinstance(a, dict) and isinstance(b, dict):
        out = dict(a)
        for key, value in b.items():
            out[key] = merge(out[key], value) if key in out else value
        return out
    raise ValueError("conflicting values; review both copies before migrating")


def database_snapshot(path: Path) -> dict:
    with closing(sqlite3.connect(path.as_uri() + "?mode=ro", uri=True)) as db:
        integrity = db.execute("PRAGMA integrity_check").fetchall()
        if integrity != [("ok",)]:
            raise ValueError(f"database integrity failed: {path.name}")
        tables = db.execute("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'").fetchall()
        return {name: db.execute('SELECT count(*) FROM "' + name.replace('"', '""') + '"').fetchone()[0]
                for (name,) in tables}


class Migration:
    def __init__(self, root: Path):
        self.requested_root = root.absolute()
        self.root = root.resolve(strict=True)
        if self.root == Path.home().resolve() or self.root == Path("/") or (self.root / ".git").exists():
            raise ValueError("refusing a broad filesystem root")
        self.writes: dict[str, tuple[bytes | str, int]] = {}
        self.removals: set[str] = set()
        self.sources: dict[str, tuple[int, int]] = {}
        self.moves: dict[str, str] = {}
        self.databases: dict[str, dict] = {}
        self.conflicts: list[str] = []
        self.materialize: set[str] = set()

    def remember(self, path):
        stat = path.lstat()
        self.sources[str(path.relative_to(self.root))] = (stat.st_mtime_ns, stat.st_size)

    def read(self, path):
        self.remember(path)
        return os.readlink(path) if path.is_symlink() else path.read_bytes()

    def put(self, rel, data, mode=0o600):
        self.writes[rel] = (data, mode)

    def prepare(self):
        if (self.root / IN_PROGRESS).exists() or (self.root / IN_PROGRESS).is_symlink():
            raise RuntimeError("incomplete migration: inspect backups/layout-in-progress.json and restore its backup before retrying")
        paths = sorted(self.root.rglob("*"))
        skills = self.root / "skills"
        if skills.is_dir():
            for entry in skills.iterdir():
                if entry.is_symlink() and os.readlink(entry) == "../.agents/skills/" + entry.name:
                    self.remember(entry)
                    self.materialize.add(entry.relative_to(self.root).as_posix())
        # Skip entire preserved trees, including browser profile symlinks and backups.
        for src in paths:
            rel = src.relative_to(self.root).as_posix()
            if rel.startswith(("backups/", "browser/", "workspace/")):
                continue
            if src.is_dir() and not src.is_symlink():
                continue
            if src.name == ".DS_Store":
                self.remember(src)
                self.removals.add(rel)
                continue
            if rel.endswith(("-wal", "-shm")) and src.with_name(src.name[:-4]).suffix == ".db":
                # Checkpoint only after the full backup; no independent sidecar deletion.
                if rel.startswith("data/") and rel[:-4] not in FILES:
                    raise ValueError(f"unrecognized database sidecar; retained: {rel}")
                continue
            if rel.startswith("delegate_async/") or rel in RETIRED or (
                rel.startswith("agents/") and src.name in {"mcp.json", "mcp.json.migrated.bak", "config.toml"}
            ):
                self.remember(src)
                self.removals.add(rel)
                continue
            target = FILES.get(rel)
            if rel.startswith("prompt-backup."):
                target = "backups/prompts/" + rel
            if rel.startswith("agents/") and src.name == "tool-calls.jsonl":
                target = "security/audit/" + rel
            if rel.startswith("agents/") and src.name == "usage-stats.json":
                target = "usage/" + rel.removesuffix("usage-stats.json") + "stats.json"
            if rel.startswith("agents/workspace/"):
                target = rel.replace("agents/workspace/", "agents/default/", 1)
            if rel.startswith(".claude/"):
                if not src.is_symlink() or os.readlink(src) != "../../.agents/skills/" + src.name:
                    raise ValueError(f"unrecognized external-tool file; retained: {rel}")
                self.remember(src)
                self.removals.add(rel)
                continue
            if rel.startswith(".agents/skills/"):
                folder = rel.split("/")[2]
                if (self.root / "skills" / folder).exists() and "skills/" + folder not in self.materialize:
                    # Preserve the entire current skill, never mix two versions.
                    self.remember(src)
                    self.removals.add(rel)
                    continue
                target = rel.removeprefix(".agents/")
            if target is None:
                for old, new in PREFIXES.items():
                    if rel.startswith(old):
                        target = new + rel[len(old):]
                        break
            if target is None and any(rel.startswith(prefix + "/") for prefix in RETIRED_DIRS):
                raise ValueError(f"unmapped file in retired directory; retained: {rel}")
            if target is None or target == rel:
                continue
            dst = self.root / target
            self.validate_destination(target)
            data = self.read(src)
            if src.suffix == ".db":
                self.databases[target] = database_snapshot(src)
            if dst.is_symlink() and target not in self.materialize:
                raise ValueError(f"destination is a symlink: {target}")
            if dst.exists() or target in self.writes:
                previous = self.writes[target][0] if target in self.writes else self.read(dst)
                if data != previous:
                    if target.endswith(".jsonl"):
                        rows = [json.loads(line) for body in (previous, data)
                                for line in body.decode().splitlines() if line.strip()]
                        unique = {json.dumps(row, sort_keys=True, ensure_ascii=False): row for row in rows}
                        data = ("\n".join(unique) + "\n").encode()
                    elif target == "evolution/learning/skill-usage.json":
                        a, b = json.loads(previous), json.loads(data)
                        loaded = dict(a.get("last_loaded", {}))
                        for key, timestamp in b.get("last_loaded", {}).items():
                            loaded[key] = max(loaded.get(key, ""), timestamp)
                        data = json.dumps({"last_loaded": loaded}, ensure_ascii=False, indent=2).encode()
                    elif target == "memory/dreaming.json":
                        # Directory names/mtimes do not establish which state is authoritative.
                        data = json.dumps(merge(json.loads(data), json.loads(previous)),
                                          ensure_ascii=False, indent=2).encode()
                    elif rel.startswith("agents/workspace/") and target.endswith(".json"):
                        data = json.dumps(merge(json.loads(data), json.loads(previous)),
                                          ensure_ascii=False, indent=2).encode()
                    else:
                        raise ValueError(f"unresolved collision: {rel} -> {target}")
                    self.conflicts.append(target)
            self.put(target, data, src.lstat().st_mode & 0o777)
            self.removals.add(rel)
            self.moves[rel] = target
        yaml_path = self.root / "config.yaml"
        if yaml_path.exists():
            import yaml
            legacy = yaml.safe_load(self.read(yaml_path)) or {}
            canonical = self.root / "config.toml"
            current = tomllib.loads(self.read(canonical).decode()) if canonical.exists() else {}
            self.put("config.toml", encode_toml(merge(legacy, current)))
            self.removals.add("config.yaml")
        # Relocate venv script shebangs/activation files, preserving executable permissions.
        old_venv = str(self.root / "evolution-dspy")
        new_venv = str(self.root / "evolution/dspy")
        for rel, (data, mode) in list(self.writes.items()):
            if rel.startswith("evolution/dspy/") and isinstance(data, bytes) and b"\0" not in data:
                rewritten = data.replace(old_venv.encode(), new_venv.encode())
                rewritten = rewritten.replace(str(self.requested_root / "evolution-dspy").encode(),
                                               str(self.requested_root / "evolution/dspy").encode())
                self.put(rel, rewritten, mode)
        # A relative link moved to a shallower domain must retain its target.
        for source, destination in self.moves.items():
            data, mode = self.writes[destination]
            if not isinstance(data, str):
                continue
            original_target = Path(os.path.normpath(str((self.root / source).parent / data)))
            try:
                relative_target = original_target.relative_to(self.root).as_posix()
            except ValueError:
                relocated_target = original_target
            else:
                relocated_target = self.root / self.moves.get(relative_target, relative_target)
                if any(relative_target.startswith(p + "/") for p in RETIRED_DIRS) and relative_target not in self.moves:
                    raise ValueError(f"symlink target has no migration mapping: {source}")
            self.put(destination, os.path.relpath(relocated_target, (self.root / destination).parent), mode)
        return self

    def validate_destination(self, relative):
        path = self.root / relative
        for ancestor in [path, *path.parents]:
            if ancestor == self.root:
                break
            name = ancestor.relative_to(self.root).as_posix()
            if ancestor.is_symlink() and name not in self.materialize:
                raise ValueError(f"destination crosses a symlink: {relative}")

    def relocated_resource_path(self, value):
        """Resolve only an exact, actually moved file in a whitelisted path column."""
        if not isinstance(value, str):
            return value
        for base in {self.root, self.requested_root}:
            prefix = str(base) + "/"
            if value.startswith(prefix):
                target = self.moves.get(value[len(prefix):])
                if target is not None:
                    return str(base / target)
        return value

    def summary(self):
        return {"root": str(self.root), "writes": len(self.writes), "retired_files": len(self.removals),
                "merged": sorted(set(self.conflicts)), "databases": self.databases,
                "materialized_skills": sorted(self.materialize),
                "resource_columns": RESOURCE_COLUMNS, "moves": self.moves}

    def apply(self):
        # Refuse live migration, including orphan browser/profile writers.
        if not shutil.which("lsof"):
            raise RuntimeError("lsof is required to verify offline migration")
        opened = subprocess.run(["lsof", "-nP", "+D", str(self.root)], capture_output=True, text=True)
        if opened.returncode == 0:
            raise RuntimeError("close all processes holding files under the Astro home before --apply")
        if opened.returncode != 1 or opened.stderr.strip():
            raise RuntimeError("could not verify that the Astro home is offline")
        for rel, expected in self.sources.items():
            stat = (self.root / rel).lstat()
            if (stat.st_mtime_ns, stat.st_size) != expected:
                raise RuntimeError(f"source changed during planning: {rel}")
        timestamp = dt.datetime.now().strftime("%Y%m%d-%H%M%S")
        backup = self.root / "backups" / ("layout-v2-" + timestamp)
        self.validate_destination(backup.relative_to(self.root).as_posix())
        backup.mkdir(parents=True, mode=0o700)
        archive = backup / "before.tar.gz"
        with tarfile.open(archive, "w:gz", dereference=False) as tar:
            for child in sorted(self.root.iterdir()):
                if child.name != "backups":
                    tar.add(child, arcname=child.name)
        archive.chmod(0o600)
        # Verify that the complete archive is readable before modifying any active files.
        with tarfile.open(archive, "r:gz") as tar:
            for member in tar:
                if member.isfile():
                    file = tar.extractfile(member)
                    while file.read(1024 * 1024):
                        pass
        (backup / "plan.json").write_text(json.dumps(self.summary(), ensure_ascii=False, indent=2))
        marker = self.root / IN_PROGRESS
        self.validate_destination(IN_PROGRESS)
        marker.write_text(json.dumps({"backup": str(backup), "status": "in_progress"}))
        marker.chmod(0o600)
        # A checkpoint folds WAL into the database; copying just .db before this would lose commits.
        for source, target in self.moves.items():
            if target in self.databases:
                path = self.root / source
                with closing(sqlite3.connect(path)) as db:
                    busy, _, _ = db.execute("PRAGMA wal_checkpoint(TRUNCATE)").fetchone()
                    if busy:
                        raise RuntimeError(f"database remains busy: {source}")
                self.put(target, path.read_bytes(), path.stat().st_mode & 0o777)
        for relative in self.materialize:
            (self.root / relative).unlink()
        for relative, (data, mode) in self.writes.items():
            self.validate_destination(relative)
            destination = self.root / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            if isinstance(data, str):
                if destination.is_symlink():
                    destination.unlink()
                os.symlink(data, destination)
            else:
                fd, name = tempfile.mkstemp(prefix=".layout-", dir=destination.parent)
                try:
                    with os.fdopen(fd, "wb") as file:
                        file.write(data)
                        file.flush()
                        os.fsync(file.fileno())
                    os.chmod(name, mode)
                    os.replace(name, destination)
                finally:
                    if os.path.exists(name):
                        os.unlink(name)
        for target, counts in self.databases.items():
            if database_snapshot(self.root / target) != counts:
                raise RuntimeError(f"database row counts changed: {target}; backup: {backup}")
        # Only resource-index path columns change. All response items, rollout
        # physical lines, mailbox payloads, audit contents and user text stay intact.
        for relative, resource_tables in RESOURCE_COLUMNS.items():
            if relative not in self.databases:
                continue
            with closing(sqlite3.connect(self.root / relative)) as db:
                for table, column in resource_tables.items():
                    if table not in self.databases[relative]:
                        continue
                    rows = db.execute(f'SELECT id,"{column}" FROM "{table}"').fetchall()
                    for resource_id, value in rows:
                        changed = self.relocated_resource_path(value)
                        if changed != value:
                            db.execute(f'UPDATE "{table}" SET "{column}"=? WHERE id=?', (changed, resource_id))
                db.commit()
                db.execute("PRAGMA wal_checkpoint(TRUNCATE)")
            after = database_snapshot(self.root / relative)
            for table, count in self.databases[relative].items():
                if "fts" not in table.lower() and after.get(table) != count:
                    raise RuntimeError(f"logical row count changed in {relative}: {table}")
        # Remove exact source files only after all destinations and databases are verified.
        for relative in sorted(self.removals):
            source = self.root / relative
            source.unlink()
            if source.suffix == ".db":
                for suffix in ("-wal", "-shm"):
                    sidecar = source.with_name(source.name + suffix)
                    if sidecar.exists():
                        sidecar.unlink()
        # Only empty retired directories are removed, bottom-up; never recursive delete a home.
        roots = list(RETIRED_DIRS)
        roots.extend(p.name for p in self.root.glob("prompt-backup.*") if p.is_dir())
        for relative in roots:
            directory = self.root / relative
            if directory.is_dir() and not directory.is_symlink():
                for child in sorted(directory.rglob("*"), key=lambda p: len(p.parts), reverse=True):
                    if child.is_dir() and not child.is_symlink():
                        try:
                            child.rmdir()
                        except OSError:
                            pass
                try:
                    directory.rmdir()
                except OSError:
                    raise RuntimeError(f"unmapped files remain in {relative}; retained, not deleted")
        (backup / "complete.json").write_text(json.dumps({"completed": timestamp, "archive_sha256": hashlib.sha256(archive.read_bytes()).hexdigest()}))
        marker.unlink()
        return backup


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--apply", action="store_true")
    args = parser.parse_args()
    migration = Migration(args.root).prepare()
    summary = migration.summary()
    summary.pop("moves")
    print(json.dumps(summary, ensure_ascii=False, indent=2))
    if args.apply:
        print("Backup:", migration.apply())


if __name__ == "__main__":
    main()
