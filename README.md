# VMI STUDIO

VMI STUDIO is the Beetle Labs desktop app for turning a layered drawing into game objects.

You keep painting in Clip Studio, Photoshop, Krita, or GIMP. The studio opens that file, shows the visible stack, and writes one folder per object in the VMI layout the game reads. It does not replace the drawing program, and it does not change the drawing file.

Version 0.01.

## Install the executable

The 0.01 download is a 64-bit Linux build, made on Ubuntu 24.04 (glibc 2.39). Use that or a newer glibc. The archive holds `vmi-studio` and `vmi-clip-vector`. The second program reads Clip Studio vector strokes and image materials for the preview. The studio finds it beside itself.

1. Download `vmi-studio-0.01-linux-x86_64.tar.gz` from the [releases page](https://github.com/Koolskull/vmi-studio/releases/tag/v0.01).
2. Extract it and run the program inside:

```bash
tar -xzf vmi-studio-0.01-linux-x86_64.tar.gz
./vmi-studio/vmi-studio
```

With no path, the launcher opens. Pass a drawing to skip the launcher and open that file:

```bash
./vmi-studio/vmi-studio ~/art/door.clip
```

Check the build with:

```bash
./vmi-studio/vmi-studio --version
```

It prints `VMI STUDIO 0.01`. The program is a window. It needs a normal desktop session.

## The window

The title is `VMI STUDIO — <file>`. The menu bar is File, Edit, View, and Layer. Settings, the font list, and the theme list sit at the right of that bar. Themes are Studio, Paper, Charcoal, Peach, Mint, and Lilac. Three panes sit side by side. Drag the gutters to resize them.

| Pane | What it is |
| --- | --- |
| Layers | The drawing, as a tree of folders and layers |
| Picture | The visible stack |
| Objects | The VMI level you are building |

White means the thing is highlighted: the selected row, an open arrangement lock, the active tool, the Export button. Everything else stays dark grey.

## Launcher

The browser is the leftmost tab. Its name is browser, and it stays there. File → Open selects it. The studio does not read a drawing until you open one from that tab.

Projects and tasks are named lists. A project is a body of work. A task list belongs to one project, and that same list is there for every drawing and blueprint in the project. Each file has a label, the user who tagged it, and a finished or working mark. Recent files are a separate list. The folder browser walks the disk from Home, Work, Documents, Desktop, Downloads, or the rest of the computer, and shows drawing files and folders. A folder row uses the studio folder picture. A file name has no icon. The insert-image browser uses the same rule. Shift+Left and Shift+Right move between the lists, the files, and the browser.

Choose Open on a row to load that file as a project tab to the right of the browser. File → Launcher selects the browser tab again. Quit leaves the program. A path on the command line opens that file and leaves the browser tab in place.

Each project tab keeps its own scene folder. Export scene for one tab does not write into another tab's folder.

Layer → Insert image, or Insert image on a layer, opens a browser for a picture. The rest of the studio dims while it is up. The picture becomes a layer in front of the current row. A selected folder receives it inside.

## Open a drawing

The browser tab, or a path on the command line. File → Open selects the browser tab.

| Extension | Program |
| --- | --- |
| `.clip` | Clip Studio Paint |
| `.psd`, `.psb` | Photoshop |
| `.kra` | Krita |
| `.xcf` | GIMP, 8-bit |
| `.vmib` | A VMI STUDIO blueprint |

A large `.clip` can take a minute or more the first time. While it is reading, the rest of the editor darkens and a small window stays above the studio until the picture itself is on screen. The layers and the other panes fill in behind that window. The file name sits on the left of the top line and LOADING sits on the right, with dots stepping after the word. Both are white. The line that says the studio cannot be used yet is dark grey, and thank you for your patience is a lighter grey. It plays a liquid dither in black, `#888888`, and `#CCCCCC`. Along the bottom, an ASCII bar fills as the read moves through converting, decoding, and painting. The cell at the head of that bar cycles `# 0 o , _ , * %`. The `#` marks behind that cell are `#BBBBBB`. The bar reaches the end when the picture is on screen. A saved `.vmib` skips the conversion and opens the studio project directly.

The file you opened stays the source. The studio never writes back over a `.clip`, `.psd`, `.kra`, or `.xcf`.

## Layers

Each row is one layer or folder.

- The square is the eye. Filled means the layer is in the picture and can be exported. An empty square hides it. Hiding a folder hides everything inside it.
- M is mute. S is solo. These change the picture while you look. They do not change the eye, and they do not change export. A muted folder drops everything inside it. If any solo is on, only soloed branches paint. A hidden layer stays hidden even when it is soloed.
- C is the clipping mask, the same bit as Clip Studio and Photoshop. On, and the row is not highlighted, the square is grey. On, and the row is highlighted, the square is white. Off is an empty outline. Clicking C does not change which row is selected.
- The dial between C and the blend menu is opacity, from `00` to `FF`. Drag up to raise it. One drag is one undo step.
- The menu at the end of the row is the blend mode. It is only as wide as the longest mode name, and it stays grey until that row is highlighted. Widening the pane gives the extra room to the layer names and the branches. Shrinking the pane hides the blend mode before it hides mute, solo, or clip. On a 4K screen the layers pane starts wide enough that the blend mode is visible at that small size.

The lock at the top right of Layers is closed when you open a file. Unlock it before you drag layers into a new order. Drag the top of a row to place the selection in front, the bottom to place it behind, and the middle of a folder to put it inside. Hide, rename, and grouping still work while the lock is closed. Clicking the lock does not hide the pane.

Tap the Layers title to fold that pane to the left. Tap the Objects title to fold that pane to the right. Tap the title again to open it. The picture stays put.

Double-click a name to rename it, or right-click the row and choose Rename. Shift-click selects a range. Ctrl-click toggles one row. Ctrl+G wraps the selection in a new folder named Folder. Backspace or Delete removes the selected layer. A folder takes the layers inside it with it. Right-click the row and choose Delete. One delete is one undo step.

Layers named sketch, guide, reference, template, or paper, and names that start with `_`, stay out of export. So do hidden layers.

## Make an object

An object is one thing in the level: a door, a button, a screen. Opening a file does not invent objects. You name them.

Right-click a folder that is one still picture. The first item is **create new object**. The name field starts as the folder name. Change it when the folder is not labeled the way the game should say it. An empty name cancels. A name that matches an existing object, ignoring case, is refused.

The next item, **create new object and generate slot positions from subfolders**, does the same and also reads child folders as frames. A folder named hover, pressed, `door#hover`, or `wave@f2` keeps that position. Other folders take the next free state, top of the tree first.

The new object appears in the Objects pane. That list is every object, in the order the scene draws them. The top row is behind. Drag a row up or down to change the order. Preview, at the bottom right of the pane, opens a still of that export. It starts closed. Right-click an object, or press Delete or Backspace, to remove it.

Under the object list, the same pane holds tasks for this drawing. A task is bright white. Right-click a task or the empty list: **New task**, **Execute**, and **Delete task**. Delete and Execute stay grey when the click is on empty space. Execute strikes the task through and turns it dark grey. Right-click it again and choose **Restore** to bring it back. Double-click the words to rename them. The gear at the right of TASKS opens the background and text colors. **float** lifts the task window above the studio. **dock** puts it back in the objects pane. The tasks and those two colors are saved in the blueprint, and in the session for this drawing.

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

A `.clip` animation folder is read as Clip Studio stored it. The folder is the cut. Each specified cel is a child placed on the frame Clip Studio assigned, which can be many frames after the previous cel. That cel holds until the next specified frame. The last cel holds through the end of the cut. A child that was never specified stays in the folder and is not a frame. The gutter under that row names it as not specified.

The timeline sits under the picture, between the two side panes. Each row is one animation folder. A block is a cel and the frames it holds. Each frame is an outline on a faint column, and that column continues a little above the frame. The frame in the picture has a white outline and the playhead line. Click a frame to show that cel. Drag a block to move the frame it starts on. One drag is one undo step. Drag the frame numbers or the line to scrub when the cut has frames. Comma steps back one frame, period steps forward, and W returns to the first frame. Back, Play, Forward, and Loop sit on the bar above the strip. Play follows the cut's frame rate. Loop returns to the first frame; with Loop off, playback stops on the last frame. The Timeline button and its line are one handle under the picture. The line is seven pixels: `#141414`, grey under the pointer, white while it is held. Drag it up or down to give the timeline or the picture more height. The Timeline button in the middle of that line hides the strip. Tap it again to bring the strip back. The transport stays. The number in a cel is that cel's place in the timeline, starting at 1.

## Warp targets

A layer named WT or Warp Target, or a hot-pink quad, is a warp target. Its name is dark pink until you select it. A screen, monitor, TV, poster, or window folder that holds a target is a window, and its name is purple. Any hex can be a warp target once the tool marks it. The polygon stays that bright color. The name uses a darker form of the same hex.

**Create warp target** clicks the corners of a new polygon. Enter closes it. Escape cancels, and no layer is added until the polygon closes. The new layer lands in the selected layer's folder, directly under that layer. A selected folder receives it inside the folder. **Draw warp mask** paints a tighter edge than that polygon, in one color. The brush has size, minimum, maximum, softness, antialiasing, and pressure curves for size and opacity. Both tools are on the bar under the picture and in the Layer menu.

Right-click that folder and choose **prepare as warp target object**. Name it. The object lands in the right-hand list with the warp role filled in.

## The picture

Scroll zooms. Space and drag, or the middle button, pans. Shift and drag rotates. Ctrl+0 fits the picture and resets the view. Ctrl+1 is actual size. The same commands are under View.

Click a painted pixel to select that layer in the tree. The click follows hide, mute, solo, the current animation frame, and clipping. Empty pixels fall through.

Shift+X cuts the folder that is selected in the layer tree. If you selected a layer, the cut uses the folder that holds it. The bar under the picture offers Rect, Lasso, and Polygon, then Cut to folder, Delete, and Done. Cut to folder asks for a name before any pixels move. The field starts as Cut. Leave **Make this an object** off unless that new folder should also become an object. Delete clears the pixels in the marquee and does not add a folder. Escape clears the marquee.

## Undo

Ctrl+Z undoes. Ctrl+Shift+Z redoes. On a touch screen, a two-finger tap undoes and a three-finger tap redoes. A pinch that moves is not a tap. The stack holds 20 steps.

## Save a blueprint, then export

File → Save blueprint (Ctrl+S) writes a `.vmib` file. That is the studio project: layer order, names, visibility, opacity, blend, clipping, cuts, mute and solo, animation, the object list, and the tasks. An open `.vmib` is overwritten. A drawing is not. Save blueprint as always asks for a new file. File → Cloud storage can copy that zip to an IPFS node or an FTP server on this computer. The password stays here and is not written into the blueprint. The local save still finishes when the copy does not.

Use a blueprint when you need to keep a cut or an arrangement. Reopening the original `.clip` brings back the original pixels.

File → Export scene, or the Export button, asks for a percentage, nearest or bicubic, and a directory, then writes the scene. The folder it offers belongs to the tab that is open. Two open drawings do not share that path. Choosing a folder that would write the same scene as another tab is refused. 100% keeps every picture at the canvas size. Shift+E writes one PNG of the folder selected in Layers, or of the object selected in Objects, through the same panel. Favorites and recent folders are remembered. Mute and solo still do not change the export.

The scene folder is:

```text
<scene>/HOW.txt
<scene>/scene.json
<scene>/layers.tsx
<scene>/objects/<name>/object.json
<scene>/objects/<name>/SSSXXX.PNG
```

At 100% the PNGs are the full canvas. Any other percentage scales that picture, nearest or bicubic. They are not cropped sprites. Layers that share a state and a frame are composited into one PNG. Export follows the eye. Mute and solo are ignored, so a soloed preview cannot silently drop the rest of the level.

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
cargo build --manifest-path tools/vmi-clip-vector/Cargo.toml --release
.venv/bin/pyinstaller --noconfirm --clean vmi-studio.spec
```

The program is `dist/vmi-studio/vmi-studio`. The spec copies `vmi-clip-vector` into that folder. The `.so` speeds up Krita files. Without it, those files still open through the Python decoder.

An optional font catalog can live in `~/Documents/work/Github/bgcardbuilder`, or in the directory named by `BGCARDBUILDER`. Without it, the header font list is the system monospace. That is the default face either way.

## What 0.01 does not do

The app does not write `.clip`, `.psd`, `.kra`, or `.blend`. It does not upload to 2kool.tv. GIMP files that are not 8-bit stay as names without pixels. Photoshop placed layers and smart objects are not read. The earlier `v1.0.0` download does not include the timeline, warp drawing, image-material previews, or this header. Use 0.01.

## License

MIT. See [LICENSE](LICENSE).

The Clip Studio and Photoshop readers in `third_party/clip2krita/` have their own MIT notices. PySide6 is the Qt for Python binding, under the LGPL. The source of this application is this repository.
