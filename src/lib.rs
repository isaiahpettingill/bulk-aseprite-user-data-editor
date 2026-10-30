pub mod aseprite;
#[cfg(not(target_arch = "wasm32"))]
mod external_editor;
mod io;
mod preview;
mod properties;
mod selection;
use aseprite::Document;
use eframe::egui;
use std::{collections::BTreeSet, path::PathBuf};
struct File {
    name: String,
    path: PathBuf,
    original: Vec<u8>,
    doc: Document,
    dirty: bool,
    is_new: bool,
    undo: Vec<Vec<u8>>,
    redo: Vec<Vec<u8>>,
}
#[derive(Default, PartialEq, Clone, Copy)]
enum Mode {
    #[default]
    Append,
    Override,
}
#[derive(Default, PartialEq, Clone, Copy)]
enum Tab {
    #[default]
    UserData,
    Properties,
    Tags,
}
struct SpritePreview {
    index: usize,
    image: Result<egui::TextureHandle, String>,
}

#[derive(Default)]
pub struct Editor {
    files: Vec<File>,
    selection: selection::Selection,
    collapsed: BTreeSet<String>,
    mode: Mode,
    tab: Tab,
    target: usize,
    text: String,
    properties: String,
    find: String,
    replace: String,
    status: String,
    loaded_selection: BTreeSet<usize>,
    loaded_target: usize,
    initial_text: String,
    initial_properties: String,
    color_edit: Option<Option<[u8; 4]>>,
    tag_edits: Option<Vec<aseprite::Tag>>,
    find_tab: bool,
    find_tags: bool,
    focus_find: bool,
    loaded_focus: Option<usize>,
    settings: bool,
    separator: String,
    error_files: Vec<String>,
    confirm_close: bool,
    allow_close: bool,
    pending_input: Vec<egui::Event>,
    open_clicks: u64,
    preview: Option<SpritePreview>,
    preview_decodes: u64,
    expand_right: u16,
    expand_bottom: u16,
    merge_reverse: bool,
    #[cfg(not(target_arch = "wasm32"))]
    external_editor: Option<Option<external_editor::EditorCommand>>,
    #[cfg(not(target_arch = "wasm32"))]
    external_pending: Option<usize>,
    #[cfg(not(target_arch = "wasm32"))]
    external_opened: Option<usize>,
    #[cfg(not(target_arch = "wasm32"))]
    window_focused: bool,
}
impl Editor {
    #[cfg(not(target_arch = "wasm32"))]
    fn request_external(&mut self, index: usize) {
        if self
            .external_editor
            .as_ref()
            .and_then(|e| e.as_ref())
            .is_none()
        {
            self.status = "Aseprite or LibreSprite is not installed".into();
            return;
        }
        if !self.commit_draft() {
            return;
        }
        if self.files[index].dirty {
            self.external_pending = Some(index);
        } else {
            self.launch_external(index);
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn launch_external(&mut self, index: usize) {
        let result = self
            .external_editor
            .as_ref()
            .and_then(|e| e.as_ref())
            .ok_or_else(|| "Aseprite or LibreSprite is not installed".to_string())
            .and_then(|editor| editor.launch(&self.files[index].path));
        match result {
            Ok(()) => {
                self.external_opened = Some(index);
                self.status = format!("Opened {} externally", self.files[index].name);
            }
            Err(error) => self.status = error,
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn save_one(&mut self, index: usize) -> Result<(), String> {
        let file = &mut self.files[index];
        let bytes = file.doc.encode()?;
        if file.is_new {
            io::save_new(&file.path, &bytes)?;
        } else {
            io::save(&file.path, &file.original, &bytes)?;
        }
        file.is_new = false;
        file.original = bytes;
        file.dirty = false;
        Ok(())
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn refresh_external(&mut self, index: usize) -> Result<(), String> {
        let file = &self.files[index];
        let metadata = std::fs::symlink_metadata(&file.path).map_err(|e| e.to_string())?;
        if !metadata.file_type().is_file() || metadata.len() > io::MAX_FILE as u64 {
            return Err("External file is not a supported regular file".into());
        }
        let remaining = 256 * 1024 * 1024
            - self.files.iter().map(|f| f.original.len()).sum::<usize>()
            + file.original.len();
        let limit = io::MAX_FILE.min(remaining);
        if metadata.len() > limit as u64 {
            return Err("External file exceeds session size limit".into());
        }
        use std::io::Read;
        let mut bytes = Vec::new();
        std::fs::File::open(&file.path)
            .map_err(|e| e.to_string())?
            .take(limit as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > limit {
            return Err("External file exceeds session size limit".into());
        }
        if bytes == file.original {
            return Ok(());
        }
        if file.dirty || (self.loaded_selection.contains(&index) && self.has_draft()) {
            return Err("File changed externally; unsaved edits retained. Save is blocked until the conflict is resolved.".into());
        }
        let doc = Document::parse(&bytes)?;
        let file = &mut self.files[index];
        file.original = bytes;
        file.doc = doc;
        file.undo.clear();
        file.redo.clear();
        if self
            .preview
            .as_ref()
            .is_some_and(|preview| preview.index == index)
        {
            self.preview = None;
        }
        if self.loaded_selection.contains(&index) {
            self.load_fields();
        }
        self.status = "Reloaded external changes".into();
        Ok(())
    }

    fn show_preview(&mut self, ui: &mut egui::Ui) {
        let Some(index) = self.loaded_focus else {
            return;
        };
        // Decode the staged document, not the on-disk original. Artwork actions
        // and history invalidate this cache; ordinary metadata typing does not.
        if self.preview.as_ref().is_none_or(|p| p.index != index) {
            self.preview_decodes += 1;
            let image = self.files[index]
                .doc
                .encode()
                .and_then(|bytes| preview::render_first_frame(&bytes))
                .map(|image| {
                    ui.ctx().load_texture(
                        "focused-sprite-preview",
                        egui::ColorImage::from_rgba_unmultiplied(
                            [image.width, image.height],
                            &image.rgba,
                        ),
                        egui::TextureOptions::NEAREST,
                    )
                });
            self.preview = Some(SpritePreview { index, image });
        }
        ui.add_space(8.0);
        ui.separator();
        ui.label(egui::RichText::new(&self.files[index].name).small());
        report_counter("previewDecodes", self.preview_decodes);
        match &self.preview.as_ref().unwrap().image {
            Ok(texture) => {
                let source = texture.size_vec2();
                let available = egui::vec2(ui.available_width().clamp(1.0, 240.0), 160.0);
                let fit = (available.x / source.x).min(available.y / source.y);
                let scale = if fit >= 1.0 {
                    fit.floor().min(8.0)
                } else {
                    fit
                };
                let size = source * scale;
                let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
                let painter = ui.painter().with_clip_rect(rect);
                for row in 0..(size.y / 8.0).ceil() as usize {
                    for col in 0..(size.x / 8.0).ceil() as usize {
                        let color = if (row + col) % 2 == 0 { 80 } else { 112 };
                        let cell = egui::Rect::from_min_size(
                            rect.min + egui::vec2(col as f32 * 8.0, row as f32 * 8.0),
                            egui::vec2(8.0, 8.0),
                        );
                        painter.rect_filled(cell, 0.0, egui::Color32::from_gray(color));
                    }
                }
                painter.image(
                    texture.id(),
                    rect,
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
                report_widget("preview", &response);
                report_counter("previewWidth", source.x as u64);
                report_counter("previewHeight", source.y as u64);
                response.on_hover_text("First frame · focused sprite");
            }
            Err(reason) => {
                ui.weak("Preview unavailable").on_hover_text(reason);
            }
        }
    }

    // egui TextEdit can lose text when a focus-changing pointer event is
    // coalesced into the same frame. Preserve event order across a frame
    // boundary rather than reconstructing text/cursor edits ourselves.
    fn prepare_input(&mut self, ctx: &egui::Context, input: &mut egui::RawInput) {
        let mut events = std::mem::take(&mut self.pending_input);
        events.append(&mut input.events);
        let mut pointer_seen = false;
        let mut text_seen = false;
        let mut boundary = None;
        for (i, event) in events.iter().enumerate() {
            let pointer = matches!(event, egui::Event::PointerButton { pressed: true, .. });
            let text = matches!(
                event,
                egui::Event::Text(_)
                    | egui::Event::Paste(_)
                    | egui::Event::Ime(egui::ImeEvent::Commit(_))
            );
            if (pointer && text_seen) || (text && pointer_seen) {
                boundary = Some(i);
                break;
            }
            pointer_seen |= pointer;
            text_seen |= text;
        }
        if let Some(i) = boundary {
            // A boundary always has an earlier event: each frame consumes a
            // nonempty prefix. Only the unconsumed suffix is retained, once.
            self.pending_input = events.split_off(i);
            ctx.request_repaint();
        }
        report_input(&events, self.pending_input.len(), input.focused);
        input.events = events;
    }
    fn add(&mut self, name: String, path: PathBuf, bytes: Vec<u8>) {
        if self.files.len() >= 10_000
            || self.files.iter().map(|f| f.original.len()).sum::<usize>() + bytes.len()
                > 256 * 1024 * 1024
        {
            self.error_files.push(format!(
                "{name}: session limit reached (256 MiB / 10,000 files)"
            ));
            return;
        }
        if self.files.iter().any(|f| f.path == path) {
            self.status = format!("Already open: {name}");
            return;
        }
        match Document::parse(&bytes) {
            Ok(doc) => {
                self.files.push(File {
                    name,
                    path,
                    original: bytes,
                    doc,
                    dirty: false,
                    is_new: false,
                    undo: vec![],
                    redo: vec![],
                });
                if self.selection.focus.is_none() {
                    self.selection.choose(0, false, false, &[0]);
                }
            }
            Err(e) => self.error_files.push(format!("{name}: {e}")),
        }
    }
    fn open(&mut self, directory: bool) {
        self.open_clicks += 1;
        #[cfg(not(target_arch = "wasm32"))]
        {
            let paths = if directory {
                rfd::FileDialog::new().pick_folder().map(|p| vec![p])
            } else {
                rfd::FileDialog::new()
                    .add_filter("Aseprite", &["ase", "aseprite"])
                    .pick_files()
            };
            if let Some(paths) = paths {
                match io::load(paths) {
                    Ok(files) => {
                        for (n, p, b) in files {
                            self.add(n, p, b)
                        }
                    }
                    Err(e) => self.status = e,
                }
            }
        }
        #[cfg(target_arch = "wasm32")]
        io::open(directory);
    }
    fn has_draft(&self) -> bool {
        self.text != self.initial_text
            || self.properties != self.initial_properties
            || self.color_edit.is_some()
            || self.tag_edits.is_some()
    }
    fn editing_focus(&self) -> Option<usize> {
        self.selection
            .focus
            .filter(|i| self.selection.selected.contains(i))
            .or_else(|| self.selection.selected.first().copied())
    }
    fn sync_fields(&mut self) {
        if self.loaded_focus == self.editing_focus()
            && self.loaded_selection == self.selection.selected
            && self.loaded_target == self.target
        {
            return;
        }
        if !self.commit_draft() {
            self.selection.focus = self.loaded_focus;
            self.selection.selected = self.loaded_selection.clone();
            self.target = self.loaded_target;
            return;
        }
        if self.loaded_focus != self.editing_focus() {
            self.target = 0;
        }
        self.load_fields();
    }
    fn load_fields(&mut self) {
        self.loaded_focus = self.editing_focus();
        self.loaded_selection = self.selection.selected.clone();
        self.loaded_target = self.target;
        let target = self
            .loaded_focus
            .and_then(|i| self.files.get(i))
            .and_then(|f| f.doc.targets().into_iter().find(|t| t.id == self.target));
        self.text = target
            .as_ref()
            .map(|t| {
                if self.loaded_selection.len() > 1 && self.mode == Mode::Append {
                    String::new()
                } else {
                    t.text.clone()
                }
            })
            .unwrap_or_default();
        self.properties = target
            .as_ref()
            .and_then(|t| t.properties.as_ref())
            .map(|v| serde_json::to_string_pretty(v).unwrap_or_default())
            .unwrap_or_else(|| "{\"maps\":[]}".into());
        self.initial_text = self.text.clone();
        self.initial_properties = self.properties.clone();
        self.color_edit = None;
        self.tag_edits = None;
    }
    fn commit_draft(&mut self) -> bool {
        if !self.has_draft() {
            return true;
        }
        let Some(source) = self
            .loaded_focus
            .and_then(|i| self.files.get(i))
            .and_then(|f| {
                f.doc
                    .targets()
                    .into_iter()
                    .find(|t| t.id == self.loaded_target)
            })
        else {
            self.status = "Select a file before editing".into();
            return false;
        };
        let properties = if self.properties != self.initial_properties {
            match serde_json::from_str::<serde_json::Value>(&self.properties) {
                Ok(v) => {
                    if let Err(e) = properties::validate(&v) {
                        self.status = e;
                        return false;
                    }
                    Some(v)
                }
                Err(e) => {
                    self.status = format!("Invalid properties: {e}");
                    return false;
                }
            }
        } else {
            None
        };
        let mut changes = Vec::new();
        for &i in &self.loaded_selection {
            let mut doc = self.files[i].doc.clone();
            let result: Result<(), String> = (|| {
                let target = doc
                    .targets()
                    .into_iter()
                    .find(|t| t.kind == source.kind && t.label == source.label)
                    .ok_or("Metadata scope is absent")?;
                if self.text != self.initial_text {
                    let text = if self.loaded_selection.len() > 1
                        && self.mode == Mode::Append
                        && !target.text.is_empty()
                        && !self.text.is_empty()
                    {
                        format!(
                            "{}{}{}",
                            target.text,
                            if self.separator.is_empty() {
                                "\n"
                            } else {
                                &self.separator
                            },
                            self.text
                        )
                    } else if self.loaded_selection.len() > 1
                        && self.mode == Mode::Append
                        && self.text.is_empty()
                    {
                        target.text
                    } else {
                        self.text.clone()
                    };
                    doc.set_text(target.id, &text)?;
                }
                if let Some(p) = &properties {
                    let base = serde_json::from_str(&self.initial_properties)
                        .map_err(|e| format!("Invalid baseline: {e}"))?;
                    let destination = target
                        .properties
                        .unwrap_or_else(|| serde_json::json!({"maps":[]}));
                    let merged = properties::merge_changes(&base, p, &destination)?;
                    doc.set_properties(target.id, &merged)?;
                }
                if let Some(color) = self.color_edit {
                    doc.set_color(target.id, color)?;
                }
                if Some(i) == self.loaded_focus {
                    if let Some(tags) = &self.tag_edits {
                        for tag in tags {
                            doc.set_tag(tag)?;
                        }
                    }
                }
                Ok(())
            })();
            if let Err(e) = result {
                self.status = format!("{}: {e}", self.files[i].name);
                return false;
            }
            if doc.encode().ok() != self.files[i].doc.encode().ok() {
                changes.push((i, doc));
            }
        }
        self.apply_changes(changes);
        self.initial_text = self.text.clone();
        self.initial_properties = self.properties.clone();
        self.color_edit = None;
        self.tag_edits = None;
        // Consume bulk append input once; Save cannot append it again.
        if self.loaded_selection.len() > 1 && self.mode == Mode::Append {
            self.text.clear();
            self.initial_text.clear();
        }
        true
    }
    fn apply_changes(&mut self, changes: Vec<(usize, Document)>) {
        for (i, doc) in changes {
            if let Ok(before) = self.files[i].doc.encode() {
                let f = &mut self.files[i];
                f.undo.push(before);
                f.redo.clear();
                f.doc = doc;
                f.dirty = f.is_new || f.doc.encode().map(|b| b != f.original).unwrap_or(true);
            }
        }
    }
    fn transform_selected(&mut self, operation: aseprite::ArtworkOperation) {
        if !self.commit_draft() {
            return;
        }
        self.error_files.clear();
        let mut changes = Vec::new();
        let mut unchanged = 0;
        for &index in &self.selection.selected {
            let file = &self.files[index];
            let mut doc = file.doc.clone();
            let result = (|| {
                let before = file.doc.encode()?;
                doc.transform(operation)?;
                let after = doc.encode()?;
                Ok::<bool, String>(after != before)
            })();
            match result {
                Ok(true) => changes.push((index, doc)),
                Ok(false) => unchanged += 1,
                Err(error) => self
                    .error_files
                    .push(format!("{}: skipped — {error}", file.name)),
            }
        }
        let changed = changes.len();
        if self
            .preview
            .as_ref()
            .is_some_and(|p| changes.iter().any(|(i, _)| *i == p.index))
        {
            self.preview = None;
        }
        // Keep this batch's Undo aligned across the current selection. A file
        // skipped by the transform must not lose an earlier unrelated edit
        // when its successful neighbors are undone.
        if changed > 0 {
            for &index in &self.selection.selected {
                if !changes.iter().any(|(i, _)| *i == index) {
                    changes.push((index, self.files[index].doc.clone()));
                }
            }
        }
        self.apply_changes(changes);
        self.load_fields();
        self.status = format!(
            "Artwork: {changed} changed, {unchanged} unchanged, {} skipped. Ctrl+S to save; Ctrl+Z to undo.",
            self.error_files.len()
        );
    }
    fn merge_selected(&mut self) {
        if self.selection.selected.len() != 2 || !self.commit_draft() {
            return;
        }
        let mut selected: Vec<_> = self.selection.selected.iter().copied().collect();
        if self.merge_reverse {
            selected.reverse();
        }
        let first = &self.files[selected[0]];
        let second = &self.files[selected[1]];
        let result = first
            .doc
            .merge(&second.doc)
            .and_then(|(doc, note)| doc.encode().map(|bytes| (bytes, note)));
        let (bytes, note) = match result {
            Ok(output) => output,
            Err(error) => {
                self.status = format!("Merge skipped: {error}. Neither source was changed.");
                return;
            }
        };
        let parent = first
            .path
            .parent()
            .unwrap_or_else(|| std::path::Path::new(""));
        let name_parent = first
            .name
            .rsplit_once('/')
            .map(|(parent, _)| format!("{parent}/"))
            .unwrap_or_default();
        let mut suffix = 1u32;
        let (name, path) = loop {
            let basename = format!("merged-{suffix}.aseprite");
            let path = parent.join(&basename);
            let name = format!("{name_parent}{basename}");
            let occupied = self.files.iter().any(|f| f.path == path || f.name == name);
            #[cfg(not(target_arch = "wasm32"))]
            let occupied = occupied || path.exists();
            if !occupied {
                break (name, path);
            }
            suffix += 1;
        };
        let index = self.files.len();
        self.add(name.clone(), path, bytes);
        if self.files.len() == index {
            return;
        }
        let file = &mut self.files[index];
        file.is_new = true;
        file.dirty = true;
        self.selection.choose(index, false, false, &[index]);
        self.preview = None;
        self.load_fields();
        self.status =
            format!("Created {name}. {note} Sources unchanged. Ctrl+S saves the new file.");
    }
    fn artwork_tools(&mut self, ui: &mut egui::Ui) {
        let response = ui.collapsing("Artwork tools", |ui| {
            ui.label(format!("Applies to all frames and layers in {} selected file(s).", self.selection.selected.len()));
            ui.horizontal_wrapped(|ui| {
                let response = ui.button("Flip sprite ↔").on_hover_text("Mirror horizontally across the whole canvas, in every frame.");
                report_widget("flip-sprite", &response);
                if response.clicked() {
                    self.transform_selected(aseprite::ArtworkOperation::FlipHorizontal);
                }
                let response = ui.button("Convert to indexed").on_hover_text("Lossless only: add exact missing colors to the palette. Unsupported files are skipped, never quantized.");
                report_widget("convert-indexed", &response);
                if response.clicked() {
                    self.transform_selected(aseprite::ArtworkOperation::ConvertToIndexed);
                }
                let response = ui.button("Convert to RGBA");
                report_widget("convert-rgba", &response);
                if response.clicked() {
                    self.transform_selected(aseprite::ArtworkOperation::ConvertToRgba);
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.label("Add pixels:");
                ui.add(egui::DragValue::new(&mut self.expand_right).range(0..=u16::MAX).prefix("Right "));
                ui.add(egui::DragValue::new(&mut self.expand_bottom).range(0..=u16::MAX).prefix("Bottom "));
                let response = ui.add_enabled(self.expand_right > 0 || self.expand_bottom > 0, egui::Button::new("Expand canvas"));
                report_widget("expand-canvas", &response);
                if response.clicked() {
                    self.transform_selected(aseprite::ArtworkOperation::ExpandCanvas { right: self.expand_right, bottom: self.expand_bottom });
                }
            });
            ui.small("Expands right/bottom only. No stretching, moving pixels, or cropping. Changes are staged until Save.");
            let response = ui.add_enabled(self.selection.selected.len() == 2, egui::Button::new("Merge two → new sprite"))
                .on_hover_text("Append animations in the displayed order, keeping the larger canvas, merging palettes and preserving all tags. Creates a new file; neither source is overwritten.");
            report_widget("merge-sprites", &response);
            if response.clicked() { self.merge_selected(); }
            if self.selection.selected.len() == 2 {
                ui.checkbox(&mut self.merge_reverse, "Reverse merge order");
                let mut names: Vec<_> = self.selection.selected.iter().map(|&i| self.files[i].name.as_str()).collect();
                if self.merge_reverse { names.reverse(); }
                ui.small(format!("Append: {} → {}", names[0], names[1]));
            }
        });
        report_widget("artwork-tools", &response.header_response);
    }
    fn undo(&mut self) {
        if !self.commit_draft() {
            return;
        }
        for &i in &self.selection.selected {
            let f = &mut self.files[i];
            if let Some(bytes) = f.undo.pop() {
                if let Ok(doc) = Document::parse(&bytes) {
                    if let Ok(current) = f.doc.encode() {
                        f.redo.push(current);
                    }
                    f.doc = doc;
                    f.dirty = f.is_new || bytes != f.original;
                    if self.preview.as_ref().is_some_and(|p| p.index == i) {
                        self.preview = None;
                    }
                }
            }
        }
        self.load_fields();
    }
    fn redo(&mut self) {
        if !self.commit_draft() {
            return;
        }
        for &i in &self.selection.selected {
            let f = &mut self.files[i];
            if let Some(bytes) = f.redo.pop() {
                if let Ok(doc) = Document::parse(&bytes) {
                    if let Ok(current) = f.doc.encode() {
                        f.undo.push(current);
                    }
                    f.doc = doc;
                    f.dirty = f.is_new || bytes != f.original;
                    if self.preview.as_ref().is_some_and(|p| p.index == i) {
                        self.preview = None;
                    }
                }
            }
        }
        self.load_fields();
    }
    fn scope(&self) -> Option<aseprite::Target> {
        self.loaded_focus
            .and_then(|i| self.files.get(i))
            .and_then(|f| f.doc.targets().into_iter().find(|t| t.id == self.target))
    }
    fn matches(&self) -> usize {
        if self.find.is_empty() {
            return 0;
        }
        self.selection
            .selected
            .iter()
            .map(|&i| {
                if self.find_tags {
                    self.files[i]
                        .doc
                        .tags()
                        .iter()
                        .map(|t| t.name.matches(&self.find).count())
                        .sum::<usize>()
                } else {
                    self.files[i]
                        .doc
                        .targets()
                        .iter()
                        .filter(|t| {
                            self.scope().is_some_and(|source| {
                                t.kind == source.kind && t.label == source.label
                            })
                        })
                        .map(|t| t.text.matches(&self.find).count())
                        .sum()
                }
            })
            .sum()
    }
    fn replace_all(&mut self) {
        if !self.commit_draft() || self.find.is_empty() {
            return;
        }
        let mut changes = Vec::new();
        self.error_files.clear();
        for &i in &self.selection.selected {
            let mut doc = self.files[i].doc.clone();
            let result: Result<(), String> = (|| {
                if self.find_tags {
                    for mut tag in doc.tags() {
                        tag.name = tag.name.replace(&self.find, &self.replace);
                        doc.set_tag(&tag)?;
                    }
                } else {
                    let source = self.scope().ok_or("Select a metadata scope")?;
                    let target = doc
                        .targets()
                        .into_iter()
                        .find(|t| t.kind == source.kind && t.label == source.label)
                        .ok_or("Metadata scope is absent")?;
                    let value = target.text.replace(&self.find, &self.replace);
                    if value != target.text {
                        doc.set_text(target.id, &value)?;
                    }
                }
                Ok(())
            })();
            if let Err(e) = result {
                self.status = format!("{}: {e}", self.files[i].name);
                return;
            }
            if doc.encode().ok() != self.files[i].doc.encode().ok() {
                changes.push((i, doc));
            }
        }
        self.status = format!("Replaced in {} files", changes.len());
        self.apply_changes(changes);
        self.load_fields();
    }
    fn save(&mut self) {
        self.sync_fields();
        if !self.commit_draft() {
            return;
        }
        self.error_files.clear();
        let mut saved = 0;
        #[cfg(target_arch = "wasm32")]
        let mut outputs = Vec::new();
        for &i in &self.selection.selected {
            let f = &mut self.files[i];
            if !f.dirty {
                continue;
            }
            let bytes = match f.doc.encode() {
                Ok(b) => b,
                Err(e) => {
                    self.error_files.push(format!("{}: {e}", f.name));
                    continue;
                }
            };
            #[cfg(not(target_arch = "wasm32"))]
            match if f.is_new {
                io::save_new(&f.path, &bytes)
            } else {
                io::save(&f.path, &f.original, &bytes)
            } {
                Ok(()) => {
                    f.original = bytes;
                    f.dirty = false;
                    f.is_new = false;
                    saved += 1;
                }
                Err(e) => self.error_files.push(format!("{}: {e}", f.name)),
            }
            #[cfg(target_arch = "wasm32")]
            {
                outputs.push((f.name.clone(), bytes));
                saved += 1;
            }
        }
        #[cfg(target_arch = "wasm32")]
        if !outputs.is_empty() {
            use std::io::Write;
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
            let mut names = BTreeSet::new();
            let mut result = Ok(());
            for (name, bytes) in outputs {
                let name = name.replace('\\', "/");
                if name.split('/').any(|s| s == ".." || s.is_empty()) || !names.insert(name.clone())
                {
                    result = Err("Unsafe or duplicate output path".to_owned());
                    break;
                }
                if let Err(e) = zip
                    .start_file(name, zip::write::SimpleFileOptions::default())
                    .and_then(|_| zip.write_all(&bytes).map_err(zip::result::ZipError::Io))
                {
                    result = Err(e.to_string());
                    break;
                }
            }
            match result.and_then(|_| zip.finish().map_err(|e| e.to_string())) {
                Ok(data) => io::download("edited-sprites.zip", &data.into_inner()),
                Err(e) => {
                    saved = 0;
                    self.error_files.push(e);
                }
            }
        }
        self.status = format!(
            "{saved} files {}",
            if cfg!(target_arch = "wasm32") {
                "exported; originals unchanged"
            } else {
                "saved (recovery backups retained)"
            }
        );
    }
}
impl Editor {
    fn draw(&mut self, ctx: &egui::Context) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.external_editor.is_none() {
                self.external_editor = Some(external_editor::discover());
            }
            let focused = ctx.input(|input| input.focused);
            if focused && !self.window_focused {
                if let Some(index) = self.external_opened {
                    if let Err(error) = self.refresh_external(index) {
                        self.status = error;
                    }
                }
            }
            self.window_focused = focused;
        }
        self.sync_fields();
        report_state(self.find_tab, self.matches());
        let mut save_requested = false;
        #[cfg(not(target_arch = "wasm32"))]
        if ctx.input(|i| i.viewport().close_requested())
            && !self.allow_close
            && (self.files.iter().any(|f| f.dirty) || self.has_draft())
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm_close = true;
        }
        #[cfg(target_arch = "wasm32")]
        {
            io::report(self.files.len(), self.selection.selected.len());
            io::dirty(self.files.iter().any(|f| f.dirty) || self.has_draft());
            use wasm_bindgen::JsValue;
            for item in io::poll().iter() {
                if let Ok(message) = js_sys::Reflect::get(&item, &JsValue::from_str("status")) {
                    if let Some(message) = message.as_string() {
                        self.status = message;
                        continue;
                    }
                }
                if let Ok(err) = js_sys::Reflect::get(&item, &JsValue::from_str("error")) {
                    if let Some(e) = err.as_string() {
                        self.error_files.push(e);
                        continue;
                    }
                }
                let name = js_sys::Reflect::get(&item, &JsValue::from_str("name"))
                    .ok()
                    .and_then(|v| v.as_string())
                    .unwrap_or_default();
                if let Ok(bytes) = js_sys::Reflect::get(&item, &JsValue::from_str("bytes")) {
                    self.add(
                        name.clone(),
                        PathBuf::from(name),
                        js_sys::Uint8Array::new(&bytes).to_vec(),
                    );
                }
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(150));
        }
        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let open = ui.button("Open files");
                report_widget("openFiles", &open);
                report_counter("openClicks", self.open_clicks);
                if open.clicked() {
                    self.open(false);
                }
                if ui.button("Open folder").clicked() {
                    self.open(true);
                }
                if ui
                    .add_enabled(
                        self.files.iter().any(|f| f.dirty) || self.has_draft(),
                        egui::Button::new(if cfg!(target_arch = "wasm32") {
                            "Export selected ZIP"
                        } else {
                            "Save selected"
                        }),
                    )
                    .on_hover_text("Ctrl+S")
                    .clicked()
                {
                    save_requested = true;
                }
                if ui
                    .button("Undo")
                    .on_hover_text("Ctrl+Z — undo staged changes")
                    .clicked()
                {
                    self.undo();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("⚙").clicked() {
                        self.settings = !self.settings;
                    }
                });
            });
        });
        let mut visible = Vec::new();
        egui::SidePanel::left("files")
            .resizable(true)
            .default_width(270.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Files");
                    ui.label(format!("{} selected", self.selection.selected.len()));
                });
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let mut seen = BTreeSet::new();
                    let mut chosen = None;
                    let mut action = None;
                    #[cfg(not(target_arch = "wasm32"))]
                    let mut open_external = None;
                    for (i, f) in self.files.iter().enumerate() {
                        let normalized = f.name.replace('\\', "/");
                        let parts: Vec<_> = normalized.split('/').collect();
                        let mut hidden = false;
                        for depth in 0..parts.len().saturating_sub(1) {
                            let prefix = parts[..=depth].join("/");
                            if seen.insert(prefix.clone()) {
                                ui.horizontal(|ui| {
                                    ui.add_space(depth as f32 * 12.0);
                                    let closed = self.collapsed.contains(&prefix);
                                    if ui
                                        .small_button(format!(
                                            "{} {}",
                                            if closed { "▶" } else { "▼" },
                                            parts[depth]
                                        ))
                                        .clicked()
                                    {
                                        if closed {
                                            self.collapsed.remove(&prefix);
                                        } else {
                                            self.collapsed.insert(prefix.clone());
                                        }
                                    }
                                });
                            }
                            if self.collapsed.contains(&prefix) {
                                hidden = true;
                                break;
                            }
                        }
                        if hidden {
                            continue;
                        }
                        visible.push(i);
                        ui.horizontal(|ui| {
                            ui.add_space(parts.len().saturating_sub(1) as f32 * 12.0);
                            let label = format!(
                                "{}{}",
                                parts.last().unwrap_or(&""),
                                if f.dirty
                                    || (self.loaded_selection.contains(&i) && self.has_draft())
                                {
                                    " •"
                                } else {
                                    ""
                                }
                            );
                            let response = ui
                                .selectable_label(self.selection.selected.contains(&i), label)
                                .on_hover_text(&f.name);
                            report_widget(&format!("file-{i}"), &response);
                            // Ctrl+arrow can focus an unselected row. Keep that keyboard
                            // location visible without adding a selection chevron.
                            if self.selection.focus == Some(i)
                                && !self.selection.selected.contains(&i)
                            {
                                ui.painter().rect_stroke(
                                    response.rect,
                                    2.0,
                                    ui.visuals().selection.stroke,
                                    egui::StrokeKind::Inside,
                                );
                            }
                            if response.clicked() {
                                chosen = Some((
                                    i,
                                    ui.input(|s| s.modifiers.command || s.modifiers.ctrl),
                                    ui.input(|s| s.modifiers.shift),
                                ));
                            }
                            #[cfg(not(target_arch = "wasm32"))]
                            if response.double_clicked() {
                                chosen = Some((i, false, false));
                                open_external = Some(i);
                            }
                            response.context_menu(|ui| {
                                #[cfg(not(target_arch = "wasm32"))]
                                {
                                    let editor = self.external_editor.as_ref().and_then(|e| e.as_ref());
                                    let label = editor.map(|e| format!("Open in {}", e.label())).unwrap_or_else(|| "Open in Aseprite".into());
                                    if ui.add_enabled(editor.is_some(), egui::Button::new(label))
                                        .on_disabled_hover_text("Install Aseprite or LibreSprite, then restart this app")
                                        .clicked() {
                                        open_external = Some(i);
                                        ui.close_menu();
                                    }
                                }
                                if ui.button("Select only this file").clicked() {
                                    chosen = Some((i, false, false));
                                    ui.close_menu();
                                }
                                if ui.button("Save this file").clicked() {
                                    chosen = Some((i, false, false));
                                    action = Some(true);
                                    ui.close_menu();
                                }
                                if ui.button("Undo staged edit").clicked() {
                                    chosen = Some((i, false, false));
                                    action = Some(false);
                                    ui.close_menu();
                                }
                            });
                        });
                    }
                    if let Some((i, c, s)) = chosen {
                        self.selection.choose(i, c, s, &visible);
                        self.sync_fields();
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    if let Some(index) = open_external { self.request_external(index); }
                    if let Some(save) = action {
                        if save {
                            self.save();
                        } else {
                            self.undo();
                        }
                    }
                });
            });
        let editing = ctx
            .memory(|m| m.focused())
            .is_some_and(|id| egui::TextEdit::load_state(ctx, id).is_some());
        for event in ctx.input(|input| input.events.clone()) {
            let egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } = event
            else {
                continue;
            };
            let command = modifiers.command || modifiers.ctrl;
            if command && key == egui::Key::S {
                save_requested = true;
                continue;
            }
            if command && matches!(key, egui::Key::F | egui::Key::H) {
                if self.commit_draft() {
                    self.find_tab = true;
                    self.focus_find = true;
                }
                continue;
            }
            if key == egui::Key::Escape {
                self.settings = false;
                self.confirm_close = false;
                #[cfg(not(target_arch = "wasm32"))]
                {
                    self.external_pending = None;
                }
                if self.find_tab {
                    self.find_tab = false;
                }
                continue;
            }
            if editing {
                continue;
            }
            match key {
                egui::Key::A if command => self.selection.selected.extend(visible.iter().copied()),
                egui::Key::Z if command && modifiers.shift => self.redo(),
                egui::Key::Z if command => self.undo(),
                egui::Key::Y if command => self.redo(),
                egui::Key::ArrowUp => self.selection.step(-1, command, modifiers.shift, &visible),
                egui::Key::ArrowDown => self.selection.step(1, command, modifiers.shift, &visible),
                egui::Key::Space => {
                    if let Some(i) = self.selection.focus {
                        self.selection.choose(i, true, false, &visible);
                    }
                }
                _ => {}
            }
        }
        self.sync_fields();
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.label(&self.status);
            if !self.error_files.is_empty() {
                egui::ScrollArea::vertical()
                    .id_salt("file-issues")
                    .max_height(120.0)
                    .show(ui, |ui| {
                        for error in &self.error_files {
                            ui.colored_label(egui::Color32::LIGHT_RED, error);
                        }
                    });
            }
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.files.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label("Open .ase or .aseprite files");
                });
                return;
            }
            if self.selection.selected.is_empty() {
                ui.label("Select files to edit");
                return;
            }
            self.artwork_tools(ui);
            ui.separator();
            let before = self.find_tab;
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.find_tab, false, "Edit");
                ui.selectable_value(&mut self.find_tab, true, "Find / Replace")
                    .on_hover_text("Ctrl+F or Ctrl+H");
            });
            if self.find_tab != before && !self.commit_draft() {
                self.find_tab = before;
            }
            ui.separator();
            if self.find_tab {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.find_tags, false, "User data");
                    ui.selectable_value(&mut self.find_tags, true, "Tag names");
                });
                if !self.find_tags {
                    if let Some(i) = self.loaded_focus {
                        let targets = self.files[i].doc.targets();
                        egui::ComboBox::from_id_salt("find-scope")
                            .selected_text(
                                self.scope()
                                    .map(|t| t.label)
                                    .unwrap_or_else(|| "Sprite".into()),
                            )
                            .show_ui(ui, |ui| {
                                for target in targets {
                                    ui.selectable_value(&mut self.target, target.id, target.label);
                                }
                            });
                        self.sync_fields();
                    }
                }
                ui.label(format!("{} selected files", self.selection.selected.len()));
                ui.label("Find");
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.find)
                        .id(egui::Id::new("find-text"))
                        .desired_width(f32::INFINITY),
                );
                report_widget("find", &response);
                if self.focus_find {
                    response.request_focus();
                    self.focus_find = false;
                }
                ui.label("Replace with");
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.replace)
                        .id(egui::Id::new("replace-text"))
                        .desired_width(f32::INFINITY),
                );
                report_widget("replace", &response);
                let count = self.matches();
                ui.horizontal(|ui| {
                    ui.label(format!("{count} matches"));
                    let response = ui.add_enabled(count > 0, egui::Button::new("Replace all"));
                    report_widget("replace_all", &response);
                    if response.clicked() {
                        self.replace_all();
                    }
                });
                self.show_preview(ui);
                return;
            }
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::UserData, "User data");
                ui.selectable_value(&mut self.tab, Tab::Properties, "Properties");
                ui.selectable_value(&mut self.tab, Tab::Tags, "Tags");
            });
            let Some(i) = self.loaded_focus else {
                return;
            };
            let targets = self.files[i].doc.targets();
            if self.tab != Tab::Tags {
                egui::ComboBox::from_label("Scope")
                    .selected_text(
                        targets
                            .iter()
                            .find(|t| t.id == self.target)
                            .map(|t| t.label.as_str())
                            .unwrap_or("Sprite"),
                    )
                    .show_ui(ui, |ui| {
                        for t in &targets {
                            ui.selectable_value(&mut self.target, t.id, &t.label);
                        }
                    });
                self.sync_fields();
            }
            match self.tab {
                Tab::UserData => {
                    if self.selection.selected.len() > 1 {
                        let old = self.mode;
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut self.mode, Mode::Append, "Append");
                            ui.selectable_value(&mut self.mode, Mode::Override, "Override");
                        });
                        if old != self.mode {
                            let new = self.mode;
                            self.mode = old;
                            if self.commit_draft() {
                                self.mode = new;
                                self.load_fields();
                            }
                        }
                    }
                    let response = ui.add(
                        egui::TextEdit::multiline(&mut self.text)
                            .id(egui::Id::new("metadata-text"))
                            .desired_rows(10)
                            .desired_width(f32::INFINITY),
                    );
                    report_widget("text", &response);
                    ui.collapsing("Advanced", |ui| {
                        if let Some(target) = targets.iter().find(|t| t.id == self.target) {
                            let current = self.color_edit.unwrap_or(target.color);
                            let mut rgba = current.unwrap_or([0, 0, 0, 255]);
                            ui.horizontal(|ui| {
                                ui.label("User-data color").on_hover_text(
                                    "Metadata marker only; does not change pixels or palette.",
                                );
                                if ui.color_edit_button_srgba_unmultiplied(&mut rgba).changed() {
                                    self.color_edit = Some(Some(rgba));
                                }
                                if ui
                                    .add_enabled(
                                        current.is_some() || self.selection.selected.len() > 1,
                                        egui::Button::new("Clear"),
                                    )
                                    .clicked()
                                {
                                    self.color_edit = Some(None);
                                }
                            });
                        }
                    });
                }
                Tab::Properties => {
                    if let Some(source) = targets.iter().find(|t| t.id == self.target) {
                        let mixed = self.selection.selected.iter().any(|idx| {
                            self.files[*idx]
                                .doc
                                .targets()
                                .into_iter()
                                .find(|t| t.kind == source.kind && t.label == source.label)
                                .is_none_or(|t| t.properties != source.properties)
                        });
                        if mixed {
                            ui.label("Mixed values — only edited properties change");
                        }
                    }
                    if let Some(w) = targets
                        .iter()
                        .find(|t| t.id == self.target)
                        .and_then(|t| t.warning.as_ref())
                    {
                        ui.colored_label(egui::Color32::YELLOW, w);
                    }
                    let editable = targets
                        .iter()
                        .find(|t| t.id == self.target)
                        .is_some_and(|t| t.warning.is_none());
                    egui::ScrollArea::vertical()
                        .id_salt("properties-scroll")
                        .max_height((ui.available_height() - 200.0).max(100.0))
                        .show(ui, |ui| {
                            ui.add_enabled_ui(editable, |ui| {
                                match serde_json::from_str::<serde_json::Value>(&self.properties) {
                                    Ok(mut value) => {
                                        if properties::edit(ui, &mut value) {
                                            self.properties = serde_json::to_string_pretty(&value)
                                                .unwrap_or_default();
                                        }
                                    }
                                    Err(e) => {
                                        ui.colored_label(
                                            egui::Color32::LIGHT_RED,
                                            format!("Invalid JSON: {e}"),
                                        );
                                    }
                                }
                                ui.collapsing("Advanced JSON", |ui| {
                                    ui.add(
                                        egui::TextEdit::multiline(&mut self.properties)
                                            .id(egui::Id::new("metadata-properties"))
                                            .code_editor()
                                            .desired_rows(10)
                                            .desired_width(f32::INFINITY),
                                    );
                                });
                            });
                        });
                }
                Tab::Tags => {
                    if self.selection.selected.len() > 1 {
                        ui.label("Editing focused file · use Find / Replace for bulk renaming");
                    }
                    let mut tags = self
                        .tag_edits
                        .clone()
                        .unwrap_or_else(|| self.files[i].doc.tags());
                    let mut edited = false;
                    egui::ScrollArea::vertical()
                        .max_height((ui.available_height() - 200.0).max(100.0))
                        .show(ui, |ui| {
                            for tag in &mut tags {
                                ui.horizontal(|ui| {
                                    edited |= ui.text_edit_singleline(&mut tag.name).changed();
                                    edited |= ui
                                        .add(egui::DragValue::new(&mut tag.from).prefix("From "))
                                        .changed();
                                    edited |= ui
                                        .add(egui::DragValue::new(&mut tag.to).prefix("To "))
                                        .changed();
                                    edited |= ui
                                    .add(
                                        egui::DragValue::new(&mut tag.direction)
                                            .range(0..=3)
                                            .prefix("Direction "),
                                    )
                                    .on_hover_text(
                                        "0 Forward · 1 Reverse · 2 Ping-pong · 3 Reverse ping-pong",
                                    )
                                    .changed();
                                    edited |= ui
                                        .add(
                                            egui::DragValue::new(&mut tag.repeat).prefix("Repeat "),
                                        )
                                        .changed();
                                });
                            }
                        });
                    if edited {
                        self.tag_edits = Some(tags);
                    }
                }
            }
            self.show_preview(ui);
        });
        if save_requested {
            self.save();
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(index) = self.external_pending {
            egui::Window::new("Open in sprite editor")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(format!("{} has unsaved changes", self.files[index].name));
                    ui.horizontal(|ui| {
                        if ui.button("Save and open").clicked() && self.commit_draft() {
                            match self.save_one(index) {
                                Ok(()) => {
                                    self.external_pending = None;
                                    self.launch_external(index);
                                }
                                Err(error) => self.status = error,
                            }
                        }
                        if ui
                            .add_enabled(
                                !self.files[index].is_new,
                                egui::Button::new("Open without saving"),
                            )
                            .on_disabled_hover_text(
                                "Save this new sprite first so Aseprite can open it.",
                            )
                            .clicked()
                        {
                            self.external_pending = None;
                            self.launch_external(index);
                        }
                        if ui.button("Cancel").clicked() {
                            self.external_pending = None;
                        }
                    });
                });
        }
        if self.confirm_close {
            egui::Window::new("Unsaved changes")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("Close without saving staged changes?");
                    ui.horizontal(|ui| {
                        if ui.button("Keep editing").clicked() {
                            self.confirm_close = false;
                        }
                        if ui.button("Discard and close").clicked() {
                            self.allow_close = true;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                });
        }
        egui::Window::new("Settings")
            .open(&mut self.settings)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Append separator");
                    ui.text_edit_singleline(&mut self.separator);
                });
                ui.label("Empty separator uses a new line.");
                egui::widgets::global_theme_preference_buttons(ui);
            });
    }
}
#[cfg(target_arch = "wasm32")]
fn report_input(events: &[egui::Event], pending: usize, focused: bool) {
    if events.is_empty() && pending == 0 {
        return;
    }
    let global = js_sys::global();
    let history = js_sys::Reflect::get(&global, &"editorInput".into())
        .ok()
        .filter(js_sys::Array::is_array)
        .map(|v| js_sys::Array::from(&v))
        .unwrap_or_default();
    let o = js_sys::Object::new();
    let kinds = js_sys::Array::new();
    for event in events.iter().take(32) {
        let kind = match event {
            egui::Event::Text(_) => "text",
            egui::Event::Paste(_) => "paste",
            egui::Event::PointerButton { pressed: true, .. } => "pointerDown",
            egui::Event::PointerButton { pressed: false, .. } => "pointerUp",
            egui::Event::PointerMoved(_) => "pointerMove",
            egui::Event::Key { pressed: true, .. } => "keyDown",
            egui::Event::Key { pressed: false, .. } => "keyUp",
            egui::Event::Ime(_) => "ime",
            _ => "other",
        };
        kinds.push(&kind.into());
    }
    let _ = js_sys::Reflect::set(&o, &"events".into(), &kinds);
    let _ = js_sys::Reflect::set(&o, &"pending".into(), &(pending as f64).into());
    let _ = js_sys::Reflect::set(&o, &"focused".into(), &focused.into());
    history.push(&o);
    while history.length() > 16 {
        history.shift();
    }
    let _ = js_sys::Reflect::set(&global, &"editorInput".into(), &history);
}
#[cfg(not(target_arch = "wasm32"))]
fn report_input(_: &[egui::Event], _: usize, _: bool) {}
#[cfg(target_arch = "wasm32")]
fn report_counter(name: &str, value: u64) {
    if let Ok(o) = js_sys::Reflect::get(&js_sys::global(), &"editorUi".into()) {
        let _ = js_sys::Reflect::set(&o, &name.into(), &(value as f64).into());
    }
}
#[cfg(not(target_arch = "wasm32"))]
fn report_counter(_: &str, _: u64) {}
// Geometry-only diagnostics for automated browser interaction; no file data.
#[cfg(target_arch = "wasm32")]
fn report_state(find_tab: bool, matches: usize) {
    let o = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&o, &"findTab".into(), &find_tab.into());
    let _ = js_sys::Reflect::set(&o, &"matches".into(), &(matches as f64).into());
    let _ = js_sys::Reflect::set(&js_sys::global(), &"editorUi".into(), &o);
}
#[cfg(not(target_arch = "wasm32"))]
fn report_state(_: bool, _: usize) {}
#[cfg(target_arch = "wasm32")]
fn report_widget(name: &str, response: &egui::Response) {
    if let Ok(o) = js_sys::Reflect::get(&js_sys::global(), &"editorUi".into()) {
        let p = response.rect.center();
        let a = js_sys::Array::new();
        a.push(&(p.x as f64).into());
        a.push(&(p.y as f64).into());
        let _ = js_sys::Reflect::set(&o, &name.into(), &a);
        let _ = js_sys::Reflect::set(
            &o,
            &format!("{name}Focused").into(),
            &response.has_focus().into(),
        );
    }
}
#[cfg(not(target_arch = "wasm32"))]
fn report_widget(name: &str, response: &egui::Response) {
    #[cfg(test)]
    response
        .ctx
        .data_mut(|d| d.insert_temp(egui::Id::new(format!("test-rect-{name}")), response.rect));
    #[cfg(not(test))]
    let _ = (name, response);
}
impl eframe::App for Editor {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        self.prepare_input(ctx, raw_input);
    }
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.draw(ctx);
    }
}
#[cfg(target_arch = "wasm32")]
pub async fn start() -> Result<(), wasm_bindgen::JsValue> {
    use wasm_bindgen::JsCast;
    let canvas = web_sys::window()
        .unwrap()
        .document()
        .unwrap()
        .get_element_by_id("editor")
        .unwrap()
        .dyn_into::<web_sys::HtmlCanvasElement>()?;
    eframe::WebRunner::new()
        .start(
            canvas,
            eframe::WebOptions::default(),
            Box::new(|_| Ok(Box::<Editor>::default())),
        )
        .await
}

