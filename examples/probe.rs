//! Probe and discover every registered agent adapter — what pre-flight would
//! see, and what `upstroke connect` would write about it:
//! `cargo run --example probe`
//!
//! Zero spend: `probe()` reads `--version` and `--help`, and `discover()` asks
//! each vendor's CLI about its own account. Neither runs a model.

use upstroke::agent;

fn main() {
    for adapter in agent::ADAPTERS {
        let probed = adapter.probe();
        match &probed {
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
            Err(e) => {
                println!("{}: probe failed — {e}", adapter.id());
                // Discovery on a CLI that cannot report its own version would
                // be reading tea leaves, so it is not attempted.
                continue;
            }
        }
        let Ok(caps) = &probed else { continue };
        match adapter.discover(caps) {
            Ok(discovery) => {
                println!(
                    "  discovery: auth={} shape={} models={}",
                    discovery.auth,
                    discovery
                        .shape
                        .map_or_else(|| "unknown".to_owned(), |kind| kind.to_string()),
                    if discovery.models.is_empty() {
                        // The honest answer on both adapters today: neither CLI
                        // offers non-interactive enumeration (§13), so the
                        // roster comes from the shipped catalog.
                        "(none advertised; the catalog is the roster)".to_owned()
                    } else {
                        discovery.models.join(", ")
                    }
                );
                for note in &discovery.notes {
                    println!("    {note}");
                }
            }
            Err(e) => println!("  discovery failed — {e}"),
        }
    }
}
