# VMI STUDIO

VMI STUDIO is the Beetle Labs desktop app for turning a layered drawing into game objects.

You keep painting in Clip Studio, Photoshop, Krita, or GIMP. The studio opens that file, shows the visible stack, and writes one folder per object in the VMI layout the game reads. It does not replace the drawing program, and it does not change the drawing file.

Version 1.0.0.

## Install the executable

The 1.0.0 download is a 64-bit Linux build, made on Ubuntu 24.04 (glibc 2.39). Use that or a newer glibc.

1. Download `vmi-studio-1.0.0-linux-x86_64.tar.gz` from the [releases page](https://github.com/Koolskull/vmi-studio/releases/tag/v1.0.0).
2. Extract it and run the program inside:

```bash
tar -xzf vmi-studio-1.0.0-linux-x86_64.tar.gz
./vmi-studio/vmi-studio
```

Pass a drawing to open it immediately:

```bash
./vmi-studio/vmi-studio ~/art/door.clip
```

Check the build with:

```bash
./vmi-studio/vmi-studio --version
```

It prints `VMI STUDIO 1.0.0`. The program is a window. It needs a normal desktop session.

## The window

The title is `VMI STUDIO — <file>`. Three panes sit side by side. Drag the gutters to resize them.

| Pane | What it is |
| --- | --- |
| Layers | The drawing, as a tree of folders and layers |
| Picture | The visible stack |
| Objects | The VMI level you are building |

White means the thing is highlighted: the selected row, an open arrangement lock, the active tool, the Export button. Everything else stays dark grey.

## Open a drawing

File → Open, or pass the path on the command line.

| Extension | Program |
| --- | --- |
| `.clip` | Clip Studio Paint |
| `.psd`, `.psb` | Photoshop |
| `.kra` | Krita |
| `.xcf` | GIMP, 8-bit |
| `.vmib` | A VMI STUDIO blueprint |

A large `.clip` can take a minute or more the first time. The status line tells you it is reading. A saved `.vmib` skips that conversion and opens the studio project directly.

The file you opened stays the source. The studio never writes back over a `.clip`, `.psd`, `.kra`, or `.xcf`.

## Layers

Each row is one layer or folder.

- The square is the eye. Filled means the layer is in the picture and can be exported. An empty square hides it. Hiding a folder hides everything inside it.
- M is mute. S is solo. These change the picture while you look. They do not change the eye, and they do not change export. A muted folder drops everything inside it. If any solo is on, only soloed branches paint. A hidden layer stays hidden even when it is soloed.
- C is the clipping mask, the same bit as Clip Studio and Photoshop. On, and the row is not highlighted, the square is grey. On, and the row is highlighted, the square is white. Off is an empty outline. Clicking C does not change which row is selected.
- The dial between C and the blend menu is opacity, from `00` to `FF`. Drag up to raise it. One drag is one undo step.
- The menu at the end of the row is the blend mode. It stays grey until that row is highlighted.

The lock at the top right of Layers is closed when you open a file. Unlock it before you drag layers into a new order. Drag the top of a row to place the selection in front, the bottom to place it behind, and the middle of a folder to put it inside. Hide, rename, and grouping still work while the lock is closed.

Double-click a name to rename it, or right-click the row and choose Rename. Shift-click selects a range. Ctrl-click toggles one row. Ctrl+G wraps the selection in a new folder named Folder.

Layers named sketch, guide, reference, template, or paper, and names that start with `_`, stay out of export. So do hidden layers.

## Make an object

An object is one thing in the level: a door, a button, a screen. Opening a file does not invent objects. You name them.

Right-click a folder that is one still picture. The first item is **create new object**. The name field starts as the folder name. Change it when the folder is not labeled the way the game should say it. An empty name cancels. A name that matches an existing object, ignoring case, is refused.

The next item, **create new object and generate slot positions from subfolders**, does the same and also reads child folders as frames. A folder named hover, pressed, `door#hover`, or `wave@f2` keeps that position. Other folders take the next free state, top of the tree first.

The new object appears in the Objects pane. That list is the level. The top of a group is in front. Drop an object onto another to put it inside. Right-click an object, or press Delete, to remove it.

Under the list, for the selected object:

- **Name**
- **Type**: from the layers, static, button, sequence, or warp target
- **Warp layer**: none, mask, or warp map. A mask is the painted alpha. A warp map is the quad.
- **Sound**: a Beetle Game battle effect, or none. The set is Chord 1–4, Hit 1–5, Ring 1–3, Rise 1–2, and Tik.
- **Playlist**: none, menu, or battle. This is the level, not each object. Those are the two Webamp lists in Beetle Game.
- **Add slot** adds another state and frame. The first four are rest (`000`), hover (`001`), pressed (`002`), and after (`003`). The labels can be edited. The names are written into the object file.
- **Export** writes the scene.

## Frames

Export pictures are named `SSSXXX.PNG`.

- `SSS` is the state in hex. `000` rest, `001` hover, `002` pressed, `003` after.
- `XXX` is the frame in hex.

Right-click a folder and choose Animation, or use Layer → Animation folder, when that folder is one sequence. Its direct children are the frames. The picture shows one frame at a time, plus the layers that are not in the animation. Nested folders inside an animation stay parts of a frame. They do not become a second animation.

## Warp targets

A layer named WT or Warp Target, or a hot-pink quad, is a warp target. Its name is dark pink until you select it. A screen, monitor, TV, poster, or window folder that holds a target is a window, and its name is purple.

Right-click that folder and choose **prepare as warp target object**. Name it. The object lands in the right-hand list with the warp role filled in.

## The picture

Scroll zooms. Space and drag, or the middle button, pans. Shift and drag rotates. Ctrl+0 fits the picture and resets the view. Ctrl+1 is actual size. The same commands are under View.

Click a painted pixel to select that layer in the tree. The click follows hide, mute, solo, the current animation frame, and clipping. Empty pixels fall through.

Shift+X cuts the folder that is selected in the layer tree. If you selected a layer, the cut uses the folder that holds it. The bar under the picture offers Rect, Lasso, and Polygon, then Cut to folder, Delete, and Done. Cut to folder asks for a name before any pixels move. The field starts as Cut. Leave **Make this an object** off unless that new folder should also become an object. Delete clears the pixels in the marquee and does not add a folder. Escape clears the marquee.

## Undo

Ctrl+Z undoes. Ctrl+Shift+Z redoes. On a touch screen, a two-finger tap undoes and a three-finger tap redoes. A pinch that moves is not a tap. The stack holds 20 steps.

## Save a blueprint, then export

File → Save blueprint (Ctrl+S) writes a `.vmib` file. That is the studio project: layer order, names, visibility, opacity, blend, clipping, cuts, mute and solo, animation, and the object list. An open `.vmib` is overwritten. A drawing is not. Save blueprint as always asks for a new file.

Use a blueprint when you need to keep a cut or an arrangement. Reopening the original `.clip` brings back the original pixels.

File → Export scene, or the Export button, asks for a directory and writes:

```text
<scene>/HOW.txt
<scene>/scene.json
<scene>/layers.tsx
<scene>/objects/<name>/object.json
<scene>/objects/<name>/SSSXXX.PNG
```

The PNGs are the full canvas, not cropped sprites. Layers that share a state and a frame are composited into one PNG. Export follows the eye. Mute and solo are ignored, so a soloed preview cannot silently drop the rest of the level.

## Run from source

Python 3.12, plus the packages in `requirements.txt`.

```bash
python3 -m venv .venv
.venv/bin/python -m pip install -r requirements.txt
.venv/bin/python -m vmi_studio
```

`vmi-studio --dev drawing.kra` restarts when the Python in `vmi_studio/` changes and reopens the same drawing.

To rebuild the Linux executable from this repo:

```bash
gcc -O3 -shared -fPIC -o vmi_studio/lzf_d.so vmi_studio/lzf_d.c
.venv/bin/pyinstaller --noconfirm --clean vmi-studio.spec
```

The program is `dist/vmi-studio/vmi-studio`. The `.so` speeds up Krita files. Without it, those files still open through the Python decoder.

An optional font catalog can live in `~/Documents/work/Github/bgcardbuilder`, or in the directory named by `BGCARDBUILDER`. Without it, the Font menu is the system monospace. That is the default face either way.

## What 1.0.0 does not do

It does not write `.clip`, `.psd`, `.kra`, or `.blend`. It does not upload to 2kool.tv. GIMP files that are not 8-bit stay as names without pixels.

## License

MIT. See [LICENSE](LICENSE).

The Clip Studio and Photoshop readers in `third_party/clip2krita/` have their own MIT notices. PySide6 is the Qt for Python binding, under the LGPL. The source of this application is this repository.
