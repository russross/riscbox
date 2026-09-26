#!/usr/bin/env -S uv run -q --script
# /// script
# requires-python = ">=3.13"
# dependencies = [
# ]
# ///

"""Behavioral tests for the standalone splitimg tool."""

from __future__ import annotations

import importlib.util
from pathlib import Path
import tempfile
import types
import unittest

SCRIPT = Path(__file__).parents[1] / "tools" / "splitimg.py"


def load_splitimg() -> types.ModuleType:
    """Load the standalone script as a module without changing its packaging."""
    spec = importlib.util.spec_from_file_location("splitimg", SCRIPT)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {SCRIPT}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


splitimg = load_splitimg()


class SplitImageTests(unittest.TestCase):
    def split(self, data: bytes, block_kib: int = 1) -> tuple[Path, tempfile.TemporaryDirectory[str]]:
        temporary = tempfile.TemporaryDirectory()
        root = Path(temporary.name)
        source = root / "disk.img"
        output_parent = root / "blocks"
        source.write_bytes(data)
        output_parent.mkdir()
        output, _ = splitimg.split_image(source, output_parent, block_kib)
        return output, temporary

    def test_directory_name_uses_image_and_block_size_hash(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "disk.img"
            source.write_bytes(b"abc")
            first, _ = splitimg.split_image(source, root, 1)
            second, _ = splitimg.split_image(source, root, 1024)
            self.assertEqual(first.name, "drive-8c68ad0a")
            self.assertNotEqual(first, second)
            self.assertIn("block_size: 1", (first / "blk.txt").read_text())
            self.assertIn("block_size: 1024", (second / "blk.txt").read_text())

    def test_default_block_size_is_512_kib(self) -> None:
        self.assertEqual(splitimg.DEFAULT_BLOCK_KIB, 512)
        options = splitimg.parse_args(["disk.img", "blocks"])
        self.assertEqual(options.block_size_kib, 512)

    def test_empty_image(self) -> None:
        output, temporary = self.split(b"")
        with temporary:
            self.assertEqual((output / "blk.txt").read_text(), "{\n  block_size: 1,\n  n_block: 0,\n}\n")
            self.assertEqual(list(output.glob("*.bin")), [])

    def test_exact_blocks(self) -> None:
        data = bytes(range(256)) * 8
        output, temporary = self.split(data)
        with temporary:
            self.assertEqual((output / "blk000000000.bin").read_bytes(), data[:1024])
            self.assertEqual((output / "blk000000001.bin").read_bytes(), data[1024:])

    def test_partial_block_is_zero_padded(self) -> None:
        output, temporary = self.split(b"abc")
        with temporary:
            block = (output / "blk000000000.bin").read_bytes()
            self.assertEqual(block[:3], b"abc")
            self.assertEqual(block[3:], bytes(1021))

    def test_custom_block_size_is_recorded(self) -> None:
        output, temporary = self.split(b"x", 2)
        with temporary:
            self.assertEqual((output / "blk000000000.bin").stat().st_size, 2048)
            self.assertIn("block_size: 2", (output / "blk.txt").read_text())

    def test_rejects_missing_paths(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            with self.assertRaisesRegex(ValueError, "input is not a file"):
                splitimg.split_image(root / "missing", root, 1)


if __name__ == "__main__":
    unittest.main()
