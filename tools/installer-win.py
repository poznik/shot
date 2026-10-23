#!/usr/bin/env python3
"""Builds the Windows installer: python tools/installer-win.py

Builds shot.exe, packs its icon into an .ico for the installer and runs Inno Setup 6 on
tools/installer-win.iss. Shot-<version>-windows-x64-setup.exe lands in target/installer/.
Inno Setup comes from https://jrsoftware.org/isdl.php (or `winget install JRSoftware.InnoSetup`).
"""

import os
import re
import shutil
import struct
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "target" / "installer"


def version():
    with open(ROOT / "Cargo.toml", "rb") as f:
        return tomllib.load(f)["workspace"]["package"]["version"]


def icon(path):
    """Writes the icon shot.exe carries as an .ico: the same PNGs, listed in its build script."""
    build = (ROOT / "crates" / "shot" / "build.rs").read_text(encoding="utf-8")
    names = re.findall(r'"(app-[\w-]+\.png)"', re.search(r"const IMAGES.*?\];", build, re.S)[0])
    pngs = [(ROOT / "assets" / "app" / "windows" / n).read_bytes() for n in names]
    data = struct.pack("<HHH", 0, 1, len(pngs))
    offset = len(data) + 16 * len(pngs)
    for png in pngs:
        # IHDR: width and height, big-endian, at 16; 256 is written as 0.
        width, height = struct.unpack(">II", png[16:24])
        data += struct.pack("<BBBBHHII", width % 256, height % 256, 0, 0, 1, 32, len(png),
                            offset)
        offset += len(png)
    path.write_bytes(data + b"".join(pngs))


def iscc():
    found = shutil.which("ISCC")
    if found:
        return found
    # Its installer puts it in one of these, per user or for all users.
    for base, sub in (("LOCALAPPDATA", "Programs"), ("ProgramFiles(x86)", ""),
                      ("ProgramFiles", "")):
        if base in os.environ:
            candidate = Path(os.environ[base]) / sub / "Inno Setup 6" / "ISCC.exe"
            if candidate.exists():
                return str(candidate)
    sys.exit("installer: Inno Setup 6 not found; install it: winget install JRSoftware.InnoSetup")


def main():
    v = version()
    subprocess.run(["cargo", "build", "--release", "-p", "shot"], cwd=ROOT, check=True)
    OUT.mkdir(parents=True, exist_ok=True)
    ico = OUT / "shot.ico"
    icon(ico)
    subprocess.run([iscc(), "/Q", f"/DVersion={v}",
                    f"/DExe={ROOT / 'target' / 'release' / 'shot.exe'}", f"/DIcon={ico}",
                    f"/O{OUT}", str(ROOT / "tools" / "installer-win.iss")], check=True)
    setup = OUT / f"Shot-{v}-windows-x64-setup.exe"
    print(f"{setup} ({setup.stat().st_size / 1e6:.1f} MB)")


if __name__ == "__main__":
    main()
