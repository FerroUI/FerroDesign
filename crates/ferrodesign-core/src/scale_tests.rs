use super::*;
use crate::{
    AutoLayout, Color, Document, Effect, LayoutAxis, LayoutGrid, LayoutGridAxis, Padding, Stroke,
    TextParagraph, TextRun, VectorNetwork,
};

fn near(actual: Point, expected: Point) {
    assert!(
        actual.distance(expected) < 1.0e-8,
        "{actual:?} != {expected:?}"
    );
}

fn world(document: &Document, id: NodeId, local: Point) -> Point {
    document.world_transform(id).unwrap().apply(local)
}

#[test]
fn previews_scale_heterogeneous_affine_subtrees_about_one_world_pivot_without_drift() {
    let mut d = Document::new("Scale");
    let page = d.pages[0].id;
    let mut owner = Node::new(NodeKind::Frame, "Owner", 80.0, -35.0, 200.0, 150.0);
    owner.rotation = 28.0;
    owner.affine_transform = Some(Transform {
        a: 1.5,
        b: 0.25,
        c: 0.3,
        d: -0.8,
        e: 7.0,
        f: -3.0,
    });
    let owner = d.insert_node(page, owner).unwrap();
    let mut frame = Node::new(NodeKind::Frame, "Frame", -17.0, 36.0, 100.0, 50.0);
    frame.rotation = -19.0;
    frame.flip_x = true;
    frame.affine_transform = Some(Transform::translation(2.0, 5.0));
    frame.corner_radii = Some([2.0, 4.0, 6.0, 8.0]);
    frame.layout = Some(AutoLayout {
        axis: LayoutAxis::Grid,
        column_tracks: vec![GridTrack::Fixed(10.0), GridTrack::Fraction(2.0)],
        padding: Padding::symmetric(4.0, 6.0),
        ..AutoLayout::default()
    });
    frame.layout_grid = Some(LayoutGrid {
        size: 8.0,
        pattern: LayoutGridPattern::Rows {
            axis: LayoutGridAxis {
                gutter: 6.0,
                offset: 1.0,
                ..Default::default()
            },
        },
        ..Default::default()
    });
    let frame = d.insert_node(owner, frame).unwrap();
    let mut text = Node::new(NodeKind::Text, "Label", 4.0, 7.0, 60.0, 20.0).with_text(
        "Hello",
        12.0,
        Color::BLACK,
    );
    text.text_runs.push(TextRun {
        start: 0,
        end: 5,
        font_size: Some(16.0),
        font_family: None,
        font_weight: None,
        color: None,
        italic: None,
        decoration: None,
    });
    text.text_paragraphs.push(TextParagraph {
        start: 0,
        end: 5,
        line_height: Some(1.5),
        indent: Some(3.0),
        ..Default::default()
    });
    let child = d.insert_node(frame, text).unwrap();
    let mut other = Node::new(NodeKind::Line, "Other", -50.0, 40.0, 70.0, 0.0);
    other.strokes = vec![Stroke {
        dash: vec![3.0, 2.0],
        ..Stroke::new(Color::BLACK, 2.0)
    }];
    other.effects = vec![Effect::shadow(Color::BLACK, Point::new(3.0, -5.0), 4.0)];
    let other = d.insert_node(page, other).unwrap();

    let original = d.to_json().unwrap();
    let pivot = Point::new(41.0, -17.0);
    let landmarks = [
        (frame, Point::new(23.0, 41.0)),
        (child, Point::new(17.0, 6.0)),
        (other, Point::new(70.0, 0.0)),
    ]
    .map(|(id, p)| (id, p, world(&d, id, p)));
    let mut editor = Editor::new(d);
    editor.selection = vec![frame, child, other];
    let mut gesture = editor.begin_scale_selection().unwrap();
    assert_eq!(Some(gesture.bounds()), editor.selection_scale_bounds());
    for factor in [0.5, 3.0, 1.5, 2.0] {
        assert!(gesture.preview(&mut editor, pivot, factor));
        for (id, local, before) in landmarks {
            near(
                world(&editor.document, id, local * factor),
                pivot + (before - pivot) * factor,
            );
        }
    }
    assert!(gesture.commit(&mut editor));
    assert_eq!(editor.history_len(), 1);
    let f = &editor.document.nodes[&frame];
    let layout = f.layout.as_ref().unwrap();
    assert_eq!(
        layout.column_tracks,
        [GridTrack::Fixed(20.0), GridTrack::Fraction(2.0)]
    );
    assert_eq!(layout.padding, Padding::symmetric(8.0, 12.0));
    assert_eq!(f.corner_radii, Some([4.0, 8.0, 12.0, 16.0]));
    let grid = f.layout_grid.unwrap();
    assert_eq!(grid.size, 16.0);
    assert_eq!(grid.pattern.axis().unwrap().gutter, 12.0);
    let text = &editor.document.nodes[&child];
    assert_eq!(text.font_size, 24.0);
    assert_eq!(text.text_runs[0].font_size, Some(32.0));
    assert_eq!(text.text_paragraphs[0].line_height, Some(1.5));
    assert_eq!(text.text_paragraphs[0].indent, Some(6.0));
    let other = &editor.document.nodes[&other];
    assert_eq!(other.strokes[0].width, 4.0);
    assert_eq!(other.strokes[0].dash, [6.0, 4.0]);
    assert_eq!(other.effects[0].offset, Point::new(6.0, -10.0));
    assert_eq!(other.effects[0].radius, 8.0);
    let scaled = editor.document.to_json().unwrap();
    assert!(editor.undo());
    assert_eq!(editor.document.to_json().unwrap(), original);
    assert!(editor.redo());
    assert_eq!(editor.document.to_json().unwrap(), scaled);
}

