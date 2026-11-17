//! Runs the packaged propellers plugin inside SolidCraft's real Wasm runtime with a
//! recording host, so manifest parsing, sandbox limits and host calls are exercised.
use craft_plugin::{CommandSchema, Effect, FileGrant, PluginHost, Result};
use serde_json::{json, Value};
use std::path::Path;

#[derive(Default)]
struct Recorder { calls: Vec<(String, Value)>, committed: bool, rolled_back: bool }

impl PluginHost for Recorder {
    fn command_catalog(&self) -> Vec<CommandSchema> {
        ["sketch.create", "sketch.line", "sketch.spline.fit_point", "sketch.circle.center", "sketch.finish", "solid.extrude", "solid.loft", "solid.pattern.circular"]
            .iter()
            .map(|n| CommandSchema { name: n.to_string(), input_schema: json!({"type":"object"}), output_schema: json!({}), effect: Effect::Edit, group: None })
            .collect()
    }
    fn run_command(&mut self, name: &str, args: &Value) -> Result<Value> { self.calls.push((name.into(), args.clone())); Ok(json!({"ok": true})) }
    fn begin_transaction(&mut self) -> Result<()> { Ok(()) }
    fn commit_transaction(&mut self) -> Result<()> { self.committed = true; Ok(()) }
    fn rollback_transaction(&mut self) -> Result<()> { self.rolled_back = true; Ok(()) }
    fn granted_folders(&self) -> Vec<FileGrant> { vec![] }
    fn log(&mut self, _: &str) {}
    fn progress(&mut self, _: f32, _: &str) {}
}

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../package");
    let manifest_bytes = std::fs::read(dir.join("plugin.json")).expect("manifest");
    let manifest = craft_plugin::parse_manifest(&manifest_bytes, "propellers").expect("manifest parses");
    let wasm = std::fs::read(dir.join("propellers.wasm")).expect("component");
    let cases = [
        ("propeller", "defaults", json!({})),
        ("propeller", "3-blade 5x4.3 cw", json!({"blades": 3, "diameter": 127, "pitch": 109, "rotation": "cw", "rpm": 25000})),
        ("propeller", "bad: bore 14 in a 16 mm hub", json!({"bore": 14, "hub_diameter": 16})),
        ("marine", "defaults", json!({})),
        ("marine", "4-blade 13x19 left", json!({"blades": 4, "diameter": 330, "pitch": 483, "rotation": "left", "area_ratio": 0.7, "bore": 20})),
        ("wind-turbine", "defaults", json!({})),
        ("wind-turbine", "one blade", json!({"build": "blade"})),
        ("wind-turbine", "bad: tsr 30", json!({"tip_speed_ratio": 30})),
        ("ducted-fan", "defaults", json!({})),
        ("ducted-fan", "12 blades, no duct", json!({"blades": 12, "duct": false, "diameter": 120, "pitch": 90})),
        ("ducted-fan", "40 sections, 60 points", json!({"sections": 40, "section_points": 60})),
        ("wind-turbine", "bad: 40 sections", json!({"sections": 40})),
        ("toroidal", "defaults", json!({})),
        ("toroidal", "4 loops, cw, 45 sections", json!({"loops": 4, "rotation": "cw", "sections": 45})),
        ("toroidal-marine", "defaults", json!({})),
        ("toroidal-marine", "bad: spread 1.5", json!({"spread": 1.5})),
    ];
    for (entry, label, params) in cases {
        let (host, result) = craft_plugin_wasm::run(Recorder::default(), &manifest, &wasm, entry, &params);
        let mut counts: Vec<(String, usize)> = Vec::new();
        for (c, _) in &host.calls {
            match counts.iter_mut().find(|(n, _)| n == c) { Some(e) => e.1 += 1, None => counts.push((c.clone(), 1)) }
        }
        println!("== {entry} {label}: committed={} rolled_back={}", host.committed, host.rolled_back);
        println!("   calls ({}): {}", host.calls.len(), counts.iter().map(|(c, n)| format!("{c}×{n}")).collect::<Vec<_>>().join(", "));
        match result {
            Ok(v) => {
                let keep = ["loops", "spread_deg", "bodies", "diameter_mm", "hub_diameter_mm", "hub_length_mm", "pitch_at_75_mm", "blade_angle_at_75_deg", "blade_area_ratio", "design_rpm", "duct", "warnings"];
                let short: serde_json::Map<String, Value> = v.as_object().unwrap().iter().filter(|(k, _)| keep.contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect();
                println!("   result: {}", Value::Object(short));
            }
            Err(e) => println!("   error: {e:?}"),
        }
    }
    let (_, r) = craft_plugin_wasm::run(Recorder::default(), &manifest, &wasm, "nope", &json!({}));
    println!("== unknown entry: {:?}", r.err());
}
