//! What the Rust desk has, and the test that proves it.
//! A row is `Have` only when that test is in this crate and passes.

pub struct Row {
    pub name: &'static str,
    pub state: &'static str,
    pub proof: &'static str,
}

pub fn report() -> Vec<Row> {
    vec![
        Row { name: "One dab, smooth and hard", state: "Have", proof: "dab_matches_the_python_disc" },
        Row { name: "KOOLDRAW dither", state: "Have", proof: "dither_stays_ink_or_a_load_gate_tone" },
        Row { name: "Tool palette and right-click settings", state: "Have", proof: "the_palette_names_tools_and_right_click_edits_one" },
        Row { name: "Raster release drops the path", state: "Have", proof: "raster_release_writes_pixels_and_vector_release_keeps_traits" },
        Row { name: "Smudge carry", state: "Have", proof: "smudge_carry_crosses_a_move" },
        Row { name: "Vector traits and bone", state: "Have", proof: "a_vector_stroke_keeps_traits_and_a_bone" },
        Row { name: "Undo limit 20", state: "Have", proof: "undo_keeps_twenty" },
        Row { name: "Leaf rasters from the bridge", state: "Have", proof: "the_bridge_manifest_loads_a_leaf" },
        Row { name: "Cut to a folder", state: "Have", proof: "a_rect_cut_moves_pixels" },
        Row { name: "Blueprint round trip", state: "Have", proof: "a_blueprint_round_trip_keeps_the_stroke" },
        Row { name: "Objects drag", state: "Have", proof: "objects_reorder" },
        Row { name: "Export percent, nearest", state: "Have", proof: "export_percent_is_nearest" },
        Row { name: "Session file is not the Python one", state: "Have", proof: "the_rust_session_is_separate" },
        Row { name: "Six themes", state: "Have", proof: "six_themes_and_paper_inverts" },
        Row { name: "Nearest picture filter", state: "Have", proof: "the_viewfinder_filter_is_nearest" },
        Row { name: "Cloud upload", state: "Gap", proof: "Needs a signed-in 2kool.tv account. Not required to draw." },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use vmi_app::desk::{Button, Desk};
    use vmi_app::gpu::PictureFilter;
    use vmi_core::{
        load_vmib, present, save_vmib, scale_nearest, session_path, stamp, theme_or_studio, Art, Edge, History, Rgba,
        Session, Stamp, StampMode, Theme, Tool, ToolSettings, VectorStroke, THEMES,
    };
    use vmi_paint::paint_samples;

    #[test]
    fn gap_rows_are_the_only_ones_without_a_proof_name() {
        let have: Vec<_> = report().into_iter().filter(|row| row.state == "Have").collect();
        assert!(have.len() >= 15);
        assert!(have.iter().all(|row| !row.proof.is_empty() && !row.proof.contains(' ')));
        assert!(report().iter().any(|row| row.name == "Cloud upload" && row.state == "Gap"));
    }

    #[test]
    fn dab_matches_the_python_disc() {
        let mut hard = vec![0u8; 24 * 24 * 4];
        stamp(&mut hard, 24, 24, Stamp {
            cx: 12.0,
            cy: 12.0,
            radius: 4.0,
            softness: 0.0,
            fringe: 0.0,
            opacity: 1.0,
            color: Rgba::rgb(255, 255, 255),
            edge: Edge::Hard,
            chunk: 1,
            tones: false,
            mode: StampMode::Over,
        });
        let row: Vec<u8> = (0..24).map(|x| hard[((12 * 24 + x) * 4 + 3) as usize]).collect();
        assert_eq!(row, vec![0, 0, 0, 0, 0, 0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn dither_stays_ink_or_a_load_gate_tone() {
        let mut buf = vec![0u8; 32 * 32 * 4];
        let mut settings = ToolSettings::default_for(Tool::Dither);
        settings.size = 16.0;
        settings.size_pressure = false;
        settings.opacity_pressure = false;
        let mut carry = None;
        paint_samples(&mut buf, 32, 32, &[(16.0, 16.0, 1.0)], Tool::Dither, &settings, &mut carry);
        assert!(buf.chunks(4).any(|pixel| pixel[3] == 255));
        for pixel in buf.chunks(4) {
            if pixel[3] == 0 {
                continue;
            }
            assert_eq!(pixel[3], 255);
            assert!(
                pixel[..3] == [0, 0, 0] || pixel[..3] == [0x44, 0x44, 0x44] || pixel[..3] == [0xCC, 0xCC, 0xCC],
                "{pixel:?}"
            );
        }
    }

    #[test]
    fn the_palette_names_tools_and_right_click_edits_one() {
        let mut desk = Desk::new(Session::default());
        assert!(desk.layer_rows().iter().any(|row| row.name == "Ink"));
        assert_eq!(desk.tool(), Tool::Pen);
        desk.palette_click(Tool::Dither, true);
        assert_eq!(desk.tool(), Tool::Pen, "right-click must not change the tool");
        assert_eq!(desk.tool_window, Some(Tool::Dither));
        desk.edit_preset(Tool::Dither, |slot| slot.chunk = 4);
        assert_eq!(desk.preset_of(Tool::Dither).chunk, 4);
        desk.reset_preset(Tool::Dither);
        assert_eq!(desk.preset_of(Tool::Dither).chunk, 2);
        desk.palette_click(Tool::Paint, false);
        assert_eq!(desk.tool(), Tool::Paint);
        assert_eq!(desk.tool_window, Some(Tool::Dither));
    }

    #[test]
    fn raster_release_writes_pixels_and_vector_release_keeps_traits() {
        let mut desk = Desk::new(Session::default());
        desk.new_document();
        // A small canvas keeps the proof fast. new_document is the 1280 desk;
        // swap in a tiny one through the same release path the window uses.
        desk.art = Art::new(32, 24);
        desk.selected = Some(desk.art.layers[0].id.clone());
        desk.edit_preset(Tool::Pen, |slot| {
            slot.size = 6.0;
            slot.edge = Edge::Hard;
            slot.fringe = 0.0;
            slot.size_pressure = false;
            slot.opacity_pressure = false;
        });
        desk.down_doc(8.0, 8.0, Button::Primary);
        desk.up_doc(8.0, 8.0);
        assert!(desk.art.layers[0].pixels.iter().skip(3).step_by(4).any(|alpha| *alpha > 0));
        assert!(desk.art.layers[0].strokes.is_empty());
        desk.undo();
        assert!(desk.art.layers[0].pixels.iter().skip(3).step_by(4).all(|alpha| *alpha == 0));

        desk.set_tool(Tool::Vector);
        desk.down_doc(4.0, 12.0, Button::Primary);
        desk.move_doc(18.0, 12.0);
        desk.up_doc(18.0, 12.0);
        assert_eq!(desk.art.layers[0].strokes.len(), 1);
        assert!(desk.art.layers[0].pixels.iter().skip(3).step_by(4).all(|alpha| *alpha == 0));
        let mut plate = present(&mut desk.art);
        assert!(plate.iter().skip(3).step_by(4).any(|alpha| *alpha > 0));
        let _ = plate.iter_mut();
    }

    #[test]
    fn smudge_carry_crosses_a_move() {
        let mut buf = vec![0u8; 24 * 8 * 4];
        let index = (4 * 24 + 4) * 4;
        buf[index] = 255;
        buf[index + 3] = 255;
        let mut settings = ToolSettings::default_for(Tool::Smudge);
        settings.size = 4.0;
        settings.edge = Edge::Hard;
        settings.fringe = 0.0;
        settings.softness = 0.0;
        settings.size_pressure = false;
        settings.opacity_pressure = false;
        settings.opacity = 1.0;
        let mut carry = None;
        paint_samples(&mut buf, 24, 8, &[(4.0, 4.0, 1.0)], Tool::Smudge, &settings, &mut carry);
        paint_samples(&mut buf, 24, 8, &[(4.0, 4.0, 1.0), (16.0, 4.0, 1.0)], Tool::Smudge, &settings, &mut carry);
        assert!(carry.unwrap().r > 0);
        assert!(buf[(4 * 24 + 12) * 4] > 0);
    }

    #[test]
    fn a_vector_stroke_keeps_traits_and_a_bone() {
        let settings = ToolSettings::default_for(Tool::Curve);
        let mut stroke = VectorStroke::from_gesture(3, Tool::Curve, &[(0.0, 0.0, 1.0), (12.0, 8.0, 1.0), (24.0, 0.0, 1.0)], &settings).unwrap();
        assert!(stroke.vertices[1].handle_out.is_some());
        stroke.bone = Some(vmi_core::Bone { x0: 0.0, y0: 0.0, x1: 0.0, y1: 10.0, reach: 30.0 });
        stroke.touch();
        let baked = vmi_core::bake(&stroke, 40, 20);
        assert!(baked.is_some());
        assert_eq!(stroke.vertices.len(), 3);
    }

    #[test]
    fn undo_keeps_twenty() {
        assert_eq!(History::LIMIT, 20);
        let mut history = History::default();
        for index in 0..25 {
            let mut art = Art::blank(2, 2);
            art.file_name = index.to_string();
            history.push(art);
        }
        assert_eq!(history.len(), 20);
    }

    #[test]
    fn the_bridge_manifest_loads_a_leaf() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("vmi-audit-bridge-{stamp}"));
        std::fs::create_dir_all(dir.join("rasters")).unwrap();
        let manifest = dir.join("manifest.json");
        let script = format!(
            "from vmi_studio.document import ArtFile, Node, Raster\n\
             from vmi_studio.bridge import dump_art\n\
             layer = Node('n1', 'Ink', 'layer', raster=Raster(0, 0, 1, 1, bytes([7, 0, 0, 255])))\n\
             layer.strokes = [{{'color': '#FFFFFF', 'size': 4, 'softness': 0, 'antialias': False, 'points': [[0, 0, 1]], 'seed': 11, 'grain': 'stipple'}}]\n\
             art = ArtFile('leaf', 'leaf.clip', 'clip', 1, 1, [layer])\n\
             dump_art(art, r'{}', r'{}')\n",
            manifest.display(),
            dir.join("rasters").display()
        );
        let output = std::process::Command::new("python3")
            .arg("-c")
            .arg(script)
            .current_dir(root)
            .output()
            .expect("python3");
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        let art = vmi_core::load_manifest(&manifest).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(art.layers[0].pixels[0], 7);
        assert!(art.layers[0].embedded);
        assert_eq!(art.layers[0].strokes[0].seed, 11);
    }

    #[test]
    fn a_rect_cut_moves_pixels() {
        let mut art = Art::blank(4, 4);
        let id = art.add_layer(None, "Ink");
        art.layers[0].pixels[(1 * 4 + 1) * 4 + 3] = 255;
        let made = vmi_core::cut_folder(&mut art, &id, &vmi_core::rect_points(0.0, 0.0, 2.0, 2.0), "Piece").unwrap();
        assert_eq!(art.layers[0].pixels[(1 * 4 + 1) * 4 + 3], 0);
        let folder = vmi_core::find(&art.layers, &made).unwrap();
        assert!(!folder.is_layer());
        assert_eq!(folder.children[0].pixels[(1 * 4 + 1) * 4 + 3], 255);
    }

    #[test]
    fn a_blueprint_round_trip_keeps_the_stroke() {
        let mut art = Art::blank(8, 8);
        let _ = art.add_layer(None, "Ink");
        let settings = ToolSettings::default_for(Tool::Vector);
        let stroke = VectorStroke::from_gesture(2, Tool::Vector, &[(1.0, 1.0, 1.0), (6.0, 4.0, 0.4)], &settings).unwrap();
        art.layers[0].strokes.push(stroke);
        art.layers[0].pixels[3] = 255;
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let path = std::env::temp_dir().join(format!("vmi-audit-{stamp}.vmib"));
        save_vmib(&mut art, &path).unwrap();
        let loaded = load_vmib(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(loaded.layers[0].strokes[0].vertices.len(), 2);
        assert!(!loaded.layers[0].embedded);
        assert_eq!(loaded.layers[0].pixels[3], 255);
    }

    #[test]
    fn objects_reorder() {
        let mut desk = Desk::new(Session::default());
        desk.add_object();
        desk.add_object();
        let first = desk.art.objects[0].id.clone();
        desk.reorder_object(0, 1);
        assert_ne!(desk.art.objects[0].id, first);
        desk.undo();
        assert_eq!(desk.art.objects[0].id, first);
    }

    #[test]
    fn export_percent_is_nearest() {
        let mut src = vec![0u8; 2 * 2 * 4];
        src[0] = 10;
        src[3] = 255;
        src[4] = 200;
        src[7] = 255;
        let (w, h, dst) = scale_nearest(&src, 2, 2, 50);
        assert_eq!((w, h), (1, 1));
        assert_eq!(dst[0], 10);
    }

    #[test]
    fn the_rust_session_is_separate() {
        let path = session_path();
        assert!(path.ends_with("rust-desk.json"));
        assert!(!path.ends_with("session.json"));
        let session = Session::default();
        assert!((session.brush_width - 232.0).abs() < 0.1);
        assert_eq!(session.presets.len(), Tool::PALETTE.len());
    }

    #[test]
    fn six_themes_and_paper_inverts() {
        assert_eq!(THEMES.len(), 6);
        let paper = theme_or_studio("paper");
        let studio = theme_or_studio("studio");
        assert_eq!(paper.field[0], 255 - studio.field[0]);
        assert_eq!(paper.ink[0], 255 - studio.ink[0]);
        assert!(Theme { ..*paper }.name == "Paper" || paper.name == "Paper");
        assert_eq!(theme_or_studio("nope").id, "studio");
    }

    #[test]
    fn the_viewfinder_filter_is_nearest() {
        assert_eq!(vmi_app::gpu::PICTURE_FILTER, PictureFilter::Nearest);
    }
}
