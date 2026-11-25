use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

use weather_contracts::retained_forecast_observation::station_catalog;

use crate::api_maintenance_candidate_files::open_read_no_follow;
use crate::{
    ApiMaintenanceShadowEvidenceValues, WindFitInputCalendar, WindFitInputError,
    run_wind_maintenance_document,
};

const INPUT_PATH: &str = "/input/data/wind.json";
const OUTPUT_PATH: &str = "/output/wind.json";
const INPUT_MAXIMUM_BYTES: usize = 512 * 1_024 * 1_024;
const OUTPUT_MAXIMUM_BYTES: usize = 8 * 1_024 * 1_024;

/// Stable file identity observed before and after opening the fixed input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindMaintenanceFileMetadata {
    pub device: u64,
    pub inode: u64,
    pub regular: bool,
    pub size: u64,
    pub symlink: bool,
}

/// Fixed-path filesystem and stdout effects owned by the command boundary.
pub trait WindMaintenanceCliIo {
    type Error;
    type Reader;
    type Writer;

    /// Inspect the fixed input path without following a symbolic link.
    fn lstat(&mut self, path: &'static str) -> Result<WindMaintenanceFileMetadata, Self::Error>;
    /// Open the inspected input with the platform no-follow primitive.
    fn open_read_no_follow(&mut self, path: &'static str) -> Result<Self::Reader, Self::Error>;
    /// Inspect the already-open descriptor before reading bytes.
    fn reader_metadata(
        &mut self,
        reader: &mut Self::Reader,
    ) -> Result<WindMaintenanceFileMetadata, Self::Error>;
    /// Read at most the supplied byte ceiling plus one overflow witness.
    fn read_to_end(
        &mut self,
        reader: &mut Self::Reader,
        maximum_bytes: usize,
    ) -> Result<Vec<u8>, Self::Error>;
    /// Close the input descriptor after either successful or failed reading.
    fn close_reader(&mut self, reader: Self::Reader) -> Result<(), Self::Error>;
    /// Create the fixed output exclusively with the supplied Unix mode.
    fn create_new(&mut self, path: &'static str, mode: u32) -> Result<Self::Writer, Self::Error>;
    /// Write the complete canonical output to the retained descriptor.
    fn write_all(&mut self, writer: &mut Self::Writer, bytes: &[u8]) -> Result<(), Self::Error>;
    /// Make the completed output durable before closing it.
    fn sync_all(&mut self, writer: &mut Self::Writer) -> Result<(), Self::Error>;
    /// Close the output before any bytes are echoed to stdout.
    fn close_writer(&mut self, writer: Self::Writer) -> Result<(), Self::Error>;
    /// Echo the already-durable canonical result to process stdout.
    fn write_stdout(&mut self, bytes: &[u8]) -> Result<(), Self::Error>;
}

/// Ordinary command failure with filesystem, decode, and fit phases kept distinct.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WindMaintenanceCliError {
    Arguments,
    InputFile,
    InputTooLarge,
    Decode,
    Fit(WindFitInputError),
    OutputFile,
    OutputTooLarge,
    Stdout,
}

/// Native Unix implementation for the credential-isolated fixed-path command.
#[derive(Debug, Default, Clone, Copy)]
pub struct NativeWindMaintenanceCliIo;

impl WindMaintenanceCliIo for NativeWindMaintenanceCliIo {
    type Error = io::Error;
    type Reader = File;
    type Writer = File;

    /// Inspect one fixed path without following a final symbolic link.
    fn lstat(&mut self, path: &'static str) -> Result<WindMaintenanceFileMetadata, Self::Error> {
        fs::symlink_metadata(path).map(|metadata| native_metadata(&metadata))
    }

    /// Reuse the repository's reviewed native no-follow reader.
    fn open_read_no_follow(&mut self, path: &'static str) -> Result<Self::Reader, Self::Error> {
        open_read_no_follow(Path::new(path))
    }

    /// Inspect the opened input descriptor rather than resolving its path again.
    fn reader_metadata(
        &mut self,
        reader: &mut Self::Reader,
    ) -> Result<WindMaintenanceFileMetadata, Self::Error> {
        reader.metadata().map(|metadata| native_metadata(&metadata))
    }

    /// Bound memory growth even if the opened file changes after metadata admission.
    fn read_to_end(
        &mut self,
        reader: &mut Self::Reader,
        maximum_bytes: usize,
    ) -> Result<Vec<u8>, Self::Error> {
        let limit = u64::try_from(maximum_bytes)
            .unwrap_or(u64::MAX)
            .saturating_add(1);
        let mut bytes = Vec::new();
        reader.take(limit).read_to_end(&mut bytes)?;
        Ok(bytes)
    }

    /// Close the input descriptor by consuming its native owner.
    fn close_reader(&mut self, reader: Self::Reader) -> Result<(), Self::Error> {
        drop(reader);
        Ok(())
    }

    /// Create one output without replacing any prior path entry.
    fn create_new(&mut self, path: &'static str, mode: u32) -> Result<Self::Writer, Self::Error> {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .open(path)
    }

    /// Write all output bytes to the exclusive descriptor.
    fn write_all(&mut self, writer: &mut Self::Writer, bytes: &[u8]) -> Result<(), Self::Error> {
        writer.write_all(bytes)
    }

    /// Flush the output file through the native durability boundary.
    fn sync_all(&mut self, writer: &mut Self::Writer) -> Result<(), Self::Error> {
        writer.sync_all()
    }

    /// Close the output descriptor by consuming its native owner.
    fn close_writer(&mut self, writer: Self::Writer) -> Result<(), Self::Error> {
        drop(writer);
        Ok(())
    }

    /// Write the canonical result to the inherited stdout stream.
    fn write_stdout(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        io::stdout().lock().write_all(bytes)
    }
}

/// Execute the original fixed-path fit-only command over injected custody effects.
pub fn run_wind_maintenance_cli(
    argv: &[String],
    values: &impl ApiMaintenanceShadowEvidenceValues,
    calendar: &impl WindFitInputCalendar,
    io: &mut impl WindMaintenanceCliIo,
) -> Result<u8, WindMaintenanceCliError> {
    // reject every path, confirmation, or qualification override
    if argv.len() != 1 || argv[0] != "--fit-only" {
        return Err(WindMaintenanceCliError::Arguments);
    }

    let before = io
        .lstat(INPUT_PATH)
        .map_err(|_| WindMaintenanceCliError::InputFile)?;

    // reject a link, non-regular input, or declared overflow before opening
    if !before.regular || before.symlink || before.size > INPUT_MAXIMUM_BYTES as u64 {
        return Err(if before.size > INPUT_MAXIMUM_BYTES as u64 {
            WindMaintenanceCliError::InputTooLarge
        } else {
            WindMaintenanceCliError::InputFile
        });
    }

    let mut reader = io
        .open_read_no_follow(INPUT_PATH)
        .map_err(|_| WindMaintenanceCliError::InputFile)?;
    let read_result = read_input(io, &mut reader, before);
    let close_result = io.close_reader(reader);
    let bytes = read_result?;
    close_result.map_err(|_| WindMaintenanceCliError::InputFile)?;

    // retain the byte ceiling if the file grew after descriptor admission
    if bytes.len() > INPUT_MAXIMUM_BYTES {
        return Err(WindMaintenanceCliError::InputTooLarge);
    }

    let document = values
        .decode_shadow_json(&bytes)
        .ok_or(WindMaintenanceCliError::Decode)?;
    // release the bounded raw input before the decoded fit allocates fold working sets
    drop(bytes);
    // use the original frozen physical station table rather than a caller-provided roster
    let prepared = run_wind_maintenance_document(values, calendar, station_catalog(), document)
        .map_err(WindMaintenanceCliError::Fit)?;

    // reject an oversized private result before creating the output path
    if prepared.bytes.len() > OUTPUT_MAXIMUM_BYTES {
        return Err(WindMaintenanceCliError::OutputTooLarge);
    }

    let mut writer = io
        .create_new(OUTPUT_PATH, 0o600)
        .map_err(|_| WindMaintenanceCliError::OutputFile)?;
    let write_result = write_output(io, &mut writer, &prepared.bytes);
    let close_result = io.close_writer(writer);
    write_result?;
    close_result.map_err(|_| WindMaintenanceCliError::OutputFile)?;
    io.write_stdout(&prepared.bytes)
        .map_err(|_| WindMaintenanceCliError::Stdout)?;
    Ok(prepared.fit_exit_code)
}

/// Map any completed fit attempt, including insufficiency, to process success.
pub fn wind_maintenance_process_exit_code(result: &Result<u8, WindMaintenanceCliError>) -> u8 {
    // preserve the source entrypoint's completed-attempt success mapping
    if result.is_ok() { 0 } else { 1 }
}

/// Verify descriptor identity and read the bounded input bytes.
fn read_input<Io: WindMaintenanceCliIo>(
    io: &mut Io,
    reader: &mut Io::Reader,
    before: WindMaintenanceFileMetadata,
) -> Result<Vec<u8>, WindMaintenanceCliError> {
    let opened = io
        .reader_metadata(reader)
        .map_err(|_| WindMaintenanceCliError::InputFile)?;

    // require the same regular descriptor identity observed before open
    if !opened.regular
        || opened.symlink
        || opened.device != before.device
        || opened.inode != before.inode
        || opened.size != before.size
    {
        return Err(WindMaintenanceCliError::InputFile);
    }

    io.read_to_end(reader, INPUT_MAXIMUM_BYTES)
        .map_err(|_| WindMaintenanceCliError::InputFile)
}

/// Write and synchronize the full canonical output before descriptor close.
fn write_output<Io: WindMaintenanceCliIo>(
    io: &mut Io,
    writer: &mut Io::Writer,
    bytes: &[u8],
) -> Result<(), WindMaintenanceCliError> {
    io.write_all(writer, bytes)
        .map_err(|_| WindMaintenanceCliError::OutputFile)?;
    io.sync_all(writer)
        .map_err(|_| WindMaintenanceCliError::OutputFile)
}

/// Project native Unix descriptor metadata into the injected command contract.
fn native_metadata(metadata: &fs::Metadata) -> WindMaintenanceFileMetadata {
    WindMaintenanceFileMetadata {
        device: metadata.dev(),
        inode: metadata.ino(),
        regular: metadata.is_file(),
        size: metadata.len(),
        symlink: metadata.file_type().is_symlink(),
    }
}
