use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use weather_contracts::{
    EcowittConfiguration, EcowittConfigurationProvider, EcowittStationConfiguration, JsonValue,
    SourceCapability, TempestConfiguration, TempestConfigurationProvider,
    TempestStationConfiguration,
};
use weather_database::{
    BootstrapBind, BootstrapJsonEncoder, BootstrapPool, BootstrapRows, BootstrapSession,
    ECOWITT_STATION_SQL, PHYSICAL_SOURCE_SQL, SerializedJson, TEMPEST_STATION_SQL,
    bootstrap_ecowitt_configuration, bootstrap_tempest_configuration,
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

    /// return one isolated fixture session with the queued row witnesses.
    fn checkout(&mut self) -> Result<Self::Session, &'static str> {
        self.0.borrow_mut().push(Event::Checkout);
        Ok(Session(Rc::clone(&self.0), std::mem::take(&mut self.1)))
    }
}

impl BootstrapSession for Session {
    type Error = &'static str;

    /// retain the transaction start.
    fn begin(&mut self) -> Result<(), Self::Error> {
        self.0.borrow_mut().push(Event::Begin);
        Ok(())
    }

    /// retain exact typed binds without submitting a database command.
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
                    // expose the variant and value for exact bind assertions
                    |bind| format!("{bind:?}"),
                )
                .collect(),
        ));
        self.1.pop_front().ok_or("extra fixture query")
    }

    /// retain one complete sensor-catalog commit.
    fn commit(&mut self) -> Result<(), Self::Error> {
        self.0.borrow_mut().push(Event::Commit);
        Ok(())
    }

    /// retain an unexpected rollback for useful failure output.
    fn rollback(&mut self) -> Result<(), Self::Error> {
        self.0.borrow_mut().push(Event::Rollback);
        Ok(())
    }

    /// retain return of the same fixture session.
    fn release(&mut self) {
        self.0.borrow_mut().push(Event::Release);
    }
}

#[derive(Default)]
struct Encoder {
    capabilities: Vec<Vec<SourceCapability>>,
}

impl BootstrapJsonEncoder for Encoder {
    type Error = &'static str;

    /// encode the admitted empty fixture material through the approved boundary.
    fn value(&mut self, value: &JsonValue) -> Result<SerializedJson, Self::Error> {
        assert_eq!(value, &JsonValue::Object(Vec::new()));
        Ok(SerializedJson::admitted("{}"))
    }

    /// reject an object projection unused by either sensor family.
    fn object(
        &mut self,
        _entries: &[(Vec<u16>, JsonValue)],
    ) -> Result<SerializedJson, Self::Error> {
        Err("unexpected object projection")
    }

    /// retain capability order and return the matching fixture JSON.
    fn capabilities(&mut self, values: &[SourceCapability]) -> Result<SerializedJson, Self::Error> {
        self.capabilities.push(values.to_vec());
        match values {
            [SourceCapability::Current, SourceCapability::Historical] => {
                Ok(SerializedJson::admitted("[\"current\",\"historical\"]"))
            }
            [SourceCapability::Current] => Ok(SerializedJson::admitted("[\"current\"]")),
            _ => Err("unexpected capabilities"),
        }
    }
}

/// construct one successful driver row.
fn row(id: &str) -> BootstrapRows {
    BootstrapRows {
        row_count: Some(1),
        first_id: Some(id.to_owned()),
    }
}

/// construct one generated Tempest station catalog.
fn tempest_configuration() -> TempestConfiguration {
    TempestConfiguration {
        provider: TempestConfigurationProvider {
            active: true,
            attributionLabel: "Tempest label".to_owned(),
            attributionUrl: "https://tempest.example".to_owned(),
            displayName: "Tempest".to_owned(),
            key: "tempest".to_owned(),
        },
        siteKey: "site".to_owned(),
        stations: vec![TempestStationConfiguration {
            active: true,
            adapterConfig: JsonValue::Object(Vec::new()),
            cadenceSeconds: 60,
            deviceId: 11,
            displayName: "Tempest Station".to_owned(),
            fingerprint: "tempest-fingerprint".to_owned(),
            historyStartDate: "2025-01-01".to_owned(),
            key: "tempest-station".to_owned(),
            latitude: 47.95,
            locationId: 12,
            longitude: -122.43,
            serial: "ST-1".to_owned(),
            sourceKey: "tempest-source".to_owned(),
            timezone: "America/Los_Angeles".to_owned(),
        }],
        version: 1,
    }
}

