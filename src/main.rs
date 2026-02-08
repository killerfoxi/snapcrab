#![windows_subsystem = "windows"]

use arboard::Clipboard;
use color_eyre::eyre::Result;
use eframe::egui;
use egui::{Color32, Painter, Pos2, Rect, Stroke, StrokeKind, Vec2};
use xcap::{image, Monitor, Window};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tool {
    Arrow,
    Rect,
    Text,
    Crop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppState {
    Normal,
    PickingWindow,
    PickingArea,
}

#[derive(Debug, Clone)]
enum Annotation {
    Arrow {
        start: Pos2,
        end: Pos2,
        color: Color32,
        thickness: f32,
    },
    Rect {
        rect: Rect,
        color: Color32,
        thickness: f32,
    },
    Text {
        pos: Pos2,
        text: String,
        color: Color32,
        size: f32,
    },
}

struct WindowInfo {
    rect: Rect,
    title: String,
    app_name: String,
}

impl Annotation {
    fn label(&self) -> String {
        match self {
            Annotation::Arrow { .. } => "↗ Arrow".to_string(),
            Annotation::Rect { .. } => "⬜ Box".to_string(),
            Annotation::Text { text, .. } => format!("T \"{}\"", text),
        }
    }

    fn hit_test(&self, p: Pos2, threshold: f32) -> bool {
        match self {
            Annotation::Arrow { start, end, .. } => {
                let line_vec = *end - *start;
                let len_sq = line_vec.length_sq();
                if len_sq < 1.0 {
                    return p.distance(*start) < threshold;
                }
                let t = ((p - *start).dot(line_vec) / len_sq).clamp(0.0, 1.0);
                let projection = *start + line_vec * t;
                p.distance(projection) < threshold
            }
            Annotation::Rect { rect, .. } => {
                rect.expand(threshold).contains(p)
                    && (!rect.shrink(threshold).contains(p) || rect.contains(p))
            }
            Annotation::Text {
                pos, text, size, ..
            } => {
                let rect =
                    Rect::from_min_size(*pos, Vec2::new(text.len() as f32 * *size * 0.6, *size));
                rect.expand(threshold).contains(p)
            }
        }
    }

    fn translate(&mut self, delta: Vec2) {
        match self {
            Annotation::Arrow { start, end, .. } => {
                *start += delta;
                *end += delta;
            }
            Annotation::Rect { rect, .. } => {
                *rect = rect.translate(delta);
            }
            Annotation::Text { pos, .. } => {
                *pos += delta;
            }
        }
    }
}

struct SnapCrabApp {
    image: Option<egui::TextureHandle>,
    original_image: Option<image::RgbaImage>,
    annotations: Vec<Annotation>,
    current_tool: Tool,
    current_color: Color32,
    stroke_thickness: f32,
    text_size: f32,

    // Interaction
    drag_start: Option<Pos2>,
    active_annotation_index: Option<usize>,
    show_layers: bool,
    temp_text: String,
    editing_text_pos: Option<Pos2>,

    // Picking State
    state: AppState,
    fullscreen_bg: Option<egui::TextureHandle>,
    fullscreen_bg_image: Option<image::RgbaImage>,
    windows: Vec<WindowInfo>,
    hovered_window_index: Option<usize>,
}

impl SnapCrabApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let visuals = egui::Visuals::dark();
        cc.egui_ctx.set_visuals(visuals);
        egui_extras::install_image_loaders(&cc.egui_ctx);

        Self {
            image: None,
            original_image: None,
            annotations: Vec::new(),
            current_tool: Tool::Arrow,
            current_color: Color32::RED,
            stroke_thickness: 4.0,
            text_size: 24.0,
            drag_start: None,
            active_annotation_index: None,
            show_layers: true,
            temp_text: String::new(),
            editing_text_pos: None,
            state: AppState::Normal,
            fullscreen_bg: None,
            fullscreen_bg_image: None,
            windows: Vec::new(),
            hovered_window_index: None,
        }
    }

    fn enter_pick_mode(&mut self, state: AppState, ctx: &egui::Context) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        std::thread::sleep(std::time::Duration::from_millis(350));

        if let Ok(monitors) = Monitor::all() {
            if let Some(monitor) = monitors.first() {
                if let Ok(image) = monitor.capture_image() {
                    let color_image = egui::ColorImage::from_rgba_unmultiplied(
                        [image.width() as usize, image.height() as usize],
                        image.as_flat_samples().as_slice(),
                    );
                    self.fullscreen_bg =
                        Some(ctx.load_texture("fullscreen_bg", color_image, Default::default()));
                    self.fullscreen_bg_image = Some(image);
                }
            }
        }

        if state == AppState::PickingWindow {
            self.windows.clear();
            if let Ok(windows) = Window::all() {
                for w in windows {
                    let title = w.title().unwrap_or_default();
                    let app_name = w.app_name().unwrap_or_default();
                    let minimized = w.is_minimized().unwrap_or(false);

                    let is_valid = !minimized
                        && !title.is_empty()
                        && title != "SnapCrab"
                        && app_name != "SnapCrab"
                        && title != "Program Manager"
                        && !title.starts_with("ms-")
                        && w.width().unwrap_or(0) > 10
                        && w.height().unwrap_or(0) > 10;

                    if is_valid {
                        self.windows.push(WindowInfo {
                            rect: Rect::from_min_size(
                                Pos2::new(w.x().unwrap_or(0) as f32, w.y().unwrap_or(0) as f32),
                                Vec2::new(
                                    w.width().unwrap_or(0) as f32,
                                    w.height().unwrap_or(0) as f32,
                                ),
                            ),
                            title,
                            app_name,
                        });
                    }
                }
                self.windows.sort_by(|a, b| {
                    (a.rect.width() * a.rect.height())
                        .partial_cmp(&(b.rect.width() * b.rect.height()))
                        .unwrap()
                });
            }
        }

        self.state = state;
        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
    }

    fn exit_pick_mode(&mut self, ctx: &egui::Context) {
        self.state = AppState::Normal;
        self.fullscreen_bg = None;
        self.fullscreen_bg_image = None;
        self.windows.clear();
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(true));
    }

    fn load_captured_image(&mut self, image: image::RgbaImage, ctx: &egui::Context) {
        let width = image.width();
        let height = image.height();
        let color_image = egui::ColorImage::from_rgba_unmultiplied(
            [width as usize, height as usize],
            image.as_flat_samples().as_slice(),
        );
        self.image = Some(ctx.load_texture("screenshot", color_image, Default::default()));
        self.original_image = Some(image);
        self.annotations.clear();
    }

    fn ui_to_image(&self, ui_pos: Pos2, image_rect: Rect) -> Pos2 {
        if let Some(ref original) = self.original_image {
            let normalized = (ui_pos - image_rect.min) / image_rect.size();
            Pos2::new(
                normalized.x * original.width() as f32,
                normalized.y * original.height() as f32,
            )
        } else {
            ui_pos
        }
    }

    fn image_to_ui(&self, img_pos: Pos2, image_rect: Rect) -> Pos2 {
        if let Some(ref original) = self.original_image {
            let normalized = Vec2::new(
                img_pos.x / original.width() as f32,
                img_pos.y / original.height() as f32,
            );
            image_rect.min + normalized * image_rect.size()
        } else {
            img_pos
        }
    }

    fn draw_picking_ui(&mut self, ctx: &egui::Context) {
        egui::Area::new(egui::Id::new("picking_area"))
            .fixed_pos(Pos2::ZERO)
            .show(ctx, |ui| {
                let screen_rect = ctx.viewport_rect();
                let (response, painter) =
                    ui.allocate_painter(screen_rect.size(), egui::Sense::click_and_drag());

                if let Some(texture) = &self.fullscreen_bg {
                    painter.image(
                        texture.id(),
                        screen_rect,
                        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }

                let pointer_pos = ctx.pointer_latest_pos().unwrap_or_default();

                match self.state {
                    AppState::PickingWindow => {
                        self.hovered_window_index = self
                            .windows
                            .iter()
                            .enumerate()
                            .find(|(_, w)| w.rect.contains(pointer_pos))
                            .map(|(i, _)| i);
                        if let Some(idx) = self.hovered_window_index {
                            let w = &self.windows[idx];
                            painter.rect_filled(
                                w.rect,
                                0.0,
                                Color32::from_rgba_unmultiplied(0, 100, 255, 60),
                            );
                            painter.rect_stroke(
                                w.rect,
                                0.0,
                                Stroke::new(2.5, Color32::from_rgb(0, 200, 255)),
                                StrokeKind::Outside,
                            );
                            painter.text(
                                w.rect.left_top() + Vec2::new(10.0, 10.0),
                                egui::Align2::LEFT_TOP,
                                format!("{} ({})", w.title, w.app_name),
                                egui::FontId::proportional(16.0),
                                Color32::WHITE,
                            );
                        }
                        if response.clicked() {
                            if let Some(idx) = self.hovered_window_index {
                                let rect = self.windows[idx].rect;
                                let title = self.windows[idx].title.clone();
                                let app = self.windows[idx].app_name.clone();

                                let mut captured = false;
                                if let Ok(windows) = Window::all() {
                                    if let Some(target) = windows.iter().find(|w| {
                                        w.title().unwrap_or_default() == title
                                            && w.app_name().unwrap_or_default() == app
                                    }) {
                                        if let Ok(img) = target.capture_image() {
                                            self.load_captured_image(img, ctx);
                                            captured = true;
                                        }
                                    }
                                }

                                if !captured {
                                    if let Some(bg) = &self.fullscreen_bg_image {
                                        let x = rect.min.x.max(0.0) as u32;
                                        let y = rect.min.y.max(0.0) as u32;
                                        let w =
                                            (rect.width() as u32).min(bg.width().saturating_sub(x));
                                        let h = (rect.height() as u32)
                                            .min(bg.height().saturating_sub(y));
                                        if w > 0 && h > 0 {
                                            let img = image::imageops::crop_imm(bg, x, y, w, h)
                                                .to_image();
                                            self.load_captured_image(img, ctx);
                                        }
                                    }
                                }
                            }
                            self.exit_pick_mode(ctx);
                        }
                    }
                    AppState::PickingArea => {
                        if response.drag_started() {
                            self.drag_start = response.interact_pointer_pos();
                        }
                        if let Some(start) = self.drag_start {
                            let area = Rect::from_two_pos(start, pointer_pos);
                            let black = Color32::from_black_alpha(180);
                            painter.rect_filled(
                                Rect::from_min_max(
                                    screen_rect.min,
                                    Pos2::new(screen_rect.max.x, area.min.y),
                                ),
                                0.0,
                                black,
                            );
                            painter.rect_filled(
                                Rect::from_min_max(
                                    Pos2::new(screen_rect.min.x, area.min.y),
                                    Pos2::new(area.min.x, area.max.y),
                                ),
                                0.0,
                                black,
                            );
                            painter.rect_filled(
                                Rect::from_min_max(
                                    Pos2::new(area.max.x, area.min.y),
                                    Pos2::new(screen_rect.max.x, area.max.y),
                                ),
                                0.0,
                                black,
                            );
                            painter.rect_filled(
                                Rect::from_min_max(
                                    Pos2::new(screen_rect.min.x, area.max.y),
                                    screen_rect.max,
                                ),
                                0.0,
                                black,
                            );
                            painter.rect_stroke(
                                area,
                                0.0,
                                Stroke::new(2.0, Color32::WHITE),
                                StrokeKind::Outside,
                            );
                        }
                        if response.drag_stopped() {
                            if let (Some(start), Some(end)) =
                                (self.drag_start, response.interact_pointer_pos())
                            {
                                let rect = Rect::from_two_pos(start, end);
                                if rect.width() > 5.0 && rect.height() > 5.0 {
                                    if let Some(bg) = &self.fullscreen_bg_image {
                                        let x = rect.min.x.max(0.0) as u32;
                                        let y = rect.min.y.max(0.0) as u32;
                                        let w =
                                            (rect.width() as u32).min(bg.width().saturating_sub(x));
                                        let h = (rect.height() as u32)
                                            .min(bg.height().saturating_sub(y));
                                        if w > 0 && h > 0 {
                                            let img = image::imageops::crop_imm(bg, x, y, w, h)
                                                .to_image();
                                            self.load_captured_image(img, ctx);
                                        }
                                    }
                                }
                            }
                            self.drag_start = None;
                            self.exit_pick_mode(ctx);
                        }
                    }
                    _ => {}
                }
                if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                    self.exit_pick_mode(ctx);
                }
            });
    }

    fn draw_annotation(
        &self,
        painter: &Painter,
        ann: &Annotation,
        image_rect: Rect,
        is_active: bool,
    ) {
        let display_scale = image_rect.width()
            / self
                .original_image
                .as_ref()
                .map(|i| i.width() as f32)
                .unwrap_or(1.0);
        if is_active {
            match ann {
                Annotation::Arrow { start, end, .. } => {
                    painter.line_segment(
                        [
                            self.image_to_ui(*start, image_rect),
                            self.image_to_ui(*end, image_rect),
                        ],
                        Stroke::new(10.0 * display_scale, Color32::from_white_alpha(30)),
                    );
                }
                Annotation::Rect { rect, .. } => {
                    let r = Rect::from_min_max(
                        self.image_to_ui(rect.min, image_rect),
                        self.image_to_ui(rect.max, image_rect),
                    );
                    painter.rect_filled(r.expand(2.0), 0.0, Color32::from_white_alpha(20));
                }
                Annotation::Text {
                    pos, text, size, ..
                } => {
                    let p = self.image_to_ui(*pos, image_rect);
                    let s = *size * display_scale;
                    let r = Rect::from_min_size(p, Vec2::new(text.len() as f32 * s * 0.6, s));
                    painter.rect_filled(r.expand(4.0), 0.0, Color32::from_white_alpha(30));
                }
            }
        }
        match ann {
            Annotation::Arrow {
                start,
                end,
                color,
                thickness,
            } => {
                let s_ui = self.image_to_ui(*start, image_rect);
                let e_ui = self.image_to_ui(*end, image_rect);
                let thick = *thickness * display_scale;
                painter.line_segment([s_ui, e_ui], Stroke::new(thick, *color));
                let dir = (e_ui - s_ui).normalized();
                if dir.is_finite() {
                    let side = Vec2::new(-dir.y, dir.x);
                    let head = thick * 3.0;
                    painter.line_segment(
                        [e_ui, e_ui - dir * head + side * head],
                        Stroke::new(thick, *color),
                    );
                    painter.line_segment(
                        [e_ui, e_ui - dir * head - side * head],
                        Stroke::new(thick, *color),
                    );
                }
            }
            Annotation::Rect {
                rect,
                color,
                thickness,
            } => {
                let r_ui = Rect::from_min_max(
                    self.image_to_ui(rect.min, image_rect),
                    self.image_to_ui(rect.max, image_rect),
                );
                painter.rect_stroke(
                    r_ui,
                    0.0,
                    Stroke::new(*thickness * display_scale, *color),
                    StrokeKind::Outside,
                );
            }
            Annotation::Text {
                pos,
                text,
                color,
                size,
            } => {
                painter.text(
                    self.image_to_ui(*pos, image_rect),
                    egui::Align2::LEFT_TOP,
                    text,
                    egui::FontId::proportional(*size * display_scale),
                    *color,
                );
            }
        }
    }
}

