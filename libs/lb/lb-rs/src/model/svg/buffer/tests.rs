use super::*;

const ID: &str = "12345678-1234-1234-1234-123456789abc";

fn drawing(data: &str, transform: &str) -> String {
    format!(
        "<svg xmlns='http://www.w3.org/2000/svg'><path id='{ID}' d='{data}' stroke='black' stroke-width='2' fill='none' transform='{transform}'/></svg>"
    )
}

fn path(buffer: &Buffer) -> &Path {
    let Element::Path(path) = &buffer.elements[&Uuid::parse_str(ID).unwrap()] else { panic!() };
    path
}

fn bounds(buffer: &Buffer) -> [DVec2; 2] {
    path(buffer).data.bounding_box().unwrap()
}

#[test]
fn imported_element_transforms_are_in_document_geometry() {
    let buffer = Buffer::new(&drawing("M 20 30 L 40 50", "matrix(2 0 0 2 10 20)"));
    assert_eq!(bounds(&buffer), [DVec2::new(50.0, 80.0), DVec2::new(90.0, 120.0)]);
    let restored = Buffer::new(&buffer.serialize());
    assert_eq!(bounds(&restored), bounds(&buffer));
    assert_eq!(path(&restored).stroke.unwrap().width, 4.0);
}

#[test]
fn legacy_geometry_is_not_transformed_by_the_saved_viewport() {
    let settings = WeakViewportSettings {
        master_transform: Transform::from_scale(3.0, 3.0)
            .post_translate(100.0, 200.0)
            .into(),
        bounded_rect: Some(WeakRect { min: (160.0, 290.0), max: (220.0, 350.0) }),
        ..Default::default()
    };
    let encoded = base64::encode(bincode::serialize(&settings).unwrap());
    let legacy = drawing("M 20 30 L 40 50", "matrix(2 0 0 2 10 20)").replace(
        "</svg>",
        &format!("<g id='{WEAK_VIEWPORT_SETTINGS_G_ID}'><g id='{encoded}'/></g></svg>"),
    );
    let buffer = Buffer::new(&legacy);
    assert!(buffer.has_legacy_viewport);
    assert_eq!(bounds(&buffer), [DVec2::new(20.0, 30.0), DVec2::new(40.0, 50.0)]);
    let saved = buffer.serialize();
    assert!(!saved.contains(WEAK_VIEWPORT_SETTINGS_G_ID));
    assert!(!saved.contains(ZOOM_G_ID));
    let restored = Buffer::new(&saved);
    assert_eq!(bounds(&restored), bounds(&buffer));
    assert_eq!(path(&restored).stroke.unwrap().width, 4.0);
}

#[test]
fn serialization_ignores_legacy_view_state_and_preserves_images_and_pressure() {
    let mut buffer = Buffer::new(&drawing("M 20 30 L 40 50", ""));
    let id = Uuid::parse_str(ID).unwrap();
    buffer.weak_path_pressures.insert(id, vec![0.2, 0.5]);
    buffer.weak_images.insert(
        Uuid::new_v4(),
        WeakImage {
            href: Uuid::new_v4(),
            transform: WeakTransform::default(),
            opacity: 0.6,
            x: 15.0,
            y: 25.0,
            width: 40.0,
            height: 80.0,
            z_index: 0,
        },
    );
    let saved = buffer.serialize();
    buffer.weak_viewport_settings.master_transform = Transform::from_scale(7.0, 7.0)
        .post_translate(-700.0, 400.0)
        .into();
    assert_eq!(buffer.serialize(), saved);
    let restored = Buffer::new(&saved);
    assert_eq!(restored.weak_images, buffer.weak_images);
    assert_eq!(restored.weak_path_pressures, buffer.weak_path_pressures);
}

#[test]
fn element_edit_roundtrips_without_double_transforming_or_rescaling_strokes() {
    let mut buffer = Buffer::new(&drawing("M 20 30 L 40 50", ""));
    buffer
        .elements
        .values_mut()
        .next()
        .unwrap()
        .transform(Transform::from_scale(2.0, 3.0).post_translate(10.0, 20.0));
    let expected = bounds(&buffer);
    let first = Buffer::new(&buffer.serialize());
    let second = Buffer::new(&first.serialize());
    assert_eq!(bounds(&first), expected);
    assert_eq!(bounds(&second), expected);
    assert_eq!(path(&second).stroke.unwrap().width, 5.0);
}

#[test]
fn reload_detects_changed_points_even_when_path_length_is_unchanged() {
    let base = Buffer::new(&drawing("M 20 30 L 40 50", ""));
    let mut local = base.clone();
    local.weak_viewport_settings.master_transform = Transform::from_scale(4.0, 4.0)
        .post_translate(100.0, 200.0)
        .into();
    let remote = Buffer::new(&drawing("M 30 40 L 50 60", ""));
    Buffer::reload(
        &mut local.elements,
        &mut local.weak_images,
        &mut local.weak_path_pressures,
        &base,
        &remote,
    );
    assert_eq!(bounds(&local), bounds(&remote));
    assert!(path(&local).diff_state.data_changed);
    assert_eq!(Transform::from(local.weak_viewport_settings.master_transform).sx, 4.0);
}

#[test]
fn reload_keeps_unrelated_local_edits() {
    let base = Buffer::new(&drawing("M 20 30 L 40 50", ""));
    let mut local = base.clone();
    local
        .elements
        .values_mut()
        .next()
        .unwrap()
        .transform(Transform::from_translate(7.0, 8.0));
    let expected = bounds(&local);
    Buffer::reload(
        &mut local.elements,
        &mut local.weak_images,
        &mut local.weak_path_pressures,
        &base,
        &base,
    );
    assert_eq!(bounds(&local), expected);
}
