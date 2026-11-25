//! Native entrypoint for the retained evidence operator command families.

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod linux {
    use std::{
        env, fs,
        io::{self, Read, Write},
        path::{Path, PathBuf},
        process::ExitCode,
    };
    use weather_edge::{
        ArchiveWireCodec, NativeRevisionGapCommandPorts, NativeRevisionSpool,
        RevisionArchiveCanonicalClock, parse_revision_cold_page_command,
        parse_revision_gap_operator_command,
    };
    use weather_runtime::{
        CustodyAnchorControl, NativeCaptureEpochOperatorFiles, NativeColdCustodyPorts,
        NativeColdCustodyRoot, NativeCustodyAnchorCurrentFiles, NativeCustodyAnchorInstallFiles,
        NativeFutureInputSealInstallStore, NativeFutureInputSealOperatorCaptureEpoch,
        NativeFutureInputSealRevisionCustody, NativeMaintenanceAnchorStatusFiles,
        NativeProcessClock, NativeRevisionColdStartDirectories, NativeRevisionGapCodec,
        NativeShadowMetadataCustodyFiles, NativeUnsupportedTerminalProofStore,
        NativeWirePrimitives, install_custody_anchor_command, install_future_input_seal_command,
        mutate_shadow_metadata_custody_command, read_capture_epoch_operator_command,
        read_custody_anchor_current_command, read_maintenance_anchor_status_command,
        read_revision_cold_start_command, read_shadow_metadata_custody_command,
        read_unsupported_terminal_proof_current_command, run_native_cold_custody_command,
        shadow_metadata_custody_owner_command,
    };

    use weather_runtime::{
        CaptureEpochAuthorityFiles, CaptureEpochInitFilePort, MaintenanceCanonicalJson,
        NativeCaptureEpochInitializationAuthority, NativeFamilyCatalogImageVerifier,
        NativeFamilyLineageSourceClosure, NativeFamilyReleaseCurrentHostFiles,
        NativeFamilyReleaseLiveRuntimeProcess, NativeFamilyReleaseStatusHostFiles,
        NativeFutureInputSealStore, NativeMaintenanceAnchorCurrentV3Control,
        NativeMaintenanceAnchorInstallV2Control, NativeMaintenanceAnchorInstallV3Control,
        NativeMaintenanceAnchorInstallV3SealStore, NativeMaintenanceAnchorV3InstallStore,
        NativeMaintenanceLegacyMetadataV2, NativeOutboundHttp,
        NativeUnsupportedTerminalProofControl, NativeUnsupportedTerminalProofInstallStore,
        PreparedNativeFamilyLineageLiveObservation, PreparedNativeFamilyReleaseCurrentAuthority,
        finalize_maintenance_anchor_v2_command, finalize_maintenance_anchor_v3_command,
        initialize_capture_epoch_operator_command, install_maintenance_anchor_v2_command,
        install_maintenance_anchor_v3_command, install_unsupported_terminal_proof_command,
        read_future_input_seal_current_command, read_future_input_seal_current_verified,
        read_maintenance_anchor_current_v3_command, read_native_family_release_current,
        read_native_family_release_status_command, read_native_future_input_seal_process_control,
    };

    const DEFAULT_ROOT: &str = "/var/lib/weather/xweather/adjustment-evidence";
    const ROOT_ENV: &str = "WEATHER_ADJUSTMENT_EVIDENCE_ROOT";

    /// Obtain the kernel-reported effective service identity without consulting the evidence root.
    fn effective_identity() -> Option<(u32, u32)> {
        let status = fs::read_to_string("/proc/self/status").ok()?;
        let effective = |field: &str| {
            status.lines().find_map(|line| {
                line.strip_prefix(field)?
                    .split_ascii_whitespace()
                    .nth(1)?
                    .parse::<u32>()
                    .ok()
            })
        };
        Some((effective("Uid:")?, effective("Gid:")?))
    }

    /// Bind the real configured root and native wire owners to one retained archive command.
    fn run() -> Result<(), String> {
        let arguments = env::args_os()
            .skip(1)
            .map(|argument| {
                argument
                    .into_string()
                    .map_err(|_| "invalid command argument".to_owned())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let (action, operands) = arguments
            .split_first()
            .ok_or_else(|| "missing evidence command".to_owned())?;
        let root = env::var_os(ROOT_ENV).map_or_else(|| PathBuf::from(DEFAULT_ROOT), PathBuf::from);
        let (uid, gid) =
            effective_identity().ok_or_else(|| "service identity unavailable".to_owned())?;
        let operands = operands.iter().map(String::as_str).collect::<Vec<_>>();
        let mut output = io::stdout().lock();
        // dispatch only landed verbs and keep all storage selection within their owners
        match action.as_str() {
            #[cfg(feature = "node-zlib-candidate")]
            "stream-forecast-training-export" => {
                // retain the original read-only transaction input and fixed admitted scratch owner
                let [from, to, scratch] = operands.as_slice() else {
                    return Err("forecast training export operands differ".to_owned());
                };
                let receipt = weather_runtime::stream_native_forecast_training_export(
                    io::stdin().lock(),
                    from,
                    to,
                    Path::new(scratch),
                    &mut output,
                )
                .map_err(|error| error.to_string())?;
                eprint!("{}", receipt.completion_report(from, to));
                Ok(())
            }
            "validate-scorecard-install" => {
                // require the original v1/v2 mode, exact hash and current clock
                let [version, expected] = operands.as_slice() else {
                    return Err("scorecard installation operands differ".to_owned());
                };
                let now = weather_runtime::system_worker_health_milliseconds()
                    .ok_or_else(|| "scorecard clock unavailable".to_owned())?;
                weather_runtime::project_native_scorecard_install_document(
                    io::stdin().lock(),
                    &mut output,
                    version,
                    expected,
                    now,
                )
            }
            "read-scorecard-install-pointer" => {
                // restrict previous pointer reads to the fixed v2 production evidence root
                let [name] = operands.as_slice() else {
                    return Err("scorecard pointer operands differ".to_owned());
                };
                let hash = weather_runtime::read_native_scorecard_install_pointer(
                    Path::new(DEFAULT_ROOT),
                    name,
                )?;
                writeln!(output, "{hash}").map_err(|error| error.to_string())
            }
            "registration-lifecycle-status-sql-v4" => {
                // retain the original fixed owner query with no caller arguments
                if !operands.is_empty() {
                    return Err("registration lifecycle SQL takes no arguments".to_owned());
                }
                output
                    .write_all(weather_runtime::build_native_registration_lifecycle_status_sql_v4())
                    .map_err(|error| error.to_string())
            }
            "project-registration-lifecycle-status-v4" => {
                // bind the actual owner response to the retained operator epoch
                if !operands.is_empty() {
                    return Err("registration lifecycle status takes no arguments".to_owned());
                }
                let bytes =
                    weather_runtime::project_native_registration_lifecycle_status_v4_from_operator(
                        io::stdin().lock(),
                        &mut NativeCaptureEpochOperatorFiles,
                    )
                    .map_err(|error| error.to_string())?;
                output.write_all(&bytes).map_err(|error| error.to_string())
            }
            "owner-operation-sql-v3" => {
                // refuse every owner kind not yet bound to independent installed authority
                if operands.len() != 2 {
                    return Err("native owner operation is unavailable".to_owned());
                }
                let mut request = Vec::new();
                io::stdin()
                    .lock()
                    .take(128 * 1024 + 1)
                    .read_to_end(&mut request)
                    .map_err(|error| error.to_string())?;
                // bind each fixed request kind to its independently installed owner
                let sql = match operands[0] {
                    "access" => weather_runtime::build_native_host_owner_access_sql_from_installed(
                        &request, operands[1],
                    ).map(|result| result.sql).map_err(|error| error.to_string())?,
                    "terminal" => weather_runtime::build_native_host_owner_qualified_terminal_sql_from_installed(
                        &request, operands[1],
                    ).map(|result| result.sql).map_err(|error| error.to_string())?,
                    "retire" => weather_runtime::build_native_host_owner_qualified_retirement_sql_from_installed(
                        &request, operands[1],
                    ).map(|result| result.sql).map_err(|error| error.to_string())?,
                    "unsupported-terminal" => weather_runtime::build_native_host_owner_unsupported_terminal_sql_from_installed(
                        &request, operands[1],
                    ).map(|result| result.sql).map_err(|error| error.to_string())?,
                    "unsupported-retire" => weather_runtime::build_native_host_owner_unsupported_retirement_sql_from_installed(
                        &request, operands[1],
                    ).map(|result| result.sql).map_err(|error| error.to_string())?,
                    _ => return Err("native owner operation is unavailable".to_owned()),
                };
                output
                    .write_all(sql.as_bytes())
                    .map_err(|error| error.to_string())
            }
            "owner-operation-result-v3" => {
                // retain the exact canonical result projection for all supported owner response kinds
                let kind = match operands.as_slice() {
                    ["access"] => weather_runtime::NativeHostOwnerOperationKind::Access,
                    ["terminal"] => weather_runtime::NativeHostOwnerOperationKind::Terminal,
                    ["retire"] => weather_runtime::NativeHostOwnerOperationKind::Retire,
                    _ => return Err("native owner result kind differs".to_owned()),
                };
                let mut response = Vec::new();
                io::stdin()
                    .lock()
                    .take(128 * 1024 + 1)
                    .read_to_end(&mut response)
                    .map_err(|error| error.to_string())?;
                let projected =
                    weather_runtime::project_native_host_owner_operation_result(kind, &response)
                        .map_err(|error| error.to_string())?;
                output
                    .write_all(&projected.canonical_bytes)
                    .map_err(|error| error.to_string())
            }
            "frame-v2-availability" => {
                // frame the exact requested value-free function result
                if operands.len() != 1 {
                    return Err("availability requires one registration".to_owned());
                }
                let bytes = weather_runtime::frame_native_adjustment_confirmation_availability_v2(
                    io::stdin().lock(),
                    operands[0],
                )
                .map_err(|error| error.to_string())?;
                output.write_all(&bytes).map_err(|error| error.to_string())
            }
            "frame-v2-authorization" => {
                // bind the actual persisted-burn row to its closed forced operands
                if operands.len() != 3 {
                    return Err("authorization requires registration, access and chunk".to_owned());
                }
                let chunk = operands[2]
                    .parse::<u32>()
                    .ok()
                    .filter(|value| value.to_string() == operands[2])
                    .ok_or_else(|| "authorization chunk differs".to_owned())?;
                let bytes = weather_runtime::frame_native_adjustment_confirmation_authorization_v2(
                    io::stdin().lock(),
                    operands[0],
                    operands[1],
                    chunk,
                )
                .map_err(|error| error.to_string())?;
                output.write_all(&bytes).map_err(|error| error.to_string())
            }
            "verify-v2-readiness" => {
                // admit only the complete metadata-only database envelope
                if !operands.is_empty() {
                    return Err("readiness takes no arguments".to_owned());
                }
                weather_runtime::verify_native_adjustment_evaluation_v2_readiness(
                    io::stdin().lock(),
                )
                .map_err(|error| error.to_string())
            }
            "v2-authorization-dates" => {
                // read the bound local private authorization document
                if operands.len() != 1 {
                    return Err("authorization dates require one document".to_owned());
                }
                let bytes = read_export_control(operands[0], 768 * 1024)?;
                let (from, to) =
                    weather_runtime::read_native_adjustment_confirmation_authorization_dates(
                        &bytes,
                    )
                    .map_err(|error| error.to_string())?;
                writeln!(output, "{from} {to}").map_err(|error| error.to_string())
            }
            #[cfg(feature = "node-zlib-candidate")]
            "freeze-adjustment-evidence-snapshot" => {
                // freeze the actual custody catalog before the database transaction
                if !operands.is_empty() {
                    return Err("evidence snapshot takes no arguments".to_owned());
                }
                let bytes = weather_runtime::freeze_native_evaluation_edge_snapshot(
                    &root,
                    &mut NativeProcessClock,
                )
                .map_err(|error| error.to_string())?;
                output.write_all(&bytes).map_err(|error| error.to_string())
            }
            "stream-v2-envelope" => {
                // preserve the exact daily or persisted-burn envelope framing around the v1 archive
                let (kind, authorization) = match operands.as_slice() {
                    ["daily_monitoring", _, _, _, _] => (weather_runtime::NativeAdjustmentEvaluationV2ExportKind::DailyMonitoring, None),
                    ["post_burn_confirmation", _, _, _, _, authorization] => (weather_runtime::NativeAdjustmentEvaluationV2ExportKind::PostBurnConfirmation, Some(Path::new(authorization))),
                    _ => return Err("v2 export envelope operands differ".to_owned()),
                };
                weather_runtime::stream_native_adjustment_evaluation_v2_envelope(
                    weather_runtime::NativeAdjustmentEvaluationV2EnvelopeInput {
                        authorization_path: authorization,
                        edge_snapshot_path: Path::new(operands[4]),
                        export_kind: kind,
                        from_local_date: operands[2],
                        source_archive_path: Path::new(operands[1]),
                        to_local_date: operands[3],
                    },
                    &mut output,
                )
                .map_err(|error| error.to_string())?;
                Ok(())
            }
            #[cfg(feature = "node-zlib-candidate")]
            "stream-archive" => {
                // stream the complete native v1 archive using the already frozen local receipt list
                if operands.len() != 4
                    || Path::new(operands[3]) != Path::new(operands[0]).join("edge-snapshot.json")
                {
                    return Err("evaluation archive operands differ".to_owned());
                }
                let snapshot = read_export_control(operands[3], 16 * 1024 * 1024)?;
                let digest = weather_runtime::stream_native_evaluation_archive_v1(
                    io::BufReader::new(io::stdin().lock()),
                    &snapshot,
                    &root,
                    operands[1],
                    operands[2],
                    &mut output,
                )
                .map_err(|error| error.to_string())?;
                eprintln!("manifest={digest}");
                Ok(())
            }
            "project-registration-schedule-status-v3" => {
                // preserve the fixed retained epoch and metadata-only SQL response
                if !operands.is_empty() {
                    return Err("registration schedule status takes no arguments".to_owned());
                }
                let bytes =
                    weather_runtime::project_native_registration_schedule_status_v3_from_operator(
                        io::stdin().lock(),
                        &mut NativeCaptureEpochOperatorFiles,
                    )
                    .map_err(|error| error.to_string())?;
                output.write_all(&bytes).map_err(|error| error.to_string())
            }
            "registration-schedule-initialize-sql-v3" => {
                // admit the root's fixed epoch and exact canonical request before emitting SQL
                if operands.len() != 1 {
                    return Err("registration schedule initializer requires one hash".to_owned());
                }
                let bytes = weather_runtime::build_native_registration_schedule_initialize_sql(
                    io::stdin().lock(),
                    operands[0],
                    &mut NativeCaptureEpochOperatorFiles,
                )
                .map_err(|error| error.to_string())?;
                output.write_all(&bytes).map_err(|error| error.to_string())
            }
            "verify-registration-schedule-initialization-v3" => {
                // expose only the acknowledgement bound to the submitted bootstrap
                if operands.len() != 1 {
                    return Err(
                        "registration schedule acknowledgement requires one hash".to_owned()
                    );
                }
                let bytes =
                    weather_runtime::project_native_registration_schedule_initialize_result(
                        io::stdin().lock(),
                        operands[0],
                    )
                    .map_err(|error| error.to_string())?;
                output.write_all(&bytes).map_err(|error| error.to_string())
            }
            "project-maintenance-ledger-v3" => {
                // preserve the root SQL metadata projector's exact zero-operand input and output
                if !operands.is_empty() {
                    return Err("project-maintenance-ledger-v3 takes no arguments".to_owned());
                }
                let bytes = weather_runtime::project_native_maintenance_database_ledger_v3(
                    io::stdin().lock(),
                )
                .map_err(|error| error.to_string())?;
                output.write_all(&bytes).map_err(|error| error.to_string())
            }
            #[cfg(feature = "node-zlib-candidate")]
            "archive-next" | "archive-ack" | "archive-ack-final" => {
                // bind exact archive verbs only in the opt-in native gzip candidate
                let codec = weather_runtime::NativeForecastArchiveCodec::new(
                    weather_runtime::native_node_gzip_candidate as fn(&[u8]) -> Option<Vec<u8>>,
                );
                weather_runtime::run_native_installed_forecast_archive_operator(
                    arguments.iter().map(std::ffi::OsString::from),
                    codec,
                    &mut output,
                )
                .map_err(|error| error.to_string())
            }
            "revision-cold-custody-ack-v1" | "revision-cold-custody-ack-v2" => {
                let codec = ArchiveWireCodec::new(NativeWirePrimitives);
                run_native_cold_custody_command(
                    action,
                    &operands,
                    NativeColdCustodyRoot { root, uid, gid },
                    &codec,
                    NativeColdCustodyPorts {
                        archive_codec: &codec,
                        proof_codec: &mut NativeWirePrimitives,
                        time: &NativeWirePrimitives,
                        clock: &mut NativeProcessClock,
                    },
                    &mut output,
                )
                .map_err(|error| format!("{error:?}"))?;
                Ok(())
            }
            "revision-cold-page-v1" => {
                // parse all operands before observing the archive root
                let request = parse_revision_cold_page_command(action, &operands)
                    .map_err(|error| format!("{error:?}"))?
                    .ok_or_else(|| "unsupported evidence command".to_owned())?;
                let codec = ArchiveWireCodec::new(NativeWirePrimitives);
                let spool =
                    NativeRevisionSpool::new(root, &codec).map_err(|error| error.to_string())?;
                let bytes = spool
                    .read_revision_cold_page(request, &codec)
                    .map_err(|error| format!("{error:?}"))?;
                output.write_all(&bytes).map_err(|error| error.to_string())
            }
            "maintenance-anchor-status-v2" => {
                let mut files =
                    NativeMaintenanceAnchorStatusFiles::for_evidence_root(root, uid, gid);
                let bytes = read_maintenance_anchor_status_command(
                    action,
                    &operands,
                    &mut files,
                    &mut NativeWirePrimitives,
                    &NativeWirePrimitives,
                )
                .map_err(|error| error.to_string())?;
                write_receipt(&mut output, bytes)
            }
            "development-custody-anchor-current-v1" | "rain-control-custody-anchor-current-v1" => {
                let mut files = NativeCustodyAnchorCurrentFiles::for_evidence_root(root, uid, gid);
                let bytes = read_custody_anchor_current_command(
                    action,
                    &operands,
                    &mut files,
                    &mut NativeWirePrimitives,
                    &NativeWirePrimitives,
                )
                .map_err(|error| error.to_string())?;
                write_receipt(&mut output, bytes)
            }
            "development-custody-anchor-install-v1" | "rain-control-custody-anchor-install-v1" => {
                let control_sha256 = env::var("WEATHER_ANCHOR_CONTROL_SHA256").ok();
                let control_version = env::var("WEATHER_ANCHOR_CONTROL_VERSION").ok();
                // retain the fixed installed root-owned witness independently of caller operands
                let mut files = NativeCustodyAnchorInstallFiles::for_paths(
                    root,
                    PathBuf::from(
                        "/opt/weather/current/deploy/state/adjustment-capture-epoch.json",
                    ),
                    uid,
                    gid,
                    0,
                    0,
                );
                let bytes = install_custody_anchor_command(
                    action,
                    &operands,
                    &mut io::stdin().lock(),
                    &mut files,
                    &mut NativeWirePrimitives,
                    &NativeWirePrimitives,
                    CustodyAnchorControl {
                        sha256: control_sha256.as_deref(),
                        version: control_version.as_deref(),
                    },
                )
                .map_err(|error| error.to_string())?;
                write_receipt(&mut output, bytes)
            }
            "family-release-capacity" => {
                // retain the original complete six-role capacity receipt and refusal exit status
                let operands: [&str; 9] = operands
                    .as_slice()
                    .try_into()
                    .map_err(|_| "native family capacity operands differ".to_owned())?;
                let receipt =
                    weather_runtime::collect_native_family_release_capacity_from_installed(
                        operands,
                    )
                    .map_err(|error| error.to_string())?;
                let bytes = receipt
                    .canonical_bytes()
                    .ok_or_else(|| "native family capacity encoding differs".to_owned())?;
                output
                    .write_all(&bytes)
                    .map_err(|error| error.to_string())?;
                output.flush().map_err(|error| error.to_string())?;
                // source emits its measured refusal before returning the exact status code
                if receipt.exit_code() != 0 {
                    std::process::exit(receipt.exit_code());
                }
                Ok(())
            }
            "dashboard-release-capacity" => {
                // measure the unchanged core server and literal source web compensation
                let operands: [&str; 5] = operands
                    .as_slice()
                    .try_into()
                    .map_err(|_| "native dashboard capacity operands differ".to_owned())?;
                let receipt =
                    weather_runtime::collect_native_dashboard_release_capacity_from_installed(
                        operands,
                    )
                    .map_err(|error| error.to_string())?;
                let bytes = receipt
                    .canonical_bytes()
                    .ok_or_else(|| "native dashboard capacity encoding differs".to_owned())?;
                output
                    .write_all(&bytes)
                    .map_err(|error| error.to_string())?;
                output.flush().map_err(|error| error.to_string())?;
                // preserve the measured refusal after writing its receipt
                if receipt.exit_code() != 0 {
                    std::process::exit(receipt.exit_code());
                }
                Ok(())
            }
            "family-release" => {
                // retain the exact original nine-operand transaction and fixed installed owners
                let [
                    target_release,
                    compensating_release,
                    expected_current_release,
                    expected_source_release,
                    expected_settings_sha256,
                    family,
                    action_sha256,
                    report_sha256,
                    fencing_token,
                ] = operands.as_slice()
                else {
                    return Err("native family release operands differ".to_owned());
                };
                weather_runtime::run_native_family_release_from_installed(
                    weather_runtime::NativeFamilyReleaseInput {
                        target_release: (*target_release).to_owned(),
                        compensating_release: (*compensating_release).to_owned(),
                        expected_current_release: (*expected_current_release).to_owned(),
                        expected_source_release: (*expected_source_release).to_owned(),
                        expected_settings_sha256: (*expected_settings_sha256).to_owned(),
                        family: (*family).to_owned(),
                        action_sha256: (*action_sha256).to_owned(),
                        report_sha256: (*report_sha256).to_owned(),
                        fencing_token: (*fencing_token).to_owned(),
                    },
                )
                .map_err(|error| error.to_string())?;
                // project the actual retained categorical status just as the predecessor command did
                let bytes = read_native_family_release_status_command(
                    "family-release-status",
                    &[*action_sha256],
                    &mut NativeFamilyReleaseStatusHostFiles::native(),
                    &mut NativeWirePrimitives,
                    &NativeWirePrimitives,
                )
                .map_err(|error| error.to_string())?;
                write_receipt(&mut output, bytes)
            }
            "recover-family-release" => {
                // resolve only the exact durable unfinished installed mutation
                if !operands.is_empty() {
                    return Err("native family recovery takes no operands".to_owned());
                }
                let recovered = weather_runtime::recover_native_family_release_from_installed()
                    .map_err(|error| error.to_string())?;
                writeln!(output, "{}", if recovered { "recovered" } else { "none" })
                    .map_err(|error| error.to_string())
            }
            "verify-family-switch-record" => {
                // re-read the exact locked durable mutation before the installed internal image switch
                let [
                    release,
                    expected_current,
                    alternate_current,
                    commit,
                    catalog_sha256,
                    family_identity_sha256,
                    image_digests_sha256,
                    action_sha256,
                ] = operands.as_slice()
                else {
                    return Err("native family switch operands differ".to_owned());
                };
                let (family, role) = weather_runtime::verify_native_family_switch_record(
                    &weather_runtime::NativeFamilyReleaseSwitchRecordInput {
                        release,
                        expected_current,
                        alternate_current,
                        commit,
                        catalog_sha256,
                        family_identity_sha256,
                        image_digests_sha256,
                        action_sha256,
                    },
                )
                .map_err(|error| error.to_string())?;
                writeln!(output, "{family}\t{}", role.as_str()).map_err(|error| error.to_string())
            }
            "family-release-status" => {
                // read only the fixed source transaction journal and categorical receipt
                let mut files = NativeFamilyReleaseStatusHostFiles::native();
                let bytes = read_native_family_release_status_command(
                    action,
                    &operands,
                    &mut files,
                    &mut NativeWirePrimitives,
                    &NativeWirePrimitives,
                )
                .map_err(|error| error.to_string())?;
                write_receipt(&mut output, bytes)
            }
            "family-release-current" => {
                let [family] = operands.as_slice() else {
                    return Err("family-release-current requires FAMILY".to_owned());
                };
                let bytes = read_installed_family_status(family)?;
                write_receipt(&mut output, Some(bytes))
            }
            "family-release-lineage-v2" => {
                // preserve the exact zero-operand direct lineage command
                if !operands.is_empty() {
                    return Err("family-release-lineage-v2 takes no arguments".to_owned());
                }
                let bytes = read_installed_family_lineage()?;
                write_receipt(&mut output, Some(bytes))
            }
            "maintenance-anchor-install-v2" => {
                // legacy T consumes only canonical archive metadata and needs no forecast compressor
                let control = read_native_future_input_seal_process_control()
                    .map_err(|error| error.to_string())?;
                let mut files = NativeMaintenanceAnchorV3InstallStore::native(uid, gid);
                let mut archive = NativeMaintenanceLegacyMetadataV2::new(
                    PathBuf::from(DEFAULT_ROOT),
                    ArchiveWireCodec::new(NativeWirePrimitives),
                    NativeWirePrimitives,
                    ArchiveWireCodec::new(NativeWirePrimitives),
                );
                let bytes = install_maintenance_anchor_v2_command(
                    (action, &operands),
                    &mut io::stdin().lock(),
                    &mut files,
                    &mut archive,
                    &mut NativeWirePrimitives,
                    &NativeWirePrimitives,
                    NativeMaintenanceAnchorInstallV2Control {
                        version: control.version.as_deref(),
                        sha256: control.sha256.as_deref(),
                    },
                )
                .map_err(|error| error.to_string())?;
                write_receipt(&mut output, bytes)
            }
            "maintenance-anchor-finalize-v2" => {
                // use the complete canonical metadata and revision graph, then commit before exact retirement
                let mut files = NativeMaintenanceAnchorV3InstallStore::native(uid, gid);
                let mut archive = NativeMaintenanceLegacyMetadataV2::new(
                    PathBuf::from(DEFAULT_ROOT),
                    ArchiveWireCodec::new(NativeWirePrimitives),
                    NativeWirePrimitives,
                    ArchiveWireCodec::new(NativeWirePrimitives),
                );
                let bytes = finalize_maintenance_anchor_v2_command(
                    (action, &operands),
                    &mut io::stdin().lock(),
                    &mut files,
                    &mut archive,
                    &mut NativeWirePrimitives,
                    &NativeWirePrimitives,
                )
                .map_err(|error| error.to_string())?;
                write_receipt(&mut output, bytes)
            }
            "maintenance-anchor-install-v3" => {
                // preserve the source's exact optional control and raw installed seal authority
                let control = read_native_future_input_seal_process_control()
                    .map_err(|error| error.to_string())?;
                let mut files = NativeMaintenanceAnchorV3InstallStore::native(uid, gid);
                let mut seal = NativeMaintenanceAnchorInstallV3SealStore::native(uid, gid);
                let bytes = install_maintenance_anchor_v3_command(
                    (action, &operands),
                    &mut io::stdin().lock(),
                    &mut files,
                    &mut seal,
                    &mut NativeWirePrimitives,
                    &NativeWirePrimitives,
                    NativeMaintenanceAnchorInstallV3Control {
                        version: control.version.as_deref(),
                        sha256: control.sha256.as_deref(),
                    },
                )
                .map_err(|error| error.to_string())?;
                write_receipt(&mut output, bytes)
            }
            "maintenance-anchor-finalize-v3" => {
                // retain the future-only source transition without granting any legacy hot-file retirement
                let mut files = NativeMaintenanceAnchorV3InstallStore::native(uid, gid);
                let bytes = finalize_maintenance_anchor_v3_command(
                    (action, &operands),
                    &mut io::stdin().lock(),
                    &mut files,
                    &mut NativeWirePrimitives,
                    &NativeWirePrimitives,
                )
                .map_err(|error| error.to_string())?;
                write_receipt(&mut output, bytes)
            }
            "future-input-seal-current-v2" | "maintenance-anchor-current-v3" => {
                let bytes = read_installed_future_authority(action, &operands, uid, gid)?;
                write_receipt(&mut output, bytes)
            }
            "unsupported-terminal-proof-install-v1" => {
                // bind the actual independently verified live family owner to the existing custody installer
                let mut family_files = NativeFamilyReleaseCurrentHostFiles::native(0, 0);
                let mut runtime = NativeFamilyReleaseLiveRuntimeProcess::native();
                let mut verifier = NativeFamilyCatalogImageVerifier::new();
                let mut family_codec = NativeWirePrimitives;
                let mut authority = PreparedNativeFamilyReleaseCurrentAuthority {
                    files: &mut family_files,
                    runtime: &mut runtime,
                    verifier: &mut verifier,
                    codec: &mut family_codec,
                    time: &NativeWirePrimitives,
                };
                let control = read_native_future_input_seal_process_control()
                    .map_err(|error| error.to_string())?;
                let mut files = NativeUnsupportedTerminalProofInstallStore::native(uid, gid);
                let bytes = install_unsupported_terminal_proof_command(
                    (action, &operands),
                    &mut io::stdin().lock(),
                    &mut files,
                    &mut authority,
                    &mut NativeWirePrimitives,
                    &NativeWirePrimitives,
                    NativeUnsupportedTerminalProofControl {
                        version: control.version.as_deref(),
                        sha256: control.sha256.as_deref(),
                    },
                )
                .map_err(|error| error.to_string())?;
                write_receipt(&mut output, bytes)
            }
            "future-input-seal-install-v2" => {
                // use the fixed source evidence root and independently verified custody and epoch owners
                let mut files = NativeFutureInputSealInstallStore::native(uid, gid);
                let mut custody = NativeFutureInputSealRevisionCustody::native();
                let mut capture_epoch = NativeFutureInputSealOperatorCaptureEpoch::native();
                let bytes = install_future_input_seal_command(
                    (action, &operands),
                    &mut io::stdin().lock(),
                    &mut files,
                    &mut custody,
                    &mut capture_epoch,
                    &mut NativeWirePrimitives,
                    &NativeWirePrimitives,
                )
                .map_err(|error| error.to_string())?;
                write_receipt(&mut output, bytes)
            }
            "unsupported-terminal-proof-current-v1" => {
                // preserve the source's fixed direct command root independently of revision-root environment
                let mut files = NativeUnsupportedTerminalProofStore::native(uid, gid);
                let bytes = read_unsupported_terminal_proof_current_command(
                    action,
                    &operands,
                    &mut files,
                    &mut NativeWirePrimitives,
                    &NativeWirePrimitives,
                )
                .map_err(|error| error.to_string())?;
                write_receipt(&mut output, bytes)
            }
            "revision-gap-start-v1" | "revision-gap-page-v1" | "revision-gap-ack-v1" => {
                // admit the complete original operand set before opening the selected root
                let command = parse_revision_gap_operator_command(action, &operands)
                    .map_err(|error| format!("{error:?}"))?
                    .ok_or_else(|| "unsupported evidence command".to_owned())?;
                let lease = ArchiveWireCodec::new(NativeWirePrimitives);
                let mut spool =
                    NativeRevisionSpool::new(root, lease).map_err(|error| error.to_string())?;
                let mut codec = NativeRevisionGapCodec;
                let decoder = NativeRevisionGapCodec;
                let mut clock = NativeProcessClock;
                spool
                    .run_gap_operator_command(
                        command,
                        &mut NativeRevisionGapCommandPorts {
                            codec: &mut codec,
                            ack_decoder: &decoder,
                            clock: &mut clock,
                        },
                        &mut output,
                    )
                    .map_err(|error| format!("{error:?}"))
            }
            "revision-cold-start-v1" => {
                let mut files = NativeRevisionColdStartDirectories::for_root(root);
                let bytes = read_revision_cold_start_command(
                    action,
                    &operands,
                    io::stdin().lock(),
                    &mut files,
                    &mut NativeWirePrimitives,
                    &NativeWirePrimitives,
                )
                .map_err(|error| error.to_string())?;
                write_receipt(&mut output, bytes)
            }
            "revision-capture-epoch-init-v1" => {
                // retain the source root-only exact operand boundary before starting the host client
                if operands.len() != 1 || uid != 0 {
                    return Err(
                        "revision-capture-epoch-init-v1 requires root and TARGET_RELEASE"
                            .to_owned(),
                    );
                }
                let client = std::sync::Arc::new(NativeOutboundHttp::new().map_err(|_| {
                    "adjustment revision capture epoch deployment differs".to_owned()
                })?);
                let mut port = CaptureEpochInitFilePort::new(
                    NativeCaptureEpochInitializationAuthority::native(client),
                    CaptureEpochAuthorityFiles::native(),
                );
                let bytes = initialize_capture_epoch_operator_command(
                    action,
                    &operands,
                    io::stdin().lock(),
                    &mut port,
                    &mut NativeWirePrimitives,
                    &NativeWirePrimitives,
                )
                .map_err(|error| error.to_string())?;
                write_receipt(&mut output, bytes)
            }
            "revision-capture-epoch-read-v1" | "revision-capture-epoch-snapshot-read-v1" => {
                // the original authority file paths stay independent of the evidence root
                let bytes = read_capture_epoch_operator_command(
                    action,
                    &operands,
                    &mut NativeCaptureEpochOperatorFiles,
                    &mut NativeWirePrimitives,
                    &NativeWirePrimitives,
                )
                .map_err(|error| format!("{error:?}"))?;
                write_receipt(&mut output, bytes)
            }
            "shadow-metadata-custody-status-v1" | "shadow-metadata-custody-consumed-v1" => {
                let mut files = NativeShadowMetadataCustodyFiles::for_evidence_root(root, uid, gid);
                let bytes = read_shadow_metadata_custody_command(
                    action,
                    &operands,
                    &mut files,
                    &mut NativeWirePrimitives,
                    &NativeWirePrimitives,
                )
                .map_err(|error| error.to_string())?;
                write_receipt(&mut output, bytes)
            }
            "shadow-metadata-custody-prepare-v1" | "shadow-metadata-custody-consume-v1" => {
                let mut files = NativeShadowMetadataCustodyFiles::for_evidence_root(root, uid, gid);
                let now = NativeProcessClock
                    .canonical_now()
                    .ok_or_else(|| "clock unavailable".to_owned())?;
                let bytes = mutate_shadow_metadata_custody_command(
                    action,
                    &operands,
                    &mut io::stdin().lock(),
                    &mut files,
                    &mut NativeWirePrimitives,
                    &NativeWirePrimitives,
                    &now,
                )
                .map_err(|error| error.to_string())?;
                write_receipt(&mut output, bytes)
            }
            "prepare-shadow-metadata-custody-v1"
            | "consume-shadow-metadata-custody-v1"
            | "shadow-metadata-preparation-identity-v1"
            | "shadow-metadata-preparation-sql-v1"
            | "shadow-metadata-finalization-sql-v1" => {
                let mut files = NativeShadowMetadataCustodyFiles::for_evidence_root(root, uid, gid);
                let now = NativeProcessClock
                    .canonical_now()
                    .ok_or_else(|| "clock unavailable".to_owned())?;
                let bytes = shadow_metadata_custody_owner_command(
                    action,
                    &operands,
                    &mut io::stdin().lock(),
                    &mut files,
                    &mut NativeWirePrimitives,
                    &NativeWirePrimitives,
                    &now,
                )
                .map_err(|error| error.to_string())?;
                write_receipt(&mut output, bytes)
            }
            _ => Err("unsupported evidence command".to_owned()),
        }
    }

    /// Read one bounded regular export document via the existing no-follow file owner.
    fn read_export_control(path: &str, maximum: u64) -> Result<Vec<u8>, String> {
        let path = Path::new(path);
        let parent = path
            .parent()
            .and_then(Path::to_str)
            .ok_or_else(|| "export directory differs".to_owned())?;
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| "export file differs".to_owned())?;
        weather_runtime::NativeAdjustmentEvaluationPackageFiles
            .read_bounded_control(parent, name, maximum, "native export control")
            .map_err(|error| error.to_string())
    }

    /// Encode a source status envelope using the same canonical LF-framed JSON owner.
    fn family_status_bytes(
        fields: impl IntoIterator<Item = (&'static str, Option<String>)>,
    ) -> Result<Vec<u8>, String> {
        use weather_contracts::JsonValue;
        let value = JsonValue::Object(
            fields
                .into_iter()
                .map(|(key, value)| {
                    (
                        key.encode_utf16().collect(),
                        value.map_or(JsonValue::Null, |text| {
                            JsonValue::String(text.encode_utf16().collect())
                        }),
                    )
                })
                .collect(),
        );
        MaintenanceCanonicalJson::encode_canonical_json(&mut NativeWirePrimitives, &value)
            .ok_or_else(|| "family release status serialization failed".to_owned())
    }

    /// Read and return exactly the current family projection from the installed host.
    fn read_installed_family_status(family: &str) -> Result<Vec<u8>, String> {
        let mut files = NativeFamilyReleaseCurrentHostFiles::native(0, 0);
        let mut runtime = NativeFamilyReleaseLiveRuntimeProcess::native();
        let mut verifier = NativeFamilyCatalogImageVerifier::new();
        let status = read_native_family_release_current(
            family,
            &mut files,
            &mut runtime,
            &mut verifier,
            &mut NativeWirePrimitives,
            &NativeWirePrimitives,
        )
        .map_err(|error| error.to_string())?;
        family_status_bytes([
            (
                "activeInstalledReceiptSha256",
                status.active_installed_receipt_sha256,
            ),
            ("catalogSha256", Some(status.catalog_sha256)),
            ("commit", Some(status.commit)),
            (
                "controlInstalledReceiptSha256",
                status.control_installed_receipt_sha256,
            ),
            ("contractVersion", Some(status.contract_version)),
            ("family", Some(status.family)),
            ("release", Some(status.release)),
            ("settingsSha256", Some(status.settings_sha256)),
            (
                "shadowInstalledReceiptSha256",
                status.shadow_installed_receipt_sha256,
            ),
            (
                "sourceServerImageDigest",
                Some(status.source_server_image_digest),
            ),
        ])
    }

    /// Read the independently verified source and terminal release ancestry on the installed host.
    fn read_installed_family_lineage() -> Result<Vec<u8>, String> {
        let mut files = NativeFamilyReleaseCurrentHostFiles::native(0, 0);
        let mut runtime = NativeFamilyReleaseLiveRuntimeProcess::native();
        let mut verifier = NativeFamilyCatalogImageVerifier::new();
        let mut codec = NativeWirePrimitives;
        let live = PreparedNativeFamilyLineageLiveObservation {
            files: &mut files,
            runtime: &mut runtime,
            verifier: &mut verifier,
            codec: &mut codec,
            time: &NativeWirePrimitives,
        };
        let mut closure = NativeFamilyLineageSourceClosure::native(live);
        let status = closure.read_lineage().map_err(|error| error.to_string())?;
        family_status_bytes([
            ("contractVersion", Some(status.contract_version.to_owned())),
            ("controlSha256", Some(status.control_sha256)),
            ("controlVersion", Some(status.control_version)),
            ("currentActionSha256", status.current_action_sha256),
            ("currentCommit", Some(status.current_commit)),
            ("currentRelease", Some(status.current_release)),
            ("epochAncestorCommit", Some(status.epoch_ancestor_commit)),
            ("epochWitnessSha256", Some(status.epoch_witness_sha256)),
            ("state", Some(status.state.to_owned())),
            ("verifiedAt", Some(status.verified_at)),
        ])
    }

    /// Read the original installed future seal or anchor using the real retained source lineage.
    fn read_installed_future_authority(
        action: &str,
        operands: &[&str],
        uid: u32,
        gid: u32,
    ) -> Result<Option<Vec<u8>>, String> {
        // do not observe the live release or any storage on a malformed command
        if !operands.is_empty() {
            return Err(format!("{action} takes no arguments"));
        }
        let control =
            read_native_future_input_seal_process_control().map_err(|error| error.to_string())?;
        let mut files = NativeFutureInputSealStore::native(uid, gid);
        let mut capture = NativeFutureInputSealOperatorCaptureEpoch::native();
        let mut family_files = NativeFamilyReleaseCurrentHostFiles::native(0, 0);
        let mut runtime = NativeFamilyReleaseLiveRuntimeProcess::native();
        let mut verifier = NativeFamilyCatalogImageVerifier::new();
        let mut family_codec = NativeWirePrimitives;
        let live = PreparedNativeFamilyLineageLiveObservation {
            files: &mut family_files,
            runtime: &mut runtime,
            verifier: &mut verifier,
            codec: &mut family_codec,
            time: &NativeWirePrimitives,
        };
        let mut closure = NativeFamilyLineageSourceClosure::native(live);
        let mut codec = NativeWirePrimitives;
        // both original verbs require the same verified active epoch and terminal ancestry
        if action == "future-input-seal-current-v2" {
            read_future_input_seal_current_command(
                (action, operands),
                &mut files,
                &mut capture,
                control.borrowed(),
                &mut closure,
                &mut codec,
                &NativeWirePrimitives,
            )
            .map_err(|error| error.to_string())
        } else {
            let mut anchors = NativeMaintenanceAnchorStatusFiles::native(uid, gid);
            read_maintenance_anchor_current_v3_command(
                action,
                operands,
                &mut anchors,
                &mut codec,
                &NativeWirePrimitives,
                NativeMaintenanceAnchorCurrentV3Control {
                    version: control.version.as_deref(),
                    sha256: control.sha256.as_deref(),
                },
                |codec, time| {
                    read_future_input_seal_current_verified(
                        &mut files,
                        &mut capture,
                        control.borrowed(),
                        &mut closure,
                        codec,
                        time,
                    )
                },
            )
            .map_err(|error| error.to_string())
        }
    }

    /// Write only one fully formed command response to stdout.
    fn write_receipt(output: &mut impl Write, bytes: Option<Vec<u8>>) -> Result<(), String> {
        let bytes = bytes.ok_or_else(|| "unsupported evidence command".to_owned())?;
        output.write_all(&bytes).map_err(|error| error.to_string())
    }

    /// Return one ordinary process status and keep command stdout reserved for its receipt.
    pub fn main() -> ExitCode {
        // preserve the original success receipt and a nonzero ordinary command failure
        match run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("weather-evidence: {error}");
                ExitCode::FAILURE
            }
        }
    }
}

/// Dispatch only on the native Linux architectures supported by the retained store.
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
fn main() -> std::process::ExitCode {
    linux::main()
}

/// Leave unsupported targets explicit instead of changing the archive implementation.
#[cfg(not(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
)))]
fn main() -> std::process::ExitCode {
    eprintln!("weather-evidence: native archive unavailable on this target");
    std::process::ExitCode::FAILURE
}
