//! Fixed-font diagnostic for the development caller's preparation and projection.
#[allow(dead_code)]
#[path = "support/raikiri_contracts.rs"]
mod caller;
#[path = "support/link_windows.rs"]
mod measure;
#[allow(dead_code)]
#[path = "support/caller_snapshot.rs"]
mod snapshot;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    measure::run().map_err(Into::into)
}
