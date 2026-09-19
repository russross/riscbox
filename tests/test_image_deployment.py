#!/usr/bin/env -S uv run -q --script
# /// script
# requires-python = ">=3.13"
# dependencies = [
# ]
# ///

"""Behavioral tests for content-addressed image deployment helpers."""

from __future__ import annotations

import importlib.util
from pathlib import Path
import tempfile
import types
import unittest

SCRIPT = Path(__file__).parents[1] / "tools" / "image_deployment.py"


def load_helper() -> types.ModuleType:
    """Load the standalone script as a module."""
    spec = importlib.util.spec_from_file_location("image_deployment", SCRIPT)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {SCRIPT}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


helper = load_helper()


class ImageDeploymentTests(unittest.TestCase):
    def test_rewrite_changes_only_boot_and_primary_drive_assets(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "source.cfg"
            output = root / "output.cfg"
            source.write_text(
                '{bios:"fw_jump.bin",kernel:"linux",'
                'fs0:{file:"fs/head"},drive0:{file:"drive/blk.txt"}}'
            )
            helper.rewrite_config(
                source, output, "fw_jump.bin-11111111", "linux-22222222", "drive-33333333"
            )
            self.assertEqual(
                output.read_text(),
                '{bios:"fw_jump.bin-11111111",kernel:"linux-22222222",'
                'fs0:{file:"fs/head"},drive0:{file:"drive-33333333/blk.txt"}}',
            )

    def test_clean_keeps_current_generation_and_unrelated_files(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            deployment = Path(temporary)
            (deployment / "riscbox.cfg").write_text(
                '{bios:"fw_jump.bin-11111111",kernel:"linux-22222222",'
                'drive0:{file:"drive-33333333/blk.txt"}}'
            )
            for name in ["fw_jump.bin-11111111", "fw_jump.bin-aaaaaaaa", "linux-22222222", "linux-bbbbbbbb", "index.html"]:
                (deployment / name).write_text(name)
            (deployment / "drive-33333333").mkdir()
            (deployment / "drive-cccccccc").mkdir()

            removed = {path.name for path in helper.clean_deployment(deployment)}

            self.assertEqual(
                removed,
                {"fw_jump.bin-aaaaaaaa", "linux-bbbbbbbb", "drive-cccccccc"},
            )
            self.assertTrue((deployment / "drive-33333333").is_dir())
            self.assertTrue((deployment / "index.html").is_file())


if __name__ == "__main__":
    unittest.main()
