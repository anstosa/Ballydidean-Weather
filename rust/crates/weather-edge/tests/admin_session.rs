use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::process::Command;
use std::rc::Rc;

use weather_edge::{
    ADMIN_SESSION_TTL_SECONDS, AdmittedAdminCredential, AdmittedAdminCredentialDigest,
    AdmittedAdminPasswordVerifier, AdmittedAdminPersistentAuthState, AdmittedAdminSessionDigest,
    AdmittedAdminSessionNow, AdmittedAdminSessionToken, WeatherAdminAuthStatePort,
    WeatherAdminBootstrapStatus, WeatherAdminPortError, WeatherAdminSessionClock,
    WeatherAdminSessionMaterialPort, WeatherAdminStore,
};

type Trace = Rc<RefCell<Vec<&'static str>>>;

struct FixtureAuthState {
    state: Option<AdmittedAdminPersistentAuthState>,
    trace: Trace,
}

impl WeatherAdminAuthStatePort for FixtureAuthState {
    /// Read the fixture persistent auth state.
    fn read_auth_state(
        &mut self,
    ) -> Result<Option<AdmittedAdminPersistentAuthState>, WeatherAdminPortError> {
        self.trace.borrow_mut().push("read-auth");
        Ok(self.state.clone())
    }

    /// Persist the first fixture auth state.
    fn write_auth_state(
        &mut self,
        state: &AdmittedAdminPersistentAuthState,
    ) -> Result<(), WeatherAdminPortError> {
        self.trace.borrow_mut().push("write-auth");
        self.state = Some(state.clone());
        Ok(())
    }
}

struct FixtureClock {
    now_epoch_milliseconds: Rc<Cell<i64>>,
    trace: Trace,
}

impl WeatherAdminSessionClock for FixtureClock {
    /// Return the mutable admitted business-session clock.
    fn now(&mut self) -> Result<AdmittedAdminSessionNow, WeatherAdminPortError> {
        self.trace.borrow_mut().push("clock");
        Ok(AdmittedAdminSessionNow {
            epoch_milliseconds: self.now_epoch_milliseconds.get(),
        })
    }
}

struct FixtureSessionMaterial {
    next_ordinal: u64,
    issued: VecDeque<AdmittedAdminSessionToken>,
    trace: Trace,
}

impl WeatherAdminSessionMaterialPort for FixtureSessionMaterial {
    /// Issue one deterministic admitted opaque token.
    fn issue_token(&mut self) -> Result<AdmittedAdminSessionToken, WeatherAdminPortError> {
        self.trace.borrow_mut().push("token");
        self.next_ordinal += 1;
        let token = AdmittedAdminSessionToken::admitted(format!(
            "{}{:02}",
            "A".repeat(41),
            self.next_ordinal
        ));
        self.issued.push_back(token.clone());
        Ok(token)
    }

    /// Return one already admitted digest for the selected token.
    fn digest_token(
        &mut self,
        token: &AdmittedAdminSessionToken,
    ) -> Result<AdmittedAdminSessionDigest, WeatherAdminPortError> {
        self.trace.borrow_mut().push("digest");
        Ok(AdmittedAdminSessionDigest::admitted(format!(
            "admitted-digest:{}",
            token.expose()
        )))
    }
}