/// construct one generated Ecowitt gateway catalog.
fn ecowitt_configuration() -> EcowittConfiguration {
    EcowittConfiguration {
        provider: EcowittConfigurationProvider {
            active: true,
            attributionLabel: "Ecowitt label".to_owned(),
            attributionUrl: "https://ecowitt.example".to_owned(),
            displayName: "Ecowitt".to_owned(),
            key: "ecowitt".to_owned(),
        },
        siteKey: "site".to_owned(),
        stations: vec![EcowittStationConfiguration {
            active: true,
            adapterConfig: JsonValue::Object(Vec::new()),
            cadenceSeconds: 30,
            displayName: "Ecowitt Gateway".to_owned(),
            expectedMac: "AA:BB:CC:DD:EE:FF".to_owned(),
            fingerprint: "ecowitt-fingerprint".to_owned(),
            gatewayHost: "192.168.1.10".to_owned(),
            key: "ecowitt-station".to_owned(),
            latitude: 47.95,
            longitude: -122.43,
            model: "GW2000".to_owned(),
            sourceKey: "ecowitt-source".to_owned(),
            timezone: "America/Los_Angeles".to_owned(),
        }],
        version: 1,
    }
}

/// run both sensor families with exact station and source bind boundaries.
#[test]
fn sensor_catalogs_preserve_family_binds_capabilities_and_receipt_order() {
    let tempest_trace = Trace::default();
    let mut tempest_pool = Pool(
        Rc::clone(&tempest_trace),
        VecDeque::from([
            row("site-id"),
            row("tempest-provider-id"),
            row("tempest-station-id"),
            row("tempest-source-id"),
        ]),
    );
    let mut tempest_encoder = Encoder::default();
    let tempest = bootstrap_tempest_configuration(
        &mut tempest_pool,
        &tempest_configuration(),
        &mut tempest_encoder,
    )
    .unwrap();
    assert_eq!(tempest.provider_id, "tempest-provider-id");
    assert_eq!(
        tempest.station_ids,
        [(
            "tempest-station".to_owned(),
            "tempest-station-id".to_owned()
        )]
    );
    assert_eq!(
        tempest.source_ids,
        [("tempest-source".to_owned(), "tempest-source-id".to_owned())]
    );
    assert_eq!(
        tempest_encoder.capabilities,
        [vec![
            SourceCapability::Current,
            SourceCapability::Historical
        ]]
    );
    let tempest_events = tempest_trace.borrow();
    let Event::Query(station_sql, station_binds) = &tempest_events[4] else {
        panic!("missing Tempest station query")
    };
    assert_eq!(*station_sql, TEMPEST_STATION_SQL);
    assert_eq!(station_binds.len(), 7);
    assert_eq!(station_binds[0], "BigintText(\"site-id\")");
    let Event::Query(source_sql, source_binds) = &tempest_events[5] else {
        panic!("missing Tempest source query")
    };
    assert_eq!(*source_sql, PHYSICAL_SOURCE_SQL);
    assert_eq!(source_binds.len(), 8);
    assert_eq!(
        &source_binds[0..2],
        [
            "BigintText(\"tempest-station-id\")",
            "BigintText(\"tempest-provider-id\")",
        ]
    );
    assert_eq!(
        source_binds[5],
        "Json(\"[\\\"current\\\",\\\"historical\\\"]\")"
    );
    assert_eq!(&tempest_events[6..], [Event::Commit, Event::Release]);
    drop(tempest_events);

    let ecowitt_trace = Trace::default();
    let mut ecowitt_pool = Pool(
        Rc::clone(&ecowitt_trace),
        VecDeque::from([
            row("site-id"),
            row("ecowitt-provider-id"),
            row("ecowitt-station-id"),
            row("ecowitt-source-id"),
        ]),
    );
    let mut ecowitt_encoder = Encoder::default();
    let ecowitt = bootstrap_ecowitt_configuration(
        &mut ecowitt_pool,
        &ecowitt_configuration(),
        &mut ecowitt_encoder,
    )
    .unwrap();
    assert_eq!(ecowitt.provider_id, "ecowitt-provider-id");
    assert_eq!(
        ecowitt.station_ids,
        [(
            "ecowitt-station".to_owned(),
            "ecowitt-station-id".to_owned()
        )]
    );
    assert_eq!(
        ecowitt.source_ids,
        [("ecowitt-source".to_owned(), "ecowitt-source-id".to_owned())]
    );
    assert_eq!(
        ecowitt_encoder.capabilities,
        [vec![SourceCapability::Current]]
    );
    let ecowitt_events = ecowitt_trace.borrow();
    let Event::Query(station_sql, station_binds) = &ecowitt_events[4] else {
        panic!("missing Ecowitt station query")
    };
    assert_eq!(*station_sql, ECOWITT_STATION_SQL);
    assert_eq!(station_binds.len(), 8);
    assert_eq!(station_binds[0], "BigintText(\"site-id\")");
    let Event::Query(source_sql, source_binds) = &ecowitt_events[5] else {
        panic!("missing Ecowitt source query")
    };
    assert_eq!(*source_sql, PHYSICAL_SOURCE_SQL);
    assert_eq!(source_binds.len(), 8);
    assert_eq!(
        &source_binds[0..2],
        [
            "BigintText(\"ecowitt-station-id\")",
            "BigintText(\"ecowitt-provider-id\")",
        ]
    );
    assert_eq!(source_binds[5], "Json(\"[\\\"current\\\"]\")");
    assert_eq!(&ecowitt_events[6..], [Event::Commit, Event::Release]);
}
