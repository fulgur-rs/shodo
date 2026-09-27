//! Fixed development workloads. Clock/counter instrumentation lives separately.
mod digest;
mod workload;
pub use digest::{Digest, digest};
pub use workload::{Operation, Run, Workload, layout, workloads};

#[derive(Debug)]
pub struct BenchError(pub String);
impl std::fmt::Display for BenchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for BenchError {}
