//! Typed runtime adapters joining worker scheduling to database persistence.

mod adjustment_evaluation_package_edge_verification;
mod adjustment_evaluation_package_files;
mod adjustment_evaluation_package_manifest;
mod adjustment_evaluation_package_materialization;
mod adjustment_evaluation_package_row;
mod adjustment_evaluation_package_stream;
mod adjustment_evaluation_package_verification;
mod ambient_backfill;
mod api_application_startup;
mod api_forecast_model_projection;
mod api_forecast_repository;
mod api_installed_runtime_loader;
mod api_maintenance_admission;
mod api_maintenance_archive_relay;
mod api_maintenance_archive_wire;
mod api_maintenance_candidate;
mod api_maintenance_candidate_catalog;
mod api_maintenance_candidate_files;
mod api_maintenance_candidate_members;
mod api_maintenance_candidate_package;
mod api_maintenance_capture;
mod api_maintenance_capture_pipeline;
mod api_maintenance_incumbents;
mod api_maintenance_pool;
mod api_maintenance_prediction;
mod api_maintenance_receipts;
mod api_maintenance_request_body;
mod api_maintenance_response_wire;
mod api_maintenance_revision_admission;
mod api_maintenance_scalar_admission;
mod api_private_application;
mod api_process_applications;
mod api_process_clock;
mod api_process_configuration;
mod api_public_application;
mod api_qualified_bundle_projection;
mod api_qualified_canary_loader;
#[cfg(test)]
mod api_qualified_canary_loader_test;
mod api_rain_runtime_loader;
mod api_read_pool;
mod api_repository;
mod api_repository_values;
mod api_revision_group_projection;
mod api_revision_projection;
mod api_revision_rain_stage_projection;
mod api_revision_single_projection;
mod api_service_forecast_values;
mod api_service_json_values;
mod api_service_other_values;
mod api_service_weather_values;
mod api_shadow_comparator_authority;
#[cfg(test)]
mod api_shadow_comparator_authority_test;
mod api_shadow_comparator_validation;
#[cfg(test)]
mod api_shadow_comparator_validation_test;
mod api_shadow_epoch_projection;
mod api_shadow_legacy_runtime;
mod api_shadow_legacy_runtime_candidate;
mod api_shadow_legacy_runtime_receipt;
mod api_shadow_legacy_runtime_report;
#[cfg(test)]
mod api_shadow_legacy_runtime_test;
mod api_shadow_legacy_temperature;
#[cfg(test)]
mod api_shadow_legacy_temperature_test;
mod api_shadow_legacy_values;
mod api_shadow_legacy_wind_canary;
#[cfg(test)]
mod api_shadow_legacy_wind_canary_test;
mod api_shadow_legacy_wind_evidence;
mod api_shadow_legacy_wind_material;
mod api_shadow_metadata_projection;
#[cfg(test)]
mod api_shadow_metadata_projection_test;
mod api_shadow_rain_source_validation;
mod api_shadow_source_validation;
mod api_shadow_wire_validation;
#[cfg(test)]
mod api_shadow_wire_validation_test;
mod api_standard_bundle_projection;
mod api_standard_runtime_files;
mod api_standard_runtime_loader;
mod api_standard_runtime_projection;
mod api_temperature_runtime_loader;
#[cfg(test)]
mod api_temperature_runtime_loader_test;
mod api_temperature_runtime_projection;
mod api_wind_runtime_loader;
#[cfg(test)]
mod api_wind_runtime_loader_test;
mod api_wind_runtime_projection;
mod capture_epoch_init;
mod capture_epoch_init_deployment;
mod capture_epoch_init_envelope;
mod capture_epoch_init_files;
mod capture_epoch_loader;
mod capture_epoch_operator;
mod capture_epoch_witness;
mod custody_anchor_current;
mod custody_anchor_install;
mod custody_anchor_install_files;
mod custody_anchor_validation;
mod family_action_authority;
mod family_action_authority_files;
mod family_model_target_catalog;
mod family_release_identity;
mod family_release_images;
mod family_target_binding;
mod family_target_catalog;
mod family_target_catalog_validation;
mod family_target_image_process;
mod family_target_preflight;
mod forecast_anchor_backfill;
mod forecast_scorecard_cli;
mod forecast_scorecard_contract;
mod forecast_scorecard_family;
mod forecast_scorecard_fields;
mod forecast_scorecard_rain;
mod forecast_scorecard_v2;
mod forecast_training_package_files;
mod forecast_training_package_manifest;
mod forecast_training_package_row;
mod forecast_training_package_verification;
mod historical_backfill_application;
mod historical_backfill_process;
mod installed_artifact_files;
mod installed_family_catalog_verification;
mod maintenance_anchor_status;
mod maintenance_anchor_status_v2;
mod maintenance_anchor_status_v3;
mod maintenance_artifact_rain_decode;
mod maintenance_canonical_json;
mod migration_bootstrap_pool;
mod migration_command;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_admin_auth_state;
mod native_admin_session_material;
mod native_ambient_transport;
mod native_api_capture_codec;
mod native_api_capture_execution;
mod native_api_installed_owner;
mod native_api_maintenance_http;
mod native_api_model_admission;
mod native_api_process_listeners;
mod native_api_public_http;
mod native_api_revision_document_decode;
mod native_api_time_values;
mod native_backend_runtime;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_background_forecast_scheduler;
mod native_calendar_compatibility;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_capture_epoch_init_authority;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_cold_custody_command;
mod native_ecowitt_transport;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_edge_admin_admission;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_edge_admin_files;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_edge_admin_owner;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_edge_composed_process;
#[cfg(all(
    test,
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_edge_composed_process_test;
mod native_edge_http_headers;
mod native_edge_http_request;
mod native_edge_http_response;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_edge_online_archive;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_edge_post_response;
mod native_edge_preview;
mod native_edge_private_handler;
#[cfg(all(
    test,
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_edge_private_handler_test;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_edge_process_listeners;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_edge_public_handler;
#[cfg(all(
    test,
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_edge_public_handler_loopback_test;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_edge_recorded_owner;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_edge_recorded_projection;
#[cfg(all(
    test,
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_edge_recorded_projection_test;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_edge_route_execution;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_edge_route_graph;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_edge_runtime_resources;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_edge_scorecard;
mod native_edge_startup_material;
mod native_edge_tile_owner;
mod native_edge_upstream;
#[cfg(test)]
mod native_edge_upstream_test;
mod native_edge_viewer_context;
mod native_edge_widget_projection;
mod native_edge_widget_source;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_family_catalog_image_verifier;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_family_lineage_host;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_family_release_current;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_family_release_lineage;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_family_release_live_runtime;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_family_release_status;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_fit_ascii_collation;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_forecast_allocation_statfs;
mod native_forecast_archive_codec;
mod native_forecast_evidence_codec;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_forecast_scheduler_timer;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_forecast_scheduler_values;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_future_input_seal_current;
#[cfg(all(
    test,
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_future_input_seal_current_test;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_future_input_seal_install;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_historical_backfill_output;
mod native_http_body;
mod native_http_listener;
mod native_http_terminal;
mod native_installed_api_process;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_installed_edge_process;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_installed_forecast_archive_operator;
#[cfg(all(
    test,
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_installed_forecast_archive_operator_test;
#[cfg(unix)]
mod native_installed_worker_process;
mod native_js_scalar;
mod native_json;
mod native_json_utf16;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_anchor_current_v3;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_anchor_finalize_v2;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_anchor_finalize_v3;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_anchor_install_v2;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_anchor_install_v3;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_archive_genesis;
#[cfg(all(
    test,
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_archive_genesis_test;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_archive_journal;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_archive_segment;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_backing_capacity;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_backing_census;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_capture_cycle_core;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_capture_genesis_host;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_capture_host_readiness;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_capture_installed_closure;
#[cfg(all(
    test,
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_capture_installed_closure_test;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_capture_job;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_capture_native_installation;
#[cfg(all(
    test,
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_capture_native_installation_test;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_capture_readiness;
#[cfg(all(
    test,
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_capture_readiness_test;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_capture_readiness_wire;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_capture_remote_authority;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_cas_pack;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_catalog_wire;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_cold_journal;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_cold_remote_documents;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_cold_remote_host;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_cold_remote_page;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_controller_lock;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_database_ledger;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_finalization_proof_v2;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_fit_runtime_readiness;
#[cfg(all(
    test,
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_fit_runtime_readiness_test;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_full_graph;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_future_only_journal;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_gap_archive_validators;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_graph_manifest;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_graph_restore;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_journal;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_journal_store;
#[cfg(all(
    test,
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_journal_store_test;
#[cfg(all(
    test,
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_journal_test;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_legacy_metadata_v2;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_local_archive_envelope;
#[cfg(all(
    test,
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_local_archive_envelope_test;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_local_archive_readiness;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_open_pack_store;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_plaintext_archive;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_plaintext_object_store;
#[cfg(all(
    test,
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_plaintext_object_store_test;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_registration_lifecycle_host;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_registration_lifecycle_remote;
#[cfg(all(
    test,
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_registration_lifecycle_remote_test;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_registration_policy;
#[cfg(all(
    test,
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_registration_policy_test;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_retirement_expected_v2;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_retirement_files_v2;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_revision_cold_drain;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_revision_gap_drain;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_revision_retirement_v2;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_rolling_schedule;
mod native_migration_output;
mod native_model_evidence_values;
mod native_netatmo_transport;
mod native_noaa_tide_transport;
#[cfg(feature = "node-zlib-candidate")]
mod native_node_gzip_candidate;
mod native_open_meteo_scheduled_transport;
mod native_open_meteo_transport;
mod native_open_meteo_values;
mod native_outbound_http;
#[cfg(test)]
mod native_outbound_http_test;
mod native_postgres_api_capture_ports;
mod native_postgres_api_maintenance;
mod native_postgres_api_read;
mod native_postgres_api_serve;
#[cfg(test)]
mod native_postgres_api_serve_test;
mod native_postgres_api_startup;
mod native_postgres_backfill;
mod native_postgres_migration;
mod native_postgres_pool;
mod native_postgres_private_handler;
mod native_postgres_public_handler;
mod native_postgres_rain_capture;
mod native_postgres_rain_receipt;
#[cfg(all(
    test,
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_postgres_real_roundtrip_test;
mod native_postgres_scheduled;
mod native_postgres_weather_rows;
mod native_postgres_worker_iteration;
mod native_postgres_worker_rain_inference;
mod native_postgres_worker_temperature;
mod native_private_archive_http;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_private_archive_stores;
mod native_process_clock;
#[cfg(unix)]
mod native_process_shutdown;
mod native_public_backfill_time;
mod native_public_station_startup_values;
mod native_purpleair_transport;
mod native_rain_capture_values;
mod native_rain_collection_transport;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_revision_gap_artifacts;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_revision_gap_codec;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_revision_gap_decode;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_revision_gap_identity;
mod native_runtime_material;
mod native_scheduled_values;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_scheduler_capture;
mod native_shadow_producer_codec;
mod native_temperature_canary_provider;
mod native_tempest_tide_transport;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_unsupported_terminal_proof;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_unsupported_terminal_proof_install;
mod native_widget_forecast_admission;
mod native_wire_primitives;
mod native_worker_backfill_owner;
mod native_worker_configuration;
mod native_worker_database;
mod native_worker_maintenance_task_binding;
mod native_worker_process_runner;
mod native_worker_provider_factory;
mod native_worker_public_tide_catalogs;
mod native_worker_rain_capture_owner;
mod native_worker_rain_task_binding;
mod native_worker_runtime_owner;
mod native_worker_scheduler;
mod native_worker_sidecar_owner;
mod native_worker_site_catalogs;
mod native_worker_task_assembly;
mod native_worker_temperature_archive_binding;
mod native_worker_temperature_binding;
mod native_worker_temperature_task_owner;
mod native_wunderground_transport;
mod native_xweather_tile_transport;
mod open_meteo_archive_backfill_database;
mod open_meteo_archive_provider;
mod open_meteo_backfill_application;
mod open_meteo_forecast;
mod open_meteo_forecast_evidence;
mod open_meteo_previous_runs_provider;
mod public_station_archive_binding;
mod public_station_startup_backfill;
mod rain_archive_performance;
mod rain_archive_target;
mod rain_performance_adjustment_rows;
mod rain_performance_cli;
#[cfg(test)]
mod rain_performance_cli_native_full_test;
mod rain_performance_edge_rows;
mod rain_performance_package_composition;
mod rain_performance_package_loader;
mod rain_performance_result_io;
mod rain_repository;
mod revision_cold_start;
mod revision_custody_ack_reader;
mod shadow_metadata_custody;
mod shadow_metadata_custody_mutation;
mod shadow_metadata_custody_owner;
mod shadow_metadata_custody_owner_sql;
mod shadow_metadata_custody_preparation;
mod shadow_metadata_custody_proof;
mod shadow_metadata_custody_publish;
mod shadow_metadata_custody_write_files;
mod supplemental_public_station_repository;
mod temperature_canary_persistence;
mod temperature_performance_cli;
mod temperature_performance_data_projection;
mod temperature_performance_packages;
mod temperature_performance_target;
mod temperature_recent_error;
mod tempest_backfill;
mod tide_backfill;
mod wind_maintenance_archive_manifest;
mod wind_maintenance_candidate;
mod wind_maintenance_cli;
mod wind_maintenance_cutoff;
mod wind_maintenance_fit;
mod wind_maintenance_input;
mod wind_maintenance_input_rows;
mod wind_maintenance_material;
mod wind_maintenance_snapshot_manifest;
mod wind_performance_cli;
mod wind_performance_common;
mod wind_performance_counterfactual;
mod wind_performance_issued;
mod wind_performance_replay;
mod worker_ambient_weather_dispatch;
mod worker_boot_values;
mod worker_capture_epoch_startup;
mod worker_configuration;
mod worker_database_boot;
mod worker_ecmwf_revision_archive;
mod worker_ecmwf_revision_material;
mod worker_ecowitt_dispatch;
mod worker_health_output;
mod worker_ingestion_deadlines;
mod worker_iteration_factory;
mod worker_iteration_postgres;
mod worker_maintenance_capture_task;
mod worker_netatmo_operation;
mod worker_noaa_tide_dispatch;
mod worker_process;
mod worker_process_configuration;
mod worker_public_station_dispatch;
mod worker_purpleair_operation;
mod worker_rain_adjustment_inputs;
mod worker_rain_candidate_loader;
mod worker_rain_candidate_package;
mod worker_rain_capture_repository;
mod worker_rain_capture_task;
mod worker_rain_control_catalog;
mod worker_rain_control_projection;
mod worker_rain_control_reference;
mod worker_rain_feature_revision;
mod worker_rain_fixed_gauge_database_values;
mod worker_rain_fixed_gauge_identity;
mod worker_rain_fixed_gauge_projection;
mod worker_rain_fixed_gauge_task;
mod worker_rain_fixed_gauge_wire;
mod worker_rain_gauge_replay;
mod worker_rain_inference_repository;
mod worker_rain_inference_startup;
mod worker_rain_inference_task;
mod worker_rain_maintenance_http;
mod worker_rain_maintenance_wire;
mod worker_rain_persistence;
mod worker_rain_portable_candidate;
mod worker_rain_serving_catalog;
mod worker_rain_serving_registry;
mod worker_rain_shadow;
mod worker_scheduled_dispatch;
mod worker_scheduler;
mod worker_sidecar_runtime;
mod worker_sidecar_tasks;
mod worker_source_projection;
mod worker_temperature_canary_task;
mod worker_tempest_dispatch;
mod worker_weather_revision_archive;
mod worker_weather_revision_material;
mod worker_wunderground_operation;