#[test]
// preserve successful bootstrap, bounded login order, expiry, and logout metadata
fn owns_the_ordinary_admin_session_lifecycle() {
    let oracle = run_node_oracle();
    let trace = Rc::new(RefCell::new(Vec::new()));
    let now_epoch_milliseconds = Rc::new(Cell::new(1_778_065_200_000));
    let credential_digest = AdmittedAdminCredentialDigest::admitted("credential-v1");
    let persistent_state = AdmittedAdminPersistentAuthState::admitted(
        credential_digest.clone(),
        AdmittedAdminPasswordVerifier::admitted(vec![1; 64], vec![2; 16]),
    );
    let credential = AdmittedAdminCredential::verified(credential_digest);
    let mut auth = FixtureAuthState {
        state: None,
        trace: Rc::clone(&trace),
    };
    let mut clock = FixtureClock {
        now_epoch_milliseconds: Rc::clone(&now_epoch_milliseconds),
        trace: Rc::clone(&trace),
    };
    let mut material = FixtureSessionMaterial {
        next_ordinal: 0,
        issued: VecDeque::new(),
        trace: Rc::clone(&trace),
    };
    let mut store = WeatherAdminStore::new();

    let bootstrap = store
        .bootstrap(&mut auth, persistent_state)
        .expect("admitted bootstrap should persist");
    assert_eq!(bootstrap.status, WeatherAdminBootstrapStatus::Configured);
    assert_eq!(*trace.borrow(), ["read-auth", "write-auth"]);
    let saved = auth.state.as_ref().expect("auth state should be retained");
    assert_eq!(saved.version(), 1);
    assert_eq!(saved.username(), "admin");
    assert_eq!(saved.verifier().hash().len(), 64);
    assert_eq!(saved.verifier().salt().len(), 16);

    trace.borrow_mut().clear();
    let first = store
        .start_session(&mut auth, &mut clock, &mut material, &credential)
        .expect("login should execute")
        .expect("verified credential should start a session");
    assert_eq!(first.maximum_age_seconds, ADMIN_SESSION_TTL_SECONDS);
    assert_eq!(*trace.borrow(), ["read-auth", "clock", "token", "digest"]);

    trace.borrow_mut().clear();
    assert!(
        store
            .authenticate_session(&mut clock, &mut material, &first.token)
            .expect("active session read should execute")
    );
    assert_eq!(*trace.borrow(), ["digest", "clock"]);

    // fill the retained registry once beyond its fixed capacity
    let mut latest = first.clone();
    for _ in 0..64 {
        latest = store
            .start_session(&mut auth, &mut clock, &mut material, &credential)
            .expect("bounded login should execute")
            .expect("verified credential should keep issuing sessions");
    }
    trace.borrow_mut().clear();
    assert!(
        !store
            .authenticate_session(&mut clock, &mut material, &first.token)
            .expect("evicted session read should execute")
    );
    assert_eq!(*trace.borrow(), ["digest"]);
    trace.borrow_mut().clear();
    assert!(
        store
            .authenticate_session(&mut clock, &mut material, &latest.token)
            .expect("latest session read should execute")
    );

    now_epoch_milliseconds
        .set(now_epoch_milliseconds.get() + ADMIN_SESSION_TTL_SECONDS as i64 * 1_000);
    trace.borrow_mut().clear();
    assert!(
        !store
            .authenticate_session(&mut clock, &mut material, &latest.token)
            .expect("expired session read should execute")
    );
    assert_eq!(*trace.borrow(), ["digest", "clock"]);

    let revocable = store
        .start_session(&mut auth, &mut clock, &mut material, &credential)
        .expect("replacement login should execute")
        .expect("verified credential should issue a replacement session");
    trace.borrow_mut().clear();
    store
        .revoke_session(&mut material, &revocable.token)
        .expect("logout should execute");
    assert_eq!(*trace.borrow(), ["digest"]);
    trace.borrow_mut().clear();
    assert!(
        !store
            .authenticate_session(&mut clock, &mut material, &revocable.token)
            .expect("logged-out session read should execute")
    );
    assert_eq!(*trace.borrow(), ["digest"]);

    assert!(oracle.contains("bootstrap=configured:"));
    assert!(oracle.contains("login=43200:"));
    assert!(oracle.contains("read=true:digest,clock"));
    assert!(oracle.contains("cap=false:true"));
    assert!(oracle.contains("expired=false"));
    assert!(oracle.contains("logout=false:digest"));
}

/// Execute the actual Node store method oracle without host or credential I/O.
fn run_node_oracle() -> String {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repository_root = crate_root
        .parent()
        .and_then(|path| path.parent())
        .and_then(|path| path.parent())
        .expect("weather-edge must remain under rust/crates");
    let output = Command::new("node")
        .arg(crate_root.join("tests/admin-session-node-oracle.mjs"))
        .current_dir(repository_root)
        .output()
        .expect("Node admin-session oracle should run");
    assert!(
        output.status.success(),
        "Node oracle failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("Node oracle output must be UTF-8")
}
