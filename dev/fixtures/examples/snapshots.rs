//! Fixed real-glyph regression snapshots and inspectable comparison reports.
#[path = "support/snapshot_cases.rs"]
mod snapshot_cases;
#[path = "support/snapshot_report.rs"]
mod snapshot_report;

fn main() -> std::process::ExitCode {
    let result = snapshot_report::parse_args(std::env::args_os().skip(1)).and_then(|options| {
        let report = snapshot_report::run(&options)?;
        println!(
            "Snapshot report: {}",
            options.output.join("index.html").display()
        );
        if report.passed() {
            Ok(())
        } else {
            Err("snapshot comparison failed; inspect the report".into())
        }
    });
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn committed_full_snapshot_matrix_matches_current_accepted_output() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let target = std::env::var_os("CARGO_TARGET_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target")
            });
        let options = snapshot_report::Options {
            expected: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("snapshots"),
            output: target
                .join("snapshot-checks")
                .join(format!("{}-{stamp}", std::process::id())),
            case: None,
            update: false,
        };
        let report = snapshot_report::run(&options).unwrap();
        assert!(
            report.passed(),
            "snapshot mismatch: inspect {}",
            options.output.join("index.html").display()
        );
    }
}
