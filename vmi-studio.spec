# -*- mode: python ; coding: utf-8 -*-
"""VMI STUDIO executable. Run from the repository root on the target system."""

import os
import sys

root = os.path.abspath(SPECPATH)
clip_pkg = os.path.join(root, "third_party", "clip2krita", "src", "clip2krita")
clip_vendor = os.path.join(root, "third_party", "clip2krita", "third_party")
windows = sys.platform == "win32"
lzf = os.path.join(root, "vmi_studio", "lzf_d.dll" if windows else "lzf_d.so")
vector_name = "vmi-clip-vector.exe" if windows else "vmi-clip-vector"
vector = os.path.join(root, "tools", "vmi-clip-vector", "target", "release", vector_name)

datas = [
    (clip_pkg, "third_party/clip2krita/src/clip2krita"),
    (clip_vendor, "third_party/clip2krita/third_party"),
    (os.path.join(root, "third_party", "clip2krita", "LICENSE"), "third_party/clip2krita"),
    (os.path.join(root, "LICENSE"), "."),
    (os.path.join(root, "vmi_studio", "icons", "folder.png"), "vmi_studio/icons"),
]
binaries = [(lzf, "vmi_studio")] if os.path.isfile(lzf) else []
if os.path.isfile(vector):
    binaries.append((vector, "."))

a = Analysis(
    [os.path.join(root, "vmi_studio", "__main__.py")],
    pathex=[root],
    binaries=binaries,
    datas=datas,
    hiddenimports=[
        "fontTools.ttLib",
        "PIL",
        "numpy",
        "vmi_studio.catalog",
        "vmi_studio.cloud",
        "vmi_studio.csptime",
        "vmi_studio.drawbar",
        "vmi_studio.exportdialog",
        "vmi_studio.filmstrip",
        "vmi_studio.imagebake",
        "vmi_studio.launchui",
        "vmi_studio.loadgate",
        "vmi_studio.paint",
        "vmi_studio.pngout",
        "vmi_studio.settingsui",
        "vmi_studio.tabs",
        "vmi_studio.tasks",
        "vmi_studio.theme",
    ],
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
