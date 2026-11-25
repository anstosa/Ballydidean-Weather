//! Exact PostgreSQL source lookup for the Open-Meteo historical backfill command.

use weather_contracts::{BackfillSource, JsonValue, SourceKind};

/// The source-owned active historical-source lookup from `backfill-cli.ts`.
pub const OPEN_METEO_BACKFILL_SOURCE_SQL: &str = r#"
      SELECT
        si.latitude,
        si.longitude,
        s.material_provider_config AS "materialProviderConfig",
        p.provider_key AS "providerKey",
        si.slug AS "siteSlug",
        s.source_config_fingerprint AS "sourceConfigFingerprint",
        s.id AS "sourceId",
        s.source_key AS "sourceKey",
        s.source_kind AS "sourceKind",
        st.slug AS "stationSlug",
        si.timezone
      FROM sources s
      JOIN stations st ON st.id = s.station_id
      JOIN sites si ON si.id = st.site_id
      JOIN providers p ON p.id = s.provider_id
      WHERE si.slug = $1
        AND s.source_key = $2
        AND s.active
        AND st.active
        AND si.active
        AND p.active
      LIMIT 1
    "#;

/// One exact text bind in PostgreSQL placeholder order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OpenMeteoBackfillSourceBind {
    Text(String),
}

/// The exact two-bind active-source query.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenMeteoBackfillSourceQuery {
    pub sql: &'static str,
    pub binds: [OpenMeteoBackfillSourceBind; 2],
}

/// One decoded database row before the command aliases source identity fields.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenMeteoBackfillSourceRow {
    pub latitude: f64,
    pub longitude: f64,
    pub material_provider_config: JsonValue,
    pub provider_key: String,
    pub site_slug: String,
    pub source_config_fingerprint: String,
    pub source_id: String,
    pub source_key: String,
    pub source_kind: SourceKind,
    pub station_slug: String,
    pub timezone: String,
}

/// Narrow database boundary for the command's first-row-or-missing lookup.
pub trait OpenMeteoBackfillSourceReadPort {
    type Error;

    /// Read the optional row selected by the exact source-owned query.
    fn read_open_meteo_backfill_source(
        &mut self,
        query: &OpenMeteoBackfillSourceQuery,
    ) -> Result<Option<OpenMeteoBackfillSourceRow>, Self::Error>;
}

/// One database or ordinary missing-source result from source resolution.
#[derive(Clone, Debug, PartialEq)]
pub enum OpenMeteoBackfillSourceReadError<DriverError> {
    Driver(DriverError),
    Missing,
}

/// Build the exact active historical-source query without executing PostgreSQL.
pub fn open_meteo_backfill_source_query(
    site_slug: &str,
    source_key: &str,
) -> OpenMeteoBackfillSourceQuery {
    OpenMeteoBackfillSourceQuery {
        sql: OPEN_METEO_BACKFILL_SOURCE_SQL,
        binds: [
            OpenMeteoBackfillSourceBind::Text(site_slug.to_owned()),
            OpenMeteoBackfillSourceBind::Text(source_key.to_owned()),
        ],
    }
}

/// Resolve one active source and retain the command's `id` and `key` aliases.
pub fn resolve_open_meteo_backfill_source<Port: OpenMeteoBackfillSourceReadPort>(
    port: &mut Port,
    site_slug: &str,
    source_key: &str,
) -> Result<BackfillSource, OpenMeteoBackfillSourceReadError<Port::Error>> {
    let row = port
        .read_open_meteo_backfill_source(&open_meteo_backfill_source_query(site_slug, source_key))
        .map_err(OpenMeteoBackfillSourceReadError::Driver)?
        .ok_or(OpenMeteoBackfillSourceReadError::Missing)?;

    Ok(BackfillSource {
        id: row.source_id.clone(),
        key: row.source_key.clone(),
        latitude: row.latitude,
        longitude: row.longitude,
        materialProviderConfig: row.material_provider_config,
        providerKey: row.provider_key,
        siteSlug: row.site_slug,
        sourceConfigFingerprint: row.source_config_fingerprint,
        sourceKey: row.source_key,
        sourceKind: row.source_kind,
        stationSlug: row.station_slug,
        timezone: row.timezone,
    })
}

#[cfg(test)]
mod tests {
    use weather_contracts::JsonValue;

    use super::*;

    #[derive(Default)]
    struct FixturePort {
        query: Option<OpenMeteoBackfillSourceQuery>,
        row: Option<OpenMeteoBackfillSourceRow>,
    }

    impl OpenMeteoBackfillSourceReadPort for FixturePort {
        type Error = ();

        /// Retain the requested query and return the configured ordinary result.
        fn read_open_meteo_backfill_source(
            &mut self,
            query: &OpenMeteoBackfillSourceQuery,
        ) -> Result<Option<OpenMeteoBackfillSourceRow>, Self::Error> {
            self.query = Some(query.clone());
            Ok(self.row.take())
        }
    }

    /// Preserve the literal SQL and its site/source bind positions.
    #[test]
    fn builds_exact_active_source_query() {
        let query = open_meteo_backfill_source_query("ballydidean", "open-meteo-reanalysis-v1");

        assert_eq!(query.sql, OPEN_METEO_BACKFILL_SOURCE_SQL);
        assert_eq!(
            query.binds,
            [
                OpenMeteoBackfillSourceBind::Text("ballydidean".to_owned()),
                OpenMeteoBackfillSourceBind::Text("open-meteo-reanalysis-v1".to_owned()),
            ]
        );
        assert!(query.sql.contains("s.source_key = $2"));
        assert!(query.sql.contains("LIMIT 1"));
    }

    /// Project the selected database identity into both generated command aliases.
    #[test]
    fn resolves_generated_backfill_source() {
        let mut port = FixturePort {
            row: Some(OpenMeteoBackfillSourceRow {
                latitude: 56.0,
                longitude: -3.0,
                material_provider_config: JsonValue::Object(Vec::new()),
                provider_key: "open-meteo".to_owned(),
                site_slug: "ballydidean".to_owned(),
                source_config_fingerprint: "a".repeat(64),
                source_id: "source-id".to_owned(),
                source_key: "open-meteo-reanalysis-v1".to_owned(),
                source_kind: SourceKind::Reanalysis,
                station_slug: "ballydidean".to_owned(),
                timezone: "Europe/London".to_owned(),
            }),
            ..FixturePort::default()
        };

        let source = resolve_open_meteo_backfill_source(
            &mut port,
            "ballydidean",
            "open-meteo-reanalysis-v1",
        )
        .expect("active source should resolve");

        assert_eq!(source.id, "source-id");
        assert_eq!(source.key, "open-meteo-reanalysis-v1");
        assert_eq!(source.sourceKey, source.key);
        assert_eq!(source.sourceKind, SourceKind::Reanalysis);
    }

    /// Preserve the original ordinary missing-row outcome for the runtime owner.
    #[test]
    fn reports_missing_active_source() {
        let mut port = FixturePort::default();

        assert_eq!(
            resolve_open_meteo_backfill_source(
                &mut port,
                "ballydidean",
                "open-meteo-reanalysis-v1",
            ),
            Err(OpenMeteoBackfillSourceReadError::Missing)
        );
    }
}
