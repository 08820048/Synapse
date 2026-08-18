#!/usr/bin/env python3

"""Build assets/branding/synapse-app-icon.ico from the 1024px PNG master."""

from __future__ import annotations

import struct
import subprocess
import sys
import tempfile
from pathlib import Path

SIZES = (16, 32, 48, 256)


def write_ico(destination: Path, png_frames: list[tuple[int, bytes]]) -> None:
    offset = 6 + 16 * len(png_frames)
    header = struct.pack("<HHH", 0, 1, len(png_frames))
    entries = bytearray()
    payload = bytearray()

    for size, png in png_frames:
        encoded_size = 0 if size >= 256 else size
        entries.extend(
            struct.pack(
                "<BBBBHHII",
                encoded_size,
                encoded_size,
                0,
                0,
                1,
                32,
                len(png),
                offset,
            )
        )
        payload.extend(png)
        offset += len(png)

    destination.write_bytes(header + entries + payload)


def main() -> int:
    project_root = Path(__file__).resolve().parents[1]
    source = project_root / "assets" / "branding" / "synapse-app-icon.png"
    destination = project_root / "assets" / "branding" / "synapse-app-icon.ico"

    if not source.is_file():
        print(f"Missing source icon: {source}", file=sys.stderr)
        return 1

    frames: list[tuple[int, bytes]] = []
    with tempfile.TemporaryDirectory() as scratch:
        scratch_path = Path(scratch)
        for size in SIZES:
            resized = scratch_path / f"icon-{size}.png"
            subprocess.run(
                [
                    "sips",
                    "-z",
                    str(size),
                    str(size),
                    str(source),
                    "--out",
                    str(resized),
                ],
                check=True,
                capture_output=True,
            )
            frames.append((size, resized.read_bytes()))

    write_ico(destination, frames)
    print(f"Wrote {destination}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
