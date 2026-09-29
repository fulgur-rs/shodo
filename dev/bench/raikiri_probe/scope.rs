//! Nonoverlapping measurement windows; serialization happens after the window.
use serde_json::{Value, json};
use std::error::Error;

pub fn measured<T>(
    operation: impl FnOnce() -> Result<T, Box<dyn Error>>,
) -> Result<(T, Value), Box<dyn Error>> {
    #[cfg(feature = "allocation-counting")]
    {
        let scope = crate::ALLOCATOR.begin()?;
        let value = operation()?;
        let counts = scope.finish();
        Ok((value, json!({"counts": counts})))
    }
    #[cfg(not(feature = "allocation-counting"))]
    {
        let start = std::time::Instant::now();
        let value = operation()?;
        let duration_ns = u64::try_from(start.elapsed().as_nanos())?;
        Ok((value, json!({"duration_ns": duration_ns})))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn observes_real_work_with_output_retained_after_scope() {
        let (output, record) = measured(|| Ok(std::hint::black_box(vec![7_u8; 8192]))).unwrap();
        assert_eq!(output.len(), 8192);
        if cfg!(feature = "allocation-counting") {
            assert!(record.get("duration_ns").is_none());
            assert!(record["counts"]["allocated_bytes"].as_u64().unwrap() >= 8192);
            assert!(record["counts"]["net_bytes"].as_i64().unwrap() >= 8192);
        } else {
            assert!(record.get("counts").is_none());
            assert!(record["duration_ns"].as_u64().unwrap() > 0);
        }
    }
}