pub use adjustment_evaluation_package_edge_verification::*;
pub use adjustment_evaluation_package_files::*;
pub use adjustment_evaluation_package_manifest::*;
pub use adjustment_evaluation_package_materialization::*;
pub use adjustment_evaluation_package_row::*;
pub use adjustment_evaluation_package_stream::*;
pub use adjustment_evaluation_package_verification::*;
pub use ambient_backfill::*;
pub use api_application_startup::*;
pub use api_forecast_repository::*;
pub use api_installed_runtime_loader::*;
pub use api_maintenance_admission::*;
pub use api_maintenance_archive_relay::*;
pub use api_maintenance_archive_wire::*;
pub use api_maintenance_candidate::*;
pub use api_maintenance_candidate_files::*;
pub use api_maintenance_capture::*;
pub use api_maintenance_capture_pipeline::*;
pub use api_maintenance_incumbents::*;
pub use api_maintenance_pool::*;
pub use api_maintenance_prediction::*;
pub use api_maintenance_receipts::*;
pub use api_maintenance_request_body::*;
pub use api_maintenance_response_wire::*;
pub use api_maintenance_revision_admission::*;
pub use api_private_application::*;
pub use api_process_applications::*;
pub use api_process_clock::*;
pub use api_process_configuration::*;
pub use api_public_application::*;
pub use api_qualified_bundle_projection::decode_maintenance_runtime_package;
pub use api_qualified_canary_loader::*;
pub use api_rain_runtime_loader::*;
pub use api_read_pool::*;
pub use api_repository::{
    AdmittedHealthMigrationArtifacts, PostgresWeatherServiceRepository,
    WeatherServiceExternalReadPort, WeatherServicePostgresReadPort,
};
pub use api_repository_values::*;
pub use api_revision_projection::ApiMaintenanceRevisionTransportDocument;
pub use api_service_json_values::{NativeWeatherServiceWireEncoder, weather_service_body_value};
pub use api_standard_runtime_files::*;
pub use api_standard_runtime_loader::*;
pub use api_temperature_runtime_loader::*;
pub use api_wind_runtime_loader::*;
pub use capture_epoch_init::*;
pub use capture_epoch_init_deployment::*;
pub use capture_epoch_init_files::*;
pub use capture_epoch_loader::*;
pub use capture_epoch_operator::*;
pub use capture_epoch_witness::*;
pub use custody_anchor_current::*;
pub use custody_anchor_install::*;
pub use custody_anchor_install_files::*;
pub use family_action_authority::*;
pub use family_action_authority_files::*;
pub use family_release_identity::{
    compute_family_release_identity, compute_family_release_image_identity,
};
pub use family_release_images::*;
pub use family_target_catalog::{
    FamilyTargetCatalogInput, FamilyTargetImageRead, build_family_target_catalog,
};
pub use family_target_image_process::NativeFamilyTargetImage;
pub use family_target_preflight::*;
pub use forecast_anchor_backfill::{
    DatabaseForecastAnchorBackfillRepository, ForecastAnchorBackfillApplicationPort,
    ForecastAnchorBackfillInvocation, ForecastAnchorBackfillInvocationError,
    ForecastAnchorBackfillRuntimeValues, ForecastAnchorBackfillSourceReadError,
    ForecastAnchorPersistenceOperation, ForecastAnchorTimeValue,
    prepare_forecast_anchor_backfill_invocation, resolve_forecast_anchor_backfill_source,
    run_forecast_anchor_backfill_application, run_prepared_forecast_anchor_backfill_application,
};
pub use forecast_scorecard_cli::*;
pub use forecast_scorecard_contract::*;
pub use forecast_scorecard_fields::{ScorecardClock, ScorecardContractError};
pub use forecast_training_package_files::*;
pub use forecast_training_package_manifest::*;
pub use forecast_training_package_row::*;
pub use forecast_training_package_verification::*;
pub use historical_backfill_application::*;
pub use historical_backfill_process::*;
pub use installed_family_catalog_verification::{
    verify_installed_family_catalog, verify_native_installed_family_catalog,
};
pub use maintenance_anchor_status::*;
pub use maintenance_canonical_json::MaintenanceCanonicalJson;
pub use migration_bootstrap_pool::*;
pub use migration_command::*;
pub use native_shadow_producer_codec::*;
pub use open_meteo_archive_backfill_database::*;
pub use open_meteo_archive_provider::*;
pub use open_meteo_backfill_application::*;
pub use open_meteo_forecast::*;
pub use open_meteo_forecast_evidence::*;
pub use open_meteo_previous_runs_provider::*;
pub use public_station_startup_backfill::{
    PublicStationStartupBackfill, PublicStationStartupConfiguration,
    PublicStationStartupDiagnostic, PublicStationStartupDiagnosticEvent,
    PublicStationStartupDiagnostics, PublicStationStartupHandle, PublicStationStartupInput,
    PublicStationStartupOutcome, PublicStationStartupPool, PublicStationStartupTime,
};
pub use rain_archive_performance::*;
pub use rain_archive_target::*;
pub use rain_performance_adjustment_rows::*;
pub use rain_performance_cli::*;
pub use rain_performance_edge_rows::*;
pub use rain_performance_package_composition::*;
pub use rain_performance_package_loader::*;
pub use rain_performance_result_io::*;
pub use rain_repository::{
    RainRepositoryReadError, RainRepositoryTimeBoundary, read_admitted_rain_adjustment_run,
    read_admitted_rain_collection_status,
};
pub use revision_cold_start::*;
pub use shadow_metadata_custody::*;
pub use shadow_metadata_custody_mutation::*;
pub use shadow_metadata_custody_owner::*;
pub use supplemental_public_station_repository::{
    SupplementalPublicStationDatabaseAdapter, SupplementalPublicStationDatabaseRepository,
};
pub use temperature_canary_persistence::{
    DatabaseTemperatureCanaryRepository, TemperatureCanaryPersistenceDatabasePort,
    TemperatureCanaryPersistencePreparationPort, TemperatureCanaryRecentErrorDigestPort,
    TemperatureCanaryRevisionArchiveMode, TemperatureCanaryRunDigestMaterial,
};
pub use temperature_performance_cli::*;
pub use temperature_performance_data_projection::*;
pub use temperature_performance_packages::*;
pub use temperature_performance_target::*;
pub use temperature_recent_error::{
    AdmittedCausalEcmwfPriorHourReadRow, AdmittedCausalTemperatureStationReadRow,
    CausalTemperatureCalendarPort, CausalTemperatureDatabasePort,
    DatabaseTemperatureRecentErrorBuilder, build_causal_temperature_recent_error_state,
};
pub use tempest_backfill::{
    DatabaseTempestBackfillRepository, TempestBackfillPersistenceOperation,
    TempestBackfillRuntimeValues,
};
pub use tide_backfill::{
    DatabaseTideBackfillRepository, TideBackfillPersistenceOperation, TideBackfillRuntimeValues,
};
pub use wind_maintenance_candidate::*;
pub use wind_maintenance_cli::*;
pub use wind_maintenance_cutoff::*;
pub use wind_maintenance_fit::*;
pub use wind_maintenance_input::*;
pub use wind_performance_cli::*;
pub use wind_performance_common::{WindPerformanceBand, WindPerformanceError};
pub use wind_performance_counterfactual::{
    WindPerformanceCounterfactualValues, WindPerformanceReplayDecision,
};
pub use wind_performance_replay::*;
pub use worker_ambient_weather_dispatch::*;
pub use worker_boot_values::*;
pub use worker_capture_epoch_startup::*;
pub use worker_configuration::*;
pub use worker_database_boot::*;
pub use worker_ecmwf_revision_archive::*;
pub use worker_ecmwf_revision_material::*;
pub use worker_ecowitt_dispatch::*;
pub use worker_health_output::*;
pub use worker_ingestion_deadlines::*;
pub use worker_iteration_factory::*;
pub use worker_iteration_postgres::*;
pub use worker_maintenance_capture_task::*;
pub use worker_netatmo_operation::*;
pub use worker_noaa_tide_dispatch::*;
pub use worker_process::{DatabaseWorkerProcessIteration, WorkerIterationDrainPort};
pub use worker_process_configuration::*;
pub use worker_public_station_dispatch::*;
pub use worker_purpleair_operation::*;
pub use worker_rain_adjustment_inputs::*;
pub use worker_rain_candidate_loader::*;
pub use worker_rain_capture_repository::*;
pub use worker_rain_capture_task::*;
pub use worker_rain_control_projection::*;
pub use worker_rain_control_reference::*;
pub use worker_rain_feature_revision::*;
pub use worker_rain_fixed_gauge_identity::*;
pub use worker_rain_fixed_gauge_projection::*;
pub use worker_rain_fixed_gauge_task::*;
pub use worker_rain_gauge_replay::*;
pub use worker_rain_inference_repository::*;
pub use worker_rain_inference_startup::*;
pub use worker_rain_inference_task::*;
pub use worker_rain_maintenance_http::*;
pub use worker_rain_maintenance_wire::*;
pub use worker_rain_persistence::*;
pub use worker_rain_portable_candidate::*;
pub use worker_rain_serving_registry::*;
pub use worker_rain_shadow::*;
pub use worker_scheduled_dispatch::*;
pub use worker_scheduler::{AnchoredWorkerProcessScheduler, WorkerProcessOneShotTimer};
pub use worker_sidecar_runtime::*;
pub use worker_sidecar_tasks::*;
pub use worker_source_projection::*;
pub use worker_temperature_canary_task::*;
pub use worker_tempest_dispatch::*;
pub use worker_weather_revision_archive::*;
pub use worker_weather_revision_material::*;
pub use worker_wunderground_operation::*;

use std::fmt::Display;
use std::marker::PhantomData;

use weather_contracts::{
    CanonicalWeatherMetrics, DeviceMetadata, FieldPresence, JsonValue, NormalizedWeatherRecord,
    Nullable, ScheduledCheckpointState, SourceKind, WeatherRecordMetadata,
};
use weather_database::{
    AdmittedAdapterVersion, AdmittedIngestionError, AdmittedScheduledRunTimes,
    CompleteScheduledIngestion as DatabaseScheduledCompletion, FailScheduledIngestion,
    PreparedWeatherRecord, RetainedSourceSession, ScheduledCheckpointReadPort,
    ScheduledIngestionCommandPort, ScheduledIngestionExecutionError,
    ScheduledRunFailureCommandPort, ScheduledRunStartCommandPort, SerializedJson, Sha256Digest,
    StartScheduledIngestion, WeatherRecordStorageEvidence, fail_scheduled_ingestion,
    get_scheduled_checkpoint, start_scheduled_ingestion,
};
use weather_providers::{
    DEFAULT_PROVIDER_MAX_ATTEMPTS, DEFAULT_PROVIDER_TIMEOUT_MS, MAX_PROVIDER_BODY_BYTES,
    ProviderControls, ProviderIo, ProviderRequestPolicy, ProviderRunError, run_provider_request,
};
use weather_worker::{
    AmbientWeatherPayload, AmbientWeatherProviderCursor, AmbientWeatherProviderMetadata,
    AmbientWeatherQualityMetadata, AmbientWeatherRangeRequest, AmbientWeatherResponseMetadata,
    AmbientWeatherSourceKind, EcowittCanonicalMetadataAdapter, EcowittLivePayload,
    EcowittLiveRequest, EcowittPreviousCursor, EcowittProviderCursor, EcowittProviderMetadata,
    EcowittQualityMetadata, EcowittRainGaugeKind, EcowittResponseMetadata, EcowittScheduledRequest,
    EcowittSensorReadings, EcowittSensorSnapshot, EcowittSourceKind, EcowittTimeAdapter,
    HeartbeatRepository, NoaaPredictionType, NoaaTidePayload, NoaaTideProduct,
    NoaaTideProviderCursor, NoaaTideProviderMetadata, NoaaTideQualityMetadata,
    NoaaTideRangeRequest, NoaaTideResponseMetadata, NoaaTideSourceKind,
    NormalizedAmbientWeatherRecord, NormalizedEcowittRecord, NormalizedNoaaTideRecord,
    NormalizedOpenMeteoRecord, NormalizedTempestRecord, OpenMeteoCursor, OpenMeteoEnvelope,
    OpenMeteoProvenance, OpenMeteoSourceKind, OpenMeteoWeatherSample, ProviderBatch,
    ProviderRequest, ScheduledCheckpoint, ScheduledCompletion, ScheduledProvider,
    ScheduledProviderResult, ScheduledRunRepository, ScheduledRunStart, TaskFailure,
    TempestObservationEnvelope, TempestObservationRequest, TempestProviderCursor,
    TempestProviderMetadata, TempestQualityFlag, TempestQualityMetadata, TempestResponseMetadata,
    WorkerDueSource, WorkerHeartbeat, WorkerIterationRepository, normalize_ambient_weather_range,
    normalize_ecowitt_live, normalize_noaa_tide_range, normalize_open_meteo_current,
    normalize_tempest_observations, require_ecowitt_gateway_identity,
};

