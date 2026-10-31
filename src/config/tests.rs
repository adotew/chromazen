use super::*;

#[test]
fn missing_config_uses_defaults() {
    let temp = tempfile::tempdir().expect("temp directory");
    let store = ConfigStore::from_root(temp.path());

    assert_eq!(
        store.load_app_config().expect("defaults"),
        AppConfig::default()
    );
}

#[test]
fn partial_config_fills_missing_fields_with_defaults() {
    let temp = tempfile::tempdir().expect("temp directory");
    let store = ConfigStore::from_root(temp.path());
    fs::write(store.config_path(), "[brush]\nsize = 425.0\n").expect("write config");

    let config = store.load_app_config().expect("load config");

    assert_eq!(config.brush.size, 425.0);
    assert_eq!(config.brush.opacity, 1.0);
    assert_eq!(config.eraser_opacity, 1.0);
    assert_eq!(config.smudge_opacity, 1.0);
    assert_eq!(config.brush.color, CurrentBrushConfig::default().color);
    assert_eq!(config.accent_color, DEFAULT_ACCENT_COLOR);
    assert_eq!(config.active_brush, "charcoal");
    assert_eq!(config.surface_style, SurfaceStyle::Opaque);
    assert_eq!(config.workspace_background, WorkspaceBackground::Standard);
    assert_eq!(config.color_mixing, ColorMixing::Digital);
}

#[test]
fn surface_style_round_trips_and_defaults_to_opaque() {
    let default_config: AppConfig = toml::from_str("").expect("parse defaults");
    assert_eq!(default_config.surface_style, SurfaceStyle::Opaque);

    let frosted: AppConfig =
        toml::from_str("surface_style = \"frosted\"\n").expect("parse frosted style");
    assert_eq!(frosted.surface_style, SurfaceStyle::Frosted);
    assert!(
        toml::to_string(&frosted)
            .expect("serialize frosted style")
            .contains("surface_style = \"frosted\"")
    );
}

#[test]
fn existing_panel_layout_fills_missing_brush_position() {
    let temp = tempfile::tempdir().expect("temp directory");
    let store = ConfigStore::from_root(temp.path());
    fs::write(
        store.config_path(),
        "[panel_layout]\ncolor_panel_pos = [5.0, 6.0]\nlayers_panel_pos = [70.0, 80.0]\n",
    )
    .expect("write config");

    let layout = store.load_app_config().expect("load config").panel_layout;

    assert_eq!(layout.brush_panel_pos, DEFAULT_BRUSH_PANEL_POS);
    assert_eq!(layout.color_panel_pos, [5.0, 6.0]);
    assert_eq!(layout.layers_panel_pos, [70.0, 80.0]);
    assert_eq!(layout.color_panel_width, DEFAULT_COLOR_PANEL_WIDTH);
    assert_eq!(layout.brush_panel_width, DEFAULT_BRUSH_PANEL_WIDTH);
    assert_eq!(layout.layers_panel_size, DEFAULT_LAYERS_PANEL_SIZE);
}

#[test]
fn tool_brushes_are_selected_independently() {
    let mut config = AppConfig::default();
    config.set_brush_for_tool(PaintTool::Eraser, "hard-round".to_owned());
    config.set_brush_for_tool(PaintTool::Smudge, "soft-blend".to_owned());

    assert_eq!(config.brush_for_tool(PaintTool::Brush), "charcoal");
    assert_eq!(config.brush_for_tool(PaintTool::Eraser), "hard-round");
    assert_eq!(config.brush_for_tool(PaintTool::Smudge), "soft-blend");
}

