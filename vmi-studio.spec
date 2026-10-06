# -*- mode: python ; coding: utf-8 -*-
"""Linux build for the VMI STUDIO executable. Run from the repository root."""

import os

root = os.path.abspath(SPECPATH)
clip_pkg = os.path.join(root, "third_party", "clip2krita", "src", "clip2krita")
clip_vendor = os.path.join(root, "third_party", "clip2krita", "third_party")
lzf = os.path.join(root, "vmi_studio", "lzf_d.so")

datas = [
    (clip_pkg, "third_party/clip2krita/src/clip2krita"),
    (clip_vendor, "third_party/clip2krita/third_party"),
    (os.path.join(root, "third_party", "clip2krita", "LICENSE"), "third_party/clip2krita"),
    (os.path.join(root, "LICENSE"), "."),
]
binaries = [(lzf, "vmi_studio")] if os.path.isfile(lzf) else []

a = Analysis(
    [os.path.join(root, "vmi_studio", "__main__.py")],
    pathex=[root],
    binaries=binaries,
    datas=datas,
    hiddenimports=["fontTools.ttLib", "PIL", "numpy"],
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=[],
    noarchive=False,
)
pyz = PYZ(a.pure)
exe = EXE(
    pyz,
    a.scripts,
    [],
    exclude_binaries=True,
    name="vmi-studio",
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    upx=False,
    console=True,
)
coll = COLLECT(
    exe,
    a.binaries,
    a.datas,
    strip=False,
    upx=False,
    name="vmi-studio",
)