/// One typed Open-Meteo current product returned by an injected transport.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenMeteoParsedProduct {
    pub envelope: OpenMeteoEnvelope,
    pub sample: OpenMeteoWeatherSample,
}

/// Parsed OpenMeteo response plus already-admitted persistence evidence.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenMeteoAttemptPayload {
    pub product: OpenMeteoParsedProduct,
    pub received_at_ms: i64,
    pub response_metadata: Option<SerializedJson>,
    pub upstream_response_checksum: String,
}

/// Open-Meteo current-weather adapter backed by one bounded provider controller.
pub struct RetryingOpenMeteoProvider<I> {
    controls: ProviderControls,
    deadline_at_ms: Option<i64>,
    io: I,
}

impl<I> RetryingOpenMeteoProvider<I> {
    /// Bind one injected transport to production retry and deadline controls.
    pub const fn new(io: I, controls: ProviderControls, deadline_at_ms: Option<i64>) -> Self {
        Self {
            controls,
            deadline_at_ms,
            io,
        }
    }

    /// Inspect injected transport evidence after one scheduled execution.
    pub const fn io(&self) -> &I {
        &self.io
    }
}

impl<I> ScheduledProvider for RetryingOpenMeteoProvider<I>
where
    I: ProviderIo<OpenMeteoAttemptPayload>,
{
    type Cursor = Option<OpenMeteoCursor>;
    type Record = NormalizedOpenMeteoRecord;
    type Request = ProviderRequest;
    type ResponseMetadata = Option<SerializedJson>;

    /// Execute bounded provider I/O, normalize current weather, and retain evidence.
    fn fetch(
        &mut self,
        request: Self::Request,
    ) -> ScheduledProviderResult<Self::Record, Self::ResponseMetadata, Self::Cursor> {
        // forecasts require the paired transport rather than this single exchange
        if request.source_kind != OpenMeteoSourceKind::ModelCurrent {
            return Err(TaskFailure {
                attempts: 0,
                code: "invalid_payload".to_owned(),
            });
        }
        let response = run_provider_request(
            ProviderRequestPolicy {
                controls: self.controls,
                deadline_at_ms: self.deadline_at_ms,
            },
            &mut self.io,
        )
        .map_err(provider_task_failure)?;
        let payload = response.body;
        let normalized = normalize_open_meteo_current(
            payload.product.envelope,
            payload.product.sample,
            &request.source_id,
            payload.received_at_ms,
            u32::from(response.attempts),
        )?;

        Ok(ProviderBatch {
            attempts: normalized.attempts,
            provider_cursor: Some(normalized.provider_cursor),
            records: normalized.records,
            response_metadata: payload.response_metadata,
            upstream_response_checksum: payload.upstream_response_checksum,
        })
    }
}

// retain exact controller attempts and failure code at the worker boundary
fn provider_task_failure(error: ProviderRunError) -> TaskFailure {
    // distinguish invalid local controls from an attempted provider request
    match error {
        ProviderRunError::InvalidControls(_) => TaskFailure {
            attempts: 1,
            code: "provider_invalid_controls".to_owned(),
        },
        ProviderRunError::Provider(failure) => TaskFailure {
            attempts: u32::from(failure.attempts.max(1)),
            code: failure.error.code,
        },
    }
}

/// Parsed Tempest response plus already-admitted checksum evidence.
#[derive(Clone, Debug, PartialEq)]
pub struct TempestAttemptPayload {
    pub envelope: TempestObservationEnvelope,
    pub received_at_ms: i64,
    pub upstream_response_checksum: String,
}

/// Real Tempest scheduled adapter backed by the shared provider controller.
pub struct RetryingTempestProvider<I> {
    controls: ProviderControls,
    deadline_at_ms: Option<i64>,
    io: I,
}

impl<I> RetryingTempestProvider<I> {
    /// Bind one injected Tempest transport to admitted retry and deadline controls.
    pub const fn new(io: I, controls: ProviderControls, deadline_at_ms: Option<i64>) -> Self {
        Self {
            controls,
            deadline_at_ms,
            io,
        }
    }

    /// Inspect injected Tempest transport evidence after one scheduled execution.
    pub const fn io(&self) -> &I {
        &self.io
    }
}

impl<I> ScheduledProvider for RetryingTempestProvider<I>
where
    I: ProviderIo<TempestAttemptPayload>,
{
    type Cursor = Option<TempestProviderCursor>;
    type Record = NormalizedTempestRecord;
    type Request = TempestObservationRequest;
    type ResponseMetadata = TempestResponseMetadata;

    /// Execute bounded Tempest I/O and normalize its typed observation payload.
    fn fetch(
        &mut self,
        request: Self::Request,
    ) -> ScheduledProviderResult<Self::Record, Self::ResponseMetadata, Self::Cursor> {
        let response = run_provider_request(
            ProviderRequestPolicy {
                controls: self.controls,
                deadline_at_ms: self.deadline_at_ms,
            },
            &mut self.io,
        )
        .map_err(provider_task_failure)?;
        let payload = response.body;
        let normalized = normalize_tempest_observations(
            payload.envelope,
            &request,
            payload.received_at_ms,
            u32::from(response.attempts),
        )?;

        Ok(ProviderBatch {
            attempts: normalized.attempts,
            provider_cursor: normalized
                .provider_cursor_valid_at_ms
                .map(|valid_at_ms| TempestProviderCursor { valid_at_ms }),
            records: normalized.records,
            response_metadata: normalized.response_metadata,
            upstream_response_checksum: payload.upstream_response_checksum,
        })
    }
}

/// parsed NOAA tide response plus already-admitted checksum evidence
#[derive(Clone, Debug, PartialEq)]
pub struct NoaaTideAttemptPayload {
    pub payload: NoaaTidePayload,
    pub received_at_ms: i64,
    pub upstream_response_checksum: String,
}

/// real NOAA tide adapter backed by the shared provider controller
pub struct RetryingNoaaTideProvider<I> {
    controls: ProviderControls,
    deadline_at_ms: Option<i64>,
    io: I,
}

impl<I> RetryingNoaaTideProvider<I> {
    /// bind NOAA transport to source-specific production controls
    pub const fn new(io: I, deadline_at_ms: Option<i64>) -> Self {
        Self::with_controls(
            io,
            ProviderControls {
                max_attempts: DEFAULT_PROVIDER_MAX_ATTEMPTS,
                max_body_bytes: MAX_PROVIDER_BODY_BYTES,
                timeout_ms: DEFAULT_PROVIDER_TIMEOUT_MS,
            },
            deadline_at_ms,
        )
    }

    /// bind NOAA transport to explicit admitted controls
    pub const fn with_controls(
        io: I,
        controls: ProviderControls,
        deadline_at_ms: Option<i64>,
    ) -> Self {
        Self {
            controls,
            deadline_at_ms,
            io,
        }
    }

    /// inspect NOAA transport evidence after scheduled execution
    pub const fn io(&self) -> &I {
        &self.io
    }
}

impl<I> ScheduledProvider for RetryingNoaaTideProvider<I>
where
    I: ProviderIo<NoaaTideAttemptPayload>,
{
    type Cursor = Option<NoaaTideProviderCursor>;
    type Record = NormalizedNoaaTideRecord;
    type Request = NoaaTideRangeRequest;
    type ResponseMetadata = NoaaTideResponseMetadata;

    /// execute bounded NOAA I/O and normalize the selected product
    fn fetch(
        &mut self,
        request: Self::Request,
    ) -> ScheduledProviderResult<Self::Record, Self::ResponseMetadata, Self::Cursor> {
        let response = run_provider_request(
            ProviderRequestPolicy {
                controls: self.controls,
                deadline_at_ms: self.deadline_at_ms,
            },
            &mut self.io,
        )
        .map_err(provider_task_failure)?;
        let payload = response.body;
        let normalized = normalize_noaa_tide_range(
            &payload.payload,
            &request,
            payload.received_at_ms,
            u32::from(response.attempts),
        )?;

        Ok(ProviderBatch {
            attempts: normalized.attempts,
            provider_cursor: normalized
                .provider_cursor_valid_at_ms
                .map(|valid_at_ms| NoaaTideProviderCursor { valid_at_ms }),
            records: normalized.records,
            response_metadata: normalized.response_metadata,
            upstream_response_checksum: payload.upstream_response_checksum,
        })
    }
}

/// parsed Ambient response plus already-admitted checksum evidence
#[derive(Clone, Debug, PartialEq)]
pub struct AmbientWeatherAttemptPayload {
    pub payload: AmbientWeatherPayload,
    pub received_at_ms: i64,
    /// Batch digest admitted over the response's lowercase SHA-256 followed by a newline.
    pub upstream_response_checksum: String,
}

/// real Ambient adapter backed by the shared provider controller
pub struct RetryingAmbientWeatherProvider<I> {
    controls: ProviderControls,
    deadline_at_ms: Option<i64>,
    io: I,
}

impl<I> RetryingAmbientWeatherProvider<I> {
    /// bind Ambient transport to source-specific production controls
    pub const fn new(io: I, deadline_at_ms: Option<i64>) -> Self {
        Self::with_controls(
            io,
            ProviderControls {
                max_attempts: DEFAULT_PROVIDER_MAX_ATTEMPTS,
                max_body_bytes: MAX_PROVIDER_BODY_BYTES,
                timeout_ms: DEFAULT_PROVIDER_TIMEOUT_MS,
            },
            deadline_at_ms,
        )
    }

    /// bind Ambient transport to explicit admitted controls
    pub const fn with_controls(
        io: I,
        controls: ProviderControls,
        deadline_at_ms: Option<i64>,
    ) -> Self {
        Self {
            controls,
            deadline_at_ms,
            io,
        }
    }

    /// inspect Ambient transport evidence after scheduled execution
    pub const fn io(&self) -> &I {
        &self.io
    }
}

impl<I> ScheduledProvider for RetryingAmbientWeatherProvider<I>
where
    I: ProviderIo<AmbientWeatherAttemptPayload>,
{
    type Cursor = Option<AmbientWeatherProviderCursor>;
    type Record = NormalizedAmbientWeatherRecord;
    type Request = AmbientWeatherRangeRequest;
    type ResponseMetadata = AmbientWeatherResponseMetadata;

    /// execute bounded Ambient I/O and normalize device data
    fn fetch(
        &mut self,
        request: Self::Request,
    ) -> ScheduledProviderResult<Self::Record, Self::ResponseMetadata, Self::Cursor> {
        let response = run_provider_request(
            ProviderRequestPolicy {
                controls: self.controls,
                deadline_at_ms: self.deadline_at_ms,
            },
            &mut self.io,
        )
        .map_err(provider_task_failure)?;
        let payload = response.body;
        let normalized = normalize_ambient_weather_range(
            &payload.payload,
            &request,
            payload.received_at_ms,
            u32::from(response.attempts),
        )?;

        Ok(ProviderBatch {
            attempts: normalized.attempts,
            provider_cursor: normalized.provider_cursor,
            records: normalized.records,
            response_metadata: normalized.response_metadata,
            upstream_response_checksum: payload.upstream_response_checksum,
        })
    }
}

/// one typed Ecowitt local gateway request stage
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EcowittProviderStage {
    GatewayIdentity,
    LiveData,
}

/// one parsed product returned by an admitted Ecowitt transport
#[derive(Clone, Debug, PartialEq)]
pub enum EcowittParsedProduct {
    GatewayIdentity {
        gateway_mac: String,
    },
    LiveData {
        combined_upstream_response_checksum: String,
        payload: Box<EcowittLivePayload>,
        received_at_ms: i64,
    },
}

/// parsed Ecowitt response selected for one local gateway stage
#[derive(Clone, Debug, PartialEq)]
pub struct EcowittAttemptPayload {
    pub product: EcowittParsedProduct,
}

/// typed request selection layered over the shared provider controller
pub trait EcowittProviderIo: ProviderIo<EcowittAttemptPayload> {
    /// select one admitted gateway request before its retry loop
    fn select_request(
        &mut self,
        stage: EcowittProviderStage,
        request: &EcowittScheduledRequest,
    ) -> Result<(), TaskFailure>;
}

/// real Ecowitt adapter backed by two source-specific retry loops
pub struct RetryingEcowittProvider<I, T, M> {
    deadline_at_ms: Option<i64>,
    identity_controls: ProviderControls,
    io: I,
    live_controls: ProviderControls,
    metadata_adapter: M,
    time_adapter: T,
}

impl<I, T, M> RetryingEcowittProvider<I, T, M> {
    /// bind Ecowitt transport and admitted normalization adapters
    pub const fn new(
        io: I,
        deadline_at_ms: Option<i64>,
        time_adapter: T,
        metadata_adapter: M,
    ) -> Self {
        Self::with_controls(
            io,
            ProviderControls {
                max_attempts: DEFAULT_PROVIDER_MAX_ATTEMPTS,
                max_body_bytes: 32_768,
                timeout_ms: 5_000,
            },
            ProviderControls {
                max_attempts: DEFAULT_PROVIDER_MAX_ATTEMPTS,
                max_body_bytes: 262_144,
                timeout_ms: 5_000,
            },
            deadline_at_ms,
            time_adapter,
            metadata_adapter,
        )
    }

    /// bind Ecowitt transport to explicit admitted controls
    pub const fn with_controls(
        io: I,
        identity_controls: ProviderControls,
        live_controls: ProviderControls,
        deadline_at_ms: Option<i64>,
        time_adapter: T,
        metadata_adapter: M,
    ) -> Self {
        Self {
            deadline_at_ms,
            identity_controls,
            io,
            live_controls,
            metadata_adapter,
            time_adapter,
        }
    }

    /// inspect Ecowitt transport evidence after scheduled execution
    pub const fn io(&self) -> &I {
        &self.io
    }
}

