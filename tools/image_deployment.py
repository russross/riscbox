#!/usr/bin/env -S uv run -q --script
# /// script
# requires-python = ">=3.13"
# dependencies = [
# ]
# ///

"""Rewrite and clean content-addressed assets in an image deployment."""

from __future__ import annotations

import argparse
from pathlib import Path
import re
import shutil
import sys

ASSET_PATTERNS = ("drive-*", "linux-*", "fw_jump.bin-*")


def replace_string_value(config: str, key: str, value: str) -> str:
    """Replace one required quoted scalar configuration value."""
    pattern = re.compile(rf'(\b{re.escape(key)}\s*:\s*)"[^"]*"')
    updated, count = pattern.subn(rf'\1"{value}"', config, count=1)
    if count != 1:
        raise ValueError(f"configuration must contain one {key} string")
    return updated


def rewrite_config(source: Path, output: Path, bios: str, kernel: str, drive: str) -> None:
    """Write a deployment config referring to one hashed asset generation."""
    config = source.read_text(encoding="utf-8")
    config = replace_string_value(config, "bios", bios)
    config = replace_string_value(config, "kernel", kernel)
    drive_pattern = re.compile(r'(\bdrive0\s*:\s*\{[^}]*\bfile\s*:\s*)"[^"]*"')
    config, count = drive_pattern.subn(rf'\1"{drive}/blk.txt"', config, count=1)
    if count != 1:
        raise ValueError("configuration must contain one drive0 file string")
    output.write_text(config, encoding="utf-8")


def referenced_assets(config: str) -> set[str]:
    """Return locally named content-addressed assets referenced by config."""
    references = set(
        re.findall(
            r'"((?:drive|fw_jump\.bin)-[0-9a-f]{8}|linux-[0-9a-f]{8}(?:\.gz)?)(?:/blk\.txt)?"',
            config,
        )
    )
    if len(references) != 3:
        raise ValueError("configuration must reference one hashed drive, kernel, and BIOS")
    return references


def clean_deployment(deployment: Path) -> list[Path]:
    """Remove superseded hashed assets from a deployment."""
    if not deployment.is_dir():
        raise ValueError(f"deployment is not a directory: {deployment}")
    config_path = deployment / "riscbox.cfg"
    references = referenced_assets(config_path.read_text(encoding="utf-8"))
    removed: list[Path] = []
    for pattern in ASSET_PATTERNS:
        for asset in deployment.glob(pattern):
            if asset.name in references:
                continue
            if asset.is_dir():
                shutil.rmtree(asset)
            elif asset.is_file():
                asset.unlink()
            removed.append(asset)
    return removed


def parse_args(arguments: list[str]) -> argparse.Namespace:
    """Parse command-line arguments."""
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    rewrite = subparsers.add_parser("rewrite")
    rewrite.add_argument("source", type=Path)
    rewrite.add_argument("output", type=Path)
    rewrite.add_argument("bios")
    rewrite.add_argument("kernel")
    rewrite.add_argument("drive")
    clean = subparsers.add_parser("clean")
    clean.add_argument("deployment", type=Path)
    return parser.parse_args(arguments)


def main(arguments: list[str]) -> int:
    """Run the command-line interface."""
    options = parse_args(arguments)
    try:
        if options.command == "rewrite":
            rewrite_config(
                options.source, options.output, options.bios, options.kernel, options.drive
            )
        else:
            for removed in clean_deployment(options.deployment):
                print(removed.name)
    except (OSError, ValueError) as error:
        print(f"image-deployment: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