#[test]
fn valid_config_round_trips_through_atomic_save() {
    let temp = tempfile::tempdir().expect("temp directory");
    let store = ConfigStore::from_root(temp.path().join("nested"));
    let mut config = AppConfig::default();
    config.brush.size = 512.0;
    config.brush.opacity = 0.4;
    config.eraser_opacity = 0.6;
    config.smudge_opacity = 0.8;
    config.brush.color = [1, 2, 3, 255];
    config.accent_color = [4, 5, 6];
    config.workspace_background = WorkspaceBackground::NeutralGray;
    config.pressure_curve = PressureCurve {
        zero_until: 0.1,
        full_from: 0.8,
        controls: [[0.0, 0.5], [0.25, 1.0]],
    };
    config.color_mixing = ColorMixing::Pigment;
    config.panel_layout = PanelLayout {
        brush_panel_pos: [1.0, 2.0],
        color_panel_pos: [5.0, 6.0],
        layers_panel_pos: [70.0, 80.0],
        color_panel_width: 320.0,
        brush_panel_width: 300.0,
        layers_panel_size: [360.0, 500.0],
    };

    store.save_app_config(&config).expect("save config");

    assert_eq!(store.load_app_config().expect("load config"), config);
}

#[test]
fn rejected_save_preserves_previous_config() {
    let temp = tempfile::tempdir().expect("temp directory");
    let store = ConfigStore::from_root(temp.path());
    let valid = AppConfig::default();
    store.save_app_config(&valid).expect("save valid config");
    let previous = fs::read_to_string(store.config_path()).expect("read valid config");
    let mut invalid = valid;
    invalid.brush.size = -1.0;

    assert!(store.save_app_config(&invalid).is_err());
    assert_eq!(
        fs::read_to_string(store.config_path()).expect("read preserved config"),
        previous
    );
}

#[test]
fn malformed_config_is_reported_and_preserved() {
    let temp = tempfile::tempdir().expect("temp directory");
    let store = ConfigStore::from_root(temp.path());
    let malformed = "[brush\nsize = ???\n";
    fs::write(store.config_path(), malformed).expect("write config");

    assert!(store.load_app_config().is_err());
    assert_eq!(
        fs::read_to_string(store.config_path()).expect("read config"),
        malformed
    );
}

#[test]
fn unsupported_app_schema_is_rejected() {
    let temp = tempfile::tempdir().expect("temp directory");
    let store = ConfigStore::from_root(temp.path());
    fs::write(store.config_path(), "schema_version = 2\n").expect("write config");

    let error = store.load_app_config().expect_err("future schema");

    assert!(error.to_string().contains("unsupported schema_version 2"));
}

#[test]
fn invalid_brush_size_is_rejected() {
    let temp = tempfile::tempdir().expect("temp directory");
    let store = ConfigStore::from_root(temp.path());
    fs::write(store.config_path(), "[brush]\nsize = -1.0\n").expect("write config");

    let error = store.load_app_config().expect_err("invalid config");

    assert!(error.to_string().contains("brush.size"));
}

#[test]
fn color_mixing_is_saved_by_name() {
    let temp = tempfile::tempdir().expect("temp directory");
    let store = ConfigStore::from_root(temp.path());
    let config = AppConfig {
        color_mixing: ColorMixing::Pigment,
        ..AppConfig::default()
    };

    store.save_app_config(&config).expect("save config");

    let saved = fs::read_to_string(store.config_path()).expect("read config");
    assert!(saved.contains("color_mixing = \"pigment\""), "{saved}");
}

#[test]
fn unknown_color_mixing_is_rejected() {
    let temp = tempfile::tempdir().expect("temp directory");
    let store = ConfigStore::from_root(temp.path());
    fs::write(store.config_path(), "color_mixing = \"watercolor\"\n").expect("write config");

    let error = store.load_app_config().expect_err("invalid config");

    assert!(error.to_string().contains("watercolor"), "{error}");
}

#[test]
fn invalid_pressure_curve_is_rejected() {
    let temp = tempfile::tempdir().expect("temp directory");
    let store = ConfigStore::from_root(temp.path());
    fs::write(
        store.config_path(),
        "[pressure_curve]\nzero_until = 0.9\nfull_from = 0.2\n",
    )
    .expect("write config");

    let error = store.load_app_config().expect_err("invalid config");

    assert!(error.to_string().contains("pressure_curve"));
}

