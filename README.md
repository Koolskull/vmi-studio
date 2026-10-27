# VMI STUDIO

VMI STUDIO is the Beetle Labs desktop app for turning a layered drawing into game objects.

You keep painting in Clip Studio, Photoshop, Krita, or GIMP. The studio opens that file, shows the visible stack, and writes one folder per object in the VMI layout the game reads. It does not replace the drawing program, and it does not change the drawing file.

Version 0.03. KOSV production lives in this app. The working plate is 1920×1080. You can draw on the plate, start a picture from a pixel size, and store composite effects in the `.vmib`, in the same slot layout the scene export already writes. Numbers and menus drag with a mouse, a pen, or a finger. The packaged downloads below are still the 0.01 builds.

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

The Windows download is `vmi-studio-0.01-windows-x86_64.zip` on that same release. It is a 64-bit build. Unzip it and run `vmi-studio\vmi-studio.exe`. `vmi-clip-vector.exe` sits beside it. A console window stays open with the studio. Check it with `vmi-studio.exe --version`.

The Mac download is `vmi-studio-0.01-macos-arm64.zip` on that same release. It is an Apple Silicon build. Unzip it and open `VMI STUDIO.app`. The first time a downloaded copy is opened, macOS shows “VMI STUDIO” Not Opened, with Move to Trash and Done. Click Done. Then open System Settings, go to Privacy & Security, and click Open Anyway next to the VMI STUDIO note. Open the app again and confirm Open. Shortcuts written as Ctrl in this document are the Command key on a Mac.

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

Projects and tasks are named lists. A project is a body of work. A task list belongs to one project, and that same list is there for every drawing and blueprint in the project. Each file has a label, the user who tagged it, and a finished or working mark. Recent files are a separate list. The folder browser walks the disk from Home, Work, Documents, Desktop, Downloads, iCloud, or the rest of the computer, and shows drawing files and folders. iCloud is iCloud Drive. A file that is still only in the cloud stays in the list. Opening it downloads the file onto this Mac, and the loading window says so. The drawing in iCloud is not changed. A folder row uses the studio folder picture. A file name has no icon. The insert-image browser uses the same rule. Shift+Left and Shift+Right move between the lists, the files, and the browser.

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
- Outline, under the layer list, is Clip Studio's border edge. On draws that many pixels around the non-transparent pixels of the selected layer, in the color saved on the layer. The number is the width, from 1 to 250. Off leaves the paint as it is. A folder uses the merged paint inside it. One change is one undo step. A blueprint keeps the choice. Windows and Linux read the same EffectEdge data and the same control.
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

The Windows build runs on a Windows machine. The workflow is `.github/workflows/windows.yml`. It compiles `lzf_d.dll`, builds `vmi-clip-vector.exe`, and packs `dist\vmi-studio\vmi-studio.exe`.

The Mac build runs on an Apple Silicon Mac. Python 3.12 and a Rust toolchain have to be installed first. The same decoder command used on Linux writes `vmi_studio/lzf_d.so`. On macOS that file is a Mach-O shared library under the Linux name, which is the name the studio loads.

```bash
gcc -O3 -shared -fPIC -o vmi_studio/lzf_d.so vmi_studio/lzf_d.c
cargo build --manifest-path tools/vmi-clip-vector/Cargo.toml --release
.venv/bin/pyinstaller --noconfirm --clean vmi-studio.spec
```

The program is `dist/VMI STUDIO.app`. Open that bundle. Check the build from a terminal with:

```bash
"dist/VMI STUDIO.app/Contents/MacOS/vmi-studio" --version
```

It prints `VMI STUDIO 0.03`. A drawing dropped on the icon, or chosen with Open With, opens in the studio. The first time a downloaded copy is opened, macOS shows “VMI STUDIO” Not Opened, with Move to Trash and Done. Click Done. Then open System Settings, go to Privacy & Security, and click Open Anyway next to the VMI STUDIO note. Open the app again and confirm Open.

An optional font catalog can live in `~/Documents/work/Github/bgcardbuilder`, or in the directory named by `BGCARDBUILDER`. Without it, the header font list is the system monospace. That is the default face either way.

## The Rust desk

`crates/` holds the remake: `vmi-core` (the document, the blueprint, the animation model, palettes), `vmi-paint` (the brushes), and `vmi-app` (the window). The binary is still `vmi-studio`, the cache is still `~/.cache/vmi-studio`, and it reads and writes the same `.vmib` version 1.

```bash
cargo run -p vmi-app --release
```

The window is black, square-cornered, and set in Courier Pixel when `~/.cache/vmi-studio/fonts` holds it, else Sometype Mono, else the system monospace. Four areas sit like Blender's: left, center, right, bottom. Each one shows Picture, Layers, X-Sheet, Film, or Objects, from the menu in its corner or from View → Areas. The defaults are Layers, Picture, the vertical X-sheet, and the film strip. Exactly one area is the live picture; choosing a view that is already open swaps the two. The brush panel between Layers and the picture is 74 px wide: two 36 px columns of tools, and every dial and toggle row wraps to that width. Right-click the colour cell for the palette window.

