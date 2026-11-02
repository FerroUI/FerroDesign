//! Interactive retained-mode surface. Artwork uses only FerroUI's public DrawingContext API.
mod drawing;
mod drag_copy;
mod measure;
mod resize;
mod rotation;
mod selection_resize;
mod line_endpoint;
mod network;
mod network_bucket;
mod grids;
mod rulers;
mod image_crop;
mod layout_drag;
mod selection;
mod input_observations;
mod vector_width;
pub(crate) mod scale;
mod corner_radius;
mod slices;
mod gradient_edit;
use crate::{
    icons::brush,
    renderer::{RenderStats, ScenePainter},
};
use ferrodesign_core::{
    self as core, Color, Editor, Node, NodeId, NodeKind, PathPointId, PathPointKind, PathSegment,
    Point, Rect, VectorPath, Viewport,
};
use ferroui_base::input::{
    FocusChangedEventArgs, InputElement, InputElementImpl, InputElementImplExt, Key, KeyEventArgs, KeyModifiers,
    PointerCaptureLostEventArgs, PointerEventArgs, PointerPressedEventArgs,
    PointerReleasedEventArgs, PointerWheelEventArgs,
};
use ferroui_base::interactivity::InteractiveImpl;
use ferroui_base::layout::LayoutableImpl;
use ferroui_base::media::immutable::ImmutablePen;
use ferroui_base::media::{
    BoxShadows, DrawingContext, FlowDirection, FormattedText, IPen, StreamGeometry, Typeface,
};
use ferroui_base::platform::IGeometryContext;
use ferroui_base::threading::{Dispatcher, DispatcherPriority};
use ferroui_base::utilities::CultureInfo;
use ferroui_base::{
    ferro_class, ferro_impl_classes, instantiate, FerroObjectImpl, Matrix, Point as UiPoint,
    Rect as UiRect, Ref, StyledElementImpl, VisualImpl,
};
use ferroui_controls::{Control, ControlImpl, TopLevel};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Select,
    Scale,
    Frame,
    Rectangle,
    Ellipse,
    Line,
    Arrow,
    Polygon,
    Star,
    Pen,
    Pencil,
    Text,
    Hand,
    Comment,
    Section,
    Slice,
}
impl Tool {
    pub fn name(self) -> &'static str {
        match self {
            Self::Select => "Select",
            Self::Scale => "Scale",
            Self::Frame => "Frame",
            Self::Rectangle => "Rectangle",
            Self::Ellipse => "Ellipse",
            Self::Line => "Line",
            Self::Arrow => "Arrow",
            Self::Polygon => "Polygon",
            Self::Star => "Star",
            Self::Pen => "Pen",
            Self::Pencil => "Pencil",
            Self::Text => "Text",
            Self::Hand => "Hand",
            Self::Comment => "Comment",
            Self::Section => "Section",
            Self::Slice => "Slice",
        }
    }
    fn kind(self) -> NodeKind {
        match self {
            Self::Frame => NodeKind::Frame,
            Self::Ellipse => NodeKind::Ellipse,
            Self::Line | Self::Arrow => NodeKind::Line,
            Self::Polygon => NodeKind::Polygon,
            Self::Star => NodeKind::Star,
            Self::Pen | Self::Pencil => NodeKind::Vector,
            Self::Text => NodeKind::Text,
            Self::Section => NodeKind::Section,
            Self::Slice => NodeKind::Slice,
            _ => NodeKind::Rectangle,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VectorTool { #[default] Point, Bend, Erase, Connect, Bucket }

#[derive(Clone)]
pub enum CanvasEvent {
    Selection,
    Document,
    Draft,
    ResizePreview,
    Camera,
    Tool,
    EditText(NodeId),
    Comment(Point),
    ContextMenu,
    Notice(String),
    PrototypeTrigger(NodeId, core::Trigger),
    PrototypeKey(String),
    PrototypeScroll(NodeId, Point),
    PrototypePointerMoved(Point),
    PrototypePointerExited,
    ExitPresentation,
    Action(&'static str),
}
enum Gesture {
    None,
    Pan { previous: Point },
    Move {
        start: Point,
        nodes: Vec<Node>,
        bounds: Option<Rect>,
        exclude: Vec<NodeId>,
        moved: bool,
    },
    Scale(scale::ScaleDrag),
    EdgePress { hit: resize::Hit, world: Point, screen: Point, modifiers: KeyModifiers, clicks: i32 },
    Resize(resize::ResizeDrag),
    MultiResize(selection_resize::MultiResize),
    Rotate(rotation::Drag),
    RotationOrigin(rotation::OriginDrag),
    Marquee(selection::Marquee),
    AltDrag(Box<drag_copy::AltDrag>),
    Draw(drawing::ShapeDrag),
    LineEndpoint(line_endpoint::EndpointDrag),
    Pencil {
        id: NodeId,
        origin: Point,
    },
    PenHandle {
        id: NodeId,
        segment: usize,
        anchor: Point,
        start: Point,
    },
    NetworkPoint { node: NodeId, point: core::NetworkPointId, start: Point, original: Point },
    NetworkBend { node: NodeId, edge: core::VectorEdgeId, t: f64 },
    VectorBend { node: NodeId, segment: usize, t: f64 },
    VectorPoint {
        node: NodeId,
        point: PathPointId,
        start: Point,
        original: Point,
    },
}
#[repr(C)]
pub struct DesignCanvas {
    base: Control,
    pub editor: Rc<RefCell<Editor>>,
    pub viewport: Cell<Viewport>,
    pub tool: Cell<Tool>,
    gesture: RefCell<Gesture>,
    manipulation_cursor: Cell<Option<ferroui_base::input::StandardCursorType>>,
    hovered_resize_edge: Cell<Option<(NodeId, usize)>>,
    size_badge: RefCell<Option<(resize::BadgeValue, FormattedText)>>,
    size_badge_obstacle: Cell<Option<Rect>>,
    scale_bounds: RefCell<Option<scale::CachedBounds>>,
    rotation: rotation::RotationState,
    painter: RefCell<ScenePainter>,
    callback: RefCell<Option<Rc<dyn Fn(CanvasEvent)>>>,
    hover: Cell<Option<NodeId>>,
    measurements: measure::Measurements,
    layer_preview: Cell<Option<NodeId>>,
    prototype_press: Cell<Option<(NodeId, Point, bool)>>,
    space: Cell<bool>,
    initialized: Cell<bool>,
    pub show_grid: Cell<bool>,
    pub show_rulers: Cell<bool>,
    rulers: RefCell<rulers::Rulers>,
    pub dark: Cell<bool>,
    pub presentation: Cell<Option<NodeId>>,
    pub overlays: RefCell<Vec<(NodeId, Point)>>,
    pub prototype_scene: RefCell<Option<core::PrototypeScene>>,
    pub frames: Cell<u64>,
    pub snap: Cell<bool>,
    snap_guides: Cell<(Option<f64>, Option<f64>)>,
    pen_node: Cell<Option<NodeId>>,
    creation_parent: Cell<Option<(NodeId, NodeId)>>,
    pen_tangent: Cell<Option<Point>>,
    pub vector_node: Cell<Option<NodeId>>,
    pub vector_point: Cell<Option<PathPointId>>,
    pub vector_tool: Cell<VectorTool>,
    pub network_point: Cell<Option<core::NetworkPointId>>,
    pub network_vertices: RefCell<Vec<core::VectorVertexId>>,
    pub network_region: Cell<Option<core::VectorRegionId>>,
    network_bucket: RefCell<network_bucket::BucketState>,
    layout_grids: RefCell<grids::LayoutGridOverlay>,
    image_crop: Cell<Option<image_crop::ImageCropMode>>,
    image_crop_gesture: RefCell<Option<image_crop::ImageCropGesture>>,
    layout_drag: RefCell<Option<layout_drag::LayoutDragGesture>>,
    free_layout_probe: RefCell<Option<layout_drag::FreeLayoutProbe>>,
    pub select_groups: Cell<bool>,
    selection_context: RefCell<Option<selection::CachedSelectionContext>>,
    prototype_selection: RefCell<Option<selection::CachedPrototypeSelection>>,
    selection_pointer: Cell<Option<Point>>,
    pointer_presses: RefCell<input_observations::PointerPresses>,
    stroke_width: RefCell<vector_width::WidthState>,
    corner_radius: RefCell<corner_radius::CornerRadiusState>,
    linear_gradient: RefCell<gradient_edit::GradientState>,
}
ferro_class!(DesignCanvas:Control);
ferro_impl_classes!(DesignCanvas:FerroObjectImpl,StyledElementImpl,LayoutableImpl,InteractiveImpl,ControlImpl);
impl DesignCanvas {
    pub fn new(editor: Rc<RefCell<Editor>>) -> Ref<Self> {
        let this = instantiate(Self {
            base: Control::construct(),
            editor,
            viewport: Cell::new(Viewport::default()),
            tool: Cell::new(Tool::Select),
            gesture: RefCell::new(Gesture::None),
            manipulation_cursor: Cell::new(None),
            hovered_resize_edge: Cell::new(None),
            size_badge: RefCell::default(),
            size_badge_obstacle: Cell::default(),
            scale_bounds: RefCell::default(),
            rotation: Default::default(),
            painter: RefCell::new(ScenePainter::default()),
            callback: RefCell::new(None),
            hover: Cell::new(None),
            measurements: Default::default(),
            layer_preview: Cell::new(None),
            prototype_press: Cell::new(None),
            space: Cell::new(false),
            initialized: Cell::new(false),
            show_grid: Cell::new(false),
            show_rulers: Cell::new(false),
            rulers: RefCell::default(),
            dark: Cell::new(false),
            presentation: Cell::new(None),
            overlays: RefCell::new(Vec::new()),
            prototype_scene: RefCell::new(None),
            frames: Cell::new(0),
            snap: Cell::new(true),
            snap_guides: Cell::new((None, None)),
            pen_node: Cell::new(None),
            creation_parent: Cell::new(None),
            pen_tangent: Cell::new(None),
            vector_node: Cell::new(None),
            vector_point: Cell::new(None),
            vector_tool: Cell::new(VectorTool::Point),
            network_point: Cell::new(None),
            network_vertices: RefCell::new(Vec::new()),
            network_region: Cell::new(None),
            network_bucket: RefCell::new(network_bucket::BucketState::default()),
            layout_grids: RefCell::new(grids::LayoutGridOverlay::default()),
            image_crop: Cell::new(None),
            image_crop_gesture: RefCell::new(None),
            layout_drag: RefCell::new(None),
            free_layout_probe: RefCell::new(None),
            select_groups: Cell::new(false),
            selection_context: RefCell::new(None),
            prototype_selection: RefCell::new(None),
            selection_pointer: Cell::new(None),
            pointer_presses: RefCell::new(input_observations::PointerPresses::default()),
            stroke_width: RefCell::new(vector_width::WidthState::default()),
            corner_radius: RefCell::new(corner_radius::CornerRadiusState::default()),
            linear_gradient: RefCell::new(gradient_edit::GradientState::default()),
        });
        this.set_focusable(true);
        this.set_clip_to_bounds(true);
        this
    }
    pub fn on_change(&self, callback: impl Fn(CanvasEvent) + 'static) {
        *self.callback.borrow_mut() = Some(Rc::new(callback));
    }
    /// Bounded observations of real routed input; reading this never changes
    /// selection, focus, capture, history, layout or the render schedule.
    pub fn pointer_press_observations(&self) -> serde_json::Value {
        self.pointer_presses.borrow().json()
    }
    fn emit(&self, event: CanvasEvent) {
        let callback = self.callback.borrow().clone();
        if let Some(f) = callback {
            f(event)
        }
    }
    fn prototype_pick(&self, world: Point) -> Option<NodeId> {
        if self.prototype_scene.borrow().is_some() { return None; }
        let editor = self.editor.borrow();
        let current = self.presentation.get()?;
        let mut point = world;
        let mut root = current;
        if let Some(&(overlay, position)) = self.overlays.borrow().last() {
            // The top overlay owns input, including its empty regions.
            let current_bounds = editor.document.world_bounds(current)?;
            let bounds = editor.document.world_bounds(overlay)?;
            point = Point::new(
                world.x - current_bounds.x - position.x + bounds.x,
                world.y - current_bounds.y - position.y + bounds.y,
            );
            root = overlay;
        }
        self.prototype_pick_in_root(&editor, root, point)
    }
    /// Activation changes cannot reuse a press or hover from an earlier frame.
    pub fn reset_presentation_input(&self) {
        self.clear_measurement();
        self.set_resize_hover(None);
        self.leave_linear_gradient_edit();
        self.cancel_corner_radius_gesture();
        self.clear_corner_radius_hover();
        self.leave_stroke_width_edit();
        self.cancel_layout_drag();
        self.prototype_press.set(None);
        self.hover.set(None);
        self.space.set(false);
    }
    pub(crate) fn preview_layer(&self, id: Option<NodeId>) {
        if self.layer_preview.replace(id) != id {
            self.invalidate_visual();
        }
    }

    pub(crate) fn end_layer_preview(&self, id: NodeId) {
        if self.layer_preview.get() == Some(id) {
            self.preview_layer(None);
        }
    }

    pub(crate) fn highlighted_layer(&self) -> Option<NodeId> {
        self.layer_preview.get().or(self.hover.get())
    }

    pub fn set_tool(&self, tool: Tool) {
        self.forget_selected_ruler();
        self.clear_measurement();
        self.leave_linear_gradient_edit();
        self.cancel_corner_radius_gesture();
        self.clear_corner_radius_hover();
        self.leave_stroke_width_edit();
        self.cancel_network_bucket_gesture();
        self.clear_bucket_hover();
        self.cancel_layout_drag();
        self.finish_image_crop();
        self.finish_pen();
        if self.tool.get() != tool && !matches!(*self.gesture.borrow(), Gesture::None) {
            self.cancel_active_edit();
        }
        if tool != Tool::Select {
            self.leave_vector_edit();
        }
        self.set_resize_hover(None);
        self.tool.set(tool);
        self.hover.set(None);
        self.focus();
        self.invalidate_visual();
        self.emit(CanvasEvent::Tool);
    }
    pub(crate) fn has_pointer_gesture(&self) -> bool {
        self.guide_drag_active() || !matches!(*self.gesture.borrow(), Gesture::None)
    }

    pub(crate) fn editor_shortcut(&self, e: &KeyEventArgs) -> Option<&'static str> {
        let command = e.key_modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::META);
        let shift = e.key_modifiers.contains(KeyModifiers::SHIFT);
        let alt = e.key_modifiers.contains(KeyModifiers::ALT);
        if command {
            match e.key {
                Key::Add | Key::OemPlus if !alt => Some("zoom-in"),
                Key::Subtract | Key::OemMinus if !alt => Some("zoom-out"),
                Key::Z => Some(if shift { "redo" } else { "undo" }),
                Key::Y => Some("redo"),
                Key::C => Some("copy"),
                Key::X => Some("cut"),
                Key::V => Some("paste"),
                Key::D => Some("duplicate"),
                Key::A => Some(if shift { "select-inverse" } else { "select-all" }),
                Key::G => Some(if alt {
                    "frame-selection"
                } else if shift {
                    "ungroup"
                } else {
                    "group"
                }),
                Key::S => Some("save"),
                Key::O => Some("open"),
                Key::F if !alt && !shift => Some("find-layers"),
                Key::R if !alt && !shift => Some("rename-layer"),
                Key::OemQuestion if !alt && !shift => Some("actions"),
                Key::K => Some(if shift {
                    "place-image"
                } else if alt {
                    "component"
                } else {
                    "actions"
                }),
                _ => None,
            }
        } else {
            match e.key {
                Key::Delete | Key::Back => Some("delete"),
                Key::F2 => Some("rename-layer"),
                Key::V => Some("select"),
                Key::F => Some(if shift { "section" } else { "frame" }),
                Key::R if !alt => Some(if shift { "rulers" } else { "rectangle" }),
                Key::O => Some("ellipse"),
                Key::L => Some(if shift { "arrow" } else { "line" }),
                Key::P => Some(if shift { "pencil" } else { "pen" }),
                Key::B if shift && self.vector_node.get().is_some() => Some("network-bucket"),
                Key::T => Some("text"),
                Key::H => Some("hand"),
                Key::C => Some("comment"),
                Key::A if shift => Some("auto-layout"),
                Key::S => Some(if shift { "section" } else { "slice" }),
                Key::Add | Key::OemPlus if !alt => Some("zoom-in"),
                Key::Subtract | Key::OemMinus if !alt => Some("zoom-out"),
                Key::D0 if shift && !alt => Some("actual-size"),
                Key::D1 => Some("fit"),
                Key::D2 => Some("fit-selection"),
                Key::K if !shift && !alt => Some("scale"),
                _ => None,
            }
        }
    }

    pub(crate) fn page_camera(&self) -> Option<Viewport> {
        self.initialized.get().then(|| self.viewport.get())
    }

    pub(crate) fn restore_page_camera(&self, camera: Option<Viewport>) {
        self.reset_fit();
        let current = self.viewport.get();
        let mut restored = camera.unwrap_or_default();
        if camera.is_some() {
            // After a window or phone rotation, keep the same world point at center.
            restored.pan.x += (current.width - restored.width) * 0.5;
            restored.pan.y += (current.height - restored.height) * 0.5;
            self.initialized.set(true);
        }
        restored.width = current.width;
        restored.height = current.height;
        self.viewport.set(restored);
    }

    pub fn reset_fit(&self) {
        self.clear_measurement();
        self.cancel_linear_gradient_gesture();
        self.clear_gradient_hover();
        self.cancel_corner_radius_gesture();
        self.clear_corner_radius_hover();
        self.cancel_layout_drag();
        self.initialized.set(false);
        self.invalidate_visual();
    }
    pub fn set_zoom(&self, zoom: f64) {
        self.cancel_linear_gradient_gesture();
        self.clear_gradient_hover();
        self.cancel_corner_radius_gesture();
        self.clear_corner_radius_hover();
        self.cancel_layout_drag();
        let mut v = self.viewport.get();
        v.zoom_at(Point::new(v.width / 2.0, v.height / 2.0), zoom);
        self.update_camera(v);
    }
    pub fn zoom(&self, factor: f64) {
        self.set_zoom(self.viewport.get().zoom * factor);
    }
    pub(crate) fn update_camera(&self, viewport: Viewport) {
        if self.viewport.replace(viewport) != viewport {
            self.clear_measurement();
            self.set_resize_hover(None);
            self.invalidate_visual();
            self.emit(CanvasEvent::Camera);
        }
    }
    // Pointer movement and a modifier change at a stationary pointer must agree
    // on the target. Geometry handles own their hit area before layer measurement.
    fn hover_edit_handle(&self, world: Point, modifiers: KeyModifiers) -> bool {
        if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::META) && self.hover_rotation(world) { return true; }
        if !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::META) && self.multi_resize_hover(world) { return true; }
        let resize = (!modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::META))
            .then(|| self.handle_at(world)).flatten();
        self.set_resize_hover(resize);
        if resize.is_some() {
            self.clear_corner_radius_hover();
        } else if !self.corner_radius_hover(world, modifiers) {
            return false;
        }
        self.clear_measurement();
        if self.hover.take().is_some() { self.invalidate_visual(); }
        true
    }
    fn pan_pointer(&self, screen: Point) -> bool {
        // A captured pan is independent of the document. In particular, do not
        // run vector/gradient hover probes while dragging over their handles.
        let delta = match &mut *self.gesture.borrow_mut() {
            Gesture::Pan { previous } => screen - std::mem::replace(previous, screen),
            _ => return false,
        };
        let mut viewport = self.viewport.get();
        viewport.pan = viewport.pan + delta;
        self.update_camera(viewport);
        true
    }
    pub fn zoom_to_selection(&self) {
        self.clear_measurement();
        self.cancel_linear_gradient_gesture();
        self.clear_gradient_hover();
        self.cancel_corner_radius_gesture();
        self.clear_corner_radius_hover();
        self.cancel_layout_drag();
        let editor = self.editor.borrow();
        let bounds = editor
            .selection
            .iter()
            .filter_map(|&id| editor.document.world_bounds(id))
            .reduce(|a, b| a.union(b));
        if let Some(bounds) = bounds {
            let mut v = self.viewport.get();
            v.fit(bounds, 90.0);
            self.viewport.set(v);
            self.invalidate_visual();
        }
        drop(editor);
        self.emit(CanvasEvent::Camera);
    }
    pub fn render_stats(&self) -> RenderStats {
        self.painter.borrow().stats()
    }
    pub fn register_image_data(&self, href: &str) -> std::io::Result<(f64, f64)> {
        self.painter.borrow_mut().register_image_data(href)
    }
    pub fn export_png(&self) -> Result<Vec<u8>, String> {
        let selection = self.editor.borrow().selection.clone();
        self.export_png_for(&selection)
    }

    pub fn export_png_for(&self, selection: &[NodeId]) -> Result<Vec<u8>, String> {
        use ferroui_base::media::imaging::{
            BitmapEncoderOptions, PngBitmapEncoderOptions, RenderTargetBitmap,
        };
        use ferroui_base::PixelSize;
        let editor = self.editor.borrow();
        let doc = &editor.document;
        let slice = doc.export_slice(editor.active_page, selection);
        if let [id] = selection {
            if doc.nodes.get(id).is_some_and(|node| node.kind == NodeKind::Slice) && slice.is_none() {
                return Err("This slice has empty or singular bounds.".into());
            }
        }
        let roots = if slice.is_some() || selection.is_empty() {
            doc.page(editor.active_page).map(|p| p.children.clone()).unwrap_or_default()
        } else {
            editor.root_selection_for(selection)
        };
        let bounds = match slice {
            Some(slice) => slice.local,
            None => self.painter.borrow_mut().rendered_editor_bounds(&editor, &roots)
                .ok_or_else(|| "There are no layers to export.".to_string())?,
        };
        let width = bounds.width.ceil().max(1.0);
        let height = bounds.height.ceil().max(1.0);
        if width > 16384.0 || height > 16384.0 || width * height > 32_000_000.0 {
            return Err("Select a smaller region; PNG export supports up to 32 megapixels.".into());
        }
        let bitmap = RenderTargetBitmap::new(PixelSize::new(width as i32, height as i32));
        {
            let mut context = bitmap.create_drawing_context();
            let clip = slice.map(|slice| context.push_clip(rect(slice.local)));
            let matrix = slice.map_or_else(|| Matrix::create_translation(-bounds.x, -bounds.y),
                |s| crate::renderer::ui_matrix(s.world_to_local));
            let transform = context.push_transform(matrix);
            let mut painter = self.painter.borrow_mut();
            // This export has one output pixel per document unit. Canvas zoom
            // and the window's display density must not affect the saved PNG.
            painter.set_raster_scale(1.0);
            painter.render_editor(&mut context, &editor, &roots, slice.map_or(bounds, |s| s.world_bounds));
            context.pop(transform);
            if let Some(clip) = clip { context.pop(clip); }
            context.dispose();
        }
        let mut bytes = Vec::new();
        let result = bitmap.save(
            &mut bytes,
            &BitmapEncoderOptions::Png(PngBitmapEncoderOptions::default()),
        );
        bitmap.dispose();
        result.map_err(|e| e.to_string())?;
        Ok(bytes)
    }
    pub fn invalidate_scene(&self) {
        self.painter.borrow_mut().invalidate();
        self.invalidate_visual();
    }
    pub fn set_editing_text(&self, node: Option<NodeId>) {
        self.painter.borrow_mut().set_editing_text(node);
        self.invalidate_visual();
    }
    pub fn finish_pen(&self) {
        if matches!(*self.gesture.borrow(), Gesture::PenHandle { .. }) {
            *self.gesture.borrow_mut() = Gesture::None;
        }
        self.pen_tangent.set(None);
        if let Some(id) = self.pen_node.take() {
            let mut editor = self.editor.borrow_mut();
            normalize_drawn_vector(&mut editor, id);
            self.commit_created_layer(&mut editor, id);
            drop(editor);
            self.invalidate_visual();
            self.emit(CanvasEvent::Document);
        }
    }
    /// Undo during a live pointer edit cancels that transaction and its gesture.
    /// Later pointer events must never continue writing after history restored it.
    pub fn cancel_active_edit(&self) -> bool {
        if self.cancel_guide_drag() { return true; }
        if matches!(*self.gesture.borrow(), Gesture::Pan { .. }) {
            *self.gesture.borrow_mut() = Gesture::None;
            return true;
        }
        if self.cancel_linear_gradient_gesture() { return true; }
        if self.cancel_corner_radius_gesture() { return true; }
        if self.cancel_stroke_width_gesture() { return true; }
        if self.cancel_network_bucket_gesture() { return true; }
        if self.cancel_layout_drag() { return true; }
        if self.cancel_image_crop_gesture() { return true; }
        let active =
            self.pen_node.get().is_some() || !matches!(*self.gesture.borrow(), Gesture::None);
        if active {
            self.cancel();
        }
        active
    }
    pub fn cancel(&self) {
        if self.cancel_guide_drag() { return; }
        self.clear_measurement();
        if matches!(*self.gesture.borrow(), Gesture::Rotate(_) | Gesture::RotationOrigin(_)) {
            match self.gesture.replace(Gesture::None) {
                Gesture::Rotate(drag) => drag.cancel(self),
                Gesture::RotationOrigin(drag) => drag.cancel(self),
                _ => unreachable!(),
            }
            self.set_resize_hover(None);
            return;
        }
        if matches!(*self.gesture.borrow(), Gesture::MultiResize(_)) {
            if let Gesture::MultiResize(drag) = self.gesture.replace(Gesture::None) { drag.cancel(self); }
            return;
        }
        if matches!(*self.gesture.borrow(), Gesture::Marquee(_) | Gesture::AltDrag(_)) {
            *self.gesture.borrow_mut() = Gesture::None;
            self.set_resize_hover(None);
            self.snap_guides.set((None, None));
            self.layout_grids.borrow_mut().clear_snap();
            self.invalidate_visual();
            return;
        }
        if matches!(*self.gesture.borrow(), Gesture::EdgePress { .. }) {
            *self.gesture.borrow_mut() = Gesture::None;
            self.set_resize_hover(None);
            return;
        }
        if self.cancel_scale_drag() { return; }
        self.layout_grids.borrow_mut().clear_snap();
        if self.cancel_linear_gradient_gesture() { return; }
        self.clear_gradient_hover();
        if self.cancel_corner_radius_gesture() { return; }
        self.clear_corner_radius_hover();
        if self.cancel_stroke_width_gesture() { return; }
        if self.cancel_network_bucket_gesture() { return; }
        // This preview owns no transaction. In particular, Escape must not
        // cancel an unrelated edit opened after its immutable snapshot.
        if self.cancel_layout_drag() { return; }
        self.cancel_image_crop();
        self.editor.borrow_mut().cancel_transaction();
        self.creation_parent.set(None);
        self.pen_node.set(None);
        self.pen_tangent.set(None);
        *self.gesture.borrow_mut() = Gesture::None;
        self.snap_guides.set((None, None));
        self.space.set(false);
        self.invalidate_visual();
        self.emit(CanvasEvent::Document);
    }
    /// Enter direct path editing without replacing the retained document node.
    pub fn edit_vector(&self) -> bool {
        self.leave_linear_gradient_edit();
        self.cancel_corner_radius_gesture();
        self.clear_corner_radius_hover();
        self.leave_stroke_width_edit();
        self.cancel_network_bucket_gesture();
        self.cancel_layout_drag();
        self.finish_image_crop();
        self.finish_pen();
        let editor = self.editor.borrow();
        let id = editor.selection.first().copied().filter(|id| {
            editor.selection.len() == 1
                && editor.document.nodes.get(id).is_some_and(|n| {
                    n.kind == NodeKind::Vector && !n.locked && (!n.path.segments.is_empty() || n.vector_network.is_some())
                })
        });
        drop(editor);
        if let Some(id) = id {
            self.tool.set(Tool::Select);
            self.vector_node.set(Some(id));
            self.vector_point.set(None);
            self.focus();
            self.invalidate_visual();
            self.emit(CanvasEvent::Selection);
            true
        } else {
            false
        }
    }
    pub fn leave_vector_edit(&self) {
        self.cancel_network_bucket_gesture();
        self.clear_bucket_hover();
        self.vector_node.set(None);
        self.vector_point.set(None);
        self.network_region.set(None);
        self.network_point.set(None);
        self.network_vertices.borrow_mut().clear();
        self.invalidate_visual();
    }
    pub fn reconcile_vector_edit(&self) {
        self.reconcile_linear_gradient();
        self.reconcile_corner_radius();
        self.reconcile_stroke_width();
        self.reconcile_network_bucket();
        let editor = self.editor.borrow();
        let valid = self.vector_node.get().is_some_and(|id| {
            editor.selection.as_slice() == [id]
                && editor
                    .document
                    .nodes
                    .get(&id)
                    .is_some_and(|n| n.kind == NodeKind::Vector && !n.locked && n.visible)
        });
        if !valid {
            drop(editor);
            self.leave_vector_edit();
        } else if let Some(network) = self.vector_node.get().and_then(|id| editor.document.nodes.get(&id)).and_then(|n| n.vector_network.as_ref()) {
            if self.network_point.get().is_some_and(|point| network.point_position(point).is_none()) { self.network_point.set(None); }
            self.network_vertices.borrow_mut().retain(|vertex| network.vertices.contains_key(vertex));
        } else if let (Some(id), Some(point)) = (self.vector_node.get(), self.vector_point.get()) {
            if editor.document.nodes[&id]
                .path
                .point_position(point)
                .is_none()
            {
                self.vector_point.set(None);
            }
        }
    }
    pub fn convert_vector_point(&self, mode: core::AnchorMode) -> bool {
        let Some(id) = self.vector_node.get() else {
            return false;
        };
        if let Some(point) = self.network_point.get() {
            let vertex = self.editor.borrow().document.nodes.get(&id).and_then(|node| node.vector_network.as_ref()).and_then(|network| network.point_vertex(point));
            if let Some(vertex) = vertex {
                let changed = self.editor.borrow_mut().set_network_vertex_mode(id, vertex, mode);
                if changed { self.emit(CanvasEvent::Document); }
                return changed;
            }
        }
        let Some(point) = self.vector_point.get() else {
            return false;
        };
        let anchor = self
            .editor
            .borrow()
            .document
            .nodes
            .get(&id)
            .and_then(|n| n.path.edit_points().into_iter().find(|p| p.id == point))
            .map(|p| p.anchor_id);
        let Some(anchor) = anchor else {
            return false;
        };
        let changed = self
            .editor
            .borrow_mut()
            .convert_vector_anchor(id, anchor.segment, mode);
        self.vector_point.set(Some(anchor));
        if changed {
            self.invalidate_visual();
            self.emit(CanvasEvent::Document);
        }
        changed
    }
    fn vector_press(&self, world: Point, clicks: i32, additive: bool) -> bool {
        let Some(id) = self.vector_node.get() else {
            return false;
        };
        let editor = self.editor.borrow();
        let Some(node) = editor.document.nodes.get(&id) else {
            return false;
        };
        if node.vector_network.is_some() {
            drop(editor);
            return self.network_press(world, clicks, additive);
        }
        let Some(transform) = editor.document.world_transform(id) else {
            return false;
        };
        let viewport = self.viewport.get();
        let pointer = viewport.world_to_screen(world);
        if self.vector_tool.get() != VectorTool::Point {
            let hit = node.path.transformed(transform).nearest_segment(world)
                .filter(|hit| viewport.world_to_screen(hit.position).distance(pointer) <= 8.0);
            drop(editor);
            if let Some(hit) = hit {
                self.vector_point.set(None);
                if self.vector_tool.get() == VectorTool::Erase {
                    if self.editor.borrow_mut().erase_vector_segment(id, hit.segment) {
                        self.emit(CanvasEvent::Document);
                    }
                } else {
                    self.editor.borrow_mut().begin_node_transaction(&[id], "Bend vector segment");
                    *self.gesture.borrow_mut() = Gesture::VectorBend { node: id, segment: hit.segment, t: hit.t };
                }
                self.invalidate_visual();
            }
            return true;
        }
        let selected = self.vector_point.get();
        let points = node.path.edit_points();
        let active_anchor = selected
            .and_then(|id| points.iter().find(|p| p.id == id))
            .map(|p| p.anchor_id);
        // Hide unrelated handles and prioritize anchors where retracted handles overlap.
        let hit = points
            .iter()
            .filter(|p| p.id.kind == PathPointKind::Anchor || Some(p.anchor_id) == active_anchor)
            .filter_map(|p| {
                let distance = viewport
                    .world_to_screen(transform.apply(p.position))
                    .distance(pointer);
                (distance <= 8.0).then_some((p, distance))
            })
            .min_by(|(a, da), (b, db)| {
                da.total_cmp(db).then_with(|| {
                    (a.id.kind != PathPointKind::Anchor).cmp(&(b.id.kind != PathPointKind::Anchor))
                })
            })
            .map(|(p, _)| (p.id, transform.apply(p.position)));
        if let Some((point, original)) = hit {
            drop(editor);
            self.vector_point.set(Some(point));
            self.editor
                .borrow_mut()
                .begin_node_transaction(&[id], "Move vector point");
            *self.gesture.borrow_mut() = Gesture::VectorPoint {
                node: id,
                point,
                start: world,
                original,
            };
            self.invalidate_visual();
            self.emit(CanvasEvent::Selection);
            return true;
        }
        // Measure in world space so nonuniform scale and shear do not distort hit tolerance.
        let nearest = node
            .path
            .transformed(transform)
            .nearest_segment(world)
            .filter(|hit| viewport.world_to_screen(hit.position).distance(pointer) <= 7.0);
        drop(editor);
        if let Some(hit) = nearest {
            if clicks >= 2 {
                let added = self
                    .editor
                    .borrow_mut()
                    .split_vector_segment(id, hit.segment, hit.t);
                self.vector_point.set(added);
                self.emit(CanvasEvent::Document);
            } else {
                self.vector_point.set(None);
                self.emit(CanvasEvent::Selection);
            }
            self.invalidate_visual();
            return true;
        }
        self.leave_vector_edit();
        false
    }
    fn world(&self, p: UiPoint) -> Point {
        self.viewport.get().screen_to_world(Point::new(p.x, p.y))
    }
    fn snap_point(&self, p: Point) -> Point {
        if self.snap.get() {
            Point::new(p.x.round(), p.y.round())
        } else {
            p
        }
    }
    fn new_node(&self, tool: Tool, p: Point) -> Option<NodeId> {
        let destination = self.drawing_parent_at(p, tool);
        let mut editor = self.editor.borrow_mut();
        editor.begin_transaction(format!("Draw {}", tool.name()));
        let mut node = Node::new(tool.kind(), tool.name(), p.x, p.y, 1.0, 1.0)
            .with_fill(Color::rgb(217, 217, 217));
        if tool == Tool::Frame {
            node = node.with_fill(Color::WHITE);
            node.clip_content = true;
        }
        if tool == Tool::Slice {
            node.fills.clear();
            let last = editor.document.nodes.values().filter(|n| n.kind == NodeKind::Slice)
                .filter_map(|n| n.name.strip_prefix("Slice ")?.parse::<u32>().ok()).max().unwrap_or(0);
            node.name = format!("Slice {}", u64::from(last) + 1);
        }
        if tool == Tool::Section {
            node = node.with_fill(Color::rgba(216, 209, 255, 48));
            node.width = 400.0;
            node.height = 300.0;
        }
        if tool == Tool::Text {
            node = node.with_text("Text", 24.0, Color::rgb(30, 30, 30));
            node.width = 160.0;
            node.height = 36.0;
        }
        if matches!(tool, Tool::Line | Tool::Arrow | Tool::Pen | Tool::Pencil) {
            node.fills.clear();
            let mut stroke = if node.kind == NodeKind::Line {
                core::Stroke::new(Color::BLACK, 1.0)
            } else {
                core::Stroke::new(Color::rgb(30, 30, 30), 2.0)
            };
            if tool == Tool::Arrow {
                stroke.cap = core::StrokeCap::Arrow;
            }
            node.strokes = vec![stroke];
        }
        if tool == Tool::Star {
            node.polygon_sides = 5;
        }
        let parent = editor.active_page;
        let id = match editor.document.insert_node(parent, node) {
            Ok(id) => id,
            Err(error) => {
                editor.cancel_transaction();
                drop(editor);
                self.emit(CanvasEvent::Notice(error.to_string()));
                return None;
            }
        };
        self.creation_parent.set(destination.map(|parent| (id, parent)));
        editor.selection = vec![id];
        editor.refresh();
        Some(id)
    }
}
impl InputElementImpl for DesignCanvas {
    fn on_pointer_pressed(this: &Self, e: &PointerPressedEventArgs) {
        Self::parent_on_pointer_pressed(this, e);
        this.clear_measurement();
        this.layout_grids.borrow_mut().clear_snap();
        this.cancel_linear_gradient_gesture();
        this.cancel_corner_radius_gesture();
        this.cancel_stroke_width_gesture();
        this.cancel_network_bucket_gesture();
        this.cancel_layout_drag();
        this.focus();
        e.set_handled(true);
        let screen = e.get_position(Some(this));
        let screen = Point::new(screen.x, screen.y);
        this.selection_pointer.set(Some(screen));
        let world = this.viewport.get().screen_to_world(screen);
        this.pointer_presses.borrow_mut().record(e, screen, world, this.viewport.get(),
            this.editor.borrow().selection.len(), this.frames.get());
        if matches!(*this.gesture.borrow(), Gesture::Scale(_) | Gesture::MultiResize(_) | Gesture::Rotate(_) | Gesture::RotationOrigin(_)) { return; }
        if this.guide_drag_active() { return; }
        if e.properties().is_right_button_pressed {
            if this.show_rulers.get() && this.presentation.get().is_none() {
                if let Some(id) = this.guide_at(screen) {
                    this.select_ruler_guide(Some(id));
                    this.emit(CanvasEvent::Action("ruler-guide-settings"));
                    return;
                }
            }
            if this.presentation.get().is_none() {
                this.emit(CanvasEvent::ContextMenu);
            }
            return;
        }
        e.pointer()
            .capture(Some(&this.to_ref().upcast::<InputElement>()));
        if this.presentation.get().is_some() {
            if let Some(id) = this.prototype_pick(world) {
                this.prototype_press.set(Some((id, screen, false)));
                this.emit(CanvasEvent::PrototypeTrigger(id, core::Trigger::Press));
            }
            return;
        }
        if this.space.get()
            || this.tool.get() == Tool::Hand
            || e.properties().is_middle_button_pressed
        {
            this.set_resize_hover(None);
            *this.gesture.borrow_mut() = Gesture::Pan { previous: screen };
            if this.hover.take().is_some() { this.invalidate_visual(); }
            return;
        }
        if this.press_ruler(screen, e.pointer().id(), e.key_modifiers(), true) { return; }
        if this.linear_gradient_press(world, e.click_count()) { return; }
        if this.image_crop_press(world) { return; }
        if this.stroke_width_press(world) { return; }
        let additive = e.key_modifiers().contains(KeyModifiers::SHIFT);
        match this.tool.get() {
            Tool::Comment => this.emit(CanvasEvent::Comment(world)),
            Tool::Select | Tool::Scale => {
                let scaling = this.tool.get() == Tool::Scale;
                let deep = e.key_modifiers().intersects(KeyModifiers::CONTROL | KeyModifiers::META);
                if scaling && !deep && this.press_scale_handle(world) { return; }
                if !deep && this.press_rotation(world, screen, e.key_modifiers(), e.click_count()) { return; }
                if !deep && this.press_multi_resize(world, screen, e.key_modifiers(), e.click_count()) { return; }
                if !scaling && this.corner_radius_press(world, e.key_modifiers()) { return; }
                this.clear_corner_radius_hover();
                let handle = if scaling || deep { None } else { this.handle_at(world) };
                if handle.is_none() && !deep && this.press_ruler(screen, e.pointer().id(), e.key_modifiers(), false) { return; }
                this.forget_selected_ruler();
                if handle.is_none() && this.press_alt_drag(world, screen, e.key_modifiers()) { return; }
                if this.vector_press(world, e.click_count(), additive) {
                    return;
                }
                if let Some(hit) = handle {
                    let mut editor = this.editor.borrow_mut();
                    let id = editor.selection[0];
                    let node = &editor.document.nodes[&id];
                    if node.kind == NodeKind::Line {
                        if let Some(drag) = line_endpoint::EndpointDrag::new(&editor, node.clone(), hit.handle == resize::Handle::Corner(0), world) {
                            editor.begin_node_transaction(&[id], "Move line endpoint");
                            *this.gesture.borrow_mut() = Gesture::LineEndpoint(drag);
                        }
                    } else if matches!(hit.handle, resize::Handle::Edge(_)) {
                        *this.gesture.borrow_mut() = Gesture::EdgePress {
                            hit, world, screen, modifiers: e.key_modifiers(), clicks: e.click_count(),
                        };
                    } else if let Some(drag) = resize::ResizeDrag::new(node, hit, world) {
                        editor.begin_node_transaction(&[id], "Resize");
                        *this.gesture.borrow_mut() = Gesture::Resize(drag);
                    }
                } else {
                    this.select_at_pointer(world, e.key_modifiers(), e.click_count(), Some(screen));
                }
            }
            Tool::Pen => {
                let mut p = this.snap_point(world);
                if this.snap.get() {
                    let editor = this.editor.borrow();
                    let exclude: Vec<_> = this.pen_node.get().into_iter().collect();
                    if let Some(snap) = this.layout_grids.borrow_mut().snap(&editor, &[p],
                        5.0 / this.viewport.get().zoom, &exclude, None) { p = p + snap.delta; }
                }
                let id = if let Some(id) = this.pen_node.get() {
                    id
                } else {
                    let Some(id) = this.new_node(Tool::Pen, p) else { return };
                    this.pen_node.set(Some(id));
                    id
                };
                let mut close = false;
                let mut handle = None;
                {
                    let mut editor = this.editor.borrow_mut();
                    let node = editor.document.nodes.get_mut(&id).unwrap();
                    let local = Point::new(p.x - node.x, p.y - node.y);
                    if let Some(PathSegment::MoveTo { point }) = node.path.segments.first() {
                        if node.path.segments.len() > 2
                            && local.distance(*point) < 8.0 / this.viewport.get().zoom
                        {
                            if let Some(control1) = this.pen_tangent.take() {
                                let start = *point;
                                node.path.segments.push(PathSegment::CubicTo {
                                    control1,
                                    control2: start,
                                    point: start,
                                });
                            }
                            node.path.segments.push(PathSegment::Close);
                            node.fills = vec![core::Fill::solid(Color::rgb(123, 97, 255))];
                            close = true;
                        } else {
                            let segment = if let Some(control1) = this.pen_tangent.take() {
                                PathSegment::CubicTo {
                                    control1,
                                    control2: local,
                                    point: local,
                                }
                            } else {
                                PathSegment::LineTo { point: local }
                            };
                            node.path.segments.push(segment);
                        }
                    } else {
                        node.path = VectorPath::default();
                        node.path
                            .segments
                            .push(PathSegment::MoveTo { point: local });
                    }
                    if !close {
                        handle = Some(Gesture::PenHandle {
                            id,
                            segment: node.path.segments.len() - 1,
                            anchor: local,
                            start: p,
                        });
                    }
                    node.width = node.width.max(local.x);
                    node.height = node.height.max(local.y);
                    editor.refresh();
                }
                if let Some(handle) = handle {
                    *this.gesture.borrow_mut() = handle;
                }
                if close {
                    this.finish_pen();
                    this.tool.set(Tool::Select);
                }
                this.emit(CanvasEvent::Selection);
            }
            tool => {
                let mut start = this.snap_point(world);
                if this.snap.get() {
                    let editor = this.editor.borrow();
                    if let Some(snap) = this.layout_grids.borrow_mut().snap(&editor, &[start],
                        5.0 / this.viewport.get().zoom, &[], None) { start = start + snap.delta; }
                }
                let Some(id) = this.new_node(tool, start) else { return };
                if tool == Tool::Pencil {
                    this.editor
                        .borrow_mut()
                        .document
                        .nodes
                        .get_mut(&id)
                        .unwrap()
                        .path
                        .segments
                        .push(PathSegment::MoveTo {
                            point: Point::new(0.0, 0.0),
                        });
                    *this.gesture.borrow_mut() = Gesture::Pencil { id, origin: start };
                } else {
                    *this.gesture.borrow_mut() = Gesture::Draw(drawing::ShapeDrag::new(id, tool, world, start));
                }
                this.emit(CanvasEvent::Selection);
            }
        }
        this.invalidate_visual();
    }
    fn on_pointer_moved(this: &Self, e: &PointerEventArgs) {
        Self::parent_on_pointer_moved(this, e);
        let screen = e.get_position(Some(this));
        let position = Point::new(screen.x, screen.y);
        this.selection_pointer.set(Some(position));
        if this.move_ruler(position, e.pointer().id()) { e.set_handled(true); return; }
        if this.presentation.get().is_none() {
            if this.pan_pointer(position) {
                e.set_handled(true);
                return;
            }
            if (this.space.get() || this.tool.get() == Tool::Hand)
                && matches!(*this.gesture.borrow(), Gesture::None)
                && this.layout_drag.borrow().is_none()
                && this.image_crop_gesture.borrow().is_none()
            {
                return;
            }
        }
        let world = this.world(screen);
        if this.presentation.get().is_some() {
            this.emit(CanvasEvent::PrototypePointerMoved(Point::new(screen.x, screen.y)));
            let hover = this.prototype_pick(world);
            let previous = this.hover.replace(hover);
            if previous != hover {
                if let Some(id) = previous {
                    this.emit(CanvasEvent::PrototypeTrigger(id, core::Trigger::MouseLeave));
                }
                if let Some(id) = hover {
                    this.emit(CanvasEvent::PrototypeTrigger(id, core::Trigger::Hover));
                }
            }
            if let Some((id, start, dragged)) = this.prototype_press.get() {
                if !dragged && start.distance(Point::new(screen.x, screen.y)) >= 5.0 {
                    this.prototype_press.set(Some((id, start, true)));
                    this.emit(CanvasEvent::PrototypeTrigger(id, core::Trigger::Drag));
                }
            }
            return;
        }
        if this.linear_gradient_move(world) { this.set_resize_hover(None); return; }
        if this.corner_radius_move(world) { return; }
        if this.image_crop_move(world) { this.set_resize_hover(None); return; }
        if this.stroke_width_move(world) { this.set_resize_hover(None); return; }
        if this.network_bucket_move(world, e.key_modifiers().contains(KeyModifiers::SHIFT)) { return; }
        let shift = e.key_modifiers().contains(KeyModifiers::SHIFT);
        if this.layout_drag_move_constrained(world, Point::new(screen.x, screen.y), shift) { return; }
        if matches!(*this.gesture.borrow(), Gesture::None) {
            if this.measure_hover(world, e.key_modifiers()) { return; }
            if this.tool.get() == Tool::Scale {
                this.update_scale_cursor(world);
            } else if this.hover_edit_handle(world, e.key_modifiers()) {
                return;
            }
        }
        let mut gesture = this.gesture.borrow_mut();
        this.activate_edge_drag(&mut gesture, position);
        let resize_revision = matches!(*gesture, Gesture::Resize(_) | Gesture::MultiResize(_) | Gesture::Rotate(_))
            .then(|| this.editor.borrow().revision);
        match &mut *gesture {
            Gesture::EdgePress { .. } => return,
            Gesture::None => {
                let hover = this.pick_with_modifiers(world, e.key_modifiers(), false);
                if this.hover.replace(hover) != hover {
                    this.invalidate_visual();
                }
                return;
            }
            Gesture::Pan { .. } => return, // Handled before tool hit testing.
            move_drag @ Gesture::Move { .. } => this.update_free_move(move_drag, world, shift),
            Gesture::Scale(drag) => drag.update(&mut this.editor.borrow_mut(), world, e.key_modifiers().contains(KeyModifiers::ALT)),
            Gesture::Resize(drag) => drag.update(this, world, e.key_modifiers()),
            Gesture::MultiResize(drag) => drag.update(this, world, e.key_modifiers()),
            Gesture::Rotate(drag) => drag.update(this, world, e.key_modifiers()),
            Gesture::RotationOrigin(drag) => drag.update(this, world, e.key_modifiers()),
            Gesture::AltDrag(drag) => drag.update(this, world, e.key_modifiers()),
            Gesture::Marquee(drag) => { drag.update(&this.editor.borrow(), world, e.key_modifiers()); }
            Gesture::Draw(drag) => drag.update(this, world, e.key_modifiers(), false),
            Gesture::LineEndpoint(drag) => drag.update(this, world, e.key_modifiers()),
            Gesture::Pencil { id, origin } => {
                let local = world - *origin;
                let mut editor = this.editor.borrow_mut();
                if let Some(n) = editor.document.nodes.get_mut(id) {
                    // Sample at approximately one screen pixel to avoid redundant path segments.
                    let last = n.path.segments.last().and_then(|s| match s {
                        PathSegment::MoveTo { point } | PathSegment::LineTo { point } => {
                            Some(*point)
                        }
                        _ => None,
                    });
                    if last.is_none_or(|p| p.distance(local) > 0.8 / this.viewport.get().zoom) {
                        n.path.segments.push(PathSegment::LineTo { point: local });
                        n.width = n.width.max(local.x);
                        n.height = n.height.max(local.y);
                        editor.refresh_preview_nodes(&[*id]);
                    }
                }
            }
            Gesture::PenHandle {
                id,
                segment,
                anchor,
                start,
            } => {
                let mut delta = world - *start;
                if delta.distance(Point::default()) < 2.0 / this.viewport.get().zoom {
                    return;
                }
                if shift {
                    let angle = (delta.y.atan2(delta.x) / std::f64::consts::FRAC_PI_4).round()
                        * std::f64::consts::FRAC_PI_4;
                    let length = delta.distance(Point::default());
                    delta = Point::new(angle.cos() * length, angle.sin() * length);
                }
                let mut editor = this.editor.borrow_mut();
                if let Some(node) = editor.document.nodes.get_mut(id) {
                    let incoming = *anchor - delta;
                    let previous = node
                        .path
                        .segments
                        .get(segment.saturating_sub(1))
                        .and_then(|s| match s {
                            PathSegment::MoveTo { point }
                            | PathSegment::LineTo { point }
                            | PathSegment::QuadraticTo { point, .. }
                            | PathSegment::CubicTo { point, .. } => Some(*point),
                            PathSegment::Close => None,
                        })
                        .unwrap_or(*anchor);
                    match node.path.segments.get_mut(*segment) {
                        Some(s @ PathSegment::LineTo { .. }) => {
                            *s = PathSegment::CubicTo {
                                control1: previous,
                                control2: incoming,
                                point: *anchor,
                            };
                        }
                        Some(PathSegment::CubicTo { control2, .. }) => *control2 = incoming,
                        _ => {}
                    }
                    this.pen_tangent.set(Some(*anchor + delta));
                }
                editor.refresh_preview_nodes(&[*id]);
            }
            Gesture::NetworkPoint { node, point, start, original } => {
                let mut delta = world - *start;
                if shift { if delta.x.abs() > delta.y.abs() { delta.y = 0.0; } else { delta.x = 0.0; } }
                let position = if matches!(point, core::NetworkPointId::Vertex(_)) && this.snap.get() { this.snap_point(*original + delta) } else { *original + delta };
                this.editor.borrow_mut().move_network_point(*node, *point, position);
            }
            Gesture::NetworkBend { node, edge, t } => {
                this.editor.borrow_mut().bend_network_edge(*node, *edge, *t, world);
            }
            Gesture::VectorBend { node, segment, t } => {
                this.editor.borrow_mut().bend_vector_segment(*node, *segment, *t, world);
            }
            Gesture::VectorPoint {
                node,
                point,
                start,
                original,
            } => {
                let mut delta = world - *start;
                if shift {
                    if delta.x.abs() > delta.y.abs() {
                        delta.y = 0.0;
                    } else {
                        delta.x = 0.0;
                    }
                }
                let position = if point.kind == PathPointKind::Anchor && this.snap.get() {
                    this.snap_point(*original + delta)
                } else {
                    *original + delta
                };
                this.editor
                    .borrow_mut()
                    .move_vector_point(*node, *point, position);
            }
        }
        drop(gesture);
        if resize_revision.is_some_and(|revision| revision != this.editor.borrow().revision) {
            this.emit(CanvasEvent::ResizePreview);
        }
        this.invalidate_visual();
    }
    fn on_pointer_released(this: &Self, e: &PointerReleasedEventArgs) {
        Self::parent_on_pointer_released(this, e);
        if this.presentation.get().is_some() {
            let press = this.prototype_press.take();
            e.pointer().capture(None);
            if let Some((id, _, _)) = press {
                this.emit(CanvasEvent::PrototypeTrigger(id, core::Trigger::Release));
            }
            let point = this.world(e.get_position(Some(this)));
            if let Some((id, _, false)) = press {
                if this.prototype_pick(point) == Some(id) {
                    this.emit(CanvasEvent::PrototypeTrigger(id, core::Trigger::Click));
                }
            }
            e.set_handled(true);
            return;
        }
        let screen = e.get_position(Some(this));
        if this.release_ruler(Point::new(screen.x, screen.y), e.pointer().id()) {
            e.pointer().capture(None); e.set_handled(true); return;
        }
        if this.pan_pointer(Point::new(screen.x, screen.y)) {
            *this.gesture.borrow_mut() = Gesture::None;
            e.pointer().capture(None);
            e.set_handled(true);
            return;
        }
        if this.linear_gradient_active() {
            this.linear_gradient_move(this.world(screen));
        }
        if this.finish_linear_gradient_edit() {
            e.pointer().capture(None);
            e.set_handled(true);
            return;
        }
        if this.corner_radius_active() {
            this.corner_radius_move(this.world(e.get_position(Some(this))));
        }
        if this.finish_corner_radius_edit() {
            e.pointer().capture(None);
            e.set_handled(true);
            return;
        }
        if this.image_crop_release() {
            e.pointer().capture(None);
            e.set_handled(true);
            return;
        }
        if this.stroke_width_active() {
            this.stroke_width_move(this.world(e.get_position(Some(this))));
        }
        if this.finish_stroke_width_edit() {
            e.pointer().capture(None);
            e.set_handled(true);
            return;
        }
        if this.network_bucket_state().1 {
            this.network_bucket_move(this.world(e.get_position(Some(this))), e.key_modifiers().contains(KeyModifiers::SHIFT));
        }
        if this.finish_network_bucket_gesture(true) {
            e.pointer().capture(None);
            e.set_handled(true);
            return;
        }
        let release_screen = e.get_position(Some(this));
        if this.layout_drag_release_constrained(this.world(release_screen), Point::new(release_screen.x, release_screen.y),
            e.key_modifiers().contains(KeyModifiers::SHIFT)) {
            e.pointer().capture(None);
            e.set_handled(true);
            return;
        }
        let mut gesture = std::mem::replace(&mut *this.gesture.borrow_mut(), Gesture::None);
        this.activate_edge_drag(&mut gesture, Point::new(release_screen.x, release_screen.y));
        // Hosts may coalesce the last move into the release event.
        match &mut gesture {
            move_drag @ Gesture::Move { .. } => this.update_free_move(
                move_drag, this.world(release_screen), e.key_modifiers().contains(KeyModifiers::SHIFT),
            ),
            Gesture::Scale(drag) => drag.update(&mut this.editor.borrow_mut(), this.world(release_screen), e.key_modifiers().contains(KeyModifiers::ALT)),
            Gesture::Draw(drag) => drag.update(this, this.world(release_screen), e.key_modifiers(), true),
            Gesture::LineEndpoint(drag) => drag.update(this, this.world(release_screen), e.key_modifiers()),
            Gesture::Resize(drag) => drag.update(this, this.world(release_screen), e.key_modifiers()),
            Gesture::Rotate(drag) => drag.update(this, this.world(release_screen), e.key_modifiers()),
            Gesture::RotationOrigin(drag) => drag.update(this, this.world(release_screen), e.key_modifiers()),
            Gesture::MultiResize(drag) => drag.update(this, this.world(release_screen), e.key_modifiers()),
            _ => {}
        }
        this.snap_guides.set((None, None));
        this.layout_grids.borrow_mut().clear_snap();
        // Clear the gesture before releasing capture: capture-lost is synchronous.
        e.pointer().capture(None);
        match gesture {
            Gesture::EdgePress { world, modifiers, clicks, .. } => {
                this.set_resize_hover(None);
                if !this.cycle_pointer_click(world, modifiers) {
                    this.select_at_pointer(world, modifiers, clicks, None);
                }
            }
            Gesture::Rotate(drag) => drag.finish(this),
            Gesture::MultiResize(drag) => drag.finish(this),
            Gesture::AltDrag(drag) => drag.finish(this, this.world(release_screen), e.key_modifiers()),
            Gesture::Marquee(mut drag) => {
                let mut editor = this.editor.borrow_mut();
                if drag.update(&editor, this.world(release_screen), e.key_modifiers())
                    && editor.selection != drag.preview {
                    editor.selection = drag.preview;
                    drop(editor);
                    this.emit(CanvasEvent::Selection);
                }
            }
            Gesture::Scale(drag) => {
                let changed = drag.commit(&mut this.editor.borrow_mut());
                this.emit(if changed { CanvasEvent::Document } else { CanvasEvent::Selection });
            }
            Gesture::Move { moved: false, .. } => {
                this.editor.borrow_mut().cancel_transaction();
            }
            Gesture::Pencil { id, .. } => {
                let mut editor = this.editor.borrow_mut();
                normalize_drawn_vector(&mut editor, id);
                this.commit_created_layer(&mut editor, id);
                drop(editor);
                this.tool.set(Tool::Select);
                this.emit(CanvasEvent::Document);
            }
            Gesture::Draw(drag) => {
                this.commit_created_layer(&mut this.editor.borrow_mut(), drag.id);
                this.tool.set(Tool::Select);
                this.emit(CanvasEvent::Document);
                if drag.tool == Tool::Text {
                    this.emit(CanvasEvent::EditText(drag.id));
                }
            }
            Gesture::Move { .. }
            | Gesture::LineEndpoint(_)
            | Gesture::Resize(_)
            | Gesture::VectorPoint { .. }
            | Gesture::VectorBend { .. }
            | Gesture::NetworkPoint { .. }
            | Gesture::NetworkBend { .. } => {
                this.editor.borrow_mut().commit_transaction();
                if this.tool.get() != Tool::Scale { this.tool.set(Tool::Select); }
                this.emit(CanvasEvent::Document);
            }
            _ => {}
        }
        this.invalidate_visual();
    }
    fn on_pointer_capture_lost(this: &Self, e: &PointerCaptureLostEventArgs) {
        Self::parent_on_pointer_capture_lost(this, e);
        if this.lost_ruler_capture(e.pointer().id()) { return; }
        if matches!(*this.gesture.borrow(), Gesture::Pan { .. }) {
            // A temporary hand drag can coexist with an unfinished pen path.
            // Losing camera capture must never roll back that transaction.
            *this.gesture.borrow_mut() = Gesture::None;
            return;
        }
        if this.cancel_linear_gradient_gesture() { return; }
        if this.cancel_corner_radius_gesture() { return; }
        if this.cancel_stroke_width_gesture() { return; }
        if this.cancel_network_bucket_gesture() { return; }
        if this.cancel_layout_drag() { return; }
        if this.cancel_image_crop_gesture() { return; }
        if let Some((id, _, _)) = this.prototype_press.take() {
            if this.presentation.get().is_some() {
                this.emit(CanvasEvent::PrototypeTrigger(id, core::Trigger::Release));
            }
        }
        if !matches!(*this.gesture.borrow(), Gesture::None) {
            this.cancel();
        }
    }
    fn on_pointer_exited(this: &Self, e: &PointerEventArgs) {
        Self::parent_on_pointer_exited(this, e);
        this.clear_measurement();
        this.set_resize_hover(None);
        this.clear_gradient_hover();
        this.clear_corner_radius_hover();
        this.clear_stroke_width_hover();
        this.clear_bucket_hover();
        if this.presentation.get().is_some() {
            this.emit(CanvasEvent::PrototypePointerExited);
            if let Some(id) = this.hover.take() {
                this.emit(CanvasEvent::PrototypeTrigger(id, core::Trigger::MouseLeave));
            }
        } else if this.hover.take().is_some() {
            this.invalidate_visual();
        }
    }
    fn on_pointer_wheel_changed(this: &Self, e: &PointerWheelEventArgs) {
        Self::parent_on_pointer_wheel_changed(this, e);
        this.cancel_linear_gradient_gesture();
        this.clear_gradient_hover();
        this.cancel_corner_radius_gesture();
        this.clear_corner_radius_hover();
        this.cancel_stroke_width_gesture();
        this.clear_stroke_width_hover();
        this.cancel_network_bucket_gesture();
        this.clear_bucket_hover();
        e.set_handled(true);
        if let Some(root) = this.presentation.get() {
            let world = this.world(e.get_position(Some(this)));
            let target = this.prototype_pick(world).unwrap_or(root);
            this.emit(CanvasEvent::PrototypeScroll(target, Point::new(-e.delta().x * 36.0, -e.delta().y * 36.0)));
            return;
        }
        let mut v = this.viewport.get();
        this.cancel_layout_drag();
        if e.key_modifiers()
            .intersects(KeyModifiers::CONTROL | KeyModifiers::META)
        {
            let p = e.get_position(Some(this));
            v.zoom_at(Point::new(p.x, p.y), v.zoom * (e.delta().y * 0.16).exp());
        } else {
            v.pan = v.pan + Point::new(e.delta().x * 36.0, e.delta().y * 36.0);
        }
        this.update_camera(v);
    }
    fn on_lost_focus(this: &Self, e: &FocusChangedEventArgs) {
        Self::parent_on_lost_focus(this, e);
        this.cancel_guide_drag();
        this.clear_measurement();
        if matches!(*this.gesture.borrow(), Gesture::Marquee(_) | Gesture::AltDrag(_) | Gesture::MultiResize(_) | Gesture::Rotate(_) | Gesture::RotationOrigin(_)) {
            this.cancel();
        }
        // KeyUp is routed to the new focus owner.
        this.space.set(false);
    }
    fn on_key_down(this: &Self, e: &KeyEventArgs) {
        Self::parent_on_key_down(this, e);
        if this.presentation.get().is_some() {
            this.emit(CanvasEvent::PrototypeKey(prototype_key_chord(
                e.key,
                e.key_modifiers,
                e.key_symbol.as_deref(),
            )));
            e.set_handled(true);
            return;
        }
        if this.ruler_key(e) { e.set_handled(true); return; }
        if this.rotation_key(e) { e.set_handled(true); return; }
        if this.rotation_modifier(e, true) || this.multi_resize_modifier(e, true) || this.marquee_modifier(e, true) || this.alt_drag_modifier(e, true) {
            e.set_handled(true);
            return;
        }
        if matches!(e.key, Key::LeftAlt | Key::RightAlt) && this.measure_at_pointer(e.key_modifiers | KeyModifiers::ALT) {
            e.set_handled(true);
            return;
        }
        if matches!(e.key, Key::LeftCtrl | Key::RightCtrl | Key::LWin | Key::RWin) && e.key_modifiers.contains(KeyModifiers::ALT) {
            this.measure_at_pointer(e.key_modifiers);
        }
        if this.linear_gradient_key(e.key, e.key_modifiers) { e.set_handled(true); return; }
        if e.key == Key::Escape && this.cancel_corner_radius_gesture() {
            e.set_handled(true);
            return;
        }
        if this.image_crop_key(e.key) { e.set_handled(true); return; }
        if this.stroke_width_key(e.key, e.key_modifiers) { e.set_handled(true); return; }
        if e.key == Key::Escape && this.cancel_network_bucket_gesture() {
            e.set_handled(true);
            return;
        }
        if e.key == Key::Space {
            this.clear_measurement();
            if this.space.replace(true) {
                e.set_handled(true);
                return;
            }
            this.set_resize_hover(None);
            if this.hover.take().is_some() { this.invalidate_visual(); }
            this.cancel_linear_gradient_gesture();
            this.clear_gradient_hover();
            this.cancel_corner_radius_gesture();
            this.clear_corner_radius_hover();
            this.cancel_stroke_width_gesture();
            this.clear_stroke_width_hover();
            this.cancel_network_bucket_gesture();
            this.clear_bucket_hover();
            e.set_handled(true);
            return;
        }
        if e.key == Key::Escape {
            if this.presentation.get().is_some() {
                this.emit(CanvasEvent::ExitPresentation)
            } else if this.vector_node.get().is_some()
                && matches!(*this.gesture.borrow(), Gesture::None)
            {
                this.leave_vector_edit();
                this.emit(CanvasEvent::Selection);
            } else {
                if !this.cancel_active_edit() && !this.editor.borrow().has_active_transaction() {
                    this.editor.borrow_mut().clear_selection();
                    this.hover.set(None);
                    this.invalidate_visual();
                    this.emit(CanvasEvent::Selection);
                }
            }
            e.set_handled(true);
            return;
        }
        if e.key == Key::Enter && this.pen_node.get().is_some() {
            this.finish_pen();
            this.tool.set(Tool::Select);
            this.emit(CanvasEvent::Tool);
            e.set_handled(true);
            return;
        }
        let command = e
            .key_modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::META);
        let shift = e.key_modifiers.contains(KeyModifiers::SHIFT);
        if matches!(e.key, Key::Left | Key::Right | Key::Up | Key::Down | Key::Enter | Key::Tab | Key::Delete | Key::Back) {
            this.finish_corner_radius_edit();
            this.finish_network_bucket();
        }
        if this.selection_key(e) {
            e.set_handled(true);
            return;
        }
        if !command && matches!(e.key, Key::Delete | Key::Back) && this.vector_node.get().is_some()
        {
            if this.delete_network_selection() { e.set_handled(true); return; }
            if let (Some(id), Some(point)) = (this.vector_node.get(), this.vector_point.get()) {
                if point.kind == PathPointKind::Anchor {
                    this.editor
                        .borrow_mut()
                        .delete_vector_anchor(id, point.segment);
                    this.vector_point.set(None);
                    this.emit(CanvasEvent::Document);
                }
            }
            e.set_handled(true);
            return;
        }
        let action = this.editor_shortcut(e);
        if let Some(action) = action {
            if !matches!(action, "undo" | "redo") { this.cancel_layout_drag(); }
            this.emit(CanvasEvent::Action(action));
            e.set_handled(true);
            return;
        }
        let step = if shift { 10.0 } else { 1.0 };
        let delta = match e.key {
            Key::Left => Some((-step, 0.0)),
            Key::Right => Some((step, 0.0)),
            Key::Up => Some((0.0, -step)),
            Key::Down => Some((0.0, step)),
            _ => None,
        };
        if let Some((x, y)) = delta {
            this.cancel_layout_drag();
            if let (Some(id), Some(point)) = (this.vector_node.get(), this.vector_point.get()) {
                let mut editor = this.editor.borrow_mut();
                let world = editor
                    .document
                    .nodes
                    .get(&id)
                    .and_then(|n| n.path.point_position(point))
                    .zip(editor.document.world_transform(id))
                    .map(|(p, t)| t.apply(p));
                if let Some(world) = world {
                    editor.move_vector_point(id, point, world + Point::new(x, y));
                }
            } else {
                this.editor.borrow_mut().move_selection(x, y);
            }
            this.emit(CanvasEvent::Document);
            this.invalidate_visual();
            e.set_handled(true);
        }
    }
    fn on_key_up(this: &Self, e: &KeyEventArgs) {
        Self::parent_on_key_up(this, e);
        if this.rotation_modifier(e, false) || this.multi_resize_modifier(e, false) || this.marquee_modifier(e, false) || this.alt_drag_modifier(e, false) {
            e.set_handled(true);
            return;
        }
        if matches!(e.key, Key::LeftAlt | Key::RightAlt) { this.clear_measurement(); }
        if matches!(e.key, Key::LeftCtrl | Key::RightCtrl | Key::LWin | Key::RWin) && e.key_modifiers.contains(KeyModifiers::ALT) {
            this.measure_at_pointer(e.key_modifiers);
        }
        if e.key == Key::Space {
            this.space.set(false);
            e.set_handled(true);
        }
    }
}
fn prototype_key_chord(key: Key, modifiers: KeyModifiers, symbol: Option<&str>) -> String {
    let mut chord = String::new();
    for (modifier, name) in [
        (KeyModifiers::CONTROL, "Ctrl+"),
        (KeyModifiers::ALT, "Alt+"),
        (KeyModifiers::SHIFT, "Shift+"),
        (KeyModifiers::META, "Meta+"),
    ] {
        if modifiers.contains(modifier) {
            chord.push_str(name);
        }
    }
    let fallback = key.to_string();
    let punctuation = symbol.filter(|symbol| {
        symbol.chars().count() == 1
            && symbol.chars().next().is_some_and(|symbol| {
                !symbol.is_alphanumeric() && !symbol.is_whitespace() && !symbol.is_control()
            })
    });
    let name = if key == Key::Back {
        "Backspace"
    } else if let Some(symbol) = punctuation {
        // Use the typed symbol instead of a layout-specific Oem virtual-key
        // name. The core absorbs redundant Shift in symbolic chords.
        symbol
    } else if fallback.len() == 2
        && fallback.starts_with('D')
        && fallback.as_bytes()[1].is_ascii_digit()
    {
        &fallback[1..]
    } else {
        &fallback
    };
    chord.push_str(name);
    chord
}