impl<I, T, M> ScheduledProvider for RetryingEcowittProvider<I, T, M>
where
    I: EcowittProviderIo,
    M: EcowittCanonicalMetadataAdapter,
    T: EcowittTimeAdapter,
{
    type Cursor = EcowittProviderCursor;
    type Record = NormalizedEcowittRecord;
    type Request = EcowittScheduledRequest;
    type ResponseMetadata = EcowittResponseMetadata;

    /// execute identity and live retries before typed Ecowitt normalization
    fn fetch(
        &mut self,
        request: Self::Request,
    ) -> ScheduledProviderResult<Self::Record, Self::ResponseMetadata, Self::Cursor> {
        self.io
            .select_request(EcowittProviderStage::GatewayIdentity, &request)?;
        let identity = run_provider_request(
            ProviderRequestPolicy {
                controls: self.identity_controls,
                deadline_at_ms: self.deadline_at_ms,
            },
            &mut self.io,
        )
        .map_err(provider_task_failure)?;
        // require the identity endpoint's typed product
        let gateway_mac = match identity.body.product {
            EcowittParsedProduct::GatewayIdentity { gateway_mac } => gateway_mac,
            EcowittParsedProduct::LiveData { .. } => {
                return Err(TaskFailure {
                    attempts: u32::from(identity.attempts),
                    code: "invalid_payload".to_owned(),
                });
            }
        };

        require_ecowitt_gateway_identity(&gateway_mac, &request.expected_mac)?;
        self.io
            .select_request(EcowittProviderStage::LiveData, &request)?;
        let live = run_provider_request(
            ProviderRequestPolicy {
                controls: self.live_controls,
                deadline_at_ms: self.deadline_at_ms,
            },
            &mut self.io,
        )
        .map_err(provider_task_failure)?;
        // require the live endpoint's typed product
        let (payload, received_at_ms, upstream_response_checksum) = match live.body.product {
            EcowittParsedProduct::LiveData {
                combined_upstream_response_checksum,
                payload,
                received_at_ms,
            } => (
                *payload,
                received_at_ms,
                combined_upstream_response_checksum,
            ),
            EcowittParsedProduct::GatewayIdentity { .. } => {
                return Err(TaskFailure {
                    attempts: u32::from(live.attempts),
                    code: "invalid_payload".to_owned(),
                });
            }
        };
        // retain prior rain counter state for interval normalization
        let previous_cursor =
            request
                .previous_cursor
                .as_ref()
                .map(|cursor| EcowittPreviousCursor {
                    rain_daily_total_mm: cursor.rain_daily_total_mm,
                    rain_day: Some(cursor.rain_day.clone()),
                });
        let attempts = u32::from(identity.attempts) + u32::from(live.attempts);
        let normalized = normalize_ecowitt_live(
            &gateway_mac,
            &payload,
            &EcowittLiveRequest {
                expected_mac: request.expected_mac,
                model: request.model,
                previous_cursor,
                source_id: request.source_id,
                timezone: request.timezone,
            },
            received_at_ms,
            attempts,
            &mut self.time_adapter,
            &mut self.metadata_adapter,
        )?;

        Ok(ProviderBatch {
            attempts: normalized.attempts,
            provider_cursor: normalized.provider_cursor,
            records: normalized.records,
            response_metadata: normalized.response_metadata,
            upstream_response_checksum,
        })
    }
}

/// one runtime time value crossing the admitted time boundary
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeTimeValue {
    AbandonAsOf,
    CheckpointWindowEnd,
    Deadline,
    LastValidAt,
    ProductRunAt,
    ReceivedAt,
    RequestedEndExclusive,
    RequestedStart,
    ValidAt,
}

/// one persistence operation whose adapter failure needs worker classification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimePersistenceOperation {
    CheckpointRead,
    Complete,
    DueSourceDiscovery,
    FailureFinalize,
    Heartbeat,
    Start,
}

/// admitted time, JSON, hashing, and failure-classification boundaries
pub trait OpenMeteoRuntimeValues {
    /// serialize one canonical UTC instant without implementing a time codec here
    fn utc_instant(
        &mut self,
        milliseconds: i64,
        value: RuntimeTimeValue,
    ) -> Result<String, TaskFailure>;

    /// decode one canonical UTC instant without implementing a time codec here
    fn epoch_milliseconds(
        &mut self,
        instant: &str,
        value: RuntimeTimeValue,
    ) -> Result<i64, TaskFailure>;

    /// serialize one generated JSON object without implementing JSON here
    fn serialize_json_object(
        &mut self,
        value: &[(Vec<u16>, JsonValue)],
    ) -> Result<SerializedJson, TaskFailure>;

    /// serialize the exact OpenMeteo endpoint request evidence
    fn request_metadata_json(&mut self, endpoint: &str) -> Result<SerializedJson, TaskFailure>;

    /// map one worker cursor to exact durable JSON
    fn provider_cursor_json(
        &mut self,
        cursor: Option<&OpenMeteoCursor>,
    ) -> Result<Option<SerializedJson>, TaskFailure>;

    /// map durable checkpoint JSON back to the worker cursor
    fn checkpoint_cursor(
        &mut self,
        checkpoint: &ScheduledCheckpointState,
    ) -> Result<Option<OpenMeteoCursor>, TaskFailure>;

    /// calculate the canonical normalized-record content hash
    fn weather_record_content_hash(
        &mut self,
        record: &NormalizedWeatherRecord,
    ) -> Result<Sha256Digest, TaskFailure>;

    /// admit one externally calculated SHA-256 value
    fn sha256_digest(&mut self, value: &str) -> Result<Sha256Digest, TaskFailure>;

    /// classify one worker failure through the shared domain boundary
    fn ingestion_error(
        &mut self,
        failure: &TaskFailure,
    ) -> Result<AdmittedIngestionError, TaskFailure>;

    /// classify one typed database adapter error for the worker result
    fn persistence_failure(
        &mut self,
        operation: RuntimePersistenceOperation,
        message: &str,
    ) -> TaskFailure;
}

/// admitted Tempest cursor mappings layered on the shared runtime values
pub trait TempestRuntimeValues: OpenMeteoRuntimeValues {
    /// map one typed Tempest cursor to the exact durable JSON contract
    fn tempest_provider_cursor_json(
        &mut self,
        cursor: Option<&TempestProviderCursor>,
    ) -> Result<Option<SerializedJson>, TaskFailure>;

    /// decode one exact durable Tempest cursor without implementing JSON here
    fn tempest_checkpoint_cursor(
        &mut self,
        checkpoint: &ScheduledCheckpointState,
    ) -> Result<Option<TempestProviderCursor>, TaskFailure>;
}

/// admitted NOAA tide cursor mappings layered on the shared runtime values
pub trait NoaaTideRuntimeValues: OpenMeteoRuntimeValues {
    /// map one typed NOAA cursor to the exact durable JSON contract
    fn noaa_tide_provider_cursor_json(
        &mut self,
        cursor: Option<&NoaaTideProviderCursor>,
    ) -> Result<Option<SerializedJson>, TaskFailure>;

    /// decode one exact durable NOAA cursor without implementing JSON here
    fn noaa_tide_checkpoint_cursor(
        &mut self,
        checkpoint: &ScheduledCheckpointState,
    ) -> Result<Option<NoaaTideProviderCursor>, TaskFailure>;
}

/// admitted Ambient cursor mappings layered on the shared runtime values
pub trait AmbientWeatherRuntimeValues: OpenMeteoRuntimeValues {
    /// map one typed Ambient cursor to the exact durable JSON contract
    fn ambient_weather_provider_cursor_json(
        &mut self,
        cursor: Option<&AmbientWeatherProviderCursor>,
    ) -> Result<Option<SerializedJson>, TaskFailure>;

    /// decode one exact durable Ambient cursor without implementing JSON here
    fn ambient_weather_checkpoint_cursor(
        &mut self,
        checkpoint: &ScheduledCheckpointState,
    ) -> Result<Option<AmbientWeatherProviderCursor>, TaskFailure>;
}

/// admitted Ecowitt cursor mappings layered on the shared runtime values
pub trait EcowittRuntimeValues: OpenMeteoRuntimeValues {
    /// map one typed Ecowitt rain-counter cursor to exact durable JSON
    fn ecowitt_provider_cursor_json(
        &mut self,
        cursor: &EcowittProviderCursor,
    ) -> Result<Option<SerializedJson>, TaskFailure>;

    /// decode one exact durable Ecowitt cursor without implementing JSON here
    fn ecowitt_checkpoint_cursor(
        &mut self,
        checkpoint: &ScheduledCheckpointState,
    ) -> Result<EcowittProviderCursor, TaskFailure>;
}

/// Scheduled completion failure from either the retained pool or its transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerScheduledCompletionError<CommandError> {
    Transaction(ScheduledIngestionExecutionError<CommandError>),
    Task(TaskFailure),
}

impl<CommandError: Display> Display for WorkerScheduledCompletionError<CommandError> {
    /// Preserve transaction diagnostics and the pool-owned task category.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // keep the repository's existing persistence-failure message source
        match self {
            Self::Transaction(error) => Display::fmt(error, formatter),
            Self::Task(error) => formatter.write_str(&error.code),
        }
    }
}

/// source-session acquisition, expiry, and release boundary
pub trait OpenMeteoDatabaseBoundary {
    type Session: ScheduledCheckpointReadPort
        + ScheduledRunStartCommandPort
        + ScheduledIngestionCommandPort
        + ScheduledRunFailureCommandPort;

    /// acquire one retained source-scoped database session
    fn acquire_source_session(
        &mut self,
        source_id: &str,
    ) -> Result<Option<Self::Session>, TaskFailure>;

    /// abandon expired work using an admitted canonical instant
    fn abandon_expired_runs(
        &mut self,
        session: &mut Self::Session,
        as_of: &str,
    ) -> Result<(), TaskFailure>;

    /// Complete one scheduled transaction through the configured archive strategy.
    fn complete_scheduled_ingestion(
        &mut self,
        input: &DatabaseScheduledCompletion,
        session: &mut Self::Session,
    ) -> Result<
        (),
        WorkerScheduledCompletionError<<Self::Session as ScheduledIngestionCommandPort>::Error>,
    > {
        // preserve the archive-disabled path until a database boundary opts into callbacks
        weather_database::complete_scheduled_ingestion(input, session)
            .map_err(WorkerScheduledCompletionError::Transaction)
    }

    /// release the retained source-scoped database session
    fn release_source_session(&mut self, session: Self::Session) -> Result<(), TaskFailure>;
}

/// provider-specific record and evidence projection for shared scheduled persistence
pub trait ScheduledDatabaseAdapter<Values: OpenMeteoRuntimeValues> {
    type Cursor;
    type Record;
    type ResponseMetadata;

    /// decode one provider cursor from the shared checkpoint row
    fn checkpoint_cursor(
        values: &mut Values,
        checkpoint: &ScheduledCheckpointState,
    ) -> Result<Self::Cursor, TaskFailure>;

    /// project one normalized provider record into database storage evidence
    fn project_record(
        values: &mut Values,
        record: Self::Record,
    ) -> Result<PreparedWeatherRecord, TaskFailure>;

    /// encode one provider cursor through the admitted runtime boundary
    fn provider_cursor_json(
        values: &mut Values,
        cursor: &Self::Cursor,
    ) -> Result<Option<SerializedJson>, TaskFailure>;

    /// encode one provider response metadata object for successful finalization
    fn response_metadata_json(
        values: &mut Values,
        metadata: &Self::ResponseMetadata,
    ) -> Result<Option<SerializedJson>, TaskFailure>;
}

/// typed database commands required by the ordinary worker drain
pub trait WorkerIterationDatabasePort {
    type Error: Display;

    /// return ready sources in the database's durable order
    fn discover_due_sources(&mut self, as_of_ms: i64) -> Result<Vec<WorkerDueSource>, Self::Error>;

    /// persist one completed iteration heartbeat
    fn update_worker_heartbeat(&mut self, heartbeat: WorkerHeartbeat) -> Result<(), Self::Error>;
}

/// runtime adapter joining typed worker drain commands to persistence
pub struct WorkerIterationDatabaseRepository<'a, Database, Values> {
    database: &'a mut Database,
    values: &'a mut Values,
}

impl<'a, Database, Values> WorkerIterationDatabaseRepository<'a, Database, Values> {
    /// bind admitted worker iteration database and value boundaries
    pub const fn new(database: &'a mut Database, values: &'a mut Values) -> Self {
        Self { database, values }
    }

    /// Let one configured sidecar use the retained pool and canonical values without another checkout.
    pub fn with_retained_resources<ResultValue>(
        &mut self,
        task: impl FnOnce(&mut Database, &mut Values) -> ResultValue,
    ) -> ResultValue {
        task(self.database, self.values)
    }

    /// Reborrow the same pool and value owner for one scheduled source transaction.
    pub(crate) fn scheduled_resources(&mut self) -> (&mut Database, &mut Values) {
        (self.database, self.values)
    }
}

impl<Database, Values> HeartbeatRepository
    for WorkerIterationDatabaseRepository<'_, Database, Values>
where
    Database: WorkerIterationDatabasePort,
    Values: OpenMeteoRuntimeValues,
{
    /// persist one heartbeat through the typed database port
    fn update_worker_heartbeat(&mut self, heartbeat: WorkerHeartbeat) -> Result<(), TaskFailure> {
        self.database
            .update_worker_heartbeat(heartbeat)
            .map_err(|error| {
                self.values
                    .persistence_failure(RuntimePersistenceOperation::Heartbeat, &error.to_string())
            })
    }
}

impl<Database, Values> WorkerIterationRepository
    for WorkerIterationDatabaseRepository<'_, Database, Values>
where
    Database: WorkerIterationDatabasePort,
    Values: OpenMeteoRuntimeValues,
{
    /// discover ready sources through the typed database port
    fn discover_due_sources(&mut self, as_of_ms: i64) -> Result<Vec<WorkerDueSource>, TaskFailure> {
        self.database
            .discover_due_sources(as_of_ms)
            .map_err(|error| {
                self.values.persistence_failure(
                    RuntimePersistenceOperation::DueSourceDiscovery,
                    &error.to_string(),
                )
            })
    }
}

/// OpenMeteo projection marker for the shared scheduled repository
pub struct OpenMeteoDatabaseAdapter;

/// Tempest projection marker for the shared scheduled repository
pub struct TempestDatabaseAdapter;

