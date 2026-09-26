use shodo::limits::Limits;
use shodo_fixtures::{FONTS, load_fonts};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let fonts = load_fonts(&Limits::default())?;
    for (fixture, id) in FONTS.iter().zip(fonts.ids) {
        let metrics = fonts.collection.metrics_with_coords(id, 16., &[]).unwrap();
        println!(
            "{}",
            serde_json::json!({
                "id": fixture.id, "family": fixture.family, "face_index": fixture.face_index,
                "bytes": fixture.bytes.len(), "sha256": fixture.sha256,
                "size": 16, "ascent": metrics.ascent, "descent": metrics.descent,
                "shaper_data": fonts.collection.shaper_data(id).is_some(),
            })
        );
    }
    Ok(())
}
