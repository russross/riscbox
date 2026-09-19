#!/usr/bin/env -S uv run -q --script
# /// script
# requires-python = ">=3.13"
# dependencies = [
# ]
# ///

"""Split a disk image into fixed-size blocks for the HTTP block backend."""

from __future__ import annotations

import argparse
from pathlib import Path
import sys

DEFAULT_BLOCK_KIB = 256


def positive_integer(value: str) -> int:
    """Parse a positive integer accepted by Python's conventional base syntax."""
    try:
        parsed = int(value, 0)
    except ValueError as error:
        raise argparse.ArgumentTypeError(f"invalid integer: {value}") from error
    if parsed <= 0:
        raise argparse.ArgumentTypeError("block size must be greater than zero")
    return parsed


def split_image(source: Path, output: Path, block_kib: int) -> int:
    """Split source into output and return the number of blocks written."""
    if not source.is_file():
        raise ValueError(f"input is not a file: {source}")
    if not output.is_dir():
        raise ValueError(f"output is not a directory: {output}")

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
    return block_count


def parse_args(arguments: list[str]) -> argparse.Namespace:
    """Parse command-line arguments."""
    parser = argparse.ArgumentParser(
        description="Create a multi-file disk image for the Riscbox HTTP block device"
    )
    parser.add_argument("input", type=Path)
    parser.add_argument("output", type=Path, help="existing output directory")
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
        block_count = split_image(
            options.input, options.output, options.block_size_kib
        )
    except (OSError, ValueError) as error:
        print(f"splitimg: {error}", file=sys.stderr)
        return 1
    print(f"{block_count} blocks")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
