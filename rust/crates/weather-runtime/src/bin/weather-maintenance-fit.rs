//! Native fixed-input/output rain and temperature fitter process.

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
fn main() -> std::process::ExitCode {
    // emit only bounded process-stage diagnostics
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("weather-maintenance-fit: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// Run one complete native family using the unchanged sandbox file boundary.
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
fn run() -> Result<(), &'static str> {
    use std::fs::OpenOptions;
    use std::io::{Read, Write};
    use std::os::unix::fs::OpenOptionsExt;
    use weather_edge::ArchiveWirePrimitives;
    use weather_runtime::{
        NativeWirePrimitives, run_native_rain_fit_document, run_native_temperature_fit_document,
    };

    const MAX_INPUT: u64 = 512 * 1024 * 1024;
    const MAX_OUTPUT: usize = 8 * 1024 * 1024;
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    // admit only the two fixed family commands before opening any input
    let [mode, family] = arguments.as_slice() else {
        return Err("expected --fit-only rain|temperature");
    };
    if mode != "--fit-only" || !matches!(family.as_str(), "rain" | "temperature") {
        return Err("expected --fit-only rain|temperature");
    }
    let input = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(format!("/input/data/{family}.json"))
        .map_err(|_| "fit input unavailable")?;
    let metadata = input.metadata().map_err(|_| "fit input unavailable")?;
    // retain the source's exact regular-file and decoded input bound
    if !metadata.is_file() || metadata.len() > MAX_INPUT {
        return Err("fit input differs");
    }
    let mut bytes = Vec::new();
    input
        .take(MAX_INPUT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "fit input read failed")?;
    if bytes.len() as u64 > MAX_INPUT {
        return Err("fit input exceeds ceiling");
    }
    let document = NativeWirePrimitives
        .decode_json(&bytes)
        .ok_or("fit input JSON differs")?;
    drop(bytes);
    // invoke the actual native application owner without an interpreter or child bridge
    let output = match family.as_str() {
        "rain" => run_native_rain_fit_document(document).map_err(|_| "rain fit computation failed"),
        "temperature" => run_native_temperature_fit_document(document)
            .map_err(|_| "temperature fit computation failed"),
        _ => unreachable!(),
    }?;
    if output.len() > MAX_OUTPUT || !output.ends_with(b"\n") {
        return Err("fit output differs");
    }
    // publish only the one exclusive canonical output before echoing it to the controller
    let mut destination = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(format!("/output/{family}.json"))
        .map_err(|_| "fit output unavailable")?;
    destination
        .write_all(&output)
        .map_err(|_| "fit output write failed")?;
    destination
        .sync_all()
        .map_err(|_| "fit output sync failed")?;
    drop(destination);
    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(&output)
        .map_err(|_| "fit stdout write failed")?;
    stdout.flush().map_err(|_| "fit stdout flush failed")
}

/// Refuse unsupported native host architectures directly.
#[cfg(not(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
)))]
fn main() -> std::process::ExitCode {
    std::process::ExitCode::FAILURE
}