/// NOAA tide projection marker for the shared scheduled repository
pub struct NoaaTideDatabaseAdapter;

/// Ambient Weather projection marker for the shared scheduled repository
pub struct AmbientWeatherDatabaseAdapter;

/// Ecowitt projection marker for the shared scheduled repository
pub struct EcowittDatabaseAdapter;

/// one shared worker-to-database scheduled repository adapter
pub struct ScheduledDatabaseRepository<'a, Database, Values, Adapter> {
    database: &'a mut Database,
    values: &'a mut Values,
    adapter: PhantomData<Adapter>,
}

impl<'a, Database, Values, Adapter> ScheduledDatabaseRepository<'a, Database, Values, Adapter> {
    /// bind the admitted database and value boundaries
    pub fn new(database: &'a mut Database, values: &'a mut Values) -> Self {
        Self {
            database,
            values,
            adapter: PhantomData,
        }
    }
}

/// the OpenMeteo specialization of the shared scheduled repository
pub type OpenMeteoDatabaseRepository<'a, Database, Values> =
    ScheduledDatabaseRepository<'a, Database, Values, OpenMeteoDatabaseAdapter>;

/// the Tempest specialization of the shared scheduled repository
pub type TempestDatabaseRepository<'a, Database, Values> =
    ScheduledDatabaseRepository<'a, Database, Values, TempestDatabaseAdapter>;

/// the NOAA tide specialization of the shared scheduled repository
pub type NoaaTideDatabaseRepository<'a, Database, Values> =
    ScheduledDatabaseRepository<'a, Database, Values, NoaaTideDatabaseAdapter>;

/// the Ambient Weather specialization of the shared scheduled repository
pub type AmbientWeatherDatabaseRepository<'a, Database, Values> =
    ScheduledDatabaseRepository<'a, Database, Values, AmbientWeatherDatabaseAdapter>;

/// the Ecowitt specialization of the shared scheduled repository
pub type EcowittDatabaseRepository<'a, Database, Values> =
    ScheduledDatabaseRepository<'a, Database, Values, EcowittDatabaseAdapter>;

impl<Database, Values, Adapter>
    ScheduledRunRepository<Adapter::Record, Adapter::ResponseMetadata, Adapter::Cursor>
    for ScheduledDatabaseRepository<'_, Database, Values, Adapter>
where
    Database: OpenMeteoDatabaseBoundary,
    Values: OpenMeteoRuntimeValues,
    Adapter: ScheduledDatabaseAdapter<Values>,
    <Database::Session as ScheduledCheckpointReadPort>::Error: Display,
    <Database::Session as ScheduledRunStartCommandPort>::Error: Display,
    <Database::Session as ScheduledIngestionCommandPort>::Error: Display,
    <Database::Session as ScheduledRunFailureCommandPort>::Error: Display,
{
    type Session = Database::Session;

    /// acquire the injected retained source session
    fn acquire_source_session(
        &mut self,
        source_id: &str,
    ) -> Result<Option<Self::Session>, TaskFailure> {
        let session = self.database.acquire_source_session(source_id)?;

        // reject an incorrectly retained source before any session command
        match session {
            Some(session) if session.source_id() != source_id => {
                // release the unusable resource without changing the identity failure
                let _release = self.database.release_source_session(session);
                Err(TaskFailure {
                    attempts: 0,
                    code: "source_session_mismatch".to_owned(),
                })
            }
            session => Ok(session),
        }
    }

    /// convert the worker clock only at the admitted time boundary
    fn abandon_expired_runs(
        &mut self,
        session: &mut Self::Session,
        now_ms: i64,
    ) -> Result<(), TaskFailure> {
        let as_of = self
            .values
            .utc_instant(now_ms, RuntimeTimeValue::AbandonAsOf)?;
        self.database.abandon_expired_runs(session, &as_of)
    }

    /// read and map the exact durable scheduled checkpoint
    fn scheduled_checkpoint(
        &mut self,
        session: &mut Self::Session,
    ) -> Result<Option<ScheduledCheckpoint<Adapter::Cursor>>, TaskFailure> {
        let checkpoint = get_scheduled_checkpoint(session).map_err(|error| {
            self.values.persistence_failure(
                RuntimePersistenceOperation::CheckpointRead,
                &error.to_string(),
            )
        })?;

        // preserve the uninitialized checkpoint as none
        match checkpoint {
            None => Ok(None),
            Some(checkpoint) => Ok(Some(ScheduledCheckpoint {
                provider_cursor: Adapter::checkpoint_cursor(self.values, &checkpoint)?,
                version: checkpoint_version(checkpoint.version)?,
                window_end_exclusive_ms: self.values.epoch_milliseconds(
                    &checkpoint.windowEndExclusive,
                    RuntimeTimeValue::CheckpointWindowEnd,
                )?,
            })),
        }
    }

    /// commit the exact scheduled running row before provider traffic
    fn start_ingestion_run(
        &mut self,
        session: &mut Self::Session,
        start: ScheduledRunStart,
    ) -> Result<String, TaskFailure> {
        let input = StartScheduledIngestion {
            adapter_version: AdmittedAdapterVersion::admitted(start.adapter_version),
            attempts: 0,
            request_metadata_json: Some(self.values.request_metadata_json(start.endpoint)?),
            source_config_fingerprint: self
                .values
                .sha256_digest(&start.source_config_fingerprint)?,
            times: AdmittedScheduledRunTimes::admitted(
                self.values.utc_instant(
                    start.requested_window.start_ms,
                    RuntimeTimeValue::RequestedStart,
                )?,
                self.values.utc_instant(
                    start.requested_window.end_exclusive_ms,
                    RuntimeTimeValue::RequestedEndExclusive,
                )?,
                self.values
                    .utc_instant(start.deadline_at_ms, RuntimeTimeValue::Deadline)?,
            ),
        };
        start_scheduled_ingestion(&input, session)
            .map(|row| row.id)
            .map_err(|error| {
                self.values
                    .persistence_failure(RuntimePersistenceOperation::Start, &error.to_string())
            })
    }

    /// project real normalized records into one atomic database completion
    fn complete_scheduled_ingestion(
        &mut self,
        session: &mut Self::Session,
        completion: ScheduledCompletion<
            Adapter::Record,
            Adapter::ResponseMetadata,
            Adapter::Cursor,
        >,
    ) -> Result<(), TaskFailure> {
        let mut records = Vec::with_capacity(completion.records.len());

        // preserve provider order through the storage bind plan
        for record in completion.records {
            records.push(Adapter::project_record(self.values, record)?);
        }

        let provider_cursor_json =
            Adapter::provider_cursor_json(self.values, &completion.provider_cursor)?;
        let response_metadata_json =
            Adapter::response_metadata_json(self.values, &completion.response_metadata)?;

        let input = DatabaseScheduledCompletion {
            attempts: u64::from(completion.attempts),
            expected_checkpoint_version: completion.expected_checkpoint_version,
            last_valid_at: self
                .values
                .utc_instant(completion.last_valid_at_ms, RuntimeTimeValue::LastValidAt)?,
            provider_cursor_json,
            records,
            response_metadata_json,
            run_id: completion.run_id,
            upstream_response_checksum: Some(
                self.values
                    .sha256_digest(&completion.upstream_response_checksum)?,
            ),
            window_end_exclusive: self.values.utc_instant(
                completion.window.end_exclusive_ms,
                RuntimeTimeValue::RequestedEndExclusive,
            )?,
            window_start: self
                .values
                .utc_instant(completion.window.start_ms, RuntimeTimeValue::RequestedStart)?,
        };
        self.database
            .complete_scheduled_ingestion(&input, session)
            .map_err(|error| {
                self.values
                    .persistence_failure(RuntimePersistenceOperation::Complete, &error.to_string())
            })
    }

    /// finalize one worker failure through the admitted domain classifier
    fn fail_ingestion_run(
        &mut self,
        session: &mut Self::Session,
        run_id: &str,
        failure: &TaskFailure,
    ) -> Result<(), TaskFailure> {
        let input = FailScheduledIngestion {
            attempts: u64::from(failure.attempts),
            error: self.values.ingestion_error(failure)?,
            response_metadata_json: None,
            run_id: run_id.to_owned(),
        };
        fail_scheduled_ingestion(&input, session).map_err(|error| {
            self.values.persistence_failure(
                RuntimePersistenceOperation::FailureFinalize,
                &error.to_string(),
            )
        })
    }

    /// release the retained source session after the primary outcome
    fn release_source_session(&mut self, session: Self::Session) -> Result<(), TaskFailure> {
        self.database.release_source_session(session)
    }
}

impl<Values: OpenMeteoRuntimeValues> ScheduledDatabaseAdapter<Values> for OpenMeteoDatabaseAdapter {
    type Cursor = Option<OpenMeteoCursor>;
    type Record = NormalizedOpenMeteoRecord;
    type ResponseMetadata = Option<SerializedJson>;

    /// decode the established OpenMeteo cursor contract
    fn checkpoint_cursor(
        values: &mut Values,
        checkpoint: &ScheduledCheckpointState,
    ) -> Result<Self::Cursor, TaskFailure> {
        values.checkpoint_cursor(checkpoint)
    }

    /// project one OpenMeteo record through the existing storage contract
    fn project_record(
        values: &mut Values,
        record: Self::Record,
    ) -> Result<PreparedWeatherRecord, TaskFailure> {
        project_open_meteo_record(record, values)
    }

    /// encode the established OpenMeteo cursor contract
    fn provider_cursor_json(
        values: &mut Values,
        cursor: &Self::Cursor,
    ) -> Result<Option<SerializedJson>, TaskFailure> {
        values.provider_cursor_json(cursor.as_ref())
    }

    /// retain already-admitted OpenMeteo response metadata
    fn response_metadata_json(
        _values: &mut Values,
        metadata: &Self::ResponseMetadata,
    ) -> Result<Option<SerializedJson>, TaskFailure> {
        Ok(metadata.clone())
    }
}

impl<Values: TempestRuntimeValues> ScheduledDatabaseAdapter<Values> for TempestDatabaseAdapter {
    type Cursor = Option<TempestProviderCursor>;
    type Record = NormalizedTempestRecord;
    type ResponseMetadata = TempestResponseMetadata;

    /// decode the typed Tempest cursor at the admitted JSON boundary
    fn checkpoint_cursor(
        values: &mut Values,
        checkpoint: &ScheduledCheckpointState,
    ) -> Result<Self::Cursor, TaskFailure> {
        values.tempest_checkpoint_cursor(checkpoint)
    }

    /// project one Tempest record and its physical-sensor provenance
    fn project_record(
        values: &mut Values,
        record: Self::Record,
    ) -> Result<PreparedWeatherRecord, TaskFailure> {
        project_tempest_record(record, values)
    }

    /// encode the typed Tempest cursor at the admitted JSON boundary
    fn provider_cursor_json(
        values: &mut Values,
        cursor: &Self::Cursor,
    ) -> Result<Option<SerializedJson>, TaskFailure> {
        values.tempest_provider_cursor_json(cursor.as_ref())
    }

    /// encode exact Tempest response diagnostics in TypeScript field order
    fn response_metadata_json(
        values: &mut Values,
        metadata: &Self::ResponseMetadata,
    ) -> Result<Option<SerializedJson>, TaskFailure> {
        values
            .serialize_json_object(&tempest_response_metadata(metadata))
            .map(Some)
    }
}

impl<Values: NoaaTideRuntimeValues> ScheduledDatabaseAdapter<Values> for NoaaTideDatabaseAdapter {
    type Cursor = Option<NoaaTideProviderCursor>;
    type Record = NormalizedNoaaTideRecord;
    type ResponseMetadata = NoaaTideResponseMetadata;

    /// decode the typed NOAA cursor at the admitted JSON boundary
    fn checkpoint_cursor(
        values: &mut Values,
        checkpoint: &ScheduledCheckpointState,
    ) -> Result<Self::Cursor, TaskFailure> {
        values.noaa_tide_checkpoint_cursor(checkpoint)
    }

    /// project one NOAA observation or prediction record
    fn project_record(
        values: &mut Values,
        record: Self::Record,
    ) -> Result<PreparedWeatherRecord, TaskFailure> {
        project_noaa_tide_record(record, values)
    }

    /// encode the typed NOAA cursor at the admitted JSON boundary
    fn provider_cursor_json(
        values: &mut Values,
        cursor: &Self::Cursor,
    ) -> Result<Option<SerializedJson>, TaskFailure> {
        values.noaa_tide_provider_cursor_json(cursor.as_ref())
    }

    /// encode exact NOAA response diagnostics in TypeScript field order
    fn response_metadata_json(
        values: &mut Values,
        metadata: &Self::ResponseMetadata,
    ) -> Result<Option<SerializedJson>, TaskFailure> {
        values
            .serialize_json_object(&noaa_tide_response_metadata(metadata))
            .map(Some)
    }
}

impl<Values: AmbientWeatherRuntimeValues> ScheduledDatabaseAdapter<Values>
    for AmbientWeatherDatabaseAdapter
{
    type Cursor = Option<AmbientWeatherProviderCursor>;
    type Record = NormalizedAmbientWeatherRecord;
    type ResponseMetadata = AmbientWeatherResponseMetadata;

    /// decode the typed Ambient cursor at the admitted JSON boundary
    fn checkpoint_cursor(
        values: &mut Values,
        checkpoint: &ScheduledCheckpointState,
    ) -> Result<Self::Cursor, TaskFailure> {
        values.ambient_weather_checkpoint_cursor(checkpoint)
    }

    /// project one Ambient physical-sensor record
    fn project_record(
        values: &mut Values,
        record: Self::Record,
    ) -> Result<PreparedWeatherRecord, TaskFailure> {
        project_ambient_weather_record(record, values)
    }

    /// encode the typed Ambient cursor at the admitted JSON boundary
    fn provider_cursor_json(
        values: &mut Values,
        cursor: &Self::Cursor,
    ) -> Result<Option<SerializedJson>, TaskFailure> {
        values.ambient_weather_provider_cursor_json(cursor.as_ref())
    }

    /// encode exact Ambient response diagnostics in TypeScript field order
    fn response_metadata_json(
        values: &mut Values,
        metadata: &Self::ResponseMetadata,
    ) -> Result<Option<SerializedJson>, TaskFailure> {
        values
            .serialize_json_object(&ambient_weather_response_metadata(metadata))
            .map(Some)
    }
}