#[cfg(test)]
mod app_tests {
    use super::*;
    fn fixture() -> Vec<u8> {
        let mut b = vec![0; 144];
        b[..4].copy_from_slice(&144u32.to_le_bytes());
        b[4..6].copy_from_slice(&0xa5e0u16.to_le_bytes());
        for p in [6, 8, 10] {
            b[p..p + 2].copy_from_slice(&1u16.to_le_bytes());
        }
        b[12] = 32;
        b[128..132].copy_from_slice(&16u32.to_le_bytes());
        b[132..134].copy_from_slice(&0xf1fau16.to_le_bytes());
        b
    }
    fn editor() -> Editor {
        let mut e = Editor::default();
        for name in ["one.aseprite", "two.aseprite"] {
            e.add(name.into(), name.into(), fixture());
        }
        e.sync_fields();
        e
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn external_refresh_keeps_dirty_edits_and_refreshes_clean_files() {
        let directory = tempfile::tempdir().unwrap();
        let mut editor = editor();
        let path = directory.path().join("one.aseprite");
        editor.files[0].path = path.clone();
        let mut external = editor.files[0].doc.clone();
        external.set_text(0, "external").unwrap();
        std::fs::write(&path, external.encode().unwrap()).unwrap();
        editor.text = "pending local text".into();
        assert!(editor.refresh_external(0).is_err());
        assert_eq!(editor.text, "pending local text");
        assert!(editor.commit_draft());
        assert!(editor.save_one(0).is_err());
        assert!(editor.files[0].dirty);
        editor.undo();
        assert!(!editor.files[0].dirty);
        editor.refresh_external(0).unwrap();
        assert_eq!(editor.text, "external");
        assert!(editor.files[0].undo.is_empty());
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn refreshing_other_file_preserves_current_text_draft() {
        let directory = tempfile::tempdir().unwrap();
        let mut editor = editor();
        let path = directory.path().join("two.aseprite");
        editor.files[1].path = path.clone();
        let mut external = editor.files[1].doc.clone();
        external.set_text(0, "external").unwrap();
        std::fs::write(&path, external.encode().unwrap()).unwrap();
        editor.text = "keep my current draft".into();
        editor.refresh_external(1).unwrap();
        assert_eq!(editor.text, "keep my current draft");
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn double_click_opens_only_clicked_file_and_requests_dirty_confirmation() {
        let mut editor = editor();
        editor.external_editor = Some(Some(external_editor::EditorCommand::for_test(
            "never-launched".into(),
        )));
        editor.files[1]
            .doc
            .set_text(0, "dirty second file")
            .unwrap();
        editor.files[1].dirty = true;
        editor.selection.choose(1, true, false, &[0, 1]);
        editor.sync_fields();
        let ctx = egui::Context::default();
        let frame = |time, events| egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100.0, 750.0),
            )),
            time: Some(time),
            events,
            ..Default::default()
        };
        let _ = ctx.run(frame(0.0, vec![]), |ctx| editor.draw(ctx));
        let pos = ctx
            .data(|data| data.get_temp::<egui::Rect>(egui::Id::new("test-rect-file-1")))
            .unwrap()
            .center();
        for (time, pressed) in [(0.1, true), (0.11, false), (0.2, true), (0.21, false)] {
            let _ = ctx.run(
                frame(
                    time,
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::default(),
                        },
                    ],
                ),
                |ctx| editor.draw(ctx),
            );
        }
        assert_eq!(editor.selection.selected, BTreeSet::from([1]));
        assert_eq!(editor.external_pending, Some(1));
        assert!(editor.external_opened.is_none());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn dirty_external_open_requires_confirmation_without_saving_or_launching() {
        let mut editor = editor();
        editor.external_editor = Some(Some(external_editor::EditorCommand::for_test(
            "never-launched".into(),
        )));
        editor.text = "pending".into();
        editor.request_external(0);
        assert_eq!(editor.external_pending, Some(0));
        assert!(editor.files[0].dirty);
        assert_eq!(editor.files[0].doc.targets()[0].text, "pending");
        editor.external_pending = None;
        assert!(editor.files[0].dirty);
        assert!(editor.external_opened.is_none());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn unavailable_external_editor_does_not_apply_draft() {
        let mut editor = editor();
        editor.external_editor = Some(None);
        editor.text = "pending".into();
        editor.request_external(0);
        assert!(editor.external_pending.is_none());
        assert!(editor.files[0].undo.is_empty());
        assert_eq!(editor.text, "pending");
    }

    #[test]
    fn sprite_preview_is_cached_across_metadata_edits_and_tracks_focus() {
        let mut editor = editor();
        let ctx = egui::Context::default();
        let show = |editor: &mut Editor| {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| editor.show_preview(ui));
            });
        };
        show(&mut editor);
        assert_eq!(editor.preview_decodes, 1);
        editor.text = "changed metadata".into();
        assert!(editor.commit_draft());
        show(&mut editor);
        assert_eq!(editor.preview_decodes, 1);
        editor.selection.choose(1, false, false, &[0, 1]);
        editor.sync_fields();
        show(&mut editor);
        assert_eq!(editor.preview_decodes, 2);
        assert_eq!(editor.preview.as_ref().unwrap().index, 1);
    }

    #[test]
    fn preview_renders_staged_canvas_not_original_file() {
        let mut bytes = fixture();
        bytes[8..10].copy_from_slice(&2u16.to_le_bytes());
        bytes[14] = 1;
        let mut layer = vec![0; 18];
        layer[0] = 3;
        layer[12] = 255;
        let mut cel = vec![0; 20];
        cel[6] = 255;
        cel[16] = 2;
        cel[18] = 1;
        cel.extend([255, 0, 0, 255, 0, 255, 0, 255]);
        for (kind, data) in [(0x2004u16, layer), (0x2005u16, cel)] {
            bytes.extend(((data.len() + 6) as u32).to_le_bytes());
            bytes.extend(kind.to_le_bytes());
            bytes.extend(data);
        }
        let size = bytes.len() as u32;
        bytes[..4].copy_from_slice(&size.to_le_bytes());
        bytes[128..132].copy_from_slice(&(size - 128).to_le_bytes());
        bytes[134..136].copy_from_slice(&2u16.to_le_bytes());
        let mut e = Editor::default();
        e.add(
            "pixels.aseprite".into(),
            "pixels.aseprite".into(),
            bytes.clone(),
        );
        e.sync_fields();
        let ctx = egui::Context::default();
        let show = |e: &mut Editor| {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| e.show_preview(ui));
            });
        };
        show(&mut e);
        assert_eq!(
            e.preview.as_ref().unwrap().image.as_ref().unwrap().size(),
            [2, 1]
        );
        e.transform_selected(aseprite::ArtworkOperation::ExpandCanvas {
            right: 1,
            bottom: 1,
        });
        show(&mut e);
        assert_eq!(
            e.preview.as_ref().unwrap().image.as_ref().unwrap().size(),
            [3, 2]
        );
        assert_eq!(e.files[0].original, bytes);
        e.undo();
        show(&mut e);
        assert_eq!(
            e.preview.as_ref().unwrap().image.as_ref().unwrap().size(),
            [2, 1]
        );
    }

    #[test]
    fn artwork_batch_skips_failure_and_preserves_unselected_files() {
        let mut e = editor();
        let mut huge = fixture();
        huge[8..10].copy_from_slice(&u16::MAX.to_le_bytes());
        e.files[0].doc = Document::parse(&huge).unwrap();
        e.files[0].original = huge.clone();
        e.add(
            "unselected.aseprite".into(),
            "unselected.aseprite".into(),
            fixture(),
        );
        e.selection.selected.extend([0, 1]);
        e.sync_fields();
        e.transform_selected(aseprite::ArtworkOperation::ExpandCanvas {
            right: 1,
            bottom: 2,
        });
        assert_eq!(e.files[0].doc.encode().unwrap(), huge);
        assert!(!e.files[0].dirty);
        assert_eq!(e.files[1].doc.dimensions(), (2, 3));
        assert!(e.files[1].dirty);
        assert_eq!(e.files[2].doc.encode().unwrap(), fixture());
        assert_eq!(e.error_files.len(), 1);
        assert!(e.error_files[0].contains("one.aseprite"));
        assert_eq!(e.files[1].undo.len(), 1);
    }

    #[test]
    fn failed_artwork_batch_does_not_add_history() {
        let mut e = editor();
        let mut huge = fixture();
        huge[8..10].copy_from_slice(&u16::MAX.to_le_bytes());
        e.files[0].doc = Document::parse(&huge).unwrap();
        e.files[0].original = huge;
        e.transform_selected(aseprite::ArtworkOperation::ExpandCanvas {
            right: 1,
            bottom: 0,
        });
        assert!(e.files[0].undo.is_empty());
        assert!(e.files[0].redo.is_empty());
        assert!(!e.files[0].dirty);
    }

    #[test]
    fn partial_batch_undo_does_not_undo_skipped_files_previous_edit() {
        let mut e = editor();
        let mut huge = fixture();
        huge[8..10].copy_from_slice(&u16::MAX.to_le_bytes());
        e.files[0].doc = Document::parse(&huge).unwrap();
        e.files[0].original = huge;
        e.text = "keep earlier edit".into();
        assert!(e.commit_draft());
        e.selection.selected.extend([0, 1]);
        e.sync_fields();
        e.transform_selected(aseprite::ArtworkOperation::ExpandCanvas {
            right: 1,
            bottom: 0,
        });
        e.undo();
        assert_eq!(e.files[0].doc.targets()[0].text, "keep earlier edit");
        assert_eq!(e.files[1].doc.dimensions(), (1, 1));
        e.redo();
        assert_eq!(e.files[0].doc.targets()[0].text, "keep earlier edit");
        assert_eq!(e.files[1].doc.dimensions(), (2, 1));
    }

    #[test]
    fn artwork_commits_pending_draft_and_undo_redo_refresh_preview() {
        let mut e = editor();
        let ctx = egui::Context::default();
        let show = |e: &mut Editor| {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| e.show_preview(ui));
            });
        };
        show(&mut e);
        e.text = "preserved draft".into();
        e.transform_selected(aseprite::ArtworkOperation::ExpandCanvas {
            right: 2,
            bottom: 3,
        });
        assert_eq!(e.files[0].doc.targets()[0].text, "preserved draft");
        assert_eq!(e.files[0].doc.dimensions(), (3, 4));
        assert!(e.preview.is_none());
        show(&mut e);
        assert_eq!(e.preview_decodes, 2);
        e.undo();
        assert_eq!(e.files[0].doc.dimensions(), (1, 1));
        assert_eq!(e.files[0].doc.targets()[0].text, "preserved draft");
        assert!(e.preview.is_none());
        show(&mut e);
        e.redo();
        assert_eq!(e.files[0].doc.dimensions(), (3, 4));
        assert!(e.preview.is_none());
        show(&mut e);
        assert_eq!(e.preview_decodes, 4);
    }

    #[test]
    fn invalid_metadata_draft_prevents_artwork_changes() {
        let mut e = editor();
        e.properties = "invalid json".into();
        e.transform_selected(aseprite::ArtworkOperation::ExpandCanvas {
            right: 1,
            bottom: 1,
        });
        assert_eq!(e.files[0].doc.encode().unwrap(), fixture());
        assert!(e.files[0].undo.is_empty());
        assert!(e.status.contains("Invalid properties"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn new_sprite_save_is_no_clobber_and_dirty_survives_failed_save() {
        let directory = tempfile::tempdir().unwrap();
        let mut e = editor();
        let path = directory.path().join("new.aseprite");
        e.files[0].path = path.clone();
        e.files[0].is_new = true;
        e.files[0].dirty = true;
        std::fs::write(&path, b"unrelated file").unwrap();
        assert!(e.save_one(0).is_err());
        assert!(e.files[0].dirty && e.files[0].is_new);
        assert_eq!(std::fs::read(&path).unwrap(), b"unrelated file");
        std::fs::remove_file(&path).unwrap();
        e.save_one(0).unwrap();
        assert!(!e.files[0].dirty && !e.files[0].is_new);
        assert_eq!(
            std::fs::read(&path).unwrap(),
            e.files[0].doc.encode().unwrap()
        );
        e.text = "subsequent edit".into();
        e.save();
        assert!(!e.files[0].dirty);
        assert_eq!(
            Document::parse(&std::fs::read(&path).unwrap())
                .unwrap()
                .targets()[0]
                .text,
            "subsequent edit"
        );
    }

    #[test]
    fn merge_stages_distinct_copy_and_keeps_sources_unchanged() {
        let mut e = editor();
        let mut larger = fixture();
        larger[8..10].copy_from_slice(&5u16.to_le_bytes());
        larger[10..12].copy_from_slice(&3u16.to_le_bytes());
        e.files[1].doc = Document::parse(&larger).unwrap();
        e.files[1].original = larger.clone();
        e.selection.selected.extend([0, 1]);
        e.sync_fields();
        e.merge_selected();
        assert_eq!(e.files.len(), 3, "{}", e.status);
        assert_eq!(e.files[0].doc.encode().unwrap(), fixture());
        assert_eq!(e.files[1].doc.encode().unwrap(), larger);
        assert!(!e.files[0].dirty && !e.files[1].dirty);
        assert!(e.files[2].dirty && e.files[2].is_new);
        assert_eq!(e.files[2].doc.dimensions(), (5, 3));
        let merged = e.files[2].doc.encode().unwrap();
        assert_eq!(u16::from_le_bytes([merged[6], merged[7]]), 2);
        assert_eq!(e.selection.selected, BTreeSet::from([2]));
        assert!(!e.files[2].original.is_empty());
        e.selection.selected = BTreeSet::from([0, 1]);
        e.sync_fields();
        e.merge_selected();
        assert_eq!(e.files.len(), 4);
        assert_ne!(e.files[2].path, e.files[3].path);
        assert_ne!(e.files[2].name, e.files[3].name);
    }

    #[test]
    fn new_sprite_undo_stays_dirty_until_first_save() {
        let mut e = editor();
        e.files[0].is_new = true;
        e.files[0].dirty = true;
        e.transform_selected(aseprite::ArtworkOperation::ExpandCanvas {
            right: 1,
            bottom: 0,
        });
        e.undo();
        assert!(e.files[0].dirty);
        assert_eq!(e.files[0].doc.encode().unwrap(), fixture());
    }

    #[test]
    fn artwork_controls_fit_normal_window_and_click_expand_once() {
        let mut e = editor();
        e.expand_right = 2;
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1100.0, 750.0));
        let frame = |events| egui::RawInput {
            screen_rect: Some(screen),
            events,
            ..Default::default()
        };
        let rect = |name: &str| {
            ctx.data(|d| d.get_temp::<egui::Rect>(egui::Id::new(format!("test-rect-{name}"))))
                .unwrap()
        };
        let click = |pos| {
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                },
            ]
        };
        let _ = ctx.run(frame(vec![]), |ctx| e.draw(ctx));
        let _ = ctx.run(frame(click(rect("artwork-tools").center())), |ctx| {
            e.draw(ctx)
        });
        // Finish the collapsing animation before checking and interacting.
        ctx.style_mut(|style| style.animation_time = 0.0);
        let _ = ctx.run(frame(vec![]), |ctx| e.draw(ctx));
        for name in [
            "flip-sprite",
            "convert-indexed",
            "convert-rgba",
            "expand-canvas",
            "merge-sprites",
        ] {
            assert!(screen.contains_rect(rect(name)), "{name}: {:?}", rect(name));
        }
        let _ = ctx.run(frame(click(rect("expand-canvas").center())), |ctx| {
            e.draw(ctx)
        });
        assert_eq!(e.files[0].doc.dimensions(), (3, 1));
        let _ = ctx.run(frame(vec![]), |ctx| e.draw(ctx));
        assert_eq!(e.files[0].doc.dimensions(), (3, 1));
        e.error_files = (0..1000)
            .map(|i| format!("file-{i}.aseprite: skipped — too many colors"))
            .collect();
        let _ = ctx.run(frame(vec![]), |ctx| e.draw(ctx));
        for name in [
            "flip-sprite",
            "convert-indexed",
            "expand-canvas",
            "merge-sprites",
        ] {
            assert!(screen.contains_rect(rect(name)), "large batch hid {name}");
        }
    }

    #[test]
    fn single_edit_commit_is_direct_and_idempotent() {
        let mut e = editor();
        e.text = "hello".into();
        assert!(e.commit_draft());
        assert_eq!(e.files[0].doc.targets()[0].text, "hello");
        assert_eq!(e.files[0].undo.len(), 1);
        assert!(e.commit_draft());
        assert_eq!(e.files[0].undo.len(), 1);
        e.text = "changed".into();
        assert!(e.commit_draft());
        assert_eq!(e.files[0].doc.targets()[0].text, "changed");
    }
    #[test]
    fn bulk_append_consumed_once() {
        let mut e = editor();
        e.files[0].doc.set_text(0, "old").unwrap();
        e.files[1].doc.set_text(0, "other").unwrap();
        e.selection.selected.extend([0, 1]);
        e.sync_fields();
        assert!(e.text.is_empty());
        e.text = "new".into();
        assert!(e.commit_draft());
        assert!(e.text.is_empty());
        assert!(e.commit_draft());
        assert_eq!(e.files[0].doc.targets()[0].text, "old\nnew");
        assert_eq!(e.files[1].doc.targets()[0].text, "other\nnew");
    }
    #[test]
    fn switching_selection_stages_old_draft_without_leaking() {
        let mut e = editor();
        e.text = "one only".into();
        e.selection.choose(1, false, false, &[0, 1]);
        e.sync_fields();
        assert_eq!(e.files[0].doc.targets()[0].text, "one only");
        assert!(e.text.is_empty());
        assert_eq!(e.loaded_focus, Some(1));
        assert_eq!(e.files[1].doc.targets()[0].text, "");
    }
    #[test]
    fn invalid_draft_prevents_context_switch() {
        let mut e = editor();
        e.properties = "invalid".into();
        e.selection.choose(1, false, false, &[0, 1]);
        e.sync_fields();
        assert_eq!(e.selection.focus, Some(0));
        assert_eq!(e.properties, "invalid");
        assert!(!e.files[0].dirty);
    }
    #[test]
    fn replacement_is_explicit_undoable_not_repeated_by_save() {
        let mut e = editor();
        e.text = "cat cat".into();
        e.commit_draft();
        e.find = "cat".into();
        e.replace = "catcat".into();
        assert_eq!(e.matches(), 2);
        e.replace_all();
        assert_eq!(e.files[0].doc.targets()[0].text, "catcat catcat");
        e.commit_draft();
        assert_eq!(e.files[0].doc.targets()[0].text, "catcat catcat");
        e.undo();
        assert_eq!(e.files[0].doc.targets()[0].text, "cat cat");
        e.redo();
        assert_eq!(e.files[0].doc.targets()[0].text, "catcat catcat");
    }
    #[test]
    fn save_commits_visible_draft_without_apply() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sprite.aseprite");
        std::fs::write(&path, fixture()).unwrap();
        let mut e = Editor::default();
        e.add("sprite.aseprite".into(), path.clone(), fixture());
        e.sync_fields();
        e.text = "credits=Artist".into();
        e.save();
        assert!(e.error_files.is_empty(), "{:?}", e.error_files);
        let saved = std::fs::read(&path).unwrap();
        assert_eq!(
            Document::parse(&saved).unwrap().targets()[0].text,
            "credits=Artist"
        );
        e.save();
        assert_eq!(std::fs::read(&path).unwrap(), saved);
        assert!(!e.files[0].dirty);
    }
    #[test]
    fn color_draft_does_not_discard_text_and_clear_is_intentional() {
        let mut e = editor();
        e.text = "note".into();
        e.color_edit = Some(Some([1, 2, 3, 80]));
        e.commit_draft();
        assert_eq!(e.files[0].doc.targets()[0].text, "note");
        assert_eq!(e.files[0].doc.targets()[0].color, Some([1, 2, 3, 80]));
        e.color_edit = Some(None);
        e.commit_draft();
        assert_eq!(e.files[0].doc.targets()[0].color, None);
    }
    fn key(key: egui::Key, shift: bool) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers {
                ctrl: true,
                command: true,
                shift,
                ..Default::default()
            },
        }
    }
    #[test]
    fn headless_all_tabs_and_find_shortcut_focus() {
        let ctx = egui::Context::default();
        let mut e = editor();
        for tab in [Tab::UserData, Tab::Properties, Tab::Tags] {
            e.tab = tab;
            let out = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1100., 750.),
                    )),
                    ..Default::default()
                },
                |ctx| e.draw(ctx),
            );
            assert!(!out.shapes.is_empty());
        }
        let _ = ctx.run(
            egui::RawInput {
                events: vec![key(egui::Key::H, false)],
                ..Default::default()
            },
            |ctx| e.draw(ctx),
        );
        assert!(e.find_tab);
        assert_eq!(
            ctx.memory(|m| m.focused()),
            Some(egui::Id::new("find-text"))
        );
    }
    #[test]
    fn ctrl_a_selects_files_only_outside_text() {
        let ctx = egui::Context::default();
        let mut e = editor();
        let _ = ctx.run(Default::default(), |ctx| e.draw(ctx));
        let _ = ctx.run(
            egui::RawInput {
                events: vec![key(egui::Key::A, false)],
                ..Default::default()
            },
            |ctx| e.draw(ctx),
        );
        assert_eq!(e.selection.selected.len(), 2);
    }
    #[test]
    fn control_arrow_focus_does_not_change_editing_target() {
        let mut e = editor();
        e.files[0].doc.set_text(0, "selected").unwrap();
        e.files[1].doc.set_text(0, "unselected").unwrap();
        e.load_fields();
        e.selection.step(1, true, false, &[0, 1]);
        e.sync_fields();
        assert_eq!(e.selection.focus, Some(1));
        assert_eq!(e.loaded_focus, Some(0));
        assert_eq!(e.text, "selected");
        e.text = "edited selected".into();
        e.commit_draft();
        assert_eq!(e.files[0].doc.targets()[0].text, "edited selected");
        assert_eq!(e.files[1].doc.targets()[0].text, "unselected");
    }
    #[test]
    fn find_replace_respects_layer_scope() {
        let mut bytes = fixture();
        let mut body = vec![0; 16];
        body.extend([1, 0, b'L']);
        let size = (body.len() + 6) as u32;
        bytes.extend(size.to_le_bytes());
        bytes.extend(0x2004u16.to_le_bytes());
        bytes.extend(body);
        let len = bytes.len() as u32;
        bytes[..4].copy_from_slice(&len.to_le_bytes());
        bytes[128..132].copy_from_slice(&(len - 128).to_le_bytes());
        bytes[134..136].copy_from_slice(&1u16.to_le_bytes());
        let mut e = Editor::default();
        e.add("layer.aseprite".into(), "layer.aseprite".into(), bytes);
        e.sync_fields();
        e.files[0].doc.set_text(0, "cat global").unwrap();
        e.files[0].doc.set_text(1, "cat layer").unwrap();
        e.target = 1;
        e.sync_fields();
        e.find = "cat".into();
        e.replace = "dog".into();
        assert_eq!(e.matches(), 1);
        e.replace_all();
        assert_eq!(e.files[0].doc.targets()[0].text, "cat global");
        assert_eq!(e.files[0].doc.targets()[1].text, "dog layer");
    }
    #[test]
    fn focused_replace_text_is_used_by_following_action_frame() {
        let ctx = egui::Context::default();
        let mut e = editor();
        e.text = "Original".into();
        e.commit_draft();
        e.find_tab = true;
        e.find = "Original".into();
        let frame = |events| egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100., 750.),
            )),
            events,
            ..Default::default()
        };
        let _ = ctx.run(frame(vec![]), |ctx| e.draw(ctx));
        let replace = ctx
            .data(|d| d.get_temp::<egui::Rect>(egui::Id::new("test-rect-replace")))
            .unwrap()
            .center();
        let click = |p| {
            vec![
                egui::Event::PointerMoved(p),
                egui::Event::PointerButton {
                    pos: p,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
                egui::Event::PointerButton {
                    pos: p,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                },
            ]
        };
        let _ = ctx.run(frame(click(replace)), |ctx| e.draw(ctx));
        assert_eq!(
            ctx.memory(|m| m.focused()),
            Some(egui::Id::new("replace-text"))
        );
        let _ = ctx.run(frame(vec![egui::Event::Text("Updated".into())]), |ctx| {
            e.draw(ctx)
        });
        assert_eq!(e.replace, "Updated");
        let button = ctx
            .data(|d| d.get_temp::<egui::Rect>(egui::Id::new("test-rect-replace_all")))
            .unwrap()
            .center();
        let _ = ctx.run(frame(click(button)), |ctx| e.draw(ctx));
        assert_eq!(e.files[0].doc.targets()[0].text, "Updated");
    }
    #[test]
    fn coalesced_click_type_click_preserves_text_before_replace() {
        let ctx = egui::Context::default();
        let mut e = editor();
        e.text = "Original".into();
        e.commit_draft();
        e.find_tab = true;
        e.find = "Original".into();
        let frame = |events| egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100., 750.),
            )),
            events,
            ..Default::default()
        };
        let _ = ctx.run(frame(vec![]), |ctx| e.draw(ctx));
        let center = |name: &str| {
            ctx.data(|d| d.get_temp::<egui::Rect>(egui::Id::new(format!("test-rect-{name}"))))
                .unwrap()
                .center()
        };
        let click = |p| {
            vec![
                egui::Event::PointerMoved(p),
                egui::Event::PointerButton {
                    pos: p,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
                egui::Event::PointerButton {
                    pos: p,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                },
            ]
        };
        let mut events = click(center("replace"));
        events.push(egui::Event::Text("Updated".into()));
        events.extend(click(center("replace_all")));
        let original_event_count = events.len();
        let mut input = frame(events);
        e.prepare_input(&ctx, &mut input);
        assert_eq!(
            input.events.len() + e.pending_input.len(),
            original_event_count
        );
        let _ = ctx.run(input, |ctx| e.draw(ctx));
        assert_eq!(e.files[0].doc.targets()[0].text, "Original");
        let mut input = frame(vec![]);
        e.prepare_input(&ctx, &mut input);
        let _ = ctx.run(input, |ctx| e.draw(ctx));
        assert_eq!(e.replace, "Updated");
        let mut input = frame(vec![]);
        e.prepare_input(&ctx, &mut input);
        let _ = ctx.run(input, |ctx| e.draw(ctx));
        assert!(e.pending_input.is_empty());
        assert_eq!(e.files[0].doc.targets()[0].text, "Updated");
    }
    #[test]
    fn pending_events_precede_new_input_without_duplication() {
        let ctx = egui::Context::default();
        let mut e = Editor {
            pending_input: vec![egui::Event::Text("first".into())],
            ..Default::default()
        };
        let mut input = egui::RawInput {
            events: vec![
                egui::Event::Paste("second".into()),
                key(egui::Key::S, false),
            ],
            ..Default::default()
        };
        e.prepare_input(&ctx, &mut input);
        assert!(e.pending_input.is_empty());
        assert_eq!(input.events.len(), 3);
        assert!(matches!(&input.events[0],egui::Event::Text(t) if t=="first"));
        assert!(matches!(&input.events[1],egui::Event::Paste(t) if t=="second"));
    }
    #[test]
    fn picker_pointer_batches_are_not_deferred() {
        let ctx = egui::Context::default();
        let mut e = Editor::default();
        for _ in 0..2 {
            let events = vec![
                egui::Event::PointerMoved(egui::pos2(40., 15.)),
                egui::Event::PointerButton {
                    pos: egui::pos2(40., 15.),
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
                egui::Event::PointerButton {
                    pos: egui::pos2(40., 15.),
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                },
            ];
            let mut input = egui::RawInput {
                events: events.clone(),
                ..Default::default()
            };
            e.prepare_input(&ctx, &mut input);
            assert!(e.pending_input.is_empty());
            assert_eq!(input.events, events);
        }
    }
}
