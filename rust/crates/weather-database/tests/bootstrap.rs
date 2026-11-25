use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use weather_contracts::{
    JsonValue, Nullable, SiteConfiguration, SiteConfigurationProvider, SiteConfigurationSite,
    SiteConfigurationSource, SiteConfigurationStation, SourceCapability, SourceKind, StationKind,
};
use weather_database::{
    BootstrapBind, BootstrapError, BootstrapJsonEncoder, BootstrapPool, BootstrapRows,
    BootstrapSession, ECOWITT_STATION_SQL, PHYSICAL_SOURCE_SQL, PROVIDER_SQL, PUBLIC_STATION_SQL,
    SITE_SQL, SOURCE_SQL, STATION_SQL, SerializedJson, TEMPEST_STATION_SQL,
    bootstrap_site_configuration,
};

const NODE_REPOSITORIES: &str = include_str!("../../../../packages/database/src/repositories.ts");

#[derive(Clone, Debug, PartialEq, Eq)]
enum Event {
    Checkout,
    Begin,
    Query(&'static str, Vec<String>),
    Commit,
    Rollback,
    Release,
}

type Trace = Rc<RefCell<Vec<Event>>>;

struct Pool(Trace, VecDeque<BootstrapRows>);

struct Session(Trace, VecDeque<BootstrapRows>);

impl BootstrapPool for Pool {
    type Session = Session;

    /// Open one isolated command-session fixture, sharing only its trace.
    fn checkout(&mut self) -> Result<Session, &'static str> {
        self.0.borrow_mut().push(Event::Checkout);
        Ok(Session(Rc::clone(&self.0), std::mem::take(&mut self.1)))
    }
}

impl BootstrapSession for Session {
    type Error = &'static str;

    /// Mark the transaction before any source insert.
    fn begin(&mut self) -> Result<(), Self::Error> {
        self.0.borrow_mut().push(Event::Begin);
        Ok(())
    }

    /// Retain every ordered typed bind; never submit real SQL.
    fn query(
        &mut self,
        sql: &'static str,
        binds: &[BootstrapBind<'_>],
    ) -> Result<BootstrapRows, Self::Error> {
        self.0.borrow_mut().push(Event::Query(
            sql,
            binds
                .iter()
                .map(
                    // expose the variant as well as its unmodified input for this fixture
                    |bind| format!("{bind:?}"),
                )
                .collect(),
        ));
        self.1.pop_front().ok_or("extra fixture query")
    }

    /// Capture one complete catalog commit.
    fn commit(&mut self) -> Result<(), Self::Error> {
        self.0.borrow_mut().push(Event::Commit);
        Ok(())
    }

    /// Capture complete rollback after the ordinary fingerprint conflict.
    fn rollback(&mut self) -> Result<(), Self::Error> {
        self.0.borrow_mut().push(Event::Rollback);
        Ok(())
    }

    /// Capture return of the same session to the pool.
    fn release(&mut self) {
        self.0.borrow_mut().push(Event::Release);
    }
}

struct Encoder;

impl BootstrapJsonEncoder for Encoder {
    type Error = &'static str;

    /// Supply known closed fixture material without a runtime codec.
    fn value(&mut self, value: &JsonValue) -> Result<SerializedJson, Self::Error> {
        assert_eq!(value, &JsonValue::Object(Vec::new()));
        Ok(SerializedJson::admitted("{}"))
    }

    /// Reject unexpected ordered object conversion in the general-site fixture.
    fn object(&mut self, _: &[(Vec<u16>, JsonValue)]) -> Result<SerializedJson, Self::Error> {
        Err("unexpected material path")
    }