impl<Values: EcowittRuntimeValues> ScheduledDatabaseAdapter<Values> for EcowittDatabaseAdapter {
    type Cursor = EcowittProviderCursor;
    type Record = NormalizedEcowittRecord;
    type ResponseMetadata = EcowittResponseMetadata;

    /// decode the typed Ecowitt rain-counter cursor at the admitted JSON boundary
    fn checkpoint_cursor(
        values: &mut Values,
        checkpoint: &ScheduledCheckpointState,
    ) -> Result<Self::Cursor, TaskFailure> {
        values.ecowitt_checkpoint_cursor(checkpoint)
    }

    /// project one Ecowitt physical-sensor record
    fn project_record(
        values: &mut Values,
        record: Self::Record,
    ) -> Result<PreparedWeatherRecord, TaskFailure> {
        project_ecowitt_record(record, values)
    }

    /// encode the typed Ecowitt rain-counter cursor at the admitted JSON boundary
    fn provider_cursor_json(
        values: &mut Values,
        cursor: &Self::Cursor,
    ) -> Result<Option<SerializedJson>, TaskFailure> {
        values.ecowitt_provider_cursor_json(cursor)
    }

    /// encode exact Ecowitt response diagnostics in TypeScript field order
    fn response_metadata_json(
        values: &mut Values,
        metadata: &Self::ResponseMetadata,
    ) -> Result<Option<SerializedJson>, TaskFailure> {
        values
            .serialize_json_object(&ecowitt_response_metadata(metadata))
            .map(Some)
    }
}

/// project one worker record to the generated database contract
fn project_open_meteo_record<Values: OpenMeteoRuntimeValues>(
    record: NormalizedOpenMeteoRecord,
    values: &mut Values,
) -> Result<PreparedWeatherRecord, TaskFailure> {
    let provider = open_meteo_provider_metadata(&record.provenance);
    let provider_metadata_json = values.serialize_json_object(&provider)?;
    let product_run_at = match record.product_run_at_ms {
        None => Nullable::Null,
        Some(milliseconds) => {
            Nullable::Value(values.utc_instant(milliseconds, RuntimeTimeValue::ProductRunAt)?)
        }
    };
    let normalized = NormalizedWeatherRecord {
        metadata: WeatherRecordMetadata {
            device: Nullable::<DeviceMetadata>::Null,
            model: Nullable::Value(record.provenance.model.to_owned()),
            provider: Nullable::Value(provider),
            quality: Nullable::Null,
            upstreamTimezone: record.provenance.upstream_timezone,
        },
        metrics: project_metrics(record.metrics),
        productRunAt: product_run_at,
        receivedAt: values.utc_instant(record.received_at_ms, RuntimeTimeValue::ReceivedAt)?,
        sourceId: record.source_id,
        sourceKind: project_source_kind(record.source_kind),
        validAt: values.utc_instant(record.valid_at_ms, RuntimeTimeValue::ValidAt)?,
    };
    let content_hash = values.weather_record_content_hash(&normalized)?;

    Ok(PreparedWeatherRecord {
        record: normalized,
        storage: WeatherRecordStorageEvidence {
            content_hash,
            provider_metadata_json: Some(provider_metadata_json),
            quality_metadata_json: None,
        },
    })
}

/// project one Tempest record to the generated database contract
fn project_tempest_record<Values: OpenMeteoRuntimeValues>(
    record: NormalizedTempestRecord,
    values: &mut Values,
) -> Result<PreparedWeatherRecord, TaskFailure> {
    let provider = tempest_provider_metadata(&record.provider);
    let provider_metadata_json = values.serialize_json_object(&provider)?;
    let quality = tempest_quality_metadata(&record.quality);
    let quality_metadata_json = values.serialize_json_object(&quality)?;
    let product_run_at = match record.product_run_at_ms {
        // preserve physical-sensor product-run nullability
        None => Nullable::Null,
        Some(milliseconds) => {
            Nullable::Value(values.utc_instant(milliseconds, RuntimeTimeValue::ProductRunAt)?)
        }
    };
    let normalized = NormalizedWeatherRecord {
        metadata: WeatherRecordMetadata {
            device: Nullable::Value(DeviceMetadata {
                model: FieldPresence::Value(record.device.model.to_owned()),
                serial: FieldPresence::Value(record.device.serial),
                vendor: FieldPresence::Value(record.device.vendor.to_owned()),
            }),
            model: nullable(record.model),
            provider: Nullable::Value(provider),
            quality: Nullable::Value(quality),
            upstreamTimezone: record.upstream_timezone,
        },
        metrics: project_metrics(record.metrics),
        productRunAt: product_run_at,
        receivedAt: values.utc_instant(record.received_at_ms, RuntimeTimeValue::ReceivedAt)?,
        sourceId: record.source_id,
        sourceKind: project_tempest_source_kind(record.source_kind),
        validAt: values.utc_instant(record.valid_at_ms, RuntimeTimeValue::ValidAt)?,
    };
    let content_hash = values.weather_record_content_hash(&normalized)?;

    Ok(PreparedWeatherRecord {
        record: normalized,
        storage: WeatherRecordStorageEvidence {
            content_hash,
            provider_metadata_json: Some(provider_metadata_json),
            quality_metadata_json: Some(quality_metadata_json),
        },
    })
}

/// project one NOAA tide record to the generated database contract
fn project_noaa_tide_record<Values: OpenMeteoRuntimeValues>(
    record: NormalizedNoaaTideRecord,
    values: &mut Values,
) -> Result<PreparedWeatherRecord, TaskFailure> {
    let provider = noaa_tide_provider_metadata(&record.provider)?;
    let provider_metadata_json = values.serialize_json_object(&provider)?;
    let quality = noaa_tide_quality_metadata(&record.quality);
    let quality_metadata_json = values.serialize_json_object(&quality)?;
    let product_run_at = match record.product_run_at_ms {
        // preserve NOAA product-run nullability
        None => Nullable::Null,
        Some(milliseconds) => {
            Nullable::Value(values.utc_instant(milliseconds, RuntimeTimeValue::ProductRunAt)?)
        }
    };
    let normalized = NormalizedWeatherRecord {
        metadata: WeatherRecordMetadata {
            device: Nullable::Value(DeviceMetadata {
                model: FieldPresence::Value(record.device.model.to_owned()),
                serial: FieldPresence::Value(record.device.serial),
                vendor: FieldPresence::Value(record.device.vendor.to_owned()),
            }),
            model: nullable(record.model),
            provider: Nullable::Value(provider),
            quality: Nullable::Value(quality),
            upstreamTimezone: record.upstream_timezone,
        },
        metrics: project_metrics(record.metrics),
        productRunAt: product_run_at,
        receivedAt: values.utc_instant(record.received_at_ms, RuntimeTimeValue::ReceivedAt)?,
        sourceId: record.source_id,
        sourceKind: project_noaa_tide_source_kind(record.source_kind),
        validAt: values.utc_instant(record.valid_at_ms, RuntimeTimeValue::ValidAt)?,
    };
    let content_hash = values.weather_record_content_hash(&normalized)?;

    Ok(PreparedWeatherRecord {
        record: normalized,
        storage: WeatherRecordStorageEvidence {
            content_hash,
            provider_metadata_json: Some(provider_metadata_json),
            quality_metadata_json: Some(quality_metadata_json),
        },
    })
}

/// project one Ambient Weather record to the generated database contract
fn project_ambient_weather_record<Values: OpenMeteoRuntimeValues>(
    record: NormalizedAmbientWeatherRecord,
    values: &mut Values,
) -> Result<PreparedWeatherRecord, TaskFailure> {
    let provider = ambient_weather_provider_metadata(&record.provider);
    let provider_metadata_json = values.serialize_json_object(&provider)?;
    let quality = ambient_weather_quality_metadata(&record.quality);
    let quality_metadata_json = values.serialize_json_object(&quality)?;
    let product_run_at = match record.product_run_at_ms {
        // preserve physical-sensor product-run nullability
        None => Nullable::Null,
        Some(milliseconds) => {
            Nullable::Value(values.utc_instant(milliseconds, RuntimeTimeValue::ProductRunAt)?)
        }
    };
    let normalized = NormalizedWeatherRecord {
        metadata: WeatherRecordMetadata {
            device: Nullable::Value(DeviceMetadata {
                model: field_presence(record.device.model),
                serial: field_presence(record.device.serial),
                vendor: FieldPresence::Value(record.device.vendor.to_owned()),
            }),
            model: nullable(record.model),
            provider: Nullable::Value(provider),
            quality: Nullable::Value(quality),
            upstreamTimezone: record.upstream_timezone,
        },
        metrics: project_metrics(record.metrics),
        productRunAt: product_run_at,
        receivedAt: values.utc_instant(record.received_at_ms, RuntimeTimeValue::ReceivedAt)?,
        sourceId: record.source_id,
        sourceKind: project_ambient_weather_source_kind(record.source_kind),
        validAt: values.utc_instant(record.valid_at_ms, RuntimeTimeValue::ValidAt)?,
    };
    let content_hash = values.weather_record_content_hash(&normalized)?;

    Ok(PreparedWeatherRecord {
        record: normalized,
        storage: WeatherRecordStorageEvidence {
            content_hash,
            provider_metadata_json: Some(provider_metadata_json),
            quality_metadata_json: Some(quality_metadata_json),
        },
    })
}

/// project one Ecowitt record to the generated database contract
fn project_ecowitt_record<Values: OpenMeteoRuntimeValues>(
    record: NormalizedEcowittRecord,
    values: &mut Values,
) -> Result<PreparedWeatherRecord, TaskFailure> {
    let provider = ecowitt_provider_metadata(&record.provider);
    let provider_metadata_json = values.serialize_json_object(&provider)?;
    let quality = ecowitt_quality_metadata(&record.quality);
    let quality_metadata_json = values.serialize_json_object(&quality)?;
    let product_run_at = match record.product_run_at_ms {
        // preserve physical-sensor product-run nullability
        None => Nullable::Null,
        Some(milliseconds) => {
            Nullable::Value(values.utc_instant(milliseconds, RuntimeTimeValue::ProductRunAt)?)
        }
    };
    let normalized = NormalizedWeatherRecord {
        metadata: WeatherRecordMetadata {
            device: Nullable::Value(DeviceMetadata {
                model: FieldPresence::Value(record.device.model),
                serial: FieldPresence::Value(record.device.serial),
                vendor: FieldPresence::Value(record.device.vendor.to_owned()),
            }),
            model: nullable(record.model),
            provider: Nullable::Value(provider),
            quality: Nullable::Value(quality),
            upstreamTimezone: record.upstream_timezone,
        },
        metrics: project_metrics(record.metrics),
        productRunAt: product_run_at,
        receivedAt: values.utc_instant(record.received_at_ms, RuntimeTimeValue::ReceivedAt)?,
        sourceId: record.source_id,
        sourceKind: project_ecowitt_source_kind(record.source_kind),
        validAt: values.utc_instant(record.valid_at_ms, RuntimeTimeValue::ValidAt)?,
    };
    let content_hash = values.weather_record_content_hash(&normalized)?;

    Ok(PreparedWeatherRecord {
        record: normalized,
        storage: WeatherRecordStorageEvidence {
            content_hash,
            provider_metadata_json: Some(provider_metadata_json),
            quality_metadata_json: Some(quality_metadata_json),
        },
    })
}

/// project the worker metric shape without changing values
fn project_metrics(metrics: weather_worker::CanonicalWeatherMetrics) -> CanonicalWeatherMetrics {
    CanonicalWeatherMetrics {
        apparentTemperatureC: nullable(metrics.apparent_temperature_c),
        blackGlobeTemperatureC: nullable(metrics.black_globe_temperature_c),
        cloudCoverPercent: nullable(metrics.cloud_cover_percent),
        pm25MicrogramsPerCubicMeter: nullable(metrics.pm25_micrograms_per_cubic_meter),
        precipitationMm: nullable(metrics.precipitation_mm),
        precipitationRateMmPerHour: nullable(metrics.precipitation_rate_mm_per_hour),
        pressureHpa: nullable(metrics.pressure_hpa),
        relativeHumidityPercent: nullable(metrics.relative_humidity_percent),
        soilElectricalConductivityMicrosiemensPerCm: nullable(
            metrics.soil_electrical_conductivity_microsiemens_per_cm,
        ),
        soilMoisturePercent: nullable(metrics.soil_moisture_percent),
        solarRadiationWm2: nullable(metrics.solar_radiation_wm2),
        temperatureC: nullable(metrics.temperature_c),
        uvIndex: nullable(metrics.uv_index),
        waterLevelM: nullable(metrics.water_level_m),
        windDirectionDegrees: nullable(metrics.wind_direction_degrees),
        windGustMps: nullable(metrics.wind_gust_mps),
        windSpeedMps: nullable(metrics.wind_speed_mps),
        wetBulbGlobeTemperatureC: nullable(metrics.wet_bulb_globe_temperature_c),
    }
}

/// project one worker source kind to the generated contract
const fn project_source_kind(source_kind: OpenMeteoSourceKind) -> SourceKind {
    // preserve the exact worker source kind
    match source_kind {
        OpenMeteoSourceKind::Forecast => SourceKind::Forecast,
        OpenMeteoSourceKind::ModelCurrent => SourceKind::ModelCurrent,
    }
}

/// project the one Tempest source kind to the generated contract
const fn project_tempest_source_kind(source_kind: weather_worker::TempestSourceKind) -> SourceKind {
    // preserve the exact worker source kind
    match source_kind {
        weather_worker::TempestSourceKind::PhysicalSensor => SourceKind::PhysicalSensor,
    }
}

/// project one NOAA tide source kind to the generated contract
const fn project_noaa_tide_source_kind(source_kind: NoaaTideSourceKind) -> SourceKind {
    // preserve observed versus predicted water levels
    match source_kind {
        NoaaTideSourceKind::Observation => SourceKind::TideObservation,
        NoaaTideSourceKind::Prediction => SourceKind::TidePrediction,
    }
}