#[test]
fn invalid_tool_opacity_is_rejected() {
    for source in [
        "eraser_opacity = 0.0\n",
        "smudge_opacity = 1.1\n",
        "[brush]\nopacity = 0.0\n",
    ] {
        let config: AppConfig = toml::from_str(source).expect("parse config");
        assert!(config.validate().is_err());
    }
}

#[test]
fn tool_opacities_are_selected_independently() {
    let config = AppConfig {
        eraser_opacity: 0.5,
        smudge_opacity: 0.25,
        ..AppConfig::default()
    };

    assert_eq!(config.opacity_for_tool(PaintTool::Brush), 1.0);
    assert_eq!(config.opacity_for_tool(PaintTool::Eraser), 0.5);
    assert_eq!(config.opacity_for_tool(PaintTool::Smudge), 0.25);
}

#[test]
fn unknown_fields_are_reported() {
    let temp = tempfile::tempdir().expect("temp directory");
    let store = ConfigStore::from_root(temp.path());
    fs::write(store.config_path(), "unknown_setting = true\n").expect("write config");

    assert!(store.load_app_config().is_err());
}

#[test]
fn bundled_brushes_are_available_without_user_files() {
    let temp = tempfile::tempdir().expect("temp directory");
    let store = ConfigStore::from_root(temp.path());

    let charcoal = store.load_brush("charcoal").expect("charcoal brush");
    let sketch = store.load_brush("sketch").expect("sketch brush");
    let rounded = store.load_brush("rounded").expect("rounded brush");
    let rectangle = store.load_brush("rectangle").expect("rectangle brush");
    let paint = store.load_brush("paint").expect("paint brush");

    assert_eq!(charcoal.id, "charcoal");
    assert_eq!(charcoal.preset.spacing.ratio, 0.03);
    assert_eq!(sketch.id, "sketch");
    assert_eq!(sketch.preset.size.default, 18.0);
    assert_eq!(sketch.preset.spacing.ratio, 0.08);
    assert_eq!(sketch.preset.pressure.min_size, 0.25);
    assert_eq!(sketch.preset.pressure.min_opacity, 0.01);
    assert_eq!(sketch.preset.pressure.full_opacity_pressure, 1.0);
    assert_eq!(sketch.preset.pressure.opacity_gamma, 10.0);
    assert_eq!(rounded.preset.spacing.minimum, 0.5);
    assert_eq!(rectangle.preset.spacing.minimum, 0.5);
    assert_eq!(rounded.stamp_image.dimensions(), (128, 128));
    assert_eq!(rectangle.stamp_image.dimensions(), (192, 96));
    assert_eq!(paint.stamp_image.dimensions(), (256, 256));
    assert_eq!(rounded.stamp_image.get_pixel(0, 0)[3], 0);
    assert_eq!(rectangle.stamp_image.get_pixel(0, 0)[3], 255);
    assert_eq!(
        charcoal.stamp_image.dimensions(),
        sketch.stamp_image.dimensions()
    );
    assert_eq!(
        store
            .discover_brushes()
            .brushes
            .into_iter()
            .map(|brush| brush.id)
            .collect::<Vec<_>>(),
        ["charcoal", "sketch", "rounded", "rectangle", "paint"]
    );
}

#[test]
fn unsupported_brush_schema_is_rejected() {
    let temp = tempfile::tempdir().expect("temp directory");
    let store = ConfigStore::from_root(temp.path());
    write_test_brush(
        &store,
        "future",
        "schema_version = 2\nname = \"Future\"\nstamp = \"tip.png\"\n",
    );

    let error = store.load_brush("future").expect_err("future schema");

    assert!(
        error
            .to_string()
            .contains("unsupported brush schema_version 2")
    );
}