#[test]
fn scaling_linked_instances_survives_master_sync_without_editing_locked_or_override_layers() {
    let mut d = Document::new("Instance scale");
    let page = d.pages[0].id;
    let master = d
        .insert_node(
            page,
            Node::new(NodeKind::Component, "Master", 10.0, 10.0, 60.0, 80.0),
        )
        .unwrap();
    let source = d
        .insert_node(
            master,
            Node::new(NodeKind::Rectangle, "Part", 3.0, 5.0, 4.0, 9.0),
        )
        .unwrap();
    let mut editor = Editor::new(d);
    let instance = editor.create_instance(master, 200.0, 80.0).unwrap();
    let child = editor.document.nodes[&instance].children[0];
    editor.selection = vec![child];
    assert!(editor.begin_scale_selection().is_none());
    editor.document.nodes.get_mut(&instance).unwrap().locked = true;
    editor.refresh();
    editor.selection = vec![instance];
    assert!(editor.begin_scale_selection().is_none());
    editor.document.nodes.get_mut(&instance).unwrap().locked = false;
    editor.document.nodes.get_mut(&instance).unwrap().flip_x = true;
    editor.document.nodes.get_mut(&instance).unwrap().rotation = 21.0;
    editor.refresh();
    let p = Point::new(3.0, 4.0);
    let from = world(&editor.document, child, p);
    let pivot = Point::new(15.0, 7.0);
    assert!(editor.scale_selection(pivot, 1.8));
    let target = pivot + (from - pivot) * 1.8;
    near(world(&editor.document, child, p), target);
    assert_eq!(editor.document.nodes[&source].width, 4.0);
    assert_eq!(editor.document.nodes[&child].width, 4.0);
    editor.update_node(source, "Rename", |n| n.name = "Updated".into());
    editor.sync_instances(master);
    near(world(&editor.document, child, p), target);
    assert_eq!(editor.document.nodes[&child].name, "Updated");
}

#[test]
fn vector_network_scale_preserves_topology_and_invalid_preview_leaves_every_layer_unchanged() {
    let mut d = Document::new("Network scale");
    let page = d.pages[0].id;
    let mut node = Node::new(NodeKind::Vector, "Curve", 25.0, 30.0, 90.0, 40.0);
    let mut network = VectorNetwork::default();
    let a = network.add_vertex(Point::new(1.0, 2.0)).unwrap();
    let b = network.add_vertex(Point::new(88.0, 38.0)).unwrap();
    let edge = network
        .connect_curve(
            a,
            b,
            NetworkCurve::Cubic {
                control1: Point::new(5.0, 16.0),
                control2: Point::new(13.0, -2.0),
            },
        )
        .unwrap();
    node.vector_network = Some(network);
    let id = d.insert_node(page, node).unwrap();
    let mut blur = Node::new(NodeKind::Rectangle, "Blur", 2.0, 4.0, 4.0, 7.0);
    blur.effects.push(Effect::shadow(
        Color::BLACK,
        Point::default(),
        MAX_EFFECT_RADIUS / 3.0,
    ));
    let blur = d.insert_node(page, blur).unwrap();
    let mut editor = Editor::new(d);
    let original = editor.document.to_json().unwrap();
    editor.selection = vec![id, blur];
    let mut g = editor.begin_scale_selection().unwrap();
    assert!(g.preview(&mut editor, Point::default(), 2.0));
    let net = editor.document.nodes[&id].vector_network.as_ref().unwrap();
    assert_eq!(net.vertices[&a].position, Point::new(2.0, 4.0));
    assert_eq!(
        net.edges[&edge].curve,
        NetworkCurve::Cubic {
            control1: Point::new(10.0, 32.0),
            control2: Point::new(26.0, -4.0)
        }
    );
    let preview = editor.document.to_json().unwrap();
    for f in [4.0, 0.0, -1.0, f64::INFINITY, f64::NAN] {
        assert!(!g.preview(&mut editor, Point::default(), f));
        assert_eq!(editor.document.to_json().unwrap(), preview);
    }
    assert!(g.preview(&mut editor, Point::new(3e20, -5e34), 1.0));
    assert!(!g.commit(&mut editor));
    assert_eq!(editor.document.to_json().unwrap(), original);
    assert_eq!(editor.history_len(), 0);
}