The sheet is KoolDraw's timeline, drawn two ways. Down the X-sheet, time runs down and every level is a column, the front level at the left and the back level last. Along the film, levels are lanes and frames run right. Time is stable: a column or lane never moves when the playhead does. A key cell prints the cel's number, a held frame is one dotted cell (dashes in the film), a rest is `---`, and a red needle lies on the exact frame. The mode cell at the end of the transport cycles AUTO, FILM, X-SHEET; AUTO shows the sheet in a wide window and the film in a tall one. Click a cell to show that frame and select its cel. Shift-click or drag to pick a range, then right-click for the exposure commands: Reverse, Swing, Step, Each, Reset Step, Increase Step, Decrease Step, Autoexpose, Reframe, Repeat, Insert, Clear, Cut. The first command stores an exposure sheet on that level, as the Python desk did; from then on the grid reads that sheet, which can hold a drawing again later (Swing, Repeat), while the picture still follows the keys. Drag a key to move it. Drag the frame numbers to scrub.

The transport is Back, Play, Forward, Loop, Skin, then the counter, FPS, the hold cells 1 2 3 (ones, twos, threes), +CEL, WRAP, and the mode cell. Comma steps back, period forward, W is the first frame and Shift+W the last, Shift+Space plays, O toggles the onion skin, Shift+F is a new frame. A new frame lands after the drawing under the playhead on the hold you picked, and the cut grows so the last cel holds the same. A lone layer becomes frame 1 of a new Animation folder first, the way the Python desk does it. Moving the playhead selects the cel it shows, so the pen draws on what you see. LOOP returns playback to the first frame; WRAP lets stepping go round the ends and is separate. The onion skin is KoolDraw's: three back in purple, three ahead in green, 0.7, 0.4, 0.18 by distance, each cel's own colour pulled 0.88 of the way to its tone. It is a view and never reaches the file.

The palette window has a tab per palette: the presets (Kool Skull, DB32, DB16, Sweetie 16, PICO-8, Endesga 32, Game Boy, Game Boy Pocket, NES, Commodore 64, CGA, ZX Spectrum, Apple II, Greys) and your own, with `+` for a new one. It lists the files in Aseprite's palette folder when there is one (`~/.config/aseprite/palettes`, `~/Library/Application Support/Aseprite/palettes`, `%APPDATA%\Aseprite\palettes`), else `~/.cache/vmi-studio/palettes`, and loads `.gpl`, `.hex`, `.pal`, `.txt`, `.act`, `.ase`, `.aseprite`, and `.png`. Save writes `.gpl`, which Aseprite reads back. Recent colours sit under the hex field.

Pen pressure is the tablet's, never the pen's speed. On an X11 session, which is Zorin's default, and under XWayland, a second X connection reads the XInput2 `Abs Pressure` valuator of whichever device spoke last. On a native Wayland session the desk binds `zwp_tablet_v2` on winit's display; compositors that stop emulating a pointer for a bound pen get the seat back, which keeps the pen drawing at full pressure until the window reads tablet-v2 itself. Windows pens arrive as touch force through winit. On macOS an AppKit event monitor reads tablet-point pressure, and the Huion and XP-Pen drivers' fractional mouse pressure counts as a pen. A mouse clears the pen and presses at 1. The desk prints one `tablet:` line on start that says which listener it found.

## Intention

The studio is the illustrator's desk, and that desk is the game document. The drawing, the exposure sheet, the storyboard, and the objects in a level live together. Beetle Game reads the scene folder this app already writes. The picture is authored here.

You still paint in Clip Studio, Photoshop, Krita, or GIMP. The studio opens that file, shows the stack, and can write a blueprint and a scene. The drawing file stays the source. A `.clip`, `.psd`, `.kra`, or `.xcf` is never written back. Opening a file does not invent objects. You name them, the same way the Objects pane already describes.

The Python app in this repository is the reference for the window, the opener, and the Clip Studio loader. The crates under `crates/` are that same desk, rebuilt so the picture can stay in front of the pen. When the two disagree, this document and the Python window are the spec. The Rust window is titled KOOL SKULL STUDIO. The package stays `vmi_studio`, the crates stay `vmi-*`, the binary stays `vmi-studio`, the cache stays `~/.cache/vmi-studio`, and the blueprint stays `.vmib` version 1. The Python build in this tree is still 0.03. The packaged downloads on the v0.01 release are unchanged.

## What the Rust desk matches

This pass brings the running desk up to that spec. The paragraphs above still describe the Python window. What follows is the Rust desk beside it.

