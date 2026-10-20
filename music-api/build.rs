use serde_json::{Value, json};
use std::{env, fs, path::Path};

const METHODS: &[&str] = &["get", "put", "post", "delete", "options", "head", "patch", "trace"];
const SPOTIFY_DROP: &[&str] = &["upload-custom-playlist-cover"];
const SUCCESS_STATUS: &[(&str, &str)] = &[("post_appreciations", "201"), ("post_artists", "201")];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    generate("spotify", SPOTIFY_DROP, None);
    generate("tidal", &[], Some("crate::tidal::vnd_api"));
}

/// Normalizes `specs/<name>.json` in memory and writes the progenitor client to `$OUT_DIR/<name>.rs`.
fn generate(name: &str, drop: &[&str], pre_hook: Option<&str>) {
    let path = format!("specs/{name}.json");
    println!("cargo:rerun-if-changed={path}");
    let mut spec: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    normalize(&mut spec, drop);
    let spec = serde_json::from_value(spec).unwrap();

    let mut settings = progenitor::GenerationSettings::new();
    settings.with_interface(progenitor::InterfaceStyle::Positional);
    if let Some(hook) = pre_hook {
        settings.with_pre_hook_async(syn::parse_str(hook).unwrap());
    }
    let tokens = progenitor::Generator::new(&settings).generate_tokens(&spec).unwrap();
    let code = prettyplease::unparse(&syn::parse2(tokens).unwrap());
    fs::write(Path::new(&env::var("OUT_DIR").unwrap()).join(format!("{name}.rs")), code).unwrap();
}

/// Rewrites a spec into the subset progenitor can generate.
fn normalize(spec: &mut Value, drop: &[&str]) {
    for (path, item) in spec["paths"].as_object_mut().unwrap() {
        item.as_object_mut().unwrap().retain(|method, op| {
            if !METHODS.contains(&method.as_str()) {
                return true;
            }
            let id = match op["operationId"].as_str() {
                Some(id) => id.to_string(),
                None => snake(&format!("{method}{path}")),
            };
            if op["deprecated"] == true || drop.contains(&id.as_str()) {
                return false;
            }
            let responses = op["responses"].as_object_mut().unwrap();
            let keep = match SUCCESS_STATUS.iter().find(|(op, _)| *op == id) {
                Some((_, status)) => Some(status.to_string()),
                None => responses.keys().filter(|k| k.starts_with('2')).min().cloned(),
            };
            responses.retain(|k, _| Some(k) == keep.as_ref());
            op["operationId"] = id.into();
            true
        });
    }
    if let Some(included) = spec.pointer_mut("/components/schemas/Included") {
        included["items"] = json!({});
    }
    rewrite(spec);
}

/// Renames `application/vnd.api+json` to `application/json` and turns `oneOf: [X, null]` into a nullable X.
fn rewrite(v: &mut Value) {
    match v {
        Value::Object(m) => {
            if let Some(content) = m.remove("application/vnd.api+json") {
                m.insert("application/json".into(), content);
            }
            if let Some(Value::Array(one_of)) = m.get("oneOf")
                && one_of.len() == 2
                && one_of[1]["enum"] == json!([null])
            {
                let inner = one_of[0].clone();
                m.remove("oneOf");
                m.insert("allOf".into(), json!([inner]));
                m.insert("nullable".into(), json!(true));
            }
            m.values_mut().for_each(rewrite);
        }
        Value::Array(a) => a.iter_mut().for_each(rewrite),
        _ => {}
    }
}

/// `post/playlists/{id}/relationships/items` -> `post_playlists_id_relationships_items`.
fn snake(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_uppercase() {
            out.push('_');
        }
        out.push(if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' });
    }
    out.split('_').filter(|w| !w.is_empty()).collect::<Vec<_>>().join("_")
}
