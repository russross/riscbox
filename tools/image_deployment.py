#!/usr/bin/env -S uv run -q --script
# /// script
# requires-python = ">=3.13"
# dependencies = [
# ]
# ///

"""Rewrite and clean content-addressed assets in an image deployment."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import json
from pathlib import Path
import re
import shutil
import sys

ASSET_PATTERNS = ("drive-*", "linux-*", "u-boot.bin-*", "fw_dynamic.bin-*", "fw_jump.bin-*")


@dataclass(frozen=True)
class ExampleDescription:
    """A source example manifest declares paths; deployment adds exact sizes."""

    identifier: str
    title: str
    editable: str
    files: tuple[str, ...]
    documentation: str | None


def relative_path(value: object) -> str:
    """Require a literal relative namespace path with no empty components."""
    if not isinstance(value, str) or not value or "\0" in value:
        raise ValueError("example paths must be nonempty strings")
    if any(part in ("", ".", "..") for part in value.split("/")):
        raise ValueError(f"invalid example path: {value}")
    return value


def parse_example(value: object) -> ExampleDescription:
    """Validate the source record before inspecting its declared files."""
    if not isinstance(value, dict):
        raise ValueError("example record must be an object")
    identifier = relative_path(value.get("id"))
    if "/" in identifier:
        raise ValueError("example ID must be one path component")
    title = value.get("title")
    if not isinstance(title, str) or not title:
        raise ValueError("example title must be a nonempty string")
    editable = relative_path(value.get("editable"))
    paths = value.get("files")
    if not isinstance(paths, list):
        raise ValueError("example files must be an array")
    files = tuple(relative_path(path) for path in paths)
    documentation = value.get("documentation")
    if documentation is not None:
        documentation = relative_path(documentation)
    if len(set(files)) != len(files) or editable not in files:
        raise ValueError("example files must be unique and include the editable file")
    if documentation is not None and documentation not in files:
        raise ValueError("example documentation must be a declared file")
    return ExampleDescription(identifier, title, editable, files, documentation)


def write_example_manifest(source: Path, output: Path) -> None:
    """Publish file metadata without reading or embedding any file bodies."""
    records: object = json.loads(source.read_text(encoding="utf-8"))
    if not isinstance(records, list):
        raise ValueError("example manifest must be an array")
    examples = [parse_example(record) for record in records]
    if len({example.identifier for example in examples}) != len(examples):
        raise ValueError("example IDs must be unique")

    # Resolve each path within its own workspace, including symlink targets.
    # Assemble the entire candidate before replacing the staged manifest.
    deployed: list[dict[str, object]] = []
    for example in examples:
        root = (source.parent / example.identifier).resolve()
        files: list[dict[str, object]] = []
        for path in example.files:
            file = (root / path).resolve()
            if not file.is_relative_to(root) or not file.is_file():
                raise ValueError(f"example file is missing or outside its workspace: {path}")
            size = file.stat().st_size
            if size > 0xFFFF_FFFF:
                raise ValueError(f"example file exceeds the manifest size limit: {path}")
            files.append({"path": path, "size": size})
        record: dict[str, object] = {
            "id": example.identifier, "title": example.title,
            "editable": example.editable, "files": files,
        }
        if example.documentation is not None:
            record["documentation"] = example.documentation
        deployed.append(record)
    output.write_text(json.dumps(deployed, indent=2) + "\n", encoding="utf-8")


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
            r'"(drive-[0-9a-f]{8}|(?:fw_dynamic\.bin|fw_jump\.bin|linux|u-boot\.bin)-[0-9a-f]{8}(?:\.gz)?)(?:/blk\.txt)?"',
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
    examples = subparsers.add_parser("examples")
    examples.add_argument("source", type=Path)
    examples.add_argument("output", type=Path)
    return parser.parse_args(arguments)


def main(arguments: list[str]) -> int:
    """Run the command-line interface."""
    options = parse_args(arguments)
    try:
        if options.command == "rewrite":
            rewrite_config(
                options.source, options.output, options.bios, options.kernel, options.drive
            )
        elif options.command == "examples":
            write_example_manifest(options.source, options.output)
        else:
            for removed in clean_deployment(options.deployment):
                print(removed.name)
    except (OSError, ValueError) as error:
        print(f"image-deployment: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
