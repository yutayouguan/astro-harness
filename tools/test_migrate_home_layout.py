import json
import os
from pathlib import Path
import sqlite3
import tarfile
import tempfile
import unittest
from unittest.mock import patch

from migrate_home_layout import Migration


class LayoutMigrationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="astro-layout-fixture-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def write(self, rel, content):
        path = self.root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content)
        return path

    def apply(self, migration):
        with patch("migrate_home_layout.subprocess.run") as opened:
            opened.return_value.returncode = 1
            opened.return_value.stderr = ""
            return migration.apply()

    def test_database_references_backup_and_idempotence(self):
        db_path = self.root / "data/state.db"
        db_path.parent.mkdir()
        old_path = str(self.root / "data/tool_spills/t/one.txt")
        with sqlite3.connect(db_path) as db:
            db.execute("create table items (id integer primary key, content text)")
            db.execute("insert into items values (1, ?)", (old_path,))
        db.close()
        self.write("data/tool_spills/t/one.txt", "retained tool output")
        self.write("sessions/rollouts/a.jsonl", json.dumps({"path": old_path}) + "\n")
        self.write(".env", "KEEP_SECRET=not-logged\n")
        migration = Migration(self.root).prepare()
        self.assertTrue(db_path.exists(), "planning must be read-only")
        backup = self.apply(migration)
        self.assertFalse((self.root / "data").exists())
        with sqlite3.connect(self.root / "sessions/state.db") as db:
            content = db.execute("select content from items").fetchone()[0]
        db.close()
        # History is evidence, not a mutable path index.
        self.assertEqual(content, old_path)
        self.assertEqual(json.loads((self.root / "sessions/rollouts/a.jsonl").read_text())["path"], content)
        self.assertEqual((self.root / "sessions/tool_spills/t/one.txt").read_text(), "retained tool output")
        with tarfile.open(backup / "before.tar.gz") as archive:
            self.assertEqual(archive.extractfile(".env").read(), b"KEEP_SECRET=not-logged\n")
            self.assertIn(old_path, archive.extractfile("sessions/rollouts/a.jsonl").read().decode())
        self.assertEqual((backup / "before.tar.gz").stat().st_mode & 0o777, 0o600)
        second = Migration(self.root).prepare()
        self.assertEqual(second.writes, {})
        self.assertEqual(second.removals, set())

    def test_only_resource_index_columns_change_not_history_or_config_strings(self):
        old_path = str(self.root / "uploads/t/photo.png")
        narrative = "historical command: cat " + old_path
        row = {"type": "message", "content": narrative, "path": old_path,
               "arguments": {"path": old_path}, "output": "Spill: data/tool_spills/t/one.txt"}
        rollout = json.dumps(row, indent=None) + "\n\n" + json.dumps(row) + "\n"
        self.write("sessions/rollouts/a.jsonl", rollout)
        self.write("agents/default/config.json", json.dumps({"custom_prompt": narrative}))
        self.write("uploads/t/photo.png", "image bytes")
        for filename, table in [("artifacts.db", "artifacts"), ("knowledge.db", "contents")]:
            db_path = self.root / "data" / filename
            db_path.parent.mkdir(exist_ok=True)
            with sqlite3.connect(db_path) as db:
                db.execute(f"create table {table} (id text primary key, path text unique, title text)")
                db.execute(f"insert into {table} values ('1', ?, ?)", (old_path, narrative))
                db.execute("create table unrecognized (id text, path text)")
                db.execute("insert into unrecognized values ('1', ?)", (old_path,))
            db.close()
        self.apply(Migration(self.root).prepare())
        self.assertEqual((self.root / "sessions/rollouts/a.jsonl").read_text(), rollout)
        self.assertEqual(json.loads((self.root / "agents/default/config.json").read_text())["custom_prompt"], narrative)
        for filename, table in [("artifacts.db", "artifacts"), ("knowledge.db", "contents")]:
            with sqlite3.connect(self.root / "artifacts" / filename) as db:
                self.assertEqual(db.execute(f"select path,title from {table}").fetchone(),
                                 (str(self.root / "artifacts/uploads/t/photo.png"), narrative))
                self.assertEqual(db.execute("select path from unrecognized").fetchone()[0], old_path)
            db.close()

    def test_incomplete_migration_and_unknown_external_tool_data_fail_closed(self):
        self.write("backups/layout-in-progress.json", "{}")
        with self.assertRaisesRegex(RuntimeError, "incomplete migration"):
            Migration(self.root).prepare()
        (self.root / "backups/layout-in-progress.json").unlink()
        self.write(".claude/settings.json", '{"user_data":true}')
        with self.assertRaisesRegex(ValueError, "unrecognized external-tool file"):
            Migration(self.root).prepare()

    def test_conflicting_dreaming_state_requires_a_choice(self):
        self.write("dreaming.json", '{"enabled":true}')
        self.write("memory/dreaming.json", '{"enabled":false}')
        with self.assertRaisesRegex(ValueError, "conflicting"):
            Migration(self.root).prepare()

    def test_config_merge_keeps_domains_and_refuses_ambiguous_conflicts(self):
        self.write("config.yaml", "memory:\n  write_approval: true\n")
        self.write("config.toml", '[mcp_servers.demo]\ncommand = "server"\n')
        planned = Migration(self.root).prepare()
        import tomllib
        config = tomllib.loads(planned.writes["config.toml"][0].decode())
        self.assertTrue(config["memory"]["write_approval"])
        self.assertEqual(config["mcp_servers"]["demo"]["command"], "server")
        self.write("config.toml", "[memory]\nwrite_approval = false\n")
        with self.assertRaisesRegex(ValueError, "conflicting"):
            Migration(self.root).prepare()
        self.assertFalse((self.root / "backups").exists())

    def test_shared_skill_links_are_materialized_before_sources_are_removed(self):
        self.write(".agents/skills/demo/SKILL.md", "skill body")
        (self.root / "skills").mkdir()
        (self.root / "skills/demo").symlink_to("../.agents/skills/demo")
        (self.root / ".claude/skills").mkdir(parents=True)
        (self.root / ".claude/skills/demo").symlink_to("../../.agents/skills/demo")
        self.apply(Migration(self.root).prepare())
        self.assertFalse((self.root / "skills/demo").is_symlink())
        self.assertEqual((self.root / "skills/demo/SKILL.md").read_text(), "skill body")
        self.assertFalse((self.root / ".agents").exists())
        self.assertFalse((self.root / ".claude").exists())

    def test_audit_union_and_latest_skill_usage(self):
        self.write("audit/sandbox.jsonl", '{"id":1}\n')
        self.write("memory/audit/sandbox.jsonl", '{"id":1}\n{"id":2}\n')
        self.write("learning/skill-usage.json", '{"last_loaded":{"a":"2026-09-01"}}')
        self.write("memory/learning/skill-usage.json", '{"last_loaded":{"a":"2026-08-01","b":"2026-08-03"}}')
        planned = Migration(self.root).prepare()
        audit = planned.writes["security/audit/sandbox.jsonl"][0].decode().splitlines()
        self.assertEqual(len(audit), 2)
        usage = json.loads(planned.writes["evolution/learning/skill-usage.json"][0])
        self.assertEqual(usage["last_loaded"], {"a": "2026-09-01", "b": "2026-08-03"})

    def test_relocated_venv_scripts_preserve_mode_and_interpreter(self):
        script = self.write("evolution-dspy/.venv/bin/pip", "#!" + str(self.root / "evolution-dspy/.venv/bin/python") + "\n")
        script.chmod(0o755)
        planned = Migration(self.root).prepare()
        body, mode = planned.writes["evolution/dspy/.venv/bin/pip"]
        self.assertIn(b"/evolution/dspy/.venv/bin/python", body)
        self.assertEqual(mode, 0o755)

    def test_relocated_links_keep_external_targets_without_copying_them(self):
        target = self.write("external/python", "interpreter placeholder")
        original = self.root / "evolution-dspy/.venv/bin/python"
        original.parent.mkdir(parents=True)
        original.symlink_to("../../../external/python")
        self.apply(Migration(self.root).prepare())
        moved = self.root / "evolution/dspy/.venv/bin/python"
        self.assertTrue(moved.is_symlink())
        self.assertEqual(moved.resolve(), target.resolve())

    def test_source_change_and_live_process_abort_without_backup(self):
        path = self.write("providers.json", "{}")
        planned = Migration(self.root).prepare()
        path.write_text('{"changed":true}')
        with self.assertRaisesRegex(RuntimeError, "source changed"):
            self.apply(planned)
        with patch("migrate_home_layout.subprocess.run") as opened:
            opened.return_value.returncode = 0
            with self.assertRaisesRegex(RuntimeError, "close all processes"):
                planned.apply()
        self.assertFalse((self.root / "backups").exists())

    def test_external_destination_symlinks_are_rejected(self):
        self.write("providers.json", "{}")
        with tempfile.TemporaryDirectory() as outside:
            (self.root / "models").symlink_to(outside)
            with self.assertRaisesRegex(ValueError, "symlink"):
                Migration(self.root).prepare()

    def test_unknown_database_prevents_any_migration_before_backup(self):
        self.write("providers.json", "{}")
        self.write("data/unknown.db", "must not be discarded")
        with self.assertRaisesRegex(ValueError, "unmapped file"):
            Migration(self.root).prepare()
        self.assertFalse((self.root / "backups").exists())

    def test_repository_root_is_not_an_astro_home(self):
        (self.root / ".git").mkdir()
        with self.assertRaisesRegex(ValueError, "broad filesystem root"):
            Migration(self.root)


if __name__ == "__main__":
    unittest.main()