/// project the one Ambient source kind to the generated contract
const fn project_ambient_weather_source_kind(source_kind: AmbientWeatherSourceKind) -> SourceKind {
    // preserve the exact worker source kind
    match source_kind {
        AmbientWeatherSourceKind::PhysicalSensor => SourceKind::PhysicalSensor,
    }
}

/// project the one Ecowitt source kind to the generated contract
const fn project_ecowitt_source_kind(source_kind: EcowittSourceKind) -> SourceKind {
    // preserve the exact worker source kind
    match source_kind {
        EcowittSourceKind::PhysicalSensor => SourceKind::PhysicalSensor,
    }
}

/// build exact OpenMeteo provider metadata in TypeScript field order
fn open_meteo_provider_metadata(provenance: &OpenMeteoProvenance) -> Vec<(Vec<u16>, JsonValue)> {
    let mut provider = Vec::with_capacity(3);
    provider.push((utf16("dataset"), json_string(provenance.dataset)));

    // omit absent elevation like the TypeScript provider
    if let Some(elevation) = provenance.elevation_m {
        provider.push((utf16("elevation_m"), JsonValue::Number(elevation)));
    }

    provider.push((utf16("grid_cell"), json_string(&provenance.grid_cell)));
    provider
}

/// build exact Tempest provider metadata in TypeScript field order
fn tempest_provider_metadata(metadata: &TempestProviderMetadata) -> Vec<(Vec<u16>, JsonValue)> {
    vec![
        (
            utf16("battery_volts"),
            json_nullable_number(metadata.battery_volts),
        ),
        (
            utf16("device_id"),
            JsonValue::Number(metadata.device_id as f64),
        ),
        (
            utf16("illuminance_lux"),
            json_nullable_number(metadata.illuminance_lux),
        ),
        (
            utf16("lightning_average_distance_km"),
            json_nullable_number(metadata.lightning_average_distance_km),
        ),
        (
            utf16("lightning_strike_count"),
            json_nullable_number(metadata.lightning_strike_count),
        ),
        (
            utf16("location_id"),
            JsonValue::Number(metadata.location_id as f64),
        ),
        (
            utf16("precipitation_type"),
            json_nullable_number(metadata.precipitation_type),
        ),
        (
            utf16("rain_accumulation_nc_mm"),
            json_nullable_number(metadata.rain_accumulation_nc_mm),
        ),
        (
            utf16("report_interval_minutes"),
            json_nullable_number(metadata.report_interval_minutes),
        ),
        (
            utf16("wind_lull_mps"),
            json_nullable_number(metadata.wind_lull_mps),
        ),
        (
            utf16("wind_sample_interval_seconds"),
            json_nullable_number(metadata.wind_sample_interval_seconds),
        ),
    ]
}

/// build exact Tempest quality metadata in TypeScript field order
fn tempest_quality_metadata(metadata: &TempestQualityMetadata) -> Vec<(Vec<u16>, JsonValue)> {
    let mut quality = vec![(utf16("sampling"), json_string(metadata.sampling))];

    // include flags only when the normalizer retained them
    if let Some(flags) = &metadata.flags {
        quality.push((
            utf16("flags"),
            JsonValue::Array(
                flags
                    .iter()
                    .map(|flag| json_string(tempest_quality_flag(*flag)))
                    .collect(),
            ),
        ));
    }

    quality
}

/// map one typed Tempest quality flag to its Node wire value
const fn tempest_quality_flag(flag: TempestQualityFlag) -> &'static str {
    // preserve the provider's exact wire spelling
    match flag {
        TempestQualityFlag::UvIndexOutOfRange => "uv_index_out_of_range",
    }
}

/// build exact Tempest response metadata in TypeScript field order
fn tempest_response_metadata(metadata: &TempestResponseMetadata) -> Vec<(Vec<u16>, JsonValue)> {
    let mut response = vec![
        (
            utf16("device_id"),
            JsonValue::Number(metadata.device_id as f64),
        ),
        (
            utf16("minute_record_count"),
            JsonValue::Number(metadata.minute_record_count as f64),
        ),
        (
            utf16("raw_observation_count"),
            JsonValue::Number(metadata.raw_observation_count as f64),
        ),
    ];

    // append the optional bucket diagnostic after required fields
    if let Some(bucket_step_minutes) = metadata.bucket_step_minutes {
        response.push((
            utf16("bucket_step_minutes"),
            JsonValue::Number(bucket_step_minutes),
        ));
    }

    response
}

/// build exact NOAA provider metadata in TypeScript field order
fn noaa_tide_provider_metadata(
    metadata: &NoaaTideProviderMetadata,
) -> Result<Vec<(Vec<u16>, JsonValue)>, TaskFailure> {
    let prediction_type = match metadata.prediction_type {
        // preserve observation null and prediction event labels
        None => JsonValue::Null,
        Some(NoaaPredictionType::High) => json_string("H"),
        Some(NoaaPredictionType::Low) => json_string("L"),
        Some(NoaaPredictionType::Unsupported) => {
            return Err(TaskFailure {
                attempts: 0,
                code: "invalid_noaa_runtime_record".to_owned(),
            });
        }
    };

    Ok(vec![
        (
            utf16("dataset"),
            json_string(noaa_tide_product(metadata.dataset)),
        ),
        (utf16("datum"), json_string(metadata.datum)),
        (utf16("prediction_type"), prediction_type),
        (
            utf16("product"),
            json_string(noaa_tide_product(metadata.product)),
        ),
        (utf16("station_id"), json_string(&metadata.station_id)),
    ])
}

/// build exact NOAA quality metadata for observations and predictions
fn noaa_tide_quality_metadata(metadata: &NoaaTideQualityMetadata) -> Vec<(Vec<u16>, JsonValue)> {
    // preserve each Node quality object field order
    match metadata {
        NoaaTideQualityMetadata::HighLowPrediction { sampling } => {
            vec![(utf16("sampling"), json_string(sampling))]
        }
        NoaaTideQualityMetadata::VerifiedObservation { flags, status } => vec![
            (
                utf16("flags"),
                JsonValue::Array(flags.iter().map(|flag| json_string(flag)).collect()),
            ),
            (utf16("status"), json_string(status)),
        ],
    }
}

/// build exact NOAA response metadata in TypeScript field order
fn noaa_tide_response_metadata(metadata: &NoaaTideResponseMetadata) -> Vec<(Vec<u16>, JsonValue)> {
    vec![
        (utf16("datum"), json_string(metadata.datum)),
        (
            utf16("normalized_record_count"),
            JsonValue::Number(metadata.normalized_record_count as f64),
        ),
        (
            utf16("product"),
            json_string(noaa_tide_product(metadata.product)),
        ),
        (utf16("provider"), json_string(metadata.provider)),
        (utf16("station_id"), json_string(&metadata.station_id)),
    ]
}

/// map one typed NOAA product to its Node wire value
const fn noaa_tide_product(product: NoaaTideProduct) -> &'static str {
    // preserve the exact provider request spelling
    match product {
        NoaaTideProduct::Predictions => "predictions",
        NoaaTideProduct::WaterLevel => "water_level",
    }
}

/// build exact Ambient provider metadata in TypeScript field order
fn ambient_weather_provider_metadata(
    metadata: &AmbientWeatherProviderMetadata,
) -> Vec<(Vec<u16>, JsonValue)> {
    vec![
        (utf16("dataset"), json_string(metadata.dataset)),
        (utf16("device_id"), json_string(&metadata.device_id)),
    ]
}

/// build exact Ambient quality metadata in TypeScript field order
fn ambient_weather_quality_metadata(
    metadata: &AmbientWeatherQualityMetadata,
) -> Vec<(Vec<u16>, JsonValue)> {
    vec![(utf16("sampling"), json_string(metadata.sampling))]
}

/// build exact Ambient response metadata in TypeScript field order
fn ambient_weather_response_metadata(
    metadata: &AmbientWeatherResponseMetadata,
) -> Vec<(Vec<u16>, JsonValue)> {
    vec![
        (
            utf16("normalized_record_count"),
            JsonValue::Number(metadata.normalized_record_count as f64),
        ),
        (utf16("provider"), json_string(metadata.provider)),
        (
            utf16("raw_observation_count"),
            JsonValue::Number(metadata.raw_observation_count as f64),
        ),
    ]
}

/// build exact Ecowitt provider metadata including property-sensor scopes
fn ecowitt_provider_metadata(metadata: &EcowittProviderMetadata) -> Vec<(Vec<u16>, JsonValue)> {
    vec![
        (utf16("dataset"), json_string(metadata.dataset)),
        (utf16("device_id"), json_string(&metadata.device_id)),
        (
            utf16("property_sensors"),
            JsonValue::Array(
                metadata
                    .property_sensors
                    .iter()
                    .map(ecowitt_sensor_snapshot)
                    .collect(),
            ),
        ),
    ]
}

/// build one exact Ecowitt property-sensor JSON object
fn ecowitt_sensor_snapshot(snapshot: &EcowittSensorSnapshot) -> JsonValue {
    JsonValue::Object(vec![
        (
            utf16("channel"),
            match snapshot.channel {
                // preserve absent versus numeric property channels
                None => JsonValue::Null,
                Some(channel) => JsonValue::Number(f64::from(channel)),
            },
        ),
        (utf16("key"), json_string(&snapshot.key)),
        (utf16("model"), json_string(snapshot.model)),
        (
            utf16("readings"),
            JsonValue::Object(ecowitt_sensor_readings(&snapshot.readings)),
        ),
    ])
}

/// build one compact Ecowitt sensor-reading object in interface field order
fn ecowitt_sensor_readings(readings: &EcowittSensorReadings) -> Vec<(Vec<u16>, JsonValue)> {
    let mut object = Vec::new();
    push_json_number(
        &mut object,
        "batteryExternalPower",
        readings.battery_external_power,
    );
    push_json_number(&mut object, "batteryLevel", readings.battery_level);
    push_json_number(&mut object, "batteryLow", readings.battery_low);
    push_json_number(
        &mut object,
        "blackGlobeTemperatureC",
        readings.black_globe_temperature_c,
    );
    push_json_number(
        &mut object,
        "dailyPrecipitationMm",
        readings.daily_precipitation_mm,
    );
    push_json_number(
        &mut object,
        "pm25MicrogramsPerCubicMeter",
        readings.pm25_micrograms_per_cubic_meter,
    );
    push_json_number(
        &mut object,
        "precipitationRateMmPerHour",
        readings.precipitation_rate_mm_per_hour,
    );
    push_json_number(&mut object, "pressureHpa", readings.pressure_hpa);
    push_json_number(
        &mut object,
        "relativeHumidityPercent",
        readings.relative_humidity_percent,
    );
    push_json_number(
        &mut object,
        "soilElectricalConductivityMicrosiemensPerCm",
        readings.soil_electrical_conductivity_microsiemens_per_cm,
    );
    push_json_number(
        &mut object,
        "soilMoisturePercent",
        readings.soil_moisture_percent,
    );
    push_json_number(
        &mut object,
        "solarRadiationWm2",
        readings.solar_radiation_wm2,
    );
    push_json_number(&mut object, "temperatureC", readings.temperature_c);
    push_json_number(&mut object, "uvIndex", readings.uv_index);
    push_json_number(
        &mut object,
        "wetBulbGlobeTemperatureC",
        readings.wet_bulb_globe_temperature_c,
    );
    push_json_number(
        &mut object,
        "windDirectionDegrees",
        readings.wind_direction_degrees,
    );
    push_json_number(&mut object, "windGustMps", readings.wind_gust_mps);
    push_json_number(&mut object, "windSpeedMps", readings.wind_speed_mps);
    object
}

/// append one present Ecowitt reading without materializing missing fields
fn push_json_number(object: &mut Vec<(Vec<u16>, JsonValue)>, key: &str, value: Option<f64>) {
    // match Node's compact sensor-reading object
    if let Some(value) = value {
        object.push((utf16(key), JsonValue::Number(value)));
    }
}

/// build exact Ecowitt quality metadata in TypeScript field order
fn ecowitt_quality_metadata(metadata: &EcowittQualityMetadata) -> Vec<(Vec<u16>, JsonValue)> {
    vec![(utf16("sampling"), json_string(metadata.sampling))]
}

/// build exact Ecowitt response metadata in TypeScript field order
fn ecowitt_response_metadata(metadata: &EcowittResponseMetadata) -> Vec<(Vec<u16>, JsonValue)> {
    vec![
        (utf16("gateway_mac"), json_string(&metadata.gateway_mac)),
        (
            utf16("group_count"),
            JsonValue::Number(metadata.group_count as f64),
        ),
        (
            utf16("rain_gauge"),
            match metadata.rain_gauge {
                // preserve absent rain hardware and selected gauge identity
                None => JsonValue::Null,
                Some(EcowittRainGaugeKind::Piezo) => json_string("piezo"),
                Some(EcowittRainGaugeKind::Traditional) => json_string("traditional"),
            },
        ),
        (
            utf16("soil_channel_count"),
            JsonValue::Number(metadata.soil_channel_count as f64),
        ),
        (
            utf16("temperature_humidity_channel_count"),
            JsonValue::Number(metadata.temperature_humidity_channel_count as f64),
        ),
    ]
}

/// convert one optional metric to generated nullable storage
fn nullable<Value>(value: Option<Value>) -> Nullable<Value> {
    // preserve null versus value semantics
    match value {
        None => Nullable::Null,
        Some(value) => Nullable::Value(value),
    }
}

/// convert one optional device field to generated presence storage
fn field_presence<Value>(value: Option<Value>) -> FieldPresence<Value> {
    // preserve omitted versus present device fields
    match value {
        None => FieldPresence::Missing,
        Some(value) => FieldPresence::Value(value),
    }
}

/// encode one string for generated dependency-free JSON storage
fn json_string(value: &str) -> JsonValue {
    JsonValue::String(utf16(value))
}

/// preserve one nullable provider number in generated JSON storage
fn json_nullable_number(value: Option<f64>) -> JsonValue {
    // preserve provider null versus number semantics
    match value {
        None => JsonValue::Null,
        Some(value) => JsonValue::Number(value),
    }
}

/// encode one key for generated dependency-free JSON storage
fn utf16(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}