#[cfg(test)]
mod prototype_key_tests {
    use super::*;

    #[test]
    fn native_key_adapter_preserves_symbols_backspace_and_named_shift_chords() {
        for (key, modifiers, symbol, expected) in [
            (
                Key::OemPlus,
                KeyModifiers::CONTROL | KeyModifiers::SHIFT,
                Some("+"),
                "ctrl++",
            ),
            (Key::OemPlus, KeyModifiers::CONTROL, Some("+"), "ctrl++"),
            (Key::D1, KeyModifiers::SHIFT, Some("!"), "!"),
            (Key::Back, KeyModifiers::CONTROL, None, "ctrl+backspace"),
            (
                Key::A,
                KeyModifiers::CONTROL | KeyModifiers::SHIFT,
                Some("A"),
                "ctrl+shift+a",
            ),
            (Key::F1, KeyModifiers::SHIFT, None, "shift+f1"),
            (Key::D1, KeyModifiers::CONTROL, Some("1"), "ctrl+1"),
        ] {
            let chord = prototype_key_chord(key, modifiers, symbol);
            assert_eq!(
                core::normalize_prototype_key(&chord).as_deref(),
                Some(expected)
            );
        }
        // The shell's unmatched-Escape fallback consumes the original named
        // event before core normalization; retain that spelling.
        assert_eq!(
            prototype_key_chord(Key::Escape, KeyModifiers::empty(), None),
            "Escape"
        );
    }
}

