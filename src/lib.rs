pub mod aseprite;
mod io;
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
    undo: Vec<Vec<u8>>,
}
#[derive(Default, PartialEq)]
enum Mode {
    #[default]
    Append,
    Override,
}
#[derive(Default, PartialEq)]
enum Tab {
    #[default]
    UserData,
    Properties,
    Tags,
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
    preview: Vec<(usize, Document)>,
    loaded_focus: Option<usize>,
    settings: bool,
    separator: String,
    error_files: Vec<String>,
    confirm_close: bool,
    allow_close: bool,
}
impl Editor {
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
            Ok(doc) => self.files.push(File {
                name,
                path,
                original: bytes,
                doc,
                dirty: false,
                undo: vec![],
            }),
            Err(e) => self.error_files.push(format!("{name}: {e}")),
        }
    }
    fn open(&mut self, directory: bool) {
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
    fn sync_fields(&mut self) {
        if self.loaded_focus == self.selection.focus {
            return;
        }
        self.loaded_focus = self.selection.focus;
        self.target = 0;
        self.preview.clear();
        if let Some(f) = self.selection.focus.and_then(|i| self.files.get(i)) {
            if let Some(t) = f.doc.targets().first() {
                self.text = if self.mode == Mode::Append {
                    String::new()
                } else {
                    t.text.clone()
                };
                self.properties = t
                    .properties
                    .as_ref()
                    .map(|p| serde_json::to_string_pretty(p).unwrap_or_default())
                    .unwrap_or_else(|| "{\"maps\":[]}".into());
            }
        }
    }
    fn stage(&mut self, find_replace: bool) {
        if !find_replace
            && self.tab == Tab::UserData
            && self.mode == Mode::Append
            && self.text.is_empty()
        {
            self.preview.clear();
            self.status = "Nothing to append".into();
            return;
        }
        self.preview.clear();
        self.error_files.clear();
        for &i in &self.selection.selected {
            let mut doc = self.files[i].doc.clone();
            let result: Result<(), String> = (|| {
                if self.tab == Tab::Tags {
                    for mut tag in doc.tags() {
                        tag.name = tag.name.replace(&self.find, &self.replace);
                        doc.set_tag(&tag)?;
                    }
                    return Ok(());
                }
                let source = self
                    .selection
                    .focus
                    .and_then(|idx| {
                        self.files[idx]
                            .doc
                            .targets()
                            .into_iter()
                            .find(|t| t.id == self.target)
                    })
                    .ok_or("Select a metadata scope")?;
                let target = doc
                    .targets()
                    .into_iter()
                    .find(|t| t.kind == source.kind && t.label == source.label)
                    .ok_or("Selected metadata scope is absent")?;
                if self.tab == Tab::Properties {
                    let value =
                        serde_json::from_str(&self.properties).map_err(|e| e.to_string())?;
                    doc.set_properties(target.id, &value)?;
                } else {
                    let text = if find_replace {
                        target.text.replace(&self.find, &self.replace)
                    } else if self.mode == Mode::Override || target.text.is_empty() {
                        self.text.clone()
                    } else {
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
                    };
                    doc.set_text(target.id, &text)?;
                }
                Ok(())
            })();
            match result {
                Ok(()) => {
                    if doc.encode().ok() != self.files[i].doc.encode().ok() {
                        self.preview.push((i, doc));
                    }
                }
                Err(e) => self
                    .error_files
                    .push(format!("{}: {e}", self.files[i].name)),
            }
        }
        self.status = format!("{} files will change", self.preview.len());
    }
    fn apply(&mut self) {
        for (i, doc) in self.preview.drain(..) {
            if let Ok(before) = self.files[i].doc.encode() {
                self.files[i].undo.push(before);
                self.files[i].doc = doc;
                self.files[i].dirty = true;
            }
        }
        self.status = "Changes staged. Save when ready.".into();
        self.loaded_focus = None;
    }
    fn undo(&mut self) {
        for &i in &self.selection.selected {
            if let Some(bytes) = self.files[i].undo.pop() {
                if let Ok(doc) = Document::parse(&bytes) {
                    self.files[i].doc = doc;
                    self.files[i].dirty = bytes != self.files[i].original;
                }
            }
        }
        self.loaded_focus = None;
    }
    fn save(&mut self) {
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
            match io::save(&f.path, &f.original, &bytes) {
                Ok(()) => {
                    f.original = bytes;
                    f.dirty = false;
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
        let previous_selection = self.selection.selected.clone();
        #[cfg(not(target_arch = "wasm32"))]
        if ctx.input(|i| i.viewport().close_requested())
            && !self.allow_close
            && (self.files.iter().any(|f| f.dirty) || !self.preview.is_empty())
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm_close = true;
        }
        #[cfg(target_arch = "wasm32")]
        {
            io::report(self.files.len(), self.selection.selected.len());
            io::dirty(self.files.iter().any(|f| f.dirty) || !self.preview.is_empty());
            use wasm_bindgen::JsValue;
            for item in io::poll().iter() {
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
                if ui.button("Open folder").clicked() {
                    self.open(true);
                }
                if ui.button("Open files").clicked() {
                    self.open(false);
                }
                if ui
                    .add_enabled(
                        self.files.iter().any(|f| f.dirty),
                        egui::Button::new(if cfg!(target_arch = "wasm32") {
                            "Export selected ZIP"
                        } else {
                            "Save selected"
                        }),
                    )
                    .on_hover_text("Ctrl+S")
                    .clicked()
                {
                    self.save();
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
                                "{}{}{}",
                                if self.selection.focus == Some(i) {
                                    "› "
                                } else {
                                    ""
                                },
                                parts.last().unwrap_or(&""),
                                if f.dirty { " •" } else { "" }
                            );
                            let response = ui
                                .selectable_label(self.selection.selected.contains(&i), label)
                                .on_hover_text(&f.name);
                            if response.clicked() {
                                chosen = Some((
                                    i,
                                    ui.input(|s| s.modifiers.command || s.modifiers.ctrl),
                                    ui.input(|s| s.modifiers.shift),
                                ));
                            }
                            response.context_menu(|ui| {
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
                        self.preview.clear();
                    }
                    if let Some(save) = action {
                        if save {
                            self.save();
                        } else {
                            self.undo();
                        }
                    }
                });
            });
        let editing = ctx.wants_keyboard_input();
        ctx.input(|input| {
            if input.modifiers.command && input.key_pressed(egui::Key::S) {
                self.save();
            }
            if !editing {
                if input.modifiers.command && input.key_pressed(egui::Key::A) {
                    self.selection.selected.extend(visible.iter().copied());
                }
                if input.modifiers.command && input.key_pressed(egui::Key::Z) {
                    self.undo();
                }
                for (key, delta) in [(egui::Key::ArrowUp, -1), (egui::Key::ArrowDown, 1)] {
                    if input.key_pressed(key) {
                        self.selection.step(
                            delta,
                            input.modifiers.ctrl || input.modifiers.command,
                            input.modifiers.shift,
                            &visible,
                        );
                    }
                }
                if input.key_pressed(egui::Key::Space) {
                    if let Some(i) = self.selection.focus {
                        self.selection.choose(i, true, false, &visible);
                    }
                }
            }
        });
        if previous_selection != self.selection.selected {
            self.preview.clear();
        }
        self.sync_fields();
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.label(&self.status);
            for error in &self.error_files {
                ui.colored_label(egui::Color32::LIGHT_RED, error);
            }
        });
        egui::CentralPanel::default().show(ctx,|ui|{
   if self.files.is_empty(){ui.centered_and_justified(|ui|{ui.label("Open a folder or Aseprite file");});return;}
   if self.selection.selected.is_empty(){ui.label("Select files to edit");return;}
   ui.horizontal(|ui|{if ui.selectable_value(&mut self.tab,Tab::UserData,"User data").changed(){self.preview.clear();}
if ui.selectable_value(&mut self.tab,Tab::Properties,"Properties").changed(){self.preview.clear();}
if ui.selectable_value(&mut self.tab,Tab::Tags,"Tags").changed(){self.preview.clear();}});ui.separator();
   if let Some(i)=self.selection.focus {
    let targets=self.files[i].doc.targets();let before=self.target;
    if self.tab!=Tab::Tags {egui::ComboBox::from_label("Scope").selected_text(targets.iter().find(|t|t.id==self.target).map(|t|t.label.as_str()).unwrap_or("Sprite")).show_ui(ui,|ui|{for t in &targets{ui.selectable_value(&mut self.target,t.id,&t.label);}});}
    if before!=self.target{self.preview.clear();if let Some(t)=targets.iter().find(|t|t.id==self.target){self.text=if self.mode==Mode::Append{String::new()}else{t.text.clone()};self.properties=t.properties.as_ref().map(|p|serde_json::to_string_pretty(p).unwrap_or_default()).unwrap_or_else(||"{\"maps\":[]}".into());}}
    match self.tab {
     Tab::UserData=>{
      ui.horizontal(|ui|{if ui.selectable_value(&mut self.mode,Mode::Append,"Append").changed(){self.text.clear();}
if ui.selectable_value(&mut self.mode,Mode::Override,"Override").changed(){self.text=targets.iter().find(|t|t.id==self.target).map(|t|t.text.clone()).unwrap_or_default();}});
      if let Some(t)=targets.iter().find(|t|t.id==self.target){ui.collapsing("Current value",|ui|{ui.label(&t.text);});}
      if ui.add(egui::TextEdit::multiline(&mut self.text).desired_rows(9).desired_width(f32::INFINITY)).changed(){self.preview.clear();}
      if ui.button("Preview changes").clicked(){self.stage(false);}
      if let Some(target)=targets.iter().find(|t|t.id==self.target) {
       let mut rgba=self.preview.iter().find(|(idx,_)|*idx==i).and_then(|(_,doc)|doc.targets().into_iter().find(|t|t.id==self.target)).and_then(|t|t.color).or(target.color).unwrap_or([0,0,0,255]);
       ui.horizontal(|ui|{ui.label("Metadata color");let changed=ui.color_edit_button_srgba_unmultiplied(&mut rgba).changed();let clear=ui.small_button("Clear").clicked();
        if changed||clear {self.preview.clear();for &idx in &self.selection.selected {let mut doc=self.files[idx].doc.clone();if let Some(t)=doc.targets().into_iter().find(|t|t.kind==target.kind&&t.label==target.label){match doc.set_color(t.id,if clear{None}else{Some(rgba)}){Ok(())=>self.preview.push((idx,doc)),Err(e)=>self.status=e}}}}
       });
      }

     }
     Tab::Properties=>{
      if let Some(w)=targets.iter().find(|t|t.id==self.target).and_then(|t|t.warning.as_ref()){ui.colored_label(egui::Color32::YELLOW,w);}
      if ui.add(egui::TextEdit::multiline(&mut self.properties).code_editor().desired_rows(12).desired_width(f32::INFINITY)).changed(){self.preview.clear();}
      if ui.button("Preview property replacement").clicked(){self.stage(false);}
     }
     Tab::Tags=>{
      egui::ScrollArea::vertical().max_height(260.0).show(ui,|ui|{
       let mut tags=self.preview.iter().find(|(idx,_)|*idx==i).map(|(_,doc)|doc.tags()).unwrap_or_else(||self.files[i].doc.tags());let mut edited=false;
       for tag in &mut tags {ui.horizontal(|ui|{
        edited|=ui.text_edit_singleline(&mut tag.name).changed();
        edited|=ui.add(egui::DragValue::new(&mut tag.from).prefix("From ")).changed();
        edited|=ui.add(egui::DragValue::new(&mut tag.to).prefix("To ")).changed();
        edited|=ui.add(egui::DragValue::new(&mut tag.direction).range(0..=3).prefix("Direction ")).on_hover_text("0 Forward · 1 Reverse · 2 Ping-pong · 3 Reverse ping-pong").changed();
        edited|=ui.add(egui::DragValue::new(&mut tag.repeat).prefix("Repeat ")).changed();
       });}
       if edited{let mut doc=self.preview.iter().find(|(idx,_)|*idx==i).map(|(_,doc)|doc.clone()).unwrap_or_else(||self.files[i].doc.clone());for tag in tags{if let Err(e)=doc.set_tag(&tag){self.status=e;return;}}self.preview=vec![(i,doc)];}
      });
     }
    }
   }
   if self.tab!=Tab::Properties {ui.separator();ui.horizontal(|ui|{ui.label("Find");if ui.text_edit_singleline(&mut self.find).changed(){self.preview.clear();}ui.label("Replace");if ui.text_edit_singleline(&mut self.replace).changed(){self.preview.clear();}});if ui.add_enabled(!self.find.is_empty(),egui::Button::new(if self.tab==Tab::Tags{"Preview tag rename"}else{"Preview find & replace"})).clicked(){self.stage(true);}}
   if !self.preview.is_empty(){ui.separator();ui.heading(format!("Review · {} files",self.preview.len()));
    egui::ScrollArea::vertical().max_height(190.0).show(ui,|ui|{for (i,doc) in &self.preview {ui.collapsing(&self.files[*i].name,|ui|{
      if self.tab==Tab::Tags{ui.label(format!("Before: {}",self.files[*i].doc.tags().iter().map(|t|format!("{} [{}–{}; dir {}; repeat {}]",t.name,t.from,t.to,t.direction,t.repeat)).collect::<Vec<_>>().join(", ")));ui.label(format!("After: {}",doc.tags().iter().map(|t|format!("{} [{}–{}; dir {}; repeat {}]",t.name,t.from,t.to,t.direction,t.repeat)).collect::<Vec<_>>().join(", ")));}
      else {let before=self.files[*i].doc.targets();let after=doc.targets();for new in after{if let Some(old)=before.iter().find(|t|t.id==new.id){if old.text!=new.text{ui.label(format!("− {}",old.text));ui.label(format!("+ {}",new.text));}
if old.color!=new.color{ui.label(format!("Color: {:?} → {:?}",old.color,new.color));}
if old.properties!=new.properties{ui.label(format!("− {:?}",old.properties));ui.label(format!("+ {:?}",new.properties));}}}}
    });}});
    ui.horizontal(|ui|{if ui.button("Apply to staged files").clicked(){self.apply();}
if ui.button("Cancel").clicked(){self.preview.clear();}});
   }
  });
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
impl eframe::App for Editor {
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
        b[0..4].copy_from_slice(&144u32.to_le_bytes());
        b[4..6].copy_from_slice(&0xa5e0u16.to_le_bytes());
        b[6..8].copy_from_slice(&1u16.to_le_bytes());
        b[8..10].copy_from_slice(&1u16.to_le_bytes());
        b[10..12].copy_from_slice(&1u16.to_le_bytes());
        b[12..14].copy_from_slice(&32u16.to_le_bytes());
        b[128..132].copy_from_slice(&16u32.to_le_bytes());
        b[132..134].copy_from_slice(&0xf1fau16.to_le_bytes());
        b
    }
    #[test]
    fn preview_cancel_apply_undo_are_non_destructive() {
        let mut e = Editor::default();
        e.add("art.aseprite".into(), "art.aseprite".into(), fixture());
        e.selection.choose(0, false, false, &[0]);
        e.sync_fields();
        e.text = "credits=Artist".into();
        e.stage(false);
        assert_eq!(e.preview.len(), 1);
        assert!(!e.files[0].dirty);
        e.preview.clear();
        assert_eq!(e.files[0].doc.targets()[0].text, "");
        e.stage(false);
        e.apply();
        assert_eq!(e.files[0].doc.targets()[0].text, "credits=Artist");
        assert!(e.files[0].dirty);
        e.undo();
        assert!(!e.files[0].dirty);
        assert_eq!(e.files[0].doc.targets()[0].text, "");
    }
    #[test]
    fn blank_append_is_noop() {
        let mut e = Editor::default();
        e.add("art.aseprite".into(), "art.aseprite".into(), fixture());
        e.selection.choose(0, false, false, &[0]);
        e.stage(false);
        assert!(e.preview.is_empty());
    }
    #[test]
    fn headless_egui_renders_all_tabs_and_preview() {
        let ctx = egui::Context::default();
        let mut editor = Editor::default();
        editor.add(
            "folder/art.aseprite".into(),
            "folder/art.aseprite".into(),
            fixture(),
        );
        editor.selection.choose(0, false, false, &[0]);
        for tab in [Tab::UserData, Tab::Properties, Tab::Tags] {
            editor.tab = tab;
            let out = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1000.0, 700.0),
                    )),
                    ..Default::default()
                },
                |ctx| editor.draw(ctx),
            );
            assert!(!out.shapes.is_empty());
        }
        editor.tab = Tab::UserData;
        editor.text = "credits=Artist".into();
        editor.stage(false);
        let _ = ctx.run(Default::default(), |ctx| editor.draw(ctx));
        assert_eq!(editor.preview.len(), 1);
    }
}