impl eframe::App for SnapCrabApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.state != AppState::Normal {
            self.draw_picking_ui(ctx);
            return;
        }

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.menu_button("📸 Capture", |ui| {
                    if ui.button("🖥 Fullscreen").clicked() {
                        if let Ok(mon) = Monitor::all() {
                            if let Some(m) = mon.first() {
                                if let Ok(img) = m.capture_image() {
                                    self.load_captured_image(img, ctx);
                                }
                            }
                        }
                        ui.close_kind(egui::UiKind::Menu);
                    }
                    if ui.button("🪟 Select Window").clicked() {
                        self.enter_pick_mode(AppState::PickingWindow, ctx);
                        ui.close_kind(egui::UiKind::Menu);
                    }
                    if ui.button("✂ Select Area").clicked() {
                        self.enter_pick_mode(AppState::PickingArea, ctx);
                        ui.close_kind(egui::UiKind::Menu);
                    }
                });

                ui.separator();
                ui.selectable_value(&mut self.current_tool, Tool::Arrow, "↗ Arrow");
                ui.selectable_value(&mut self.current_tool, Tool::Rect, "⬜ Box");
                ui.selectable_value(&mut self.current_tool, Tool::Text, "T Text");
                if self.image.is_some() {
                    ui.selectable_value(&mut self.current_tool, Tool::Crop, "✂ Crop");
                }

                ui.separator();
                ui.color_edit_button_srgba(&mut self.current_color);
                ui.add(egui::Slider::new(&mut self.stroke_thickness, 1.0..=20.0).text("Size"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.toggle_value(&mut self.show_layers, "🗂 Layers");
                    ui.separator();
                    if ui.button("💾 Save").clicked() {
                        if let Some(ref original) = self.original_image {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("PNG", &["png"])
                                .set_file_name("screenshot.png")
                                .save_file()
                            {
                                let _ = original.save(path);
                            }
                        }
                    }
                    if ui.button("📋 Copy").clicked() {
                        if let Some(ref original) = self.original_image {
                            if let Ok(mut clipboard) = Clipboard::new() {
                                let _ = clipboard.set_image(arboard::ImageData {
                                    width: original.width() as usize,
                                    height: original.height() as usize,
                                    bytes: std::borrow::Cow::Borrowed(original.as_raw()),
                                });
                            }
                        }
                    }
                    if ui.button("🗑 Clear").clicked() {
                        self.annotations.clear();
                    }
                });
            });
        });

        if self.show_layers {
            egui::SidePanel::right("layers_panel")
                .default_width(200.0)
                .show(ctx, |ui| {
                    ui.heading("Layers");
                    ui.separator();
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        let mut to_remove = None;
                        for (i, ann) in self.annotations.iter().enumerate().rev() {
                            ui.horizontal(|ui| {
                                if ui
                                    .selectable_label(
                                        self.active_annotation_index == Some(i),
                                        ann.label(),
                                    )
                                    .clicked()
                                {
                                    self.active_annotation_index = Some(i);
                                }
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui.button("🗑").clicked() {
                                            to_remove = Some(i);
                                        }
                                    },
                                );
                            });
                        }
                        if let Some(i) = to_remove {
                            self.annotations.remove(i);
                            self.active_annotation_index = None;
                        }
                    });
                });
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(texture) = &self.image {
                let available_size = ui.available_size();
                let scale = (available_size.x / texture.size_vec2().x)
                    .min(available_size.y / texture.size_vec2().y)
                    .min(1.0);
                let display_size = texture.size_vec2() * scale;
                let (rect, response) = ui.allocate_at_least(display_size, egui::Sense::drag());

                let mut mesh = egui::Mesh::with_texture(texture.id());
                mesh.add_rect_with_uv(
                    rect,
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
                ui.painter().add(egui::Shape::mesh(mesh));

                if response.drag_started() {
                    if let Some(pos_ui) = response.interact_pointer_pos() {
                        let pos_img = self.ui_to_image(pos_ui, rect);
                        if self.current_tool != Tool::Crop {
                            self.active_annotation_index = self
                                .annotations
                                .iter()
                                .enumerate()
                                .rev()
                                .find(|(_, ann)| ann.hit_test(pos_img, 10.0 / scale))
                                .map(|(i, _)| i);
                        }
                        if self.active_annotation_index.is_none() {
                            self.drag_start = Some(pos_ui);
                            if self.current_tool == Tool::Text {
                                self.editing_text_pos = Some(pos_ui);
                                self.temp_text.clear();
                            }
                        }
                    }
                }

                if response.dragged() {
                    if let Some(idx) = self.active_annotation_index {
                        let delta_img = response.drag_delta() / scale;
                        if let Some(ann) = self.annotations.get_mut(idx) {
                            ann.translate(delta_img);
                        }
                    }
                }

                if response.drag_stopped() {
                    if let (Some(start_ui), Some(end_ui)) =
                        (self.drag_start, response.interact_pointer_pos())
                    {
                        let start = self.ui_to_image(start_ui, rect);
                        let end = self.ui_to_image(end_ui, rect);
                        if start.distance(end) > 1.0 {
                            match self.current_tool {
                                Tool::Arrow => self.annotations.push(Annotation::Arrow {
                                    start,
                                    end,
                                    color: self.current_color,
                                    thickness: self.stroke_thickness,
                                }),
                                Tool::Rect => self.annotations.push(Annotation::Rect {
                                    rect: Rect::from_two_pos(start, end),
                                    color: self.current_color,
                                    thickness: self.stroke_thickness,
                                }),
                                Tool::Crop => {
                                    if let Some(ref bg) = self.original_image {
                                        let crop = Rect::from_two_pos(start, end);
                                        let img = image::imageops::crop_imm(
                                            bg,
                                            crop.min.x as u32,
                                            crop.min.y as u32,
                                            crop.width() as u32,
                                            crop.height() as u32,
                                        )
                                        .to_image();
                                        self.load_captured_image(img, ctx);
                                    }
                                    self.current_tool = Tool::Arrow;
                                }
                                _ => {}
                            }
                        }
                    }
                    self.drag_start = None;
                }

                let painter = ui.painter_at(rect);
                for (i, ann) in self.annotations.iter().enumerate() {
                    self.draw_annotation(
                        &painter,
                        ann,
                        rect,
                        self.active_annotation_index == Some(i),
                    );
                }

                if let (Some(start_ui), Some(end_ui)) = (self.drag_start, ctx.pointer_latest_pos())
                {
                    if self.current_tool == Tool::Crop {
                        painter.rect_stroke(
                            Rect::from_two_pos(start_ui, end_ui),
                            0.0,
                            Stroke::new(2.0, Color32::WHITE),
                            StrokeKind::Outside,
                        );
                    } else {
                        let start = self.ui_to_image(start_ui, rect);
                        let end = self.ui_to_image(end_ui, rect);
                        let temp_ann = match self.current_tool {
                            Tool::Arrow => Some(Annotation::Arrow {
                                start,
                                end,
                                color: self.current_color,
                                thickness: self.stroke_thickness,
                            }),
                            Tool::Rect => Some(Annotation::Rect {
                                rect: Rect::from_two_pos(start, end),
                                color: self.current_color,
                                thickness: self.stroke_thickness,
                            }),
                            _ => None,
                        };
                        if let Some(ann) = temp_ann {
                            self.draw_annotation(&painter, &ann, rect, false);
                        }
                    }
                }

                if let Some(pos_ui) = self.editing_text_pos {
                    egui::Window::new("Enter Text")
                        .fixed_pos(pos_ui)
                        .title_bar(false)
                        .collapsible(false)
                        .resizable(false)
                        .show(ctx, |ui| {
                            let res = ui.text_edit_singleline(&mut self.temp_text);
                            res.request_focus();
                            if res.lost_focus() || ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
                                if !self.temp_text.is_empty() {
                                    let pos = self.ui_to_image(pos_ui, rect);
                                    self.annotations.push(Annotation::Text {
                                        pos,
                                        text: self.temp_text.clone(),
                                        color: self.current_color,
                                        size: self.text_size,
                                    });
                                }
                                self.editing_text_pos = None;
                            }
                        });
                }
            } else {
                ui.centered_and_justified(|ui| {
                    ui.vertical(|ui| {
                        ui.heading("SnapCrab");
                        ui.label("Select a capture mode to begin");
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            if ui.button("🖥 Fullscreen").clicked() {
                                if let Ok(mon) = Monitor::all() {
                                    if let Some(m) = mon.first() {
                                        if let Ok(img) = m.capture_image() {
                                            self.load_captured_image(img, ctx);
                                        }
                                    }
                                }
                            }
                            if ui.button("🪟 Window").clicked() {
                                self.enter_pick_mode(AppState::PickingWindow, ctx);
                            }
                            if ui.button("✂ Area").clicked() {
                                self.enter_pick_mode(AppState::PickingArea, ctx);
                            }
                        });
                    });
                });
            }
        });
    }
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_title("SnapCrab"),
        ..Default::default()
    };
    eframe::run_native(
        "SnapCrab",
        options,
        Box::new(|cc| Ok(Box::new(SnapCrabApp::new(cc)))),
    )
    .map_err(|e| color_eyre::eyre::eyre!(e.to_string()))?;
    Ok(())
}