    /// Supply the existing ordinary source capability bytes.
    fn capabilities(&mut self, value: &[SourceCapability]) -> Result<SerializedJson, Self::Error> {
        assert_eq!(value, [SourceCapability::Current]);
        Ok(SerializedJson::admitted("[\"current\"]"))
    }
}

/// Build the generated contract expected by the Node source bootstrap.
fn configuration() -> SiteConfiguration {
    SiteConfiguration {
        provider: SiteConfigurationProvider {
            active: true,
            attributionLabel: "Label".to_owned(),
            attributionUrl: "url".to_owned(),
            displayName: "Provider".to_owned(),
            key: "provider".to_owned(),
        },
        site: SiteConfigurationSite {
            active: true,
            displayName: "Site".to_owned(),
            key: "site".to_owned(),
            latitude: 47.95,
            longitude: -122.43,
            timezone: "America/Los_Angeles".to_owned(),
        },
        sources: vec![SiteConfigurationSource {
            active: true,
            adapterConfig: JsonValue::Object(Vec::new()),
            cadenceSeconds: Nullable::Value(60),
            capabilities: vec![SourceCapability::Current],
            fingerprint: "fixture-digest".to_owned(),
            key: "fixture".to_owned(),
            sourceKind: SourceKind::ModelCurrent,
        }],
        station: SiteConfigurationStation {
            active: true,
            displayName: "Station".to_owned(),
            key: "station".to_owned(),
            kind: StationKind::Virtual,
            latitude: 47.95,
            longitude: -122.43,
            model: Nullable::Value("model".to_owned()),
            serial: Nullable::Null,
            vendor: Nullable::Null,
        },
        version: 1,
    }
}

/// Return a typed row witness matching the driver's first-row semantics.
fn row(id: &str) -> BootstrapRows {
    BootstrapRows {
        row_count: Some(1),
        first_id: Some(id.to_owned()),
    }
}

/// Lock every checked SQL fixture to the retained source without a PostgreSQL command.
#[test]
fn every_bootstrap_sql_fixture_is_retained_from_node() {
    // these cover every distinct insert used across the five source configurations
    for sql in [
        SITE_SQL,
        PROVIDER_SQL,
        STATION_SQL,
        SOURCE_SQL,
        TEMPEST_STATION_SQL,
        ECOWITT_STATION_SQL,
        PUBLIC_STATION_SQL,
        PHYSICAL_SOURCE_SQL,
    ] {
        assert!(NODE_REPOSITORIES.contains(sql), "source SQL differs: {sql}");
    }
}

/// Check binding provenance, returned ids and ordinary source-drift rollback.
#[test]
fn site_bootstrap_commits_one_catalog_and_rolls_back_ordinary_source_conflict() {
    let trace = Trace::default();
    let mut pool = Pool(
        Rc::clone(&trace),
        VecDeque::from([row("site"), row("provider"), row("station"), row("source")]),
    );
    let receipt = bootstrap_site_configuration(&mut pool, &configuration(), &mut Encoder).unwrap();
    assert_eq!(receipt.site_id, "site");
    assert_eq!(receipt.provider_id, "provider");
    assert_eq!(receipt.station_id, "station");
    assert_eq!(receipt.source_ids, ["source"]);
    let observed = trace.borrow();
    assert_eq!(observed[0..2], [Event::Checkout, Event::Begin]);
    let Event::Query(sql, station) = &observed[4] else {
        panic!("missing station command");
    };
    assert_eq!(*sql, STATION_SQL);
    assert_eq!(station[3], "Text(\"virtual\")");
    assert_eq!(&station[6..9], ["NullText", "Text(\"model\")", "NullText"]);
    let Event::Query(sql, source) = &observed[5] else {
        panic!("missing source command");
    };
    assert_eq!(*sql, SOURCE_SQL);
    assert_eq!(
        source,
        &[
            "BigintText(\"station\")",
            "BigintText(\"provider\")",
            "Text(\"fixture\")",
            "Text(\"model_current\")",
            "Json(\"{}\")",
            "Text(\"fixture-digest\")",
            "Json(\"[\\\"current\\\"]\")",
            "Unsigned(60)",
            "Bool(true)",
        ]
    );
    assert_eq!(observed[6..], [Event::Commit, Event::Release]);
    drop(observed);

    trace.borrow_mut().clear();
    let mut pool = Pool(
        Rc::clone(&trace),
        VecDeque::from([
            row("site"),
            row("provider"),
            row("station"),
            BootstrapRows {
                row_count: Some(0),
                first_id: None,
            },
        ]),
    );
    let result = bootstrap_site_configuration(&mut pool, &configuration(), &mut Encoder);
    assert!(matches!(result, Err(BootstrapError::SourceChanged { key, .. }) if key == "fixture"));
    assert!(trace.borrow().ends_with(&[Event::Rollback, Event::Release]));
}

/// Preserve the two-insert prefix even when the first statement returns no row.
#[test]
fn site_bootstrap_checks_returned_ids_after_both_initial_inserts() {
    let trace = Trace::default();
    let mut pool = Pool(
        Rc::clone(&trace),
        VecDeque::from([
            BootstrapRows {
                row_count: Some(0),
                first_id: None,
            },
            row("provider"),
        ]),
    );
    let result = bootstrap_site_configuration(&mut pool, &configuration(), &mut Encoder);
    assert!(matches!(result, Err(BootstrapError::MissingRow(label)) if label == "site bootstrap"));
    let observed = trace.borrow();
    assert!(matches!(observed[2], Event::Query(SITE_SQL, _)));
    assert!(matches!(observed[3], Event::Query(PROVIDER_SQL, _)));
    assert_eq!(observed[4..], [Event::Rollback, Event::Release]);
}