**Open.** A `.vmib` loads in Rust. Every other drawing runs `python -m vmi_studio.bridge` from the checkout that contains `vmi_studio/bridge.py`. The interpreter is the file named by `VMI_PYTHON` when that file exists, otherwise `.venv` beside that checkout, otherwise `.venv` beside the main checkout when this folder is a linked git worktree. That bridge is the Python opener: Clip Studio time, vector previews, image materials, and outlines. The desk reads `~/.cache/vmi-studio/launcher.json` and leaves that book as the Python launcher wrote it.

**Browser.** The first page is three columns: lists, files, and a folder browser. Lists are Recent and Projects from that book. The folder column starts in the book's saved browse folder, or in `~/Documents/work` when that directory exists, or in the home directory. Places are Home, Work, Documents, Desktop, Downloads, and Computer. A drawing is `.clip`, `.psd`, `.psb`, `.kra`, `.xcf`, or `.vmib`. Nothing loads until one is opened. The line on that page reads: Pick a drawing. Nothing loads until you open it.

**Clip Studio vectors.** Build `tools/vmi-clip-vector` in release, as the source rebuild above already says. The loader looks for `VMI_CLIP_VECTOR`, then `tools/vmi-clip-vector/target/release/vmi-clip-vector`, then the debug build, beside the `vmi_studio` package it imported. When that program is missing, vector layers and image materials stay blank and the picture is only the pixels Clip Studio stored as rasters. The sidecar prints the strokes. `vmi_studio/vectorbake.py` stamps a tight preview onto a layer that came in with no pixels. Those strokes stay a preview. They are not copied onto the desk's editable spine, and the preview has no edit handles. BrushStyle composite mode 27 is the ink eraser. The stroke cuts alpha already stamped on that same layer, and the colour channels stay. Any other mode, including a missing mode, paints. A white stroke in mode 0 stays white. A spline is approximated, and the layer is marked. Image materials use the same binary, with its `images` command, and land on their four stored corners. A file of button art previewed four vector layers, hard edges, mode 0, opaque ink. A file that stores eraser strokes previewed six mode-27 splines. Those strokes cut opaque ink and left no white paint.

**The vector pen.** V on the desk is the studio's own pen. It keeps the spine on the layer and bakes pixels when the stroke ends. That spine is a studio stroke. It is separate from a Clip Studio vector layer. Opening a `.clip` shows the baked preview. It does not turn the file's strokes into handles.

**Layers.** Each row follows the Python outliner. Guides run where an ancestor still has a sibling below. A folder has a square disclosure, a minus, and a plus while it is closed. A closed folder hides its children. The eye is a square, filled when the layer is in the picture. The name is `#3a3a3a` until the row is selected, then white. The selected row is `#111111`. An animation folder keeps room for the letters ANI. A `#2a2a2a` rule runs from the end of the name to the edge of the row. M and S are quiet outlines, filled white with a black letter while they are on. C fills only while it is on: white on a selected row, quiet on any other row. The dial between C and the blend chip is opacity, `00` through `FF`, 7 o'clock at 0 and 5 o'clock at 255. Drag up to raise it. One drag is one undo step. The blend chip lists the studio modes and hides when the pane is too narrow to hold the marks and the chip. A window name is `#8b6bb8`. A warp-target name is `#5c3e4a` until that row is selected.

**Ink and the picture.** One colour is shared by every tool. It starts at `#FF3EB8`. X swaps it with the second colour. C is Clear. The current brush punches transparent pixels, and the tool stays the tool it was. Opacity on a pen, pencil, paint, airbrush, dither, eraser, line, or warp mask is one pass for the whole stroke, including where the stroke crosses itself. Flow is how hard each dab is. Smudge and blend stay per dab, because they read the pixels under them. Size starts at 24, the same as the Python brush. `[` and `]` move it by 1. Shift moves it by 10. The range is 1 to 512. Space, or the middle button, pans. Shift-drag rotates about the center of the view. Space held with Shift, then dragged, zooms about that same center. A raster stroke keeps that layer's pixels for undo and paints every sample the pointer reported, so a slow frame stays a curve. Releasing a raster pen leaves the plate where it is.

**Chrome.** The field is `#000000`. Text that is idle is `#3a3a3a`. Rules are `#2a2a2a`. A focused control, a hover, and an open menu use white text and a white border. Corners are square. There is no shadow. The status line is white, so a long read stays readable.

## What 0.03 does not do

The app does not write `.clip`, `.psd`, `.kra`, or `.blend`. It does not upload to 2kool.tv. GIMP files that are not 8-bit stay as names without pixels. Photoshop placed layers and smart objects are not read. The earlier `v1.0.0` download does not include the timeline, warp drawing, image-material previews, or this header. A build from this source is 0.03. The files on the v0.01 release are the last packaged builds.

## License

MIT. See [LICENSE](LICENSE).

The Clip Studio and Photoshop readers in `third_party/clip2krita/` have their own MIT notices. PySide6 is the Qt for Python binding, under the LGPL. The source of this application is this repository.
