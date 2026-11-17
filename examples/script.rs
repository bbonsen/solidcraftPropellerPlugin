//! Prints the commands an entry would run as a SolidCraft command script, for
//! `solidcraft-cli run` / `snapshot` without installing the plugin.
//!
//! cargo run --example script -- propeller '{"blades": 3}' > prop.json
use propellers::{build, Failure, Host, Kind};
use serde_json::{json, Value};

struct Script(Vec<Value>);
impl Host for Script {
    fn run(&mut self, command: &str, args: Value) -> Result<Value, Failure> {
        self.0.push(json!({"command": command, "params": args}));
        Ok(json!({}))
    }
    fn progress(&mut self, _: f32, _: &str) {}
    fn cancelled(&mut self) -> bool {
        false
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let entry = args.next().unwrap_or_else(|| "propeller".into());
    let params: Value = serde_json::from_str(&args.next().unwrap_or_else(|| "{}".into())).expect("params are JSON");
    let kind = Kind::from_entry(&entry).expect("an entry: propeller, marine, wind-turbine or ducted-fan");
    let mut s = Script(Vec::new());
    match build(&mut s, kind, &params) {
        Ok(reply) => {
            eprintln!("{}", serde_json::to_string_pretty(&reply).unwrap_or_default());
            println!("{}", json!({"commands": s.0}));
        }
        Err(e) => {
            eprintln!("{e:?}");
            std::process::exit(1);
        }
    }
}