fn normalize_drawn_vector(editor: &mut Editor, id: NodeId) {
    if let Some(node) = editor.document.nodes.get_mut(&id) {
        core::normalize_vector_bounds(node);
    }
    editor.refresh();
}

fn ui(p: Point) -> UiPoint {
    UiPoint::new(p.x, p.y)
}
fn rect(r: Rect) -> UiRect {
    UiRect::new(r.x, r.y, r.width, r.height)
}
fn pen(color: u32, width: f64) -> Rc<dyn IPen> {
    Rc::new(ImmutablePen::from_uint32(color, width))
}
fn label(ctx: &mut DrawingContext, text: &str, point: Point, size: f64, color: &str) {
    let text = FormattedText::new(
        text,
        CultureInfo::invariant_culture(),
        FlowDirection::LeftToRight,
        Typeface::from_name("fonts:Inter#Inter"),
        size,
        Some(brush(color)),
    );
    ctx.draw_text(&text, ui(point));
}
/// Captions stay at a readable screen font size while the artboard zooms.
fn canvas_caption(ctx: &mut DrawingContext, name: &str, origin: Point, screen_width: f64, color: &str) {
    // The root-origin query includes 400 DIPs to the left of the viewport.
    let width = (screen_width - 4.0).min(400.0);
    if width < 20.0 || name.is_empty() { return; }
    let caption = FormattedText::new(
        name, CultureInfo::invariant_culture(), FlowDirection::LeftToRight,
        Typeface::from_name("fonts:Inter#Inter"), 11.0, Some(brush(color)),
    );
    caption.set_max_text_width(width);
    caption.set_max_line_count(1);
    caption.set_trimming(<dyn ferroui_base::media::TextTrimming>::character_ellipsis());
    ctx.draw_text(&caption, ui(origin));
}
fn render_vector_editor(
    ctx: &mut DrawingContext,
    path: &VectorPath,
    transform: core::Transform,
    viewport: Viewport,
    selected: Option<PathPointId>,
) {
    let to_screen = |point| ui(viewport.world_to_screen(transform.apply(point)));
    let geometry = StreamGeometry::new();
    {
        let mut context = geometry.open();
        let mut figure = false;
        for segment in &path.segments {
            match *segment {
                PathSegment::MoveTo { point } => {
                    if figure {
                        context.end_figure(false);
                    }
                    context.begin_figure(to_screen(point), false);
                    figure = true;
                }
                PathSegment::LineTo { point } if figure => context.line_to(to_screen(point), true),
                PathSegment::QuadraticTo { control, point } if figure => {
                    context.quadratic_bezier_to(to_screen(control), to_screen(point), true);
                }
                PathSegment::CubicTo {
                    control1,
                    control2,
                    point,
                } if figure => {
                    context.cubic_bezier_to(
                        to_screen(control1),
                        to_screen(control2),
                        to_screen(point),
                        true,
                    );
                }
                PathSegment::Close => {
                    if figure {
                        context.end_figure(true);
                    }
                    figure = false;
                }
                _ => {}
            }
        }
        if figure {
            context.end_figure(false);
        }
    }
    let blue = pen(0xFF0D99FF, 1.0);
    ctx.draw_geometry(None, Some(&blue), &geometry.upcast());
    let blue_fill = brush("#0D99FF");
    let white = brush("#FFFFFF");
    let points = path.edit_points();
    let anchor = selected.and_then(|id| points.iter().find(|p| p.id == id).map(|p| p.anchor_id));
    for point in points
        .iter()
        .filter(|p| p.id.kind != PathPointKind::Anchor && Some(p.anchor_id) == anchor)
    {
        let position = to_screen(point.position);
        ctx.draw_line(&blue, to_screen(point.anchor), position);
        ctx.draw_ellipse_at(
            Some(if selected == Some(point.id) {
                &blue_fill
            } else {
                &white
            }),
            Some(&blue),
            position,
            3.5,
            3.5,
        );
    }
    for point in points.iter().filter(|p| p.id.kind == PathPointKind::Anchor) {
        let position = to_screen(point.position);
        ctx.draw_rectangle(
            Some(if Some(point.id) == anchor {
                &blue_fill
            } else {
                &white
            }),
            Some(&blue),
            UiRect::new(position.x - 3.5, position.y - 3.5, 7.0, 7.0),
            0.0,
            0.0,
            &BoxShadows::default(),
        );
    }
}
impl VisualImpl for DesignCanvas {
    fn render(this: &Self, ctx: &mut DrawingContext) {
        let bounds = this.bounds();
        let mut v = this.viewport.get();
        let resized = v.width != bounds.width || v.height != bounds.height;
        v.width = bounds.width;
        v.height = bounds.height;
        // The browser surface reports 1×1 until its first real resize. A
        // placeholder frame must not consume the pending document fit.
        if v.width <= 1.0 || v.height <= 1.0 {
            return;
        }
        let editor = this.editor.borrow();
        let prototype_scene = this.prototype_scene.borrow();
        let doc = prototype_scene.as_ref().map(|scene| &scene.document).unwrap_or(&editor.document);
        let presentation_root = prototype_scene.as_ref().map(|scene| scene.root).or(this.presentation.get());
        let roots = match &presentation_root {
            Some(root) => std::slice::from_ref(root),
            None => doc.page(editor.active_page).map_or(&[][..], |page| page.children.as_slice()),
        };
        let fitted = prototype_scene.is_none() && (!this.initialized.replace(true) || (resized && presentation_root.is_some()));
        if fitted {
            if let Some(bounds) = roots
                .iter()
                .filter_map(|&id| doc.world_bounds(id))
                .reduce(|a, b| a.union(b))
            {
                if presentation_root.is_some() {
                    crate::prototype::fit_preview_frame(&mut v, bounds);
                } else {
                    v.fit(bounds, 65.0);
                    v.pan.y -= 22.0;
                }
            }
        }
        this.viewport.set(v);
        if fitted {
            // Fitting needs the arranged viewport. Update native chrome after
            // rendering so it observes that fit without changing controls here.
            if let Some(callback) = this.callback.borrow().clone() {
                Dispatcher::ui_thread().post_local(
                    move || callback(CanvasEvent::Camera),
                    DispatcherPriority::AFTER_RENDER,
                );
            }
        }
        this.frames.set(this.frames.get() + 1);
        let page_color = doc
            .page(editor.active_page)
            .map(|p| p.background)
            .unwrap_or(Color::rgb(229, 229, 229));
        let page_hex = format!(
            "#{:02X}{:02X}{:02X}{:02X}",
            page_color.a, page_color.r, page_color.g, page_color.b
        );
        let background = brush(if this.presentation.get().is_some() {
            "#181818"
        } else if this.dark.get() {
            "#252525"
        } else {
            &page_hex
        });
        ctx.draw_rectangle(
            Some(&background),
            None,
            UiRect::new(0.0, 0.0, v.width, v.height),
            0.0,
            0.0,
            &BoxShadows::default(),
        );
        if this.show_grid.get() && this.presentation.get().is_none() {
            let step = (32.0 * v.zoom).max(8.0);
            let grid_pen = pen(
                if this.dark.get() {
                    0xFF333333
                } else {
                    0xFFD5D5D5
                },
                0.5,
            );
            let mut x = v.pan.x.rem_euclid(step);
            while x < v.width {
                ctx.draw_line(&grid_pen, UiPoint::new(x, 0.0), UiPoint::new(x, v.height));
                x += step;
            }
            let mut y = v.pan.y.rem_euclid(step);
            while y < v.height {
                ctx.draw_line(&grid_pen, UiPoint::new(0.0, y), UiPoint::new(v.width, y));
                y += step;
            }
        }
        let camera = ctx.push_transform(
            Matrix::create_scale(v.zoom, v.zoom) * Matrix::create_translation(v.pan.x, v.pan.y),
        );
        {
            let display_scale = TopLevel::get_top_level(Some(this))
                .map_or(1.0, |top_level| top_level.render_scaling());
            let mut painter = this.painter.borrow_mut();
            painter.set_raster_scale(v.zoom * display_scale);
            if prototype_scene.is_none() && this.presentation.get().is_none() {
                painter.render_editor(ctx, &editor, roots, v.visible_rect());
                if let Gesture::AltDrag(drag) = &*this.gesture.borrow() {
                    drag.render(ctx, &editor, v.visible_rect(), v.zoom * display_scale);
                }
            } else {
                painter.render(ctx, doc, roots, v.visible_rect());
            }
        }
        ctx.pop(camera);
        if this.presentation.get().is_none() && prototype_scene.is_none() {
            this.layout_grids.borrow_mut().render(ctx, &editor, v);
            this.render_ruler_guides(ctx, &editor, v);
        }
        if let Some(current) = this.presentation.get() {
            for &(id, position) in this.overlays.borrow().iter().filter(|_| prototype_scene.is_none()) {
                if let (Some(current_bounds), Some(overlay_bounds)) =
                    (doc.world_bounds(current), doc.world_bounds(id))
                {
                    let dim = brush("#66000000");
                    ctx.draw_rectangle(
                        Some(&dim),
                        None,
                        UiRect::new(0.0, 0.0, v.width, v.height),
                        0.0,
                        0.0,
                        &BoxShadows::default(),
                    );
                    let dx = current_bounds.x + position.x - overlay_bounds.x;
                    let dy = current_bounds.y + position.y - overlay_bounds.y;
                    let transform = ctx.push_transform(
                        Matrix::create_translation(dx, dy)
                            * Matrix::create_scale(v.zoom, v.zoom)
                            * Matrix::create_translation(v.pan.x, v.pan.y),
                    );
                    let visible = v.visible_rect();
                    this.painter.borrow_mut().render(
                        ctx,
                        doc,
                        &[id],
                        Rect::new(
                            visible.x - dx,
                            visible.y - dy,
                            visible.width,
                            visible.height,
                        ),
                    );
                    ctx.pop(transform);
                }
            }

            return;
        }
        let blue = pen(0xFF0D99FF, 1.0);
        let (vertical, horizontal) = this.snap_guides.get();
        if vertical.is_some() || horizontal.is_some() {
            let guide = pen(0xFFDD498B, 1.0);
            if let Some(x) = vertical {
                let x = v.world_to_screen(Point::new(x, 0.0)).x;
                ctx.draw_line(&guide, UiPoint::new(x, 0.0), UiPoint::new(x, v.height));
            }
            if let Some(y) = horizontal {
                let y = v.world_to_screen(Point::new(0.0, y)).y;
                ctx.draw_line(&guide, UiPoint::new(0.0, y), UiPoint::new(v.width, y));
            }
        }
        let captions = Rect::from_points(
            v.screen_to_world(Point::new(-400.0, -30.0)),
            v.screen_to_world(Point::new(v.width, v.height)),
        );
        slices::draw(ctx, &editor, v);
        for (id, origin) in editor.scene.root_container_origins_in(captions) {
            let node = &doc.nodes[&id];
            let point = v.world_to_screen(origin);
            let color = if matches!(node.kind, NodeKind::Component | NodeKind::Instance) {
                "#9747FF"
            } else if this.dark.get() { "#999999" } else { "#777777" };
            // Reuse the world transform prepared by the spatial index, including shear/rotation.
            let transform = editor.scene.transforms[&id];
            let local = node.local_bounds();
            let world_width = transform.a.abs() * local.width + transform.c.abs() * local.height;
            canvas_caption(ctx, &node.name, Point::new(point.x, point.y - 20.0), world_width * v.zoom, color);
        }
        let measuring = this.measurement_target();
        if let Some(id) = measuring.or_else(|| this.highlighted_layer()) {
            let outline = if measuring.is_some() { pen(measure::GUIDE_COLOR, 1.0) } else { blue.clone() };
            if !editor.selection.contains(&id) && doc.effective_visible(id) {
                if let (Some(node), Some(transform)) = (doc.nodes.get(&id), editor.scene.transforms.get(&id)) {
                    let points = node.local_bounds().corners()
                        .map(|p| ui(v.world_to_screen(transform.apply(p))));
                    if node.kind == NodeKind::Line {
                        ctx.draw_line(&outline, points[0], points[2]);
                    } else {
                        for i in 0..4 {
                            ctx.draw_line(&outline, points[i], points[(i + 1) % 4]);
                        }
                    }
                }
            }
        }
        let white = brush("#FFFFFF");
        let scale_mode = this.tool.get() == Tool::Scale;
        let gesture = this.gesture.borrow();
        let marquee = match &*gesture {
            Gesture::Marquee(drag) => Some(drag),
            _ => None,
        };
        if marquee.is_none() && (scale_mode || (this.tool.get() == Tool::Select && editor.selection.len() > 1
            && matches!(*gesture, Gesture::None | Gesture::MultiResize(_) | Gesture::RotationOrigin(_)))) {
            this.render_scale_handles(ctx, &editor, v);
        }
        let selection = match &*gesture {
            Gesture::Marquee(drag) => drag.preview.as_slice(),
            Gesture::AltDrag(drag) if drag.is_previewing(&editor) => &[],
            _ => editor.selection.as_slice(),
        };
        for &id in selection {
            let Some(n) = doc.nodes.get(&id) else {
                continue;
            };
            let Some(&t) = editor.scene.transforms.get(&id) else {
                continue;
            };
            if marquee.is_some() {
                let points = n.local_bounds().corners().map(|p| ui(v.world_to_screen(t.apply(p))));
                if n.kind == NodeKind::Line {
                    ctx.draw_line(&blue, points[0], points[2]);
                } else {
                    for i in 0..4 { ctx.draw_line(&blue, points[i], points[(i + 1) % 4]); }
                }
                continue;
            }
            if this.render_linear_gradient(ctx, &editor, id) { continue; }
            if this.render_image_crop(ctx, n, t, v) { continue; }
            if this.render_stroke_width(ctx, &editor, id, t, v) { continue; }
            if this.vector_node.get() == Some(id) {
                if let Some(graph) = &n.vector_network {
                    this.render_bucket_hover(ctx, &editor, id, t, v);
                    network::render_network_editor(ctx, graph, t, v, this.network_point.get(), &this.network_vertices.borrow());
                } else { render_vector_editor(ctx, &n.path, t, v, this.vector_point.get()); }
                continue;
            }
            if this.pen_node.get() == Some(id) {
                let anchor = n
                    .path
                    .edit_points()
                    .into_iter()
                    .filter(|p| p.id.kind == PathPointKind::Anchor)
                    .last();
                render_vector_editor(ctx, &n.path, t, v, anchor.as_ref().map(|p| p.id));
                if let (Some(anchor), Some(tangent)) = (anchor, this.pen_tangent.get()) {
                    let a = v.world_to_screen(t.apply(anchor.position));
                    let b = v.world_to_screen(t.apply(tangent));
                    ctx.draw_line(&blue, ui(a), ui(b));
                    ctx.draw_ellipse_at(Some(&white), Some(&blue), ui(b), 3.5, 3.5);
                }
                continue;
            }
            let corners = n
                .local_bounds()
                .corners()
                .map(|p| v.world_to_screen(t.apply(p)));
            if scale_mode {
                for i in 0..4 { ctx.draw_line(&blue, ui(corners[i]), ui(corners[(i + 1) % 4])); }
                continue;
            }
            let show_grips = editor.selection.len() == 1 && this.tool.get() == Tool::Select
                && !this.space.get() && !doc.effective_locked(id) && doc.effective_visible(id)
                && t.inverse().is_some();
            if n.kind == NodeKind::Line {
                ctx.draw_line(&blue, ui(corners[0]), ui(corners[2]));
                if show_grips {
                    for point in [corners[0], corners[2]] {
                        ctx.draw_ellipse_at(Some(&white), Some(&blue), ui(point), 3.5, 3.5);
                    }
                }
                continue;
            }
            for i in 0..4 {
                ctx.draw_line(&blue, ui(corners[i]), ui(corners[(i + 1) % 4]));
            }
            if show_grips {
                if let Some((_, edge)) = this.hovered_resize_edge.get().filter(|(target, _)| *target == id) {
                    ctx.draw_line(&pen(0xFF0D99FF, 2.0), ui(corners[edge]), ui(corners[(edge + 1) % 4]));
                }
                for p in corners {
                    ctx.draw_rectangle(
                        Some(&white),
                        Some(&blue),
                        UiRect::new(p.x - 3.0, p.y - 3.0, 6.0, 6.0),
                        0.0,
                        0.0,
                        &BoxShadows::default(),
                    );
                }
            }
            if editor.selection.len() == 1 {
                let bounds = Rect::from_points(corners[0], corners[2])
                    .union(Rect::from_points(corners[1], corners[3]));
                if measuring.is_none() && !matches!(*gesture, Gesture::Rotate(_)) { this.draw_size_badge(ctx, resize::BadgeValue::Dimensions(n.width, n.height), bounds, v); }
                this.render_corner_radius(ctx, &editor, id);
            }
        }
        this.render_rotation(ctx, &editor, v, &gesture);
        if let Some(drag) = marquee {
            let a = v.world_to_screen(drag.start);
            let b = v.world_to_screen(drag.current);
            let fill = brush("#200D99FF");
            ctx.draw_rectangle(
                Some(&fill),
                Some(&blue),
                rect(Rect::from_points(a, b)),
                0.0,
                0.0,
                &BoxShadows::default(),
            );
        }
        if let Gesture::AltDrag(drag) = &*gesture { drag.draw_guides(ctx, &editor, v); }
        this.render_layout_drag(ctx, v);
        this.draw_measurements(ctx, &editor, v);
        for comment in &doc.comments {
            if !comment.resolved && doc.comment_page(comment) == Some(editor.active_page) {
                let p = v.world_to_screen(comment.position);
                let fill = brush("#FFCD29");
                ctx.draw_ellipse_at(Some(&fill), None, ui(p), 10.0, 10.0);
                label(ctx, "C", Point::new(p.x - 3.5, p.y - 6.0), 10.0, "#2A2200");
            }
        }
        this.render_rulers(ctx, &editor, v);
    }
}
