//! `bm-rayfish.json` beside the exe: UI language for the desktop GUI.
//!
//! Native chrome (window title, tray) and the dashboard catalog both read this
//! file. Built-in strings live in `gui_i18n_default.json`. A user adds a
//! language by copying a block under `languages` (missing keys are filled from
//! English on the next launch).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::{Map, Value};

pub const JSON_NAME: &str = "bm-rayfish.json";
pub const BUILTIN_LANG_ORDER: &[&str] = &["zh_TW", "zh_CN", "ja_JP", "en_US"];
/// Keys every language table should have after merge (tray + window chrome).
/// Dashboard keys live in `gui_i18n_default.json` and are filled the same way.
#[allow(dead_code)]
pub const REF_KEYS: &[&str] = &[
    "language_name",
    "project_name",
    "settings",
    "about",
    "exit",
    "tray_restore",
    "download_update",
    "update_available",
];

pub enum GuiWake {
    LocaleChanged,
    UpdateChanged,
}

pub struct PendingUpdate {
    pub version: String,
    pub url: String,
    pub file_name: String,
}

pub struct GuiShared {
    pub config: GuiConfig,
    pub update: Option<PendingUpdate>,
    pub wake: Option<Arc<dyn Fn(GuiWake) + Send + Sync>>,
}

pub struct GuiConfig {
    path: PathBuf,
    value: Value,
}

impl GuiConfig {
    pub fn load() -> Self {
        Self::load_from(&json_path())
    }

    pub fn load_from(path: &Path) -> Self {
        match fs::read_to_string(path) {
            Ok(text) => match serde_json::from_str::<Value>(&text) {
                Ok(value) => match merge_user_file(value) {
                    Some(value) if validate(&value) => {
                        let cfg = Self {
                            path: path.to_path_buf(),
                            value,
                        };
                        cfg.save();
                        cfg
                    }
                    _ => write_default(path),
                },
                _ => write_default(path),
            },
            Err(_) => write_default(path),
        }
    }

    pub fn save(&self) {
        if let Some(text) = pretty_file(&self.value) {
            let _ = fs::write(&self.path, text);
        }
    }

    pub fn current_code(&self) -> &str {
        self.value
            .get("settings")
            .and_then(|s| s.get("languages"))
            .and_then(Value::as_str)
            .unwrap_or("zh_TW")
    }

    pub fn t(&self, key: &str) -> String {
        table_get(&self.value, self.current_code(), key)
            .or_else(|| table_get(&self.value, "zh_TW", key))
            .or_else(|| table_get(&self.value, "en_US", key))
            .unwrap_or_else(|| key.to_string())
    }

    pub fn window_title(&self) -> String {
        format!(
            "[B.M] {} V{} By. [B.M] 圓周率 3.14",
            self.t("project_name"),
            env!("CARGO_PKG_VERSION")
        )
    }

    pub fn js_locale(&self) -> String {
        match self.current_code() {
            "zh_TW" => "zh-TW".into(),
            "zh_CN" => "zh-CN".into(),
            "ja_JP" => "ja".into(),
            "en_US" => "en".into(),
            other => other.replace('_', "-"),
        }
    }

