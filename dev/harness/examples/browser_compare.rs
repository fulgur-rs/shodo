//! Print every raw-width difference; no Chromium needed for this command.
use shodo_fixtures::{browser, load_fonts};
fn main() -> Result<(), String> {
    let data = browser::capture()?;
    data.validate(browser::cases())?;
    let fonts = load_fonts(&Default::default()).map_err(|e| e.to_string())?;
    let result = browser::compare(
        &data,
        browser::cases(),
        &mut shodo::LayoutContext::new(),
        &fonts,
    )?;
    if std::env::args().any(|a| a == "--atomics") {
        let geometry = browser::atomic_comparisons(
            &data,
            browser::cases(),
            &mut shodo::LayoutContext::new(),
            &fonts,
        )?;
        println!(
            "{}",
            serde_json::to_string_pretty(&geometry).map_err(|e| e.to_string())?
        );
    } else if std::env::args().any(|a| a == "--transitions") {
        let transitions = browser::transitions(
            &data,
            browser::cases(),
            &mut shodo::LayoutContext::new(),
            &fonts,
        )?;
        println!(
            "{}",
            serde_json::to_string_pretty(&transitions).map_err(|e| e.to_string())?
        );
    } else if std::env::args().any(|a| a == "--json") {
        println!(
            "{}",
            serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?
        );
    } else {
        println!(
            "{} samples; {} raw differences",
            result.samples,
            result.mismatches.len()
        );
        for m in &result.mismatches {
            println!("{m}");
        }
    }
    if std::env::args().any(|a| a == "--check") {
        browser::check_all(
            &data,
            browser::cases(),
            &mut shodo::LayoutContext::new(),
            &fonts,
            &browser::difference_ledger()?,
        )?;
    }
    Ok(())
}
