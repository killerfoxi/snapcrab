#![windows_subsystem = "windows"]
#![deny(clippy::pedantic)]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use arboard::Clipboard;
use color_eyre::eyre::{eyre, Result};
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
            Self::Arrow { .. } => "↗ Arrow".to_string(),
            Self::Rect { .. } => "⬜ Box".to_string(),
            Self::Text { text, .. } => format!("T \"{text}\""),
        }
    }

    fn hit_test(&self, p: Pos2, threshold: f32) -> bool {
        match self {
            Self::Arrow { start, end, .. } => {
                let line_vec = *end - *start;
                let len_sq = line_vec.length_sq();
                if len_sq < 1.0 {
                    return p.distance(*start) < threshold;
                }
                let t = ((p - *start).dot(line_vec) / len_sq).clamp(0.0, 1.0);
                p.distance(*start + line_vec * t) < threshold
            }
            Self::Rect { rect, .. } => {
                rect.expand(threshold).contains(p)
                    && (!rect.shrink(threshold).contains(p) || rect.contains(p))
            }
            Self::Text {
                pos, text, size, ..
            } => Rect::from_min_size(*pos, Vec2::new(text.len() as f32 * *size * 0.6, *size))
                .expand(threshold)
                .contains(p),
        }
    }

    fn translate(&mut self, delta: Vec2) {
        match self {
            Self::Arrow { start, end, .. } => {
                *start += delta;
                *end += delta;
            }
            Self::Rect { rect, .. } => *rect = rect.translate(delta),
            Self::Text { pos, .. } => *pos += delta,
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
    drag_start: Option<Pos2>,
    active_annotation_index: Option<usize>,
    show_layers: bool,
    temp_text: String,
    editing_text_pos: Option<Pos2>,
    state: AppState,
    fullscreen_bg: Option<egui::TextureHandle>,
    fullscreen_bg_image: Option<image::RgbaImage>,
    windows: Vec<WindowInfo>,
    hovered_window_index: Option<usize>,
}

impl SnapCrabApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
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

    fn ui_to_image(&self, ui_pos: Pos2, image_rect: Rect) -> Pos2 {
        let Some(ref original) = self.original_image else {
            return ui_pos;
        };
        let normalized = (ui_pos - image_rect.min) / image_rect.size();
        Pos2::new(
            normalized.x * original.width() as f32,
            normalized.y * original.height() as f32,
        )
    }

    fn image_to_ui(&self, img_pos: Pos2, image_rect: Rect) -> Pos2 {
        let Some(ref original) = self.original_image else {
            return img_pos;
        };
        let normalized = Vec2::new(
            img_pos.x / original.width() as f32,
            img_pos.y / original.height() as f32,
        );
        image_rect.min + normalized * image_rect.size()
    }

    fn load_captured_image(&mut self, img: image::RgbaImage, ctx: &egui::Context) {
        let color_img = egui::ColorImage::from_rgba_unmultiplied(
            [img.width() as usize, img.height() as usize],
            img.as_flat_samples().as_slice(),
        );
        self.image = Some(ctx.load_texture("screenshot", color_img, Default::default()));
        self.original_image = Some(img);
        self.annotations.clear();
    }

    fn enter_pick_mode(&mut self, state: AppState, ctx: &egui::Context) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        std::thread::sleep(std::time::Duration::from_millis(350));
        if let Ok(Some(img)) =
            Monitor::all().map(|m| m.first().and_then(|f| f.capture_image().ok()))
        {
            let color_img = egui::ColorImage::from_rgba_unmultiplied(
                [img.width() as usize, img.height() as usize],
                img.as_flat_samples().as_slice(),
            );
            self.fullscreen_bg =
                Some(ctx.load_texture("fullscreen_bg", color_img, Default::default()));
            self.fullscreen_bg_image = Some(img);
        }
        if state == AppState::PickingWindow {
            self.refresh_windows();
        }
        self.state = state;
        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
    }

    fn refresh_windows(&mut self) {
        self.windows.clear();
        let Ok(windows) = Window::all() else {
            return;
        };
        for w in windows {
            let (title, app, min) = (
                w.title().unwrap_or_default(),
                w.app_name().unwrap_or_default(),
                w.is_minimized().unwrap_or(false),
            );
            if min
                || title.is_empty()
                || title == "SnapCrab"
                || app == "SnapCrab"
                || title == "Program Manager"
                || title.starts_with("ms-")
                || w.width().unwrap_or(0) <= 10
                || w.height().unwrap_or(0) <= 10
            {
                continue;
            }
            self.windows.push(WindowInfo {
                rect: Rect::from_min_size(
                    Pos2::new(w.x().unwrap_or(0) as f32, w.y().unwrap_or(0) as f32),
                    Vec2::new(
                        w.width().unwrap_or(0) as f32,
                        w.height().unwrap_or(0) as f32,
                    ),
                ),
                title,
                app_name: app,
            });
        }
        self.windows.sort_by(|a, b| {
            (a.rect.width() * a.rect.height())
                .partial_cmp(&(b.rect.width() * b.rect.height()))
                .unwrap()
        });
    }

    fn exit_pick_mode(&mut self, ctx: &egui::Context) {
        self.state = AppState::Normal;
        self.fullscreen_bg = None;
        self.fullscreen_bg_image = None;
        self.windows.clear();
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(true));
    }

    fn draw_picking_ui(&mut self, ctx: &egui::Context) {
        egui::Area::new(egui::Id::new("picking_area"))
            .fixed_pos(Pos2::ZERO)
            .show(ctx, |ui| {
                let screen_rect = ctx.viewport_rect();
                let (resp, painter) =
                    ui.allocate_painter(screen_rect.size(), egui::Sense::click_and_drag());
                if let Some(tex) = &self.fullscreen_bg {
                    painter.image(
                        tex.id(),
                        screen_rect,
                        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
                let ptr = ctx.pointer_latest_pos().unwrap_or_default();
                match self.state {
                    AppState::PickingWindow => {
                        self.handle_picking_window(ctx, &resp, &painter, ptr)
                    }
                    AppState::PickingArea => {
                        self.handle_picking_area(ctx, &resp, &painter, ptr, screen_rect)
                    }
                    AppState::Normal => {}
                }
                if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                    self.exit_pick_mode(ctx);
                }
            });
    }

    fn handle_picking_window(
        &mut self,
        ctx: &egui::Context,
        resp: &egui::Response,
        painter: &Painter,
        ptr: Pos2,
    ) {
        self.hovered_window_index = self
            .windows
            .iter()
            .enumerate()
            .find(|(_, w)| w.rect.contains(ptr))
            .map(|(i, _)| i);
        let Some(w) = self
            .hovered_window_index
            .and_then(|idx| self.windows.get(idx))
        else {
            return;
        };
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
        if !resp.clicked() {
            return;
        }
        let (rect, title, app) = (w.rect, w.title.clone(), w.app_name.clone());
        if let Ok(Some(img)) = Window::all().map(|v| {
            v.into_iter()
                .find(|w| {
                    w.title().unwrap_or_default() == title
                        && w.app_name().unwrap_or_default() == app
                })
                .and_then(|t| t.capture_image().ok())
        }) {
            self.load_captured_image(img, ctx);
        } else if let Some(ref bg) = self.fullscreen_bg_image {
            let (x, y) = (rect.min.x.max(0.0) as u32, rect.min.y.max(0.0) as u32);
            let (w, h) = (
                (rect.width() as u32).min(bg.width().saturating_sub(x)),
                (rect.height() as u32).min(bg.height().saturating_sub(y)),
            );
            if w > 0 && h > 0 {
                self.load_captured_image(image::imageops::crop_imm(bg, x, y, w, h).to_image(), ctx);
            }
        }
        self.exit_pick_mode(ctx);
    }

    fn handle_picking_area(
        &mut self,
        ctx: &egui::Context,
        resp: &egui::Response,
        painter: &Painter,
        ptr: Pos2,
        screen: Rect,
    ) {
        if resp.drag_started() {
            self.drag_start = resp.interact_pointer_pos();
        }
        if let Some(start) = self.drag_start {
            let area = Rect::from_two_pos(start, ptr);
            let black = Color32::from_black_alpha(180);
            painter.rect_filled(
                Rect::from_min_max(screen.min, Pos2::new(screen.max.x, area.min.y)),
                0.0,
                black,
            );
            painter.rect_filled(
                Rect::from_min_max(
                    Pos2::new(screen.min.x, area.min.y),
                    Pos2::new(area.min.x, area.max.y),
                ),
                0.0,
                black,
            );
            painter.rect_filled(
                Rect::from_min_max(
                    Pos2::new(area.max.x, area.min.y),
                    Pos2::new(screen.max.x, area.max.y),
                ),
                0.0,
                black,
            );
            painter.rect_filled(
                Rect::from_min_max(Pos2::new(screen.min.x, area.max.y), screen.max),
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
        if !resp.drag_stopped() {
            return;
        }
        if let (Some(start), Some(end), Some(bg)) = (
            self.drag_start,
            resp.interact_pointer_pos(),
            &self.fullscreen_bg_image,
        ) {
            let rect = Rect::from_two_pos(start, end);
            if rect.width() > 5.0 && rect.height() > 5.0 {
                let (x, y) = (rect.min.x.max(0.0) as u32, rect.min.y.max(0.0) as u32);
                let (w, h) = (
                    (rect.width() as u32).min(bg.width().saturating_sub(x)),
                    (rect.height() as u32).min(bg.height().saturating_sub(y)),
                );
                if w > 0 && h > 0 {
                    self.load_captured_image(
                        image::imageops::crop_imm(bg, x, y, w, h).to_image(),
                        ctx,
                    );
                }
            }
        }
        self.drag_start = None;
        self.exit_pick_mode(ctx);
    }

    fn draw_annotation(&self, painter: &Painter, ann: &Annotation, rect: Rect, active: bool) {
        let scale = rect.width()
            / self
                .original_image
                .as_ref()
                .map_or(1.0, |i| i.width() as f32);
        if active {
            match ann {
                Annotation::Arrow { start, end, .. } => {
                    painter.line_segment(
                        [self.image_to_ui(*start, rect), self.image_to_ui(*end, rect)],
                        Stroke::new(10.0 * scale, Color32::from_white_alpha(30)),
                    );
                }
                Annotation::Rect { rect: r, .. } => {
                    painter.rect_filled(
                        Rect::from_min_max(
                            self.image_to_ui(r.min, rect),
                            self.image_to_ui(r.max, rect),
                        )
                        .expand(2.0),
                        0.0,
                        Color32::from_white_alpha(20),
                    );
                }
                Annotation::Text {
                    pos, text, size, ..
                } => {
                    let p = self.image_to_ui(*pos, rect);
                    let s = *size * scale;
                    painter.rect_filled(
                        Rect::from_min_size(p, Vec2::new(text.len() as f32 * s * 0.6, s))
                            .expand(4.0),
                        0.0,
                        Color32::from_white_alpha(30),
                    );
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
                let (s_ui, e_ui) = (self.image_to_ui(*start, rect), self.image_to_ui(*end, rect));
                let thick = *thickness * scale;
                painter.line_segment([s_ui, e_ui], Stroke::new(thick, *color));
                let dir = (e_ui - s_ui).normalized();
                if dir.is_finite() {
                    let (side, head) = (Vec2::new(-dir.y, dir.x), thick * 3.0);
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
                rect: r,
                color,
                thickness,
            } => {
                painter.rect_stroke(
                    Rect::from_min_max(
                        self.image_to_ui(r.min, rect),
                        self.image_to_ui(r.max, rect),
                    ),
                    0.0,
                    Stroke::new(*thickness * scale, *color),
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
                    self.image_to_ui(*pos, rect),
                    egui::Align2::LEFT_TOP,
                    text,
                    egui::FontId::proportional(*size * scale),
                    *color,
                );
            }
        }
    }

    fn draw_top_panel(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.menu_button("📸 Capture", |ui| {
                    if ui.button("🖥 Fullscreen").clicked() {
                        if let Ok(Some(img)) =
                            Monitor::all().map(|m| m.first().and_then(|f| f.capture_image().ok()))
                        {
                            self.load_captured_image(img, ctx);
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
                        self.save_to_file();
                    }
                    if ui.button("📋 Copy").clicked() {
                        self.copy_to_clipboard();
                    }
                    if ui.button("🗑 Clear").clicked() {
                        self.annotations.clear();
                    }
                });
            });
        });
    }

    fn save_to_file(&self) {
        let (Some(ref original), Some(path)) = (
            self.original_image.as_ref(),
            rfd::FileDialog::new()
                .add_filter("PNG", &["png"])
                .set_file_name("screenshot.png")
                .save_file(),
        ) else {
            return;
        };
        let _ = original.save(path);
    }

    fn copy_to_clipboard(&self) {
        let (Some(ref original), Ok(mut clipboard)) =
            (self.original_image.as_ref(), Clipboard::new())
        else {
            return;
        };
        let _ = clipboard.set_image(arboard::ImageData {
            width: original.width() as usize,
            height: original.height() as usize,
            bytes: std::borrow::Cow::Borrowed(original.as_raw()),
        });
    }

    fn draw_layers_panel(&mut self, ctx: &egui::Context) {
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

    fn draw_main_canvas(&mut self, ui: &mut egui::Ui) {
        let Some(texture) = &self.image else {
            self.draw_empty_state(ui);
            return;
        };
        let available = ui.available_size();
        let scale = (available.x / texture.size_vec2().x)
            .min(available.y / texture.size_vec2().y)
            .min(1.0);
        let (rect, resp) = ui.allocate_at_least(texture.size_vec2() * scale, egui::Sense::drag());
        let mut mesh = egui::Mesh::with_texture(texture.id());
        mesh.add_rect_with_uv(
            rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
        ui.painter().add(egui::Shape::mesh(mesh));
        self.handle_canvas_interactions(&resp, rect, scale, ui.ctx());
        let painter = ui.painter_at(rect);
        for (i, ann) in self.annotations.iter().enumerate() {
            self.draw_annotation(&painter, ann, rect, self.active_annotation_index == Some(i));
        }
        self.draw_drawing_preview(ui.ctx(), &painter, rect);
        self.handle_text_editing(ui.ctx(), rect);
    }

    fn draw_empty_state(&mut self, ui: &mut egui::Ui) {
        ui.centered_and_justified(|ui| {
            ui.vertical(|ui| {
                ui.heading("SnapCrab");
                ui.label("Select a capture mode to begin");
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("🖥 Fullscreen").clicked() {
                        if let Ok(Some(img)) =
                            Monitor::all().map(|m| m.first().and_then(|f| f.capture_image().ok()))
                        {
                            self.load_captured_image(img, ui.ctx());
                        }
                    }
                    if ui.button("🪟 Window").clicked() {
                        self.enter_pick_mode(AppState::PickingWindow, ui.ctx());
                    }
                    if ui.button("✂ Area").clicked() {
                        self.enter_pick_mode(AppState::PickingArea, ui.ctx());
                    }
                });
            });
        });
    }

    fn handle_canvas_interactions(
        &mut self,
        resp: &egui::Response,
        rect: Rect,
        scale: f32,
        ctx: &egui::Context,
    ) {
        if resp.drag_started() {
            let Some(pos_ui) = resp.interact_pointer_pos() else {
                return;
            };
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
        if resp.dragged() {
            let (Some(idx), delta) = (self.active_annotation_index, resp.drag_delta() / scale)
            else {
                return;
            };
            if let Some(ann) = self.annotations.get_mut(idx) {
                ann.translate(delta);
            }
        }
        if resp.drag_stopped() {
            if let (Some(start), Some(end)) = (self.drag_start, resp.interact_pointer_pos()) {
                self.finalize_drawing(start, end, rect, ctx);
            }
            self.drag_start = None;
        }
    }

    fn finalize_drawing(&mut self, start_ui: Pos2, end_ui: Pos2, rect: Rect, ctx: &egui::Context) {
        let (start, end) = (
            self.ui_to_image(start_ui, rect),
            self.ui_to_image(end_ui, rect),
        );
        if start.distance(end) <= 1.0 {
            return;
        }
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
                    let (x, y) = (crop.min.x as u32, crop.min.y as u32);
                    let (w, h) = (
                        (crop.width() as u32).min(bg.width().saturating_sub(x)),
                        (rect.height() as u32).min(bg.height().saturating_sub(y)),
                    );
                    if w > 0 && h > 0 {
                        self.load_captured_image(
                            image::imageops::crop_imm(bg, x, y, w, h).to_image(),
                            ctx,
                        );
                    }
                    self.current_tool = Tool::Arrow;
                }
            }
            _ => {}
        }
    }

    fn draw_drawing_preview(&self, ctx: &egui::Context, painter: &Painter, rect: Rect) {
        let (Some(start_ui), Some(end_ui)) = (self.drag_start, ctx.pointer_latest_pos()) else {
            return;
        };
        if self.current_tool == Tool::Crop {
            painter.rect_stroke(
                Rect::from_two_pos(start_ui, end_ui),
                0.0,
                Stroke::new(2.0, Color32::WHITE),
                StrokeKind::Outside,
            );
        } else {
            let temp = match self.current_tool {
                Tool::Arrow => Some(Annotation::Arrow {
                    start: self.ui_to_image(start_ui, rect),
                    end: self.ui_to_image(end_ui, rect),
                    color: self.current_color,
                    thickness: self.stroke_thickness,
                }),
                Tool::Rect => Some(Annotation::Rect {
                    rect: Rect::from_two_pos(
                        self.ui_to_image(start_ui, rect),
                        self.ui_to_image(end_ui, rect),
                    ),
                    color: self.current_color,
                    thickness: self.stroke_thickness,
                }),
                _ => None,
            };
            if let Some(ann) = temp {
                self.draw_annotation(painter, &ann, rect, false);
            }
        }
    }

    fn handle_text_editing(&mut self, ctx: &egui::Context, rect: Rect) {
        let Some(pos_ui) = self.editing_text_pos else {
            return;
        };
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
                        self.annotations.push(Annotation::Text {
                            pos: self.ui_to_image(pos_ui, rect),
                            text: self.temp_text.clone(),
                            color: self.current_color,
                            size: self.text_size,
                        });
                    }
                    self.editing_text_pos = None;
                }
            });
    }
}

impl eframe::App for SnapCrabApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.state != AppState::Normal {
            self.draw_picking_ui(ctx);
            return;
        }
        self.draw_top_panel(ctx);
        if self.show_layers {
            self.draw_layers_panel(ctx);
        }
        egui::CentralPanel::default().show(ctx, |ui| self.draw_main_canvas(ui));
    }
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let icon = image::load_from_memory(include_bytes!("../assets/snapcrab.png"))
        .map_err(|e| eyre!("Failed to load icon: {e}"))
        .ok()
        .map(|img| {
            let rgba = img.to_rgba8();
            let (width, height) = rgba.dimensions();
            egui::IconData {
                rgba: rgba.into_raw(),
                width,
                height,
            }
        });
    eframe::run_native(
        "SnapCrab",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1200.0, 800.0])
                .with_title("SnapCrab")
                .with_icon(icon.unwrap_or_default()),
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(SnapCrabApp::new(cc)))),
    )
    .map_err(|e| eyre!(e.to_string()))
}
