//! Fixed original-WPT inline comparison; never a full-page conformance count.
#[allow(dead_code)] // Shared caller has separate fixture and WPT entry points.
#[path = "support/raikiri_contracts.rs"]
mod caller;
#[path = "support/offline_wpt.rs"]
mod offline_wpt;
#[path = "support/first_line_wpt.rs"]
mod probe;
#[path = "support/source_fonts.rs"]
mod source_fonts;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err("usage: first_line_wpt <wpt-root> <output-dir>".into());
    }
    probe::run(
        std::path::Path::new(&args[1]),
        std::path::Path::new(&args[2]),
    )?;
    Ok(())
}
