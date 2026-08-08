//! Probe every registered agent adapter and print what pre-flight would see:
//! `cargo run --example probe`

use tactus::agent;

fn main() {
    for adapter in agent::ADAPTERS {
        match adapter.probe() {
            Ok(caps) => println!(
                "{}: version {} | json_output={} session_resume={} cost_reporting={} \
                 read_only_mode={} acp={} model_list={}",
                adapter.id(),
                caps.version,
                caps.json_output,
                caps.session_resume,
                caps.cost_reporting,
                caps.read_only_mode,
                caps.acp,
                caps.model_list,
            ),
            Err(e) => println!("{}: probe failed — {e}", adapter.id()),
        }
    }
}