/// require the JavaScript checkpoint version domain
fn checkpoint_version(value: f64) -> Result<u64, TaskFailure> {
    // reject non-integer or unsafe version projections
    if !value.is_finite() || value < 0.0 || value.fract() != 0.0 || value > 9_007_199_254_740_991.0
    {
        return Err(TaskFailure {
            attempts: 0,
            code: "invalid_checkpoint_version".to_owned(),
        });
    }

    Ok(value as u64)
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_admin_auth_state::*;
pub use native_admin_session_material::*;
pub use native_ambient_transport::*;
pub use native_api_capture_codec::NativeApiCaptureCodec;
pub use native_api_capture_execution::*;
pub use native_api_installed_owner::*;
pub use native_api_maintenance_http::*;
pub use native_api_model_admission::NativeApiModelAdmission;
pub use native_api_process_listeners::*;
pub use native_api_public_http::*;
pub use native_api_revision_document_decode::*;
pub use native_api_time_values::*;
pub use native_backend_runtime::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_background_forecast_scheduler::*;
pub use native_calendar_compatibility::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_capture_epoch_init_authority::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_cold_custody_command::{
    NativeColdCustodyPorts, NativeColdCustodyRoot, run_native_cold_custody_command,
};
pub use native_ecowitt_transport::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_edge_admin_admission::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_edge_admin_files::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_edge_admin_owner::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_edge_composed_process::*;
pub use native_edge_http_headers::*;
pub use native_edge_http_request::*;
pub use native_edge_http_response::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_edge_online_archive::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_edge_post_response::*;
pub use native_edge_preview::*;
pub use native_edge_private_handler::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_edge_process_listeners::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_edge_public_handler::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_edge_recorded_owner::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_edge_recorded_projection::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_edge_route_execution::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_edge_route_graph::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_edge_runtime_resources::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_edge_scorecard::*;
pub use native_edge_startup_material::*;
pub use native_edge_tile_owner::*;
pub use native_edge_upstream::*;
pub use native_edge_viewer_context::*;
pub use native_edge_widget_projection::*;
pub use native_edge_widget_source::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_family_catalog_image_verifier::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_family_lineage_host::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_family_release_current::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_family_release_lineage::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_family_release_live_runtime::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_family_release_status::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_forecast_allocation_statfs::*;
pub use native_forecast_archive_codec::*;
pub use native_forecast_evidence_codec::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_forecast_scheduler_timer::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_future_input_seal_current::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_future_input_seal_install::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_historical_backfill_output::*;
pub use native_http_body::*;
pub use native_http_listener::*;
pub use native_http_terminal::*;
pub use native_installed_api_process::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_installed_edge_process::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_installed_forecast_archive_operator::*;
#[cfg(unix)]
pub use native_installed_worker_process::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_anchor_current_v3::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_anchor_finalize_v2::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_anchor_finalize_v3::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_anchor_install_v2::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_anchor_install_v3::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_archive_genesis::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_archive_journal::{
    NativeMaintenanceArchiveJournal, NativeMaintenanceArchiveJournalError,
};
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_archive_segment::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_backing_capacity::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_backing_census::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_capture_cycle_core::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_capture_genesis_host::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_capture_host_readiness::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_capture_installed_closure::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_capture_job::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_capture_native_installation::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_capture_readiness::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_capture_readiness_wire::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_capture_remote_authority::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_cas_pack::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_catalog_wire::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_cold_journal::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_cold_remote_documents::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_cold_remote_page::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_controller_lock::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_database_ledger::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_finalization_proof_v2::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_fit_runtime_readiness::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_full_graph::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_future_only_journal::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_gap_archive_validators::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_graph_manifest::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_graph_restore::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_journal::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_journal_store::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_legacy_metadata_v2::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_local_archive_envelope::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_local_archive_readiness::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_open_pack_store::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_plaintext_archive::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_plaintext_object_store::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_registration_lifecycle_host::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_registration_lifecycle_remote::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_registration_policy::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_retirement_expected_v2::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_retirement_files_v2::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_revision_cold_drain::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_revision_gap_drain::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_revision_retirement_v2::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_rolling_schedule::*;
pub use native_migration_output::*;
pub use native_model_evidence_values::NativeModelEvidenceValues;
pub use native_netatmo_transport::*;
pub use native_noaa_tide_transport::*;
#[cfg(feature = "node-zlib-candidate")]
pub use native_node_gzip_candidate::*;
pub use native_open_meteo_scheduled_transport::*;
pub use native_open_meteo_transport::*;
pub use native_open_meteo_values::*;
pub use native_outbound_http::*;
pub use native_postgres_api_capture_ports::*;
pub use native_postgres_api_read::*;
pub use native_postgres_api_serve::*;
pub use native_postgres_api_startup::*;
pub use native_postgres_migration::*;
pub use native_postgres_pool::*;
pub use native_postgres_private_handler::*;
pub use native_postgres_public_handler::*;
pub use native_postgres_rain_capture::*;
pub use native_postgres_worker_rain_inference::*;
pub use native_postgres_worker_temperature::*;
pub use native_private_archive_http::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_private_archive_stores::bind_native_private_archive_stores;
pub use native_process_clock::*;
#[cfg(unix)]
pub use native_process_shutdown::*;
pub use native_public_backfill_time::*;
pub use native_purpleair_transport::*;
pub use native_rain_capture_values::*;
pub use native_rain_collection_transport::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_revision_gap_codec::NativeRevisionGapCodec;
pub use native_runtime_material::*;
pub use native_scheduled_values::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_scheduler_capture::*;
pub use native_temperature_canary_provider::*;
pub use native_tempest_tide_transport::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_unsupported_terminal_proof::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_unsupported_terminal_proof_install::*;
pub use native_wire_primitives::*;
pub use native_worker_backfill_owner::*;
pub use native_worker_configuration::*;
pub use native_worker_database::*;
pub use native_worker_maintenance_task_binding::*;
pub use native_worker_process_runner::*;
pub use native_worker_provider_factory::*;
pub use native_worker_rain_capture_owner::*;
pub use native_worker_rain_task_binding::*;
pub use native_worker_runtime_owner::*;
pub use native_worker_scheduler::*;
pub use native_worker_sidecar_owner::*;
pub use native_worker_task_assembly::*;
pub use native_worker_temperature_archive_binding::*;
pub use native_worker_temperature_binding::*;
pub use native_worker_temperature_task_owner::*;
pub use native_wunderground_transport::*;
pub use native_xweather_tile_transport::*;
pub use shadow_metadata_custody_publish::{
    ShadowMetadataCustodyPublishInput, commit_shadow_metadata_custody_proof,
    prepare_shadow_metadata_custody_proof,
};

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_cold_archive_segment;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_cold_archive_segment::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_open_pack;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_open_pack::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_cold_pack_seal_journal;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_cold_pack_seal_journal::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_evidence_cycle_host;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_evidence_cycle_host::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_cold_archive_validators;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_cold_archive_validators::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_capture_cycle_host;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_capture_cycle_host::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_attempt_report;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_attempt_report::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_archive_member;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_archive_member::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_due_planner;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_due_planner::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_controller_lease;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_controller_lease::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_controller_archive_validators;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_controller_archive_validators::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_due_attempt_journal;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_due_attempt_journal::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_due_work;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_due_work::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_due_execution;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_due_execution::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_due_terminal_remote;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_due_terminal_remote::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_historical_catalog;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_historical_catalog::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_daily_lifecycle;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_daily_lifecycle::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_policy_comparison;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_policy_comparison::{
    NativeMaintenancePairedComparison, NativeMaintenancePolicyComparator,
    NativeMaintenancePolicyPopulationReport, NativeMaintenancePolicyRow,
    create_native_maintenance_policy_row, decode_native_maintenance_policy_row,
    native_maintenance_date_count, native_maintenance_equal_group_comparison,
    native_maintenance_mean_absolute_error, native_maintenance_paired_comparison,
    native_maintenance_policy_population, native_maintenance_population_report,
};

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_policy_temperature_wind;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_policy_temperature_wind::{
    NativeMaintenancePolicyKind, evaluate_native_temperature_wind_maintenance_policy,
};

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_historical_fit;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_historical_fit::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_historical_catalog_journal;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_historical_catalog_journal::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_monthly_fit;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_monthly_fit::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_policy_rain;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_policy_rain::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_policy;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_policy::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_daily_evaluation_input;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_daily_evaluation_input::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_daily_scoring;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_daily_scoring::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_controller_run;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_controller_run::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_rain_fit;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_rain_fit::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_temperature_fit;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_temperature_fit::*;

#[cfg(all(
    test,
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_temperature_fit_test;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_fit_snapshot;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_fit_snapshot::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_fit_candidate;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_fit_candidate::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_fit_sandbox;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_fit_sandbox::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_wind_maintenance_fit_calendar;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_wind_maintenance_fit_calendar::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_temperature_fit_bridge;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_temperature_fit_bridge::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_fit_bridge;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_fit_bridge::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_controller_host;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_controller_host::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_semantic_catalog;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_semantic_catalog::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_incumbent;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_incumbent::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_expected_key_plan;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_expected_key_plan::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_rain_monthly_fit;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_rain_monthly_fit::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_post_fit_registration;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_post_fit_registration::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_archive_registration_validators;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_archive_registration_validators::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_confirmation_journal;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_confirmation_journal::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_candidate_parity;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_candidate_parity::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_daily_candidate_replay;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_daily_candidate_replay::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_candidate_archive;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_candidate_archive::*;

#[cfg(all(
    test,
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_candidate_parity_test;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_monthly_semantic;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_monthly_semantic::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_candidate_owner;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_candidate_owner::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_semantic_owner;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_semantic_owner::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_daily_rain_causal_replay;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_daily_rain_causal_replay::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_daily_evaluator;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_daily_evaluator::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_daily_terminal_preparation;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_daily_terminal_preparation::*;

#[cfg(all(
    test,
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_daily_terminal_preparation_test;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_confirmation_plan_archive;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_confirmation_plan_archive::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_confirmation_access_journal;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_confirmation_access_journal::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_confirmation_access_remote;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_confirmation_access_remote::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_scorecard_publication;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_scorecard_publication::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_development_publication;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_development_publication::*;

#[cfg(all(
    test,
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_development_publication_test;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_terminal_access_owner;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_terminal_access_owner::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_candidate_restore;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_candidate_restore::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_confirmation_chunk_journal;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_confirmation_chunk_journal::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_daily_terminal_values;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_daily_terminal_values::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_installed_terminal_material;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_installed_terminal_material::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_scorecard_transport;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_scorecard_transport::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_development_remote;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_development_remote::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_installed_due_ports;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_installed_due_ports::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_model_release_git;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_model_release_git::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_model_release_journal;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_model_release_journal::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_model_release_check;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_model_release_check::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_daily_rain_terminal_values;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_daily_rain_terminal_values::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_model_release_publish;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_model_release_publish::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_development_compensation;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_development_compensation::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_installed_development_release;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_installed_development_release::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_terminal_completion_owner;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_terminal_completion_owner::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_development_anchor;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_development_anchor::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_terminal_evidence_archive;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_terminal_evidence_archive::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_control_release_package;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_control_release_package::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_terminal_tombstone_adapter;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_terminal_tombstone_adapter::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_terminal_result_journal;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_terminal_result_journal::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_terminal_record_remote;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_terminal_record_remote::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_installed_control_release;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_installed_control_release::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_qualified_action;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_qualified_action::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_qualified_release_package;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_qualified_release_package::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_terminal_seal_owner;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_terminal_seal_owner::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_installed_qualified_release;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_installed_qualified_release::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_installed_actions;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_installed_actions::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_maintenance_control_reference_semantic;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_maintenance_control_reference_semantic::*;

mod native_api_schema_readiness;
pub use native_api_schema_readiness::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_registration_schedule_init;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_registration_schedule_init::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_registration_schedule_projection;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_registration_schedule_projection::*;
#[cfg(all(
    test,
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_registration_schedule_projection_test;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_host_owner_operation;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_host_owner_operation::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_registration_lifecycle_projection;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_registration_lifecycle_projection::*;
#[cfg(all(
    test,
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_registration_lifecycle_projection_test;

#[cfg(all(
    feature = "node-zlib-candidate",
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_evaluation_export_archive;
#[cfg(all(
    feature = "node-zlib-candidate",
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_evaluation_export_archive::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_confirmation_export_projection;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_confirmation_export_projection::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_host_owner_access_installed;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_host_owner_access_installed::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_adjustment_evaluation_v2_envelope;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_adjustment_evaluation_v2_envelope::*;
#[cfg(all(
    test,
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_adjustment_evaluation_v2_envelope_test;

#[cfg(all(
    test,
    feature = "node-zlib-candidate",
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_evaluation_export_archive_test;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_host_owner_terminal;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_host_owner_terminal::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_public_station_backfill_process;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_public_station_backfill_process::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_tide_backfill_process;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_tide_backfill_process::run_native_tide_backfill;

mod native_family_release_transaction;
pub use native_family_release_transaction::*;
#[cfg(test)]
mod native_family_release_transaction_test;

mod native_family_release_target_authority;
pub use native_family_release_target_authority::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_tempest_backfill_process;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_tempest_backfill_process::run_native_tempest_backfill;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_host_owner_qualified_terminal;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_host_owner_qualified_terminal::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_scorecard_install;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_scorecard_install::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_family_release_transaction_port;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_family_release_transaction_port::*;
#[cfg(all(
    test,
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_family_release_transaction_port_test;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_family_release_host_controller;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_family_release_host_controller::*;
#[cfg(all(
    test,
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_family_release_host_controller_test;

#[cfg(all(
    feature = "node-zlib-candidate",
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_forecast_training_export;
#[cfg(all(
    feature = "node-zlib-candidate",
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_forecast_training_export::*;
#[cfg(all(
    test,
    feature = "node-zlib-candidate",
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_forecast_training_export_test;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_open_meteo_backfill_process;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_open_meteo_backfill_process::*;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_family_release_installed;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_family_release_installed::*;
#[cfg(all(
    test,
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_family_release_installed_test;

#[cfg(all(
    test,
    feature = "node-zlib-candidate",
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_forecast_training_export_differential_test;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_family_release_capacity;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_family_release_capacity::*;
