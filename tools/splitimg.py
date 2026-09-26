#!/usr/bin/env -S uv run -q --script
# /// script
# requires-python = ">=3.13"
# dependencies = [
# ]
# ///

"""Split a disk image into fixed-size blocks for the HTTP block backend."""

from __future__ import annotations

import argparse
import hashlib
from pathlib import Path
import sys

DEFAULT_BLOCK_KIB = 1024


def positive_integer(value: str) -> int:
    """Parse a positive integer accepted by Python's conventional base syntax."""
    try:
        parsed = int(value, 0)
    except ValueError as error:
        raise argparse.ArgumentTypeError(f"invalid integer: {value}") from error
    if parsed <= 0:
        raise argparse.ArgumentTypeError("block size must be greater than zero")
    return parsed


def image_hash(source: Path, block_kib: int) -> str:
    """Return a content identifier that also distinguishes the split layout."""
    digest = hashlib.sha256()
    digest.update(f"block_size_kib={block_kib}\n".encode("ascii"))
    with source.open("rb") as image:
        while block := image.read(1024 * 1024):
            digest.update(block)
    return digest.hexdigest()[:8]


def split_image(source: Path, output_parent: Path, block_kib: int) -> tuple[Path, int]:
    """Split source into a content-named directory and return it and its size."""
    if not source.is_file():
        raise ValueError(f"input is not a file: {source}")
    if not output_parent.is_dir():
        raise ValueError(f"output is not a directory: {output_parent}")

    output = output_parent / f"drive-{image_hash(source, block_kib)}"
    output.mkdir(exist_ok=True)

    block_size = block_kib * 1024
    block_count = 0
    with source.open("rb") as image:
        while block := image.read(block_size):
            padded = block.ljust(block_size, b"\0")
            block_path = output / f"blk{block_count:09d}.bin"
            block_path.write_bytes(padded)
            block_count += 1

    manifest = (
        "{\n"
        f"  block_size: {block_kib},\n"
        f"  n_block: {block_count},\n"
        "}\n"
    )
    (output / "blk.txt").write_text(manifest, encoding="ascii")
    return output, block_count


def parse_args(arguments: list[str]) -> argparse.Namespace:
    """Parse command-line arguments."""
    parser = argparse.ArgumentParser(
        description="Create a multi-file disk image for the Riscbox HTTP block device"
    )
    parser.add_argument("input", type=Path)
    parser.add_argument(
        "output", type=Path, help="existing directory that will contain drive-HASH"
    )
    parser.add_argument(
        "block_size_kib",
        nargs="?",
        default=DEFAULT_BLOCK_KIB,
        type=positive_integer,
    )
    return parser.parse_args(arguments)


def main(arguments: list[str]) -> int:
    """Run the command-line interface."""
    options = parse_args(arguments)
    try:
        output, block_count = split_image(
            options.input, options.output, options.block_size_kib
        )
    except (OSError, ValueError) as error:
        print(f"splitimg: {error}", file=sys.stderr)
        return 1
    print(f"{output.name} {block_count} blocks")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
