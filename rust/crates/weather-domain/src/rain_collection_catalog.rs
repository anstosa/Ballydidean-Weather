//! Fixed prospective rain collection station catalog shared by producers and workers.

/// Exact policy site latitude retained by the TypeScript contract.
pub const RAIN_COLLECTION_SITE_LATITUDE: f64 = 47.950429954185445;
/// Exact policy site longitude retained by the TypeScript contract.
pub const RAIN_COLLECTION_SITE_LONGITUDE: f64 = -122.42797012608193;

/// One physical gauge in the fixed prospective collection catalog.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RainCollectionStation {
    pub device_id: u64,
    pub latitude: f64,
    pub location_id: u64,
    pub longitude: f64,
    pub serial: &'static str,
}

/// Exact twelve-gauge catalog copied from the TypeScript contract.
pub const RAIN_COLLECTION_STATIONS: &[RainCollectionStation] = &[
    RainCollectionStation {
        device_id: 175_727,
        latitude: 47.95008,
        location_id: 64_255,
        longitude: -122.43982,
        serial: "ST-00054713",
    },
    RainCollectionStation {
        device_id: 1_239_187,
        latitude: 47.94215,
        location_id: 225_947,
        longitude: -122.42542,
        serial: "ST-00212830",
    },
    RainCollectionStation {
        device_id: 115_866,
        latitude: 47.95293,
        location_id: 38_270,
        longitude: -122.41414,
        serial: "ST-00157152",
    },
    RainCollectionStation {
        device_id: 401_592,
        latitude: 47.95498,
        location_id: 168_853,
        longitude: -122.44074,
        serial: "ST-00170845",
    },
    RainCollectionStation {
        device_id: 313_016,
        latitude: 47.9582,
        location_id: 126_537,
        longitude: -122.44274,
        serial: "ST-00134621",
    },
    RainCollectionStation {
        device_id: 466_938,
        latitude: 47.96244,
        location_id: 201_058,
        longitude: -122.43369,
        serial: "ST-00194085",
    },
    RainCollectionStation {
        device_id: 470_937,
        latitude: 47.96505,
        location_id: 203_055,
        longitude: -122.4241,
        serial: "ST-00198967",
    },
    RainCollectionStation {
        device_id: 180_230,
        latitude: 47.93134,
        location_id: 66_270,
        longitude: -122.42912,
        serial: "ST-00173167",
    },
    RainCollectionStation {
        device_id: 107_388,
        latitude: 47.91752,
        location_id: 34_768,
        longitude: -122.41112,
        serial: "ST-00020495",
    },
    RainCollectionStation {
        device_id: 230_560,
        latitude: 47.91563,
        location_id: 88_159,
        longitude: -122.41845,
        serial: "ST-00094734",
    },
    RainCollectionStation {
        device_id: 312_302,
        latitude: 47.91413,
        location_id: 126_197,
        longitude: -122.41471,
        serial: "ST-00129187",
    },
    RainCollectionStation {
        device_id: 87_271,
        latitude: 47.98707,
        location_id: 27_140,
        longitude: -122.46295,
        serial: "ST-00000360",
    },
];
