use shodo::limits::Limits;
use shodo::style::LineOptions;
use shodo::{AtomicSizes, LayoutContext};
use shodo_fixtures::{case, cases, load_fonts};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let selected = std::env::args().nth(1);
    let selected_cases: Vec<_> = if let Some(id) = selected {
        vec![case(&id).ok_or("unknown fixture case ID")?]
    } else {
        cases().iter().collect()
    };
    let limits = Limits::default();
    let fonts = load_fonts(&limits)?;
    let mut cx = LayoutContext::new();
    eprintln!(
        "Paragraph shaping is the S0 stub until S2; these line counts are not real-font browser comparisons."
    );
    for case in selected_cases {
        let paragraph = case.build(&mut cx, &fonts, &limits)?;
        let lines = paragraph.break_all(
            &mut cx,
            &LineOptions::default(),
            case.width,
            &AtomicSizes::new(),
        );
        println!(
            "{}",
            serde_json::json!({
                "id": case.id, "font_ids": case.font_ids, "lang": case.lang,
                "direction": format!("{:?}", case.direction), "font_size": case.font_size,
                "width": case.width, "processed_bytes": paragraph.text().len(),
                "lines": lines.len(), "paragraph_shaping": "S0 stub",
            })
        );
    }
    Ok(())
}