    /// Dashboard lookup table: English, then the current language on top.
    pub fn dashboard_table(&self) -> Value {
        let langs = match self.value.get("languages").and_then(Value::as_object) {
            Some(m) => m,
            None => return Value::Object(Map::new()),
        };
        let mut out = Map::new();
        for src in ["en_US", self.current_code()] {
            if let Some(obj) = langs.get(src).and_then(Value::as_object) {
                for (k, v) in obj {
                    if v.is_string() {
                        out.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        Value::Object(out)
    }

    pub fn inject_dashboard(&self, html: &str, token: &str) -> String {
        let prelude = format!(
            "const GUI_LOCALE = {};\nconst GUI_T = {};\n",
            json_for_script(&Value::String(self.js_locale())),
            json_for_script(&self.dashboard_table()),
        );
        html.replace("/*__I18N__*/", &(prelude + include_str!("gui-i18n.js")))
            .replace("__TOKEN__", token)
            .replace("__LANG_NAME__", &html_escape(&self.t("language_name")))
    }

    pub fn cycle(&mut self) {
        let codes = cycle_codes(&self.value);
        if codes.is_empty() {
            return;
        }
        let cur = self.current_code();
        let idx = codes.iter().position(|c| c == cur).unwrap_or(0);
        let next = codes[(idx + 1) % codes.len()].clone();
        self.value["settings"]["languages"] = Value::String(next);
        self.save();
    }
}

pub fn json_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(JSON_NAME)))
        .unwrap_or_else(|| PathBuf::from(JSON_NAME))
}

pub fn new_shared() -> Arc<Mutex<GuiShared>> {
    Arc::new(Mutex::new(GuiShared {
        config: GuiConfig::load(),
        update: None,
        wake: None,
    }))
}

pub fn default_value() -> Value {
    serde_json::json!({
        "settings": { "languages": "zh_TW" },
        "languages": default_languages(),
    })
}

fn default_languages() -> Value {
    serde_json::from_str(include_str!("gui_i18n_default.json")).expect("gui_i18n_default.json")
}

fn write_default(path: &Path) -> GuiConfig {
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let value = default_value();
    if let Some(text) = pretty_file(&value) {
        let _ = fs::write(path, text);
    }
    GuiConfig {
        path: path.to_path_buf(),
        value,
    }
}

fn table_get(root: &Value, lang: &str, key: &str) -> Option<String> {
    root.get("languages")?
        .get(lang)?
        .get(key)?
        .as_str()
        .map(str::to_string)
}

fn json_for_script(v: &Value) -> String {
    serde_json::to_string(v)
        .unwrap_or_else(|_| "null".into())
        .replace('<', "\\u003c")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

/// Keep user translations; fill missing builtin languages and missing keys from
/// the baked catalog (custom languages inherit English for gaps).
fn merge_user_file(mut file: Value) -> Option<Value> {
    let defaults = default_languages();
    let def_langs = defaults.as_object()?;
    let en = def_langs.get("en_US")?.clone();
    if file
        .get("settings")
        .and_then(|s| s.get("languages"))
        .is_none()
    {
        file["settings"]["languages"] = Value::String("zh_TW".into());
    }
    let file_langs = file.get_mut("languages")?.as_object_mut()?;
    for code in BUILTIN_LANG_ORDER {
        if !file_langs.contains_key(*code) {
            file_langs.insert((*code).to_string(), def_langs.get(*code)?.clone());
        }
    }
    let codes: Vec<String> = file_langs.keys().cloned().collect();
    for code in codes {
        let fallback = def_langs.get(&code).unwrap_or(&en);
        let fb = fallback.as_object()?;
        let table = file_langs.get_mut(&code).and_then(Value::as_object_mut)?;
        if table.values().any(|v| !v.is_string()) {
            return None;
        }
        for (k, v) in fb {
            if !table.contains_key(k) {
                table.insert(k.clone(), v.clone());
            }
        }
    }
    let current = file
        .get("settings")
        .and_then(|s| s.get("languages"))
        .and_then(Value::as_str)
        .unwrap_or("zh_TW")
        .to_string();
    let has_current = file
        .get("languages")
        .and_then(Value::as_object)
        .is_some_and(|m| m.contains_key(&current));
    if !has_current {
        file["settings"]["languages"] = Value::String("zh_TW".into());
    }
    Some(file)
}

/// Pretty-print with `languages` keys in TW → CN → JP → US (then any extras).
/// `serde_json` objects are BTreeMaps, so a normal pretty-print would sort
/// them `en_US`, `ja_JP`, `zh_CN`, `zh_TW`.
fn pretty_file(root: &Value) -> Option<String> {
    let obj = root.as_object()?;
    let settings = obj.get("settings")?;
    let languages = obj.get("languages")?.as_object()?;
    let mut lang_blocks = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for code in BUILTIN_LANG_ORDER {
        if let Some(table) = languages.get(*code) {
            seen.insert(*code);
            lang_blocks.push(format!("    \"{}\": {}", code, indent_json(table, 4)?));
        }
    }
    for (code, table) in languages {
        if seen.contains(code.as_str()) {
            continue;
        }
        lang_blocks.push(format!("    \"{}\": {}", code, indent_json(table, 4)?));
    }
    let mut extras = String::new();
    for (key, val) in obj {
        if key == "settings" || key == "languages" {
            continue;
        }
        extras.push_str(&format!(",\n  \"{}\": {}", key, indent_json(val, 2)?));
    }
    Some(format!(
        "{{\n  \"settings\": {},\n  \"languages\": {{\n{}\n  }}{}\n}}\n",
        indent_json(settings, 2)?,
        lang_blocks.join(",\n"),
        extras
    ))
}

fn indent_json(value: &Value, extra: usize) -> Option<String> {
    let raw = serde_json::to_string_pretty(value).ok()?;
    let pad = " ".repeat(extra);
    let mut lines = raw.lines();
    let first = lines.next()?.to_string();
    let rest: Vec<String> = lines.map(|l| format!("{pad}{l}")).collect();
    if rest.is_empty() {
        Some(first)
    } else {
        Some(format!("{first}\n{}", rest.join("\n")))
    }
}

fn languages_map(root: &Value) -> Option<&Map<String, Value>> {
    root.get("languages")?.as_object()
}

pub fn cycle_codes(root: &Value) -> Vec<String> {
    let Some(map) = languages_map(root) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for code in BUILTIN_LANG_ORDER {
        if map.contains_key(*code) {
            out.push((*code).to_string());
        }
    }
    for key in map.keys() {
        if !BUILTIN_LANG_ORDER.contains(&key.as_str()) {
            out.push(key.clone());
        }
    }
    out
}

pub fn validate(root: &Value) -> bool {
    let Some(obj) = root.as_object() else {
        return false;
    };
    let Some(settings) = obj.get("settings").and_then(Value::as_object) else {
        return false;
    };
    let Some(current) = settings.get("languages").and_then(Value::as_str) else {
        return false;
    };
    if current.is_empty() {
        return false;
    }
    let Some(languages) = obj.get("languages").and_then(Value::as_object) else {
        return false;
    };
    if languages.is_empty() {
        return false;
    }
    if !languages.contains_key(current) {
        return false;
    }
    for table in languages.values() {
        let Some(table) = table.as_object() else {
            return false;
        };
        if table.values().any(|v| !v.is_string()) {
            return false;
        }
        match table.get("language_name").and_then(Value::as_str) {
            Some(s) if !s.is_empty() => {}
            _ => return false,
        }
    }
    true
}

pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inject_dashboard_embeds_current_language() {
        let cfg = GuiConfig {
            path: PathBuf::from(JSON_NAME),
            value: default_value(),
        };
        let page = cfg.inject_dashboard(
            "<script>/*__I18N__*/</script> __LANG_NAME__ __TOKEN__",
            "tok",
        );
        assert!(page.contains("const GUI_T"));
        assert!(page.contains("繁體中文"));
        assert!(page.contains("tok"));
        assert!(!page.contains("__TOKEN__"));
    }

    #[test]
    fn languages_file_order_is_tw_cn_jp_us() {
        let text = pretty_file(&default_value()).expect("pretty");
        let block = text.split("\"languages\": {").nth(1).expect("languages");
        let tw = block.find("\"zh_TW\"").expect("zh_TW");
        let cn = block.find("\"zh_CN\"").expect("zh_CN");
        let jp = block.find("\"ja_JP\"").expect("ja_JP");
        let us = block.find("\"en_US\"").expect("en_US");
        assert!(tw < cn && cn < jp && jp < us, "{block}");
        assert!(validate(&serde_json::from_str(&text).unwrap()));
    }

    #[test]
    fn extra_inner_key_is_kept() {
        let mut v = default_value();
        v["languages"]["zh_TW"]["nope"] = Value::String("x".into());
        assert!(validate(&v));
    }

    #[test]
    fn missing_inner_key_is_filled_from_english() {
        let mut v = default_value();
        v["languages"]["en_US"]
            .as_object_mut()
            .unwrap()
            .remove("about");
        assert!(validate(&v));
        let merged = merge_user_file(v).unwrap();
        assert_eq!(merged["languages"]["en_US"]["about"], "About");
    }

    #[test]
    fn custom_language_inherits_english_gaps() {
        let mut v = default_value();
        v["languages"]["de_DE"] = serde_json::json!({ "language_name": "Deutsch" });
        v["settings"]["languages"] = Value::String("de_DE".into());
        assert!(validate(&v));
        let merged = merge_user_file(v).unwrap();
        assert_eq!(merged["languages"]["de_DE"]["language_name"], "Deutsch");
        assert_eq!(merged["languages"]["de_DE"]["btn_invite"], "Invite");
        let codes = cycle_codes(&merged);
        assert!(codes.contains(&"de_DE".to_string()));
    }

    #[test]
    fn default_catalog_has_chrome_keys() {
        let def = default_value();
        let en = def["languages"]["en_US"].as_object().unwrap();
        for key in REF_KEYS {
            assert!(en.contains_key(*key), "missing {key}");
        }
    }

    #[test]
    fn unknown_current_language_fails() {
        let mut v = default_value();
        v["settings"]["languages"] = Value::String("de_DE".into());
        assert!(!validate(&v));
    }

    #[test]
    fn custom_lang_with_same_keys_validates_and_cycles_last() {
        let mut v = default_value();
        v["languages"]["de_DE"] = v["languages"]["en_US"].clone();
        v["languages"]["de_DE"]["language_name"] = Value::String("Deutsch".into());
        assert!(validate(&v));
        let codes = cycle_codes(&v);
        assert_eq!(codes, ["zh_TW", "zh_CN", "ja_JP", "en_US", "de_DE"]);
    }

    #[test]
    fn corrupt_file_is_overwritten() {
        let dir = std::env::temp_dir().join(format!(
            "bm-rayfish-json-{}-{}",
            std::process::id(),
            rand::random::<u32>()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(JSON_NAME);
        fs::write(&path, "{not json").unwrap();
        let cfg = GuiConfig::load_from(&path);
        assert_eq!(cfg.current_code(), "zh_TW");
        let on_disk: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(validate(&on_disk));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn js_locale_maps_builtin_codes() {
        let cfg = GuiConfig {
            path: PathBuf::from(JSON_NAME),
            value: default_value(),
        };
        assert_eq!(cfg.js_locale(), "zh-TW");
    }
}