#[test]
fn stale_gesture_cannot_cancel_or_commit_another_transaction() {
    let mut editor = Editor::new(Document::new("Scoped"));
    let id = editor.add_node(NodeKind::Rectangle, 0.0, 0.0, 50.0, 40.0);
    let original = editor.document.to_json().unwrap();
    let mut old = editor.begin_scale_selection().unwrap();
    assert!(old.preview(&mut editor, Point::default(), 2.0));
    old.clone().cancel(&mut editor);
    assert_eq!(editor.document.to_json().unwrap(), original);
    editor.begin_node_transaction(&[id], "Move");
    assert!(editor.begin_scale_selection().is_none());
    assert!(!old.preview(&mut editor, Point::default(), 0.5));
    assert!(!old.clone().commit(&mut editor));
    old.cancel(&mut editor);
    assert!(editor.has_active_transaction());
    editor.cancel_transaction();
}

#[test]
fn scaling_preserves_tiled_image_period_for_fills_strokes_and_network_faces() {
    use crate::{Fill, ImageScale, NetworkEdgeRef, Paint};

    let mut document = Document::new("Tile scale");
    let page = document.pages[0].id;
    let crop = Some(Rect::new(0.0, 0.0, 0.5, 1.0));
    let paint = |scale| {
        Paint::Image {
        href: "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aGH0AAAAASUVORK5CYII=".into(),
        scale, crop,
    }
    };
    let tile = Fill {
        paint: paint(ImageScale::Tile),
        visible: true,
        opacity: 1.0,
    };
    let mut shape = Node::new(NodeKind::Rectangle, "Tiled fill", 12.0, 16.0, 40.0, 30.0);
    shape.fills = vec![tile.clone()];
    let fill = document.insert_node(page, shape).unwrap();
    let mut line = Node::new(NodeKind::Line, "Tiled stroke", 80.0, 16.0, 50.0, 0.0);
    line.strokes[0].paint = tile.paint.clone();
    let stroke = document.insert_node(page, line).unwrap();
    let mut face = Node::new(NodeKind::Vector, "Tiled face", 120.0, 16.0, 40.0, 30.0);
    let mut network = VectorNetwork::default();
    let [a, b, c] = [
        Point::new(0.0, 0.0),
        Point::new(40.0, 0.0),
        Point::new(20.0, 30.0),
    ]
    .map(|p| network.add_vertex(p).unwrap());
    let edges = [(a, b), (b, c), (c, a)]
        .map(|(a, b)| NetworkEdgeRef::forward(network.connect(a, b).unwrap()));
    let region = network.add_region(vec![edges.to_vec()]).unwrap();
    network.regions.get_mut(&region).unwrap().fills = Some(vec![tile]);
    face.vector_network = Some(network);
    let face = document.insert_node(page, face).unwrap();
    let others = [ImageScale::Fill, ImageScale::Fit, ImageScale::Stretch].map(|mode| {
        let mut relative = Node::new(NodeKind::Rectangle, "Normalized", 0.0, 0.0, 40.0, 30.0);
        relative.fills = vec![Fill {
            paint: paint(mode),
            visible: true,
            opacity: 1.0,
        }];
        document.insert_node(page, relative).unwrap()
    });
    let mut editor = Editor::new(document);
    editor.selection = [fill, stroke, face].into_iter().chain(others).collect();
    for factor in [2.0, 0.25] {
        let before = [fill, stroke, face].map(|id| {
            editor
                .document
                .world_transform(id)
                .unwrap()
                .apply_vector(Point::new(0.5, 1.0))
        });
        assert!(editor.scale_selection(Point::new(10.0, 20.0), factor));
        for (id, period) in [fill, stroke, face].into_iter().zip(before) {
            let transformed = editor
                .document
                .world_transform(id)
                .unwrap()
                .apply_vector(Point::new(0.5, 1.0));
            near(transformed, period * factor);
        }
    }
    for id in others {
        let node = &editor.document.nodes[&id];
        assert_eq!((node.width, node.height), (20.0, 15.0));
        assert!(node.affine_transform.is_none());
        assert!(matches!(&node.fills[0].paint, Paint::Image{ crop:c, .. } if *c==crop));
    }
    editor.document.validate().unwrap();
}
