use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use weather_contracts::{
    JsonValue, Nullable, PublicStationAdapter, PublicStationConfiguration,
    PublicStationConfigurationStation, PublicStationProviderConfiguration,
    PublicStationSourceConfiguration, SourceCapability, StationKind, TideConfiguration,
    TideProviderConfiguration, TideSourceConfiguration, TideSourceConfigurationSourceKindEnum,
    TideStationConfiguration,
};
use weather_database::{
    ACTIVE_SITE_SQL, BootstrapBind, BootstrapJsonEncoder, BootstrapPool, BootstrapRows,
    BootstrapSession, PHYSICAL_SOURCE_SQL, PROVIDER_SQL, PUBLIC_STATION_SQL, SOURCE_SQL,
    STATION_SQL, SerializedJson, bootstrap_public_station_configuration,
    bootstrap_tide_configuration,
};

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

    /// Open one isolated faux session without a PostgreSQL connection.
    fn checkout(&mut self) -> Result<Session, &'static str> {
        self.0.borrow_mut().push(Event::Checkout);
        Ok(Session(Rc::clone(&self.0), std::mem::take(&mut self.1)))
    }
}

impl BootstrapSession for Session {
    type Error = &'static str;

    /// Mark the transaction before the first retained statement.
    fn begin(&mut self) -> Result<(), Self::Error> {
        self.0.borrow_mut().push(Event::Begin);
        Ok(())
    }

    /// Capture statement and typed-bind order without executing SQL.
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
                    // retain the exact typed variant and borrowed value for assertions
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

    /// Capture rollback if an assertion exposes a catalog failure.
    fn rollback(&mut self) -> Result<(), Self::Error> {
        self.0.borrow_mut().push(Event::Rollback);
        Ok(())
    }

    /// Capture return of the same faux session.
    fn release(&mut self) {
        self.0.borrow_mut().push(Event::Release);
    }
}

struct Encoder;

impl BootstrapJsonEncoder for Encoder {
    type Error = &'static str;

    /// Reject the generic JSON path unused by both assigned catalog families.
    fn value(&mut self, _: &JsonValue) -> Result<SerializedJson, Self::Error> {
        Err("unexpected generic json path")
    }

    /// Encode the empty ordered adapter object fixture.
    fn object(&mut self, entries: &[(Vec<u16>, JsonValue)]) -> Result<SerializedJson, Self::Error> {
        assert!(entries.is_empty());
        Ok(SerializedJson::admitted("{}"))
    }

    /// Encode the two source-ordered capability fixtures without a JSON package.
    fn capabilities(&mut self, values: &[SourceCapability]) -> Result<SerializedJson, Self::Error> {
        // admit only the public-current and tide-forecast fixture sequences
        match values {
            [SourceCapability::Current, SourceCapability::Historical] => {
                Ok(SerializedJson::admitted("[\"current\",\"historical\"]"))
            }
            [SourceCapability::Forecast, SourceCapability::Historical] => {
                Ok(SerializedJson::admitted("[\"forecast\",\"historical\"]"))
            }
            _ => Err("unexpected capabilities"),
        }
    }
}

/// Build one generated public-station catalog with nullable physical metadata.
fn public_configuration() -> PublicStationConfiguration {
    PublicStationConfiguration {
        siteKey: "site".to_owned(),
        providers: vec![PublicStationProviderConfiguration {
            active: true,
            attributionLabel: "Public label".to_owned(),
            attributionUrl: "https://public.example/".to_owned(),
            displayName: "Public provider".to_owned(),
            key: "public-provider".to_owned(),
        }],
        stations: vec![PublicStationConfigurationStation {
            active: true,
            displayName: "Public station".to_owned(),
            key: "public-station".to_owned(),
            latitude: 47.95,
            longitude: -122.43,
            model: Nullable::Value("model".to_owned()),
            serial: Nullable::Null,
            sources: vec![PublicStationSourceConfiguration {
                active: true,
                adapter: PublicStationAdapter::WeatherUnderground,
                adapterConfig: Vec::new(),
                cadenceSeconds: Nullable::Null,
                capabilities: vec![SourceCapability::Current, SourceCapability::Historical],
                fingerprint: "public-digest".to_owned(),
                historyEndDate: Nullable::Null,
                historyStartDate: Nullable::Null,
                key: "public-source".to_owned(),
                maximumChunkDays: 31,
                providerKey: "public-provider".to_owned(),
            }],
            timezone: "America/Los_Angeles".to_owned(),
            vendor: Nullable::Null,
        }],
        version: 1,
    }
}