#[test]
fn user_brush_loads_stamp_relative_to_preset() {
    let temp = tempfile::tempdir().expect("temp directory");
    let store = ConfigStore::from_root(temp.path());
    write_test_brush(&store, "pencil", "name = \"Pencil\"\nstamp = \"tip.png\"\n");

    let brush = store.load_brush("pencil").expect("user brush");

    assert_eq!(brush.preset.name, "Pencil");
    assert_eq!(brush.stamp_image.dimensions(), (2, 3));
}

#[test]
fn discovery_keeps_brush_preview_metadata() {
    let temp = tempfile::tempdir().expect("temp directory");
    let store = ConfigStore::from_root(temp.path());
    write_test_brush(
        &store,
        "pencil",
        "name = \"Pencil\"\nstamp = \"tip.png\"\n[spacing]\nratio = 0.4\n",
    );

    let catalog = store.discover_brushes();
    let pencil = catalog
        .brushes
        .iter()
        .find(|brush| brush.id == "pencil")
        .expect("pencil summary");

    assert_eq!(pencil.preview.spacing.ratio, 0.4);
    assert_eq!(
        pencil.preview.stamp_path,
        Some(
            fs::canonicalize(store.brushes_path().join("pencil/tip.png"))
                .expect("canonical stamp path")
        )
    );
}

#[test]
fn oversized_stamp_is_rejected_during_metadata_inspection() {
    let temp = tempfile::tempdir().expect("temp directory");
    let store = ConfigStore::from_root(temp.path());
    write_test_brush(
        &store,
        "oversized",
        "name = \"Oversized\"\nstamp = \"tip.png\"\n",
    );
    image::RgbaImage::from_pixel(4097, 1, image::Rgba([0, 0, 0, 255]))
        .save(store.brushes_path().join("oversized/tip.png"))
        .expect("oversized stamp");

    let catalog = store.discover_brushes();

    assert!(!catalog.brushes.iter().any(|brush| brush.id == "oversized"));
    assert_eq!(catalog.warnings.len(), 1);
    assert!(store.load_brush("oversized").is_err());
}

#[test]
fn stamp_paths_cannot_escape_brush_directory() {
    let temp = tempfile::tempdir().expect("temp directory");
    let store = ConfigStore::from_root(temp.path());
    let brush_dir = store.brushes_path().join("unsafe");
    fs::create_dir_all(&brush_dir).expect("brush directory");
    fs::write(
        brush_dir.join("brush.toml"),
        "name = \"Unsafe\"\nstamp = \"../outside.png\"\n",
    )
    .expect("brush config");

    assert!(store.load_brush("unsafe").is_err());
}

#[test]
fn malformed_brush_does_not_hide_valid_brushes() {
    let temp = tempfile::tempdir().expect("temp directory");
    let store = ConfigStore::from_root(temp.path());
    write_test_brush(&store, "pencil", "name = \"Pencil\"\nstamp = \"tip.png\"\n");
    let broken_dir = store.brushes_path().join("broken");
    fs::create_dir_all(&broken_dir).expect("broken brush directory");
    fs::write(broken_dir.join("brush.toml"), "not valid = [").expect("broken config");

    let catalog = store.discover_brushes();

    assert!(catalog.brushes.iter().any(|brush| brush.id == "pencil"));
    assert_eq!(catalog.warnings.len(), 1);
}

fn write_test_brush(store: &ConfigStore, id: &str, config: &str) {
    let brush_dir = store.brushes_path().join(id);
    fs::create_dir_all(&brush_dir).expect("brush directory");
    fs::write(brush_dir.join("brush.toml"), config).expect("brush config");
    image::RgbaImage::from_pixel(2, 3, image::Rgba([0, 0, 0, 255]))
        .save(brush_dir.join("tip.png"))
        .expect("stamp image");
}
