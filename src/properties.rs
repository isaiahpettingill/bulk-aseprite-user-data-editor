//! Typed property editing; no coercion until a user explicitly changes a type.
use eframe::egui;
use serde_json::{json, Value};
use std::collections::BTreeSet;
const TYPES: [&str; 19] = [
    "Boolean",
    "Int8",
    "Uint8",
    "Int16",
    "Uint16",
    "Int32",
    "Uint32",
    "Int64",
    "Uint64",
    "Fixed16.16",
    "Float32",
    "Float64",
    "String",
    "Point",
    "Size",
    "Rectangle",
    "Vector",
    "Map",
    "UUID",
];
fn default_value(ty: u64) -> Value {
    match ty {
        1 => json!(false),
        8 | 9 => json!("0"),
        13 => json!(""),
        14 | 15 => json!([0, 0]),
        16 => json!([0, 0, 0, 0]),
        17 => json!({"element_type":0,"items":[]}),
        18 => json!([]),
        19 => json!("00000000000000000000000000000000"),
        _ => json!(0),
    }
}
fn type_picker(ui: &mut egui::Ui, ty: &mut u64) -> bool {
    let before = *ty;
    egui::ComboBox::from_id_salt("type")
        .selected_text(
            TYPES
                .get(ty.saturating_sub(1) as usize)
                .copied()
                .unwrap_or("Unsupported"),
        )
        .show_ui(ui, |ui| {
            for (i, label) in TYPES.iter().enumerate() {
                ui.selectable_value(ty, i as u64 + 1, *label);
            }
        });
    before != *ty
}
fn value_ui(ui: &mut egui::Ui, ty: u64, value: &mut Value, depth: usize) -> bool {
    if depth > 32 {
        ui.label("Nested value: use Advanced JSON");
        return false;
    }
    match ty {
        1 => {
            let mut b = value.as_bool().unwrap_or(false);
            let c = ui.checkbox(&mut b, "").changed();
            if c {
                *value = json!(b);
            }
            c
        }
        2..=7 => {
            let mut n = value.as_i64().unwrap_or(0);
            let (min, max) = match ty {
                2 => (i8::MIN as i64, i8::MAX as i64),
                3 => (0, u8::MAX as i64),
                4 => (i16::MIN as i64, i16::MAX as i64),
                5 => (0, u16::MAX as i64),
                7 => (0, u32::MAX as i64),
                _ => (i32::MIN as i64, i32::MAX as i64),
            };
            let c = ui
                .add(egui::DragValue::new(&mut n).range(min..=max))
                .changed();
            if c {
                *value = json!(n);
            }
            c
        }
        10 => {
            let mut n = value.as_i64().unwrap_or(0) as f64 / 65536.0;
            let changed = ui
                .add(
                    egui::DragValue::new(&mut n)
                        .range(-32768.0..=(i32::MAX as f64 / 65536.0))
                        .speed(0.01),
                )
                .changed();
            if changed {
                *value = json!((n * 65536.0).round() as i32);
            }
            changed
        }
        11 | 12 => {
            let mut n = value.as_f64().unwrap_or(0.0);
            let limit = if ty == 11 { f32::MAX as f64 } else { f64::MAX };
            let c = ui
                .add(
                    egui::DragValue::new(&mut n)
                        .range(-limit..=limit)
                        .speed(0.01),
                )
                .changed();
            if c && n.is_finite() {
                *value = json!(n);
            }
            c
        }
        8 | 9 | 13 | 19 => {
            let mut text = value.as_str().unwrap_or_default().to_owned();
            let c = ui.text_edit_singleline(&mut text).changed();
            if c {
                *value = json!(text);
            }
            c
        }
        14..=16 => {
            let mut changed = false;
            if let Some(values) = value.as_array_mut() {
                ui.horizontal(|ui| {
                    for (i, v) in values.iter_mut().enumerate() {
                        ui.push_id(i, |ui| {
                            changed |= value_ui(ui, 6, v, depth + 1);
                        });
                    }
                });
            }
            changed
        }
        18 => {
            let mut changed = false;
            ui.collapsing("Properties", |ui| {
                changed = rows(ui, value, depth + 1);
            });
            changed
        }
        17 => {
            let mut changed = false;
            ui.collapsing("Items", |ui| {
                let et = value["element_type"].as_u64().unwrap_or(0);
                if let Some(items) = value["items"].as_array_mut() {
                    let mut remove = None;
                    for (i, item) in items.iter_mut().enumerate() {
                        ui.push_id(i, |ui| {
                            ui.horizontal(|ui| {
                                let mut t = item["type"].as_u64().unwrap_or(13);
                                if et == 0 && type_picker(ui, &mut t) {
                                    item["type"] = json!(t);
                                    item["value"] = default_value(t);
                                    changed = true;
                                }
                                changed |= value_ui(ui, t, &mut item["value"], depth + 1);
                                if ui.small_button("−").on_hover_text("Remove item").clicked() {
                                    remove = Some(i);
                                }
                            });
                        });
                    }
                    if let Some(i) = remove {
                        items.remove(i);
                        changed = true;
                    }
                    if ui.small_button("+ Item").clicked() {
                        let t = if et == 0 { 13 } else { et };
                        items.push(json!({"type":t,"value":default_value(t)}));
                        changed = true;
                    }
                }
            });
            changed
        }
        _ => {
            ui.label("Unsupported value preserved");
            false
        }
    }
}
fn rows(ui: &mut egui::Ui, properties: &mut Value, depth: usize) -> bool {
    let Some(items) = properties.as_array_mut() else {
        ui.label("Unsupported properties preserved");
        return false;
    };
    let mut changed = false;
    let mut remove = None;
    for (i, item) in items.iter_mut().enumerate() {
        ui.push_id(i, |ui| {
            ui.horizontal(|ui| {
                let mut name = item["name"].as_str().unwrap_or_default().to_owned();
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut name)
                            .hint_text("Name")
                            .desired_width(120.0),
                    )
                    .changed()
                {
                    item["name"] = json!(name);
                    changed = true;
                }
                let mut ty = item["type"].as_u64().unwrap_or(13);
                if type_picker(ui, &mut ty) {
                    item["type"] = json!(ty);
                    item["value"] = default_value(ty);
                    changed = true;
                }
                changed |= value_ui(ui, ty, &mut item["value"], depth);
                if ui
                    .small_button("−")
                    .on_hover_text("Remove property")
                    .clicked()
                {
                    remove = Some(i);
                }
            });
        });
    }
    if let Some(i) = remove {
        items.remove(i);
        changed = true;
    }
    if ui.button("+ Property").clicked() {
        let mut name = "property".to_owned();
        let mut n = 2;
        while items.iter().any(|p| p["name"] == name) {
            name = format!("property{n}");
            n += 1;
        }
        items.push(json!({"name":name,"type":13,"value":""}));
        changed = true;
    }
    changed
}
pub fn edit(ui: &mut egui::Ui, value: &mut Value) -> bool {
    let mut changed = false;
    let Some(maps) = value["maps"].as_array_mut() else {
        ui.label("Invalid property maps");
        return false;
    };
    if maps.is_empty() && ui.button("+ Property").clicked() {
        maps.push(json!({"key":0,"properties":[{"name":"property","type":13,"value":""}]}));
        changed = true;
    }
    for (i, map) in maps.iter_mut().enumerate() {
        ui.push_id(("map", i), |ui| {
            let key = map["key"].as_u64().unwrap_or(0);
            if key == 0 {
                changed |= rows(ui, &mut map["properties"], 0);
            } else {
                ui.collapsing(format!("Extension {key}"), |ui| {
                    changed |= rows(ui, &mut map["properties"], 0);
                });
            }
        });
    }
    if let Err(e) = validate(value) {
        ui.colored_label(egui::Color32::LIGHT_RED, e);
    }
    changed
}
pub fn validate(value: &Value) -> Result<(), String> {
    fn props(value: &Value, depth: usize) -> Result<(), String> {
        if depth > 128 {
            return Err("Property nesting limit".into());
        }
        let items = value.as_array().ok_or("Expected property rows")?;
        let mut names = BTreeSet::new();
        for p in items {
            let name = p["name"].as_str().ok_or("Expected name")?;
            if name.len() > 65535 {
                return Err("Property name exceeds 65535 bytes".into());
            }
            if !names.insert(name) {
                return Err(format!("Duplicate property: {name}"));
            }
            check_value(
                p["type"].as_u64().ok_or("Expected type")?,
                &p["value"],
                depth + 1,
            )?;
        }
        Ok(())
    }
    fn check_value(ty: u64, v: &Value, depth: usize) -> Result<(), String> {
        if depth > 128 {
            return Err("Property nesting limit".into());
        }
        match ty {
            1 => {
                v.as_bool().ok_or("Expected boolean")?;
            }
            2..=7 | 10 => {
                let n = v.as_i64().ok_or("Expected integer")?;
                let (min, max) = match ty {
                    2 => (i8::MIN as i64, i8::MAX as i64),
                    3 => (0, u8::MAX as i64),
                    4 => (i16::MIN as i64, i16::MAX as i64),
                    5 => (0, u16::MAX as i64),
                    7 => (0, u32::MAX as i64),
                    _ => (i32::MIN as i64, i32::MAX as i64),
                };
                if n < min || n > max {
                    return Err("Integer out of range".into());
                }
            }
            13 => {
                if v.as_str().ok_or("Expected text")?.len() > 65535 {
                    return Err("Text exceeds 65535 bytes".into());
                }
            }
            14..=16 => {
                let components = v.as_array().ok_or("Expected geometry")?;
                if components.len() != if ty == 16 { 4 } else { 2 } {
                    return Err("Invalid geometry components".into());
                }
                for component in components {
                    check_value(6, component, depth + 1)?;
                }
            }
            8 => {
                v.as_str()
                    .ok_or("Expected integer")?
                    .parse::<i64>()
                    .map_err(|_| "Int64 out of range")?;
            }
            9 => {
                v.as_str()
                    .ok_or("Expected integer")?
                    .parse::<u64>()
                    .map_err(|_| "Uint64 out of range")?;
            }
            11 | 12 => {
                let f = v.as_f64().ok_or("Expected number")?;
                if !f.is_finite() || (ty == 11 && !(f as f32).is_finite()) {
                    return Err("Float out of range".into());
                }
            }
            17 => {
                let element_type = v["element_type"]
                    .as_u64()
                    .filter(|t| *t <= 19)
                    .ok_or("Invalid vector type")?;
                for item in v["items"].as_array().ok_or("Expected vector")? {
                    if element_type != 0 && item["type"].as_u64() != Some(element_type) {
                        return Err("Vector type mismatch".into());
                    }
                    check_value(
                        item["type"].as_u64().ok_or("Expected type")?,
                        &item["value"],
                        depth + 1,
                    )?;
                }
            }
            18 => props(v, depth + 1)?,
            19 => {
                let s = v.as_str().ok_or("Expected UUID")?;
                if s.len() != 32 || !s.bytes().all(|c| c.is_ascii_hexdigit()) {
                    return Err("UUID needs 32 hexadecimal digits".into());
                }
            }
            _ => return Err("Unsupported property type".into()),
        }
        Ok(())
    }
    let mut keys = BTreeSet::new();
    for m in value["maps"].as_array().ok_or("Expected property maps")? {
        let key = m["key"]
            .as_u64()
            .filter(|k| *k <= u32::MAX as u64)
            .ok_or("Invalid map key")?;
        if !keys.insert(key) {
            return Err("Duplicate property map".into());
        }
        props(&m["properties"], 0)?;
    }
    Ok(())
}
/// Apply only explicitly changed fields. Untouched destination properties survive.
pub fn merge_changes(base: &Value, edited: &Value, destination: &Value) -> Result<Value, String> {
    validate(edited)?;
    fn patch_rows(base: &Value, edited: &Value, dest: &Value) -> Value {
        let empty = Vec::new();
        let b = base.as_array().unwrap_or(&empty);
        let e = edited.as_array().unwrap_or(&empty);
        let mut d = dest.as_array().cloned().unwrap_or_default();
        for old in b {
            if !e.iter().any(|p| p["name"] == old["name"]) {
                d.retain(|p| p["name"] != old["name"]);
            }
        }
        for new in e {
            let old = b.iter().find(|p| p["name"] == new["name"]);
            if old == Some(new) {
                continue;
            }
            if let Some(existing) = d.iter_mut().find(|p| p["name"] == new["name"]) {
                if new["type"] == 18
                    && old.is_some_and(|p| p["type"] == 18)
                    && existing["type"] == 18
                {
                    existing["value"] =
                        patch_rows(&old.unwrap()["value"], &new["value"], &existing["value"]);
                } else {
                    *existing = new.clone();
                }
            } else {
                d.push(new.clone());
            }
        }
        json!(d)
    }
    let b = base["maps"].as_array().ok_or("Invalid baseline")?;
    let e = edited["maps"].as_array().ok_or("Invalid edit")?;
    let mut d = destination["maps"]
        .as_array()
        .ok_or("Invalid destination")?
        .clone();
    for old in b {
        if !e.iter().any(|m| m["key"] == old["key"]) {
            d.retain(|m| m["key"] != old["key"]);
        }
    }
    for new in e {
        let old = b.iter().find(|m| m["key"] == new["key"]);
        if old == Some(new) {
            continue;
        }
        if let Some(existing) = d.iter_mut().find(|m| m["key"] == new["key"]) {
            existing["properties"] = patch_rows(
                old.map(|m| &m["properties"]).unwrap_or(&json!([])),
                &new["properties"],
                &existing["properties"],
            );
        } else {
            d.push(new.clone());
        }
    }
    let result = json!({"maps":d});
    validate(&result)?;
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn property(name: &str, value: &str) -> Value {
        json!({"name":name,"type":13,"value":value})
    }
    fn map(items: Vec<Value>) -> Value {
        json!({"maps":[{"key":0,"properties":items}]})
    }
    #[test]
    fn bulk_changes_preserve_other_values_and_rename_only_touched_keys() {
        let base = map(vec![property("author", "A"), property("title", "T")]);
        let edited = map(vec![property("author", "B"), property("title", "T")]);
        let dest = map(vec![
            property("author", "C"),
            property("title", "Other"),
            property("license", "CC0"),
        ]);
        let result = merge_changes(&base, &edited, &dest).unwrap();
        assert_eq!(
            result,
            map(vec![
                property("author", "B"),
                property("title", "Other"),
                property("license", "CC0")
            ])
        );
        let renamed = map(vec![property("creator", "A"), property("title", "T")]);
        assert_eq!(
            merge_changes(&base, &renamed, &dest).unwrap(),
            map(vec![
                property("title", "Other"),
                property("license", "CC0"),
                property("creator", "A")
            ])
        );
        assert_eq!(merge_changes(&base, &base, &dest).unwrap(), dest);
    }
    #[test]
    fn nested_changes_preserve_unrelated_destination_keys() {
        let wrap = |items: Vec<Value>| map(vec![json!({"name":"nested","type":18,"value":items})]);
        let b = wrap(vec![property("a", "old")]);
        let e = wrap(vec![property("a", "new")]);
        let d = wrap(vec![property("a", "local"), property("b", "keep")]);
        assert_eq!(
            merge_changes(&b, &e, &d).unwrap(),
            wrap(vec![property("a", "new"), property("b", "keep")])
        );
    }
    #[test]
    fn duplicates_and_invalid_wide_integers_are_rejected() {
        assert!(validate(&map(vec![property("a", "1"), property("a", "2")])).is_err());
        assert!(validate(&map(vec![
            json!({"name":"n","type":9,"value":"18446744073709551616"})
        ]))
        .is_err());
        assert!(validate(&map(vec![
            json!({"name":"n","type":9,"value":"18446744073709551615"})
        ]))
        .is_ok());
    }
    #[test]
    fn rendering_without_input_does_not_coerce_values() {
        let mut v = map((1..=19)
            .map(|ty| json!({"name":format!("p{ty}"),"type":ty,"value":default_value(ty)}))
            .collect());
        let before = v.clone();
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                edit(ui, &mut v);
            });
        });
        assert_eq!(v, before);
    }
}