/// Build one generated tide catalog whose inactive station disables its active source.
fn tide_configuration() -> TideConfiguration {
    TideConfiguration {
        provider: TideProviderConfiguration {
            active: true,
            attributionLabel: "NOAA".to_owned(),
            attributionUrl: "https://tidesandcurrents.noaa.gov/".to_owned(),
            displayName: "NOAA tides".to_owned(),
            key: "noaa".to_owned(),
        },
        siteKey: "site".to_owned(),
        stations: vec![TideStationConfiguration {
            active: false,
            displayName: "Tide station".to_owned(),
            key: "tide-station".to_owned(),
            kind: StationKind::Physical,
            latitude: 47.95,
            longitude: -122.43,
            model: "water-level".to_owned(),
            serial: "1234567".to_owned(),
            source: TideSourceConfiguration {
                active: true,
                adapterConfig: Vec::new(),
                cadenceSeconds: 360,
                capabilities: vec![SourceCapability::Forecast, SourceCapability::Historical],
                fingerprint: "tide-digest".to_owned(),
                historyStartDate: "2020-01-01".to_owned(),
                key: "tide-source".to_owned(),
                maximumChunkDays: 366,
                sourceKind: TideSourceConfigurationSourceKindEnum::TidePrediction,
            },
            timezone: "America/Los_Angeles".to_owned(),
            vendor: "NOAA".to_owned(),
        }],
        version: 1,
    }
}

/// Return one successful driver row with an explicit source-upsert witness.
fn row(id: &str) -> BootstrapRows {
    BootstrapRows {
        row_count: Some(1),
        first_id: Some(id.to_owned()),
    }
}

/// Lock public and tide statement order, typed binds, receipts and effective activity.
#[test]
fn public_and_tide_bootstraps_preserve_catalog_semantics() {
    let public_trace = Trace::default();
    let mut public_pool = Pool(
        Rc::clone(&public_trace),
        VecDeque::from([
            row("site-id"),
            row("public-provider-id"),
            row("public-station-id"),
            row("public-source-id"),
        ]),
    );
    let public_receipt = bootstrap_public_station_configuration(
        &mut public_pool,
        &public_configuration(),
        &mut Encoder,
    )
    .unwrap();
    assert_eq!(
        public_receipt.provider_ids,
        [(
            "public-provider".to_owned(),
            "public-provider-id".to_owned()
        )]
    );
    assert_eq!(
        public_receipt.station_ids,
        [("public-station".to_owned(), "public-station-id".to_owned())]
    );
    assert_eq!(
        public_receipt.source_ids,
        [("public-source".to_owned(), "public-source-id".to_owned())]
    );
    let observed = public_trace.borrow();
    assert_eq!(observed[0..2], [Event::Checkout, Event::Begin]);
    assert_eq!(
        observed
            .iter()
            .filter_map(
                // retain only statement identities for ordinary-order comparison
                |event| match event {
                    Event::Query(sql, _) => Some(*sql),
                    _ => None,
                },
            )
            .collect::<Vec<_>>(),
        [
            ACTIVE_SITE_SQL,
            PROVIDER_SQL,
            PUBLIC_STATION_SQL,
            PHYSICAL_SOURCE_SQL,
        ]
    );
    let Event::Query(_, source) = &observed[5] else {
        panic!("missing public source command");
    };
    assert_eq!(
        source,
        &[
            "BigintText(\"public-station-id\")",
            "BigintText(\"public-provider-id\")",
            "Text(\"public-source\")",
            "Json(\"{}\")",
            "Text(\"public-digest\")",
            "Json(\"[\\\"current\\\",\\\"historical\\\"]\")",
            "NullInteger",
            "Bool(true)",
        ]
    );
    assert_eq!(observed[6..], [Event::Commit, Event::Release]);
    drop(observed);

    let tide_trace = Trace::default();
    let mut tide_pool = Pool(
        Rc::clone(&tide_trace),
        VecDeque::from([
            row("site-id"),
            row("noaa-id"),
            row("tide-station-id"),
            row("tide-source-id"),
        ]),
    );
    let tide_receipt =
        bootstrap_tide_configuration(&mut tide_pool, &tide_configuration(), &mut Encoder).unwrap();
    assert_eq!(tide_receipt.provider_id, "noaa-id");
    assert_eq!(
        tide_receipt.station_ids,
        [("tide-station".to_owned(), "tide-station-id".to_owned())]
    );
    assert_eq!(
        tide_receipt.source_ids,
        [("tide-source".to_owned(), "tide-source-id".to_owned())]
    );
    let observed = tide_trace.borrow();
    assert_eq!(
        observed
            .iter()
            .filter_map(
                // retain only statement identities for ordinary-order comparison
                |event| match event {
                    Event::Query(sql, _) => Some(*sql),
                    _ => None,
                },
            )
            .collect::<Vec<_>>(),
        [ACTIVE_SITE_SQL, PROVIDER_SQL, STATION_SQL, SOURCE_SQL]
    );
    let Event::Query(_, source) = &observed[5] else {
        panic!("missing tide source command");
    };
    assert_eq!(
        source,
        &[
            "BigintText(\"tide-station-id\")",
            "BigintText(\"noaa-id\")",
            "Text(\"tide-source\")",
            "Text(\"tide_prediction\")",
            "Json(\"{}\")",
            "Text(\"tide-digest\")",
            "Json(\"[\\\"forecast\\\",\\\"historical\\\"]\")",
            "Unsigned(360)",
            "Bool(false)",
        ]
    );
    assert_eq!(observed[6..], [Event::Commit, Event::Release]);
}
