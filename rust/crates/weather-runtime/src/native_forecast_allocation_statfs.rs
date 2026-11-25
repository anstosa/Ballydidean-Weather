//! Native Linux filesystem-capacity samples for forecast allocation admission.

use std::ffi::CString;
use std::io;
use std::mem::MaybeUninit;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use weather_edge::{ForecastAllocationStatfsPort, ForecastAllocationStatfsSample};

/// Stateless owner of the Linux `statfs(2)` forecast-allocation sample.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeForecastAllocationStatfs;

impl NativeForecastAllocationStatfs {
    /// Construct one reusable native filesystem sampler.
    pub const fn new() -> Self {
        Self
    }
}

impl ForecastAllocationStatfsPort for NativeForecastAllocationStatfs {
    /// Project the same available blocks, block size, and free inodes as Node `statfs`.
    fn approved_sample(&mut self, root: &Path) -> io::Result<ForecastAllocationStatfsSample> {
        let root = CString::new(root.as_os_str().as_bytes()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "forecast allocation statfs root contains a nul byte",
            )
        })?;
        let mut filesystem = MaybeUninit::<libc::statfs>::uninit();

        // safety: the nul-terminated path remains live and libc writes the complete output
        let status = unsafe { libc::statfs(root.as_ptr(), filesystem.as_mut_ptr()) };

        // preserve the operating system errno for the exact rejected path
        if status != 0 {
            return Err(io::Error::last_os_error());
        }

        // safety: a successful statfs call initialized the complete output
        let filesystem = unsafe { filesystem.assume_init() };
        let block_size = u64::try_from(filesystem.f_bsize).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "forecast allocation statfs returned a negative block size",
            )
        })?;

        Ok(ForecastAllocationStatfsSample {
            available_blocks: filesystem.f_bavail,
            block_size,
            free_inodes: filesystem.f_ffree,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use weather_edge::ForecastAllocationStatfsPort;

    use super::NativeForecastAllocationStatfs;

    /// Match Node's bigint statfs projection on a stable Linux virtual filesystem.
    #[test]
    fn native_sample_matches_node_statfs_fields() {
        let root = Path::new("/dev");
        let actual = NativeForecastAllocationStatfs::new()
            .approved_sample(root)
            .unwrap();
        let node = Command::new("node")
            .args([
                "--input-type=module",
                "--eval",
                r#"import { statfsSync } from "node:fs";
const sample = statfsSync(process.argv[1], { bigint: true });
process.stdout.write(`${sample.bavail} ${sample.bsize} ${sample.ffree}`);"#,
                root.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(
            node.status.success(),
            "{}",
            String::from_utf8_lossy(&node.stderr)
        );
        let fields = String::from_utf8(node.stdout).unwrap();
        let expected = fields
            .split_ascii_whitespace()
            .map(|field| field.parse::<u64>().unwrap())
            .collect::<Vec<_>>();

        assert_eq!(
            [
                actual.available_blocks,
                actual.block_size,
                actual.free_inodes,
            ],
            expected.as_slice(),
        );
    }
}
