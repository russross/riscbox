#!/usr/bin/env -S uv run -q --script
# /// script
# requires-python = ">=3.13"
# dependencies = [
# ]
# ///

"""Check the Risclet boot disk layout and missing-input errors."""

from __future__ import annotations

from importlib.machinery import SourceFileLoader
from importlib.util import module_from_spec, spec_from_loader
from pathlib import Path
import struct
import subprocess
import tempfile
import types
import unittest

SCRIPT = Path(__file__).parents[1] / "images/risclet/assemble-disk"


def load_helper() -> types.ModuleType:
    """Load the executable image builder for focused tests."""
    loader = SourceFileLoader("risclet_disk", str(SCRIPT))
    spec = spec_from_loader(loader.name, loader)
    if spec is None:
        raise RuntimeError(f"cannot load {SCRIPT}")
    module = module_from_spec(spec)
    loader.exec_module(module)
    return module


helper = load_helper()


class RiscletDiskTests(unittest.TestCase):
    def test_boot_partition_contains_kernel_and_selects_erofs_root(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            kernel = directory / "Image"
            root = directory / "root.erofs"
            disk = directory / "disk.img"
            kernel.write_bytes(b"guest kernel")
            root.write_bytes(b"erofs root bytes")

            helper.assemble(kernel, root, disk)
            data = disk.read_bytes()
            self.assertEqual(data[510:512], b"\x55\xaa")
            boot_start, boot_sectors = struct.unpack_from("<II", data, 454)
            root_start, root_sectors = struct.unpack_from("<II", data, 470)
            self.assertEqual(boot_start, 2048)
            self.assertEqual(boot_sectors, 32768)
            self.assertEqual(root_start, boot_start + boot_sectors)
            self.assertEqual(root_sectors, 1)
            self.assertEqual(data[root_start * 512 : root_start * 512 + root.stat().st_size], root.read_bytes())

            boot_image = directory / "boot.ext4"
            boot_image.write_bytes(data[boot_start * 512 : root_start * 512])
            config = subprocess.run(
                ["/usr/sbin/debugfs", "-R", "cat /extlinux/extlinux.conf", str(boot_image)],
                check=True,
                capture_output=True,
                text=True,
            ).stdout
            self.assertIn("linux /Image", config)
            self.assertIn("root=/dev/vda2 ro rootfstype=erofs", config)
            stored_kernel = subprocess.run(
                ["/usr/sbin/debugfs", "-R", "cat /Image", str(boot_image)],
                check=True,
                capture_output=True,
            ).stdout
            self.assertEqual(stored_kernel, kernel.read_bytes())

    def test_missing_root_leaves_no_partial_disk(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            kernel = directory / "Image"
            kernel.write_bytes(b"guest kernel")
            disk = directory / "disk.img"
            with self.assertRaisesRegex(ValueError, "existing files"):
                helper.assemble(kernel, directory / "missing.erofs", disk)
            self.assertFalse(disk.exists())


if __name__ == "__main__":
    unittest.main()
