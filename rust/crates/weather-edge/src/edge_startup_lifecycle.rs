//! preserve edge owner initialization, listener requests, readiness, and shutdown ordering.

use crate::native_revision_spool::ForecastSchedulerConfiguration;

/// controller and optional listener gates parsed before owner initialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EdgeStartupLifecycleConfiguration {
    pub archive_transfer_enabled: bool,
    pub private_listener_enabled: bool,
    pub scheduler: ForecastSchedulerConfiguration,
}

/// retained archive state distinguishes an inert controller from failed optional initialization.
#[derive(Debug)]
pub enum EdgeStartupArchive<Archive> {
    Disabled,
    Unavailable,
    Ready(Archive),
}

impl<Archive> EdgeStartupArchive<Archive> {
    /// borrow only a genuinely initialized archive while preserving its unavailable state.
    pub const fn binding(&self) -> EdgeStartupArchiveBinding<'_, Archive> {
        match self {
            Self::Disabled => EdgeStartupArchiveBinding::Disabled,
            Self::Unavailable => EdgeStartupArchiveBinding::Unavailable,
            Self::Ready(archive) => EdgeStartupArchiveBinding::Ready(archive),
        }
    }
}

/// borrowed archive visibility installed into the public request owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeStartupArchiveBinding<'a, Archive> {
    Disabled,
    Unavailable,
    Ready(&'a Archive),
}

/// initialized owners captured by the public listener before its listen request.
pub struct EdgeStartupPublicOwners<'a, Admin, Evidence, Archive, Scheduler, Assets> {
    pub admin: &'a Admin,
    pub evidence: &'a Evidence,
    pub archive: EdgeStartupArchiveBinding<'a, Archive>,
    pub scheduler: &'a Scheduler,
    pub assets: &'a Assets,
}

/// readiness remains pending until the public listener's asynchronous callback fires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeStartupSchedulerReadiness {
    AwaitingPublicListener,
    Active,
    Inactive,
    Failed,
}

/// process-failing startup steps retain whether either listen request already escaped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeStartupBootError<Error> {
    pub phase: EdgeStartupBootPhase,
    pub error: Error,
    pub public_listener_requested: bool,
    pub private_listener_requested: bool,
}

/// closed startup phases with process-failing behavior in the source entry point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeStartupBootPhase {
    Admin,
    RevisionArchive,
    Assets,
    PublicListener,
    PrivateListener,
}

/// both close requests run even when either listener reports a shutdown failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EdgeStartupShutdownError<Error> {
    Public(Error),
    Private(Error),
    Both { public: Error, private: Error },
}

/// The public listener receives one process's exact admitted owner set.
pub type EdgeStartupPublicOwnerView<'a, Ports> = EdgeStartupPublicOwners<
    'a,
    <Ports as EdgeStartupLifecyclePorts>::Admin,
    <Ports as EdgeStartupLifecyclePorts>::Evidence,
    <Ports as EdgeStartupLifecyclePorts>::Archive,
    <Ports as EdgeStartupLifecyclePorts>::Scheduler,
    <Ports as EdgeStartupLifecyclePorts>::Assets,
>;

/// injectable owner, listener, scheduler, and shutdown boundaries for one edge process.
pub trait EdgeStartupLifecyclePorts {
    type Site;
    type Admin;
    type Evidence;
    type RevisionArchive;
    type Archive;
    type Scheduler;
    type Assets;
    type PublicListener;
    type PrivateListener;
    type Error;

    /// load optional map metadata without making ordinary serving depend on it.
    fn load_site(&mut self) -> Option<Self::Site>;

    /// construct the admin owner after the optional site center is known.
    fn initialize_admin(&mut self, site: Option<&Self::Site>) -> Result<Self::Admin, Self::Error>;

    /// initialize evidence capture while containing its own storage refusal.
    fn initialize_evidence(&mut self) -> Self::Evidence;

    /// construct the always-present revision archive owner before optional transport startup.
    fn initialize_revision_archive(&mut self) -> Result<Self::RevisionArchive, Self::Error>;

    /// initialize the optional online archive transport.
    fn initialize_archive(&mut self) -> Result<Self::Archive, Self::Error>;

    /// recover scheduler state and readiness without making serving depend on it.
    fn initialize_scheduler(
        &mut self,
        configuration: ForecastSchedulerConfiguration,
    ) -> Self::Scheduler;

    /// load the fixed public asset inventory before creating either listener.
    fn load_assets(&mut self) -> Result<Self::Assets, Self::Error>;

    /// request the public listen and register, but do not invoke, its readiness callback.
    fn request_public_listener(
        &mut self,
        owners: EdgeStartupPublicOwnerView<'_, Self>,
    ) -> Result<Self::PublicListener, Self::Error>;

    /// request the optional compose-private listen immediately after the public request.
    fn request_private_listener(
        &mut self,
        revision: &Self::RevisionArchive,
    ) -> Result<Self::PrivateListener, Self::Error>;

    /// start only the scheduler timer from the public listener readiness event.
    fn start_scheduler(&mut self, scheduler: &mut Self::Scheduler) -> Result<bool, Self::Error>;

    /// request closure of the already started public listener.
    fn close_public_listener(
        &mut self,
        listener: &mut Self::PublicListener,
    ) -> Result<(), Self::Error>;

    /// request closure of the optional already started private listener.
    fn close_private_listener(
        &mut self,
        listener: &mut Self::PrivateListener,
    ) -> Result<(), Self::Error>;
}

/// initialized edge process retaining listener handles and deferred scheduler readiness.
pub struct EdgeStartupLifecycle<Ports: EdgeStartupLifecyclePorts> {
    ports: Ports,
    site: Option<Ports::Site>,
    admin: Ports::Admin,
    evidence: Ports::Evidence,
    revision_archive: Ports::RevisionArchive,
    archive: EdgeStartupArchive<Ports::Archive>,
    scheduler: Ports::Scheduler,
    assets: Ports::Assets,
    public_listener: Ports::PublicListener,
    private_listener: Option<Ports::PrivateListener>,
    scheduler_readiness: EdgeStartupSchedulerReadiness,
}

/// owned initialized process state released to a concrete listener runtime.
pub struct EdgeStartupLifecycleParts<Ports: EdgeStartupLifecyclePorts> {
    pub ports: Ports,
    pub site: Option<Ports::Site>,
    pub admin: Ports::Admin,
    pub evidence: Ports::Evidence,
    pub revision_archive: Ports::RevisionArchive,
    pub archive: EdgeStartupArchive<Ports::Archive>,
    pub scheduler: Ports::Scheduler,
    pub assets: Ports::Assets,
    pub public_listener: Ports::PublicListener,
    pub private_listener: Option<Ports::PrivateListener>,
    pub scheduler_readiness: EdgeStartupSchedulerReadiness,
}

impl<Ports: EdgeStartupLifecyclePorts> EdgeStartupLifecycle<Ports> {
    /// initialize owners and issue public then optional private listen requests.
    pub fn initialize(
        mut ports: Ports,
        configuration: EdgeStartupLifecycleConfiguration,
    ) -> Result<Self, EdgeStartupBootError<Ports::Error>> {
        let site = ports.load_site();
        let admin = ports
            .initialize_admin(site.as_ref())
            .map_err(|error| boot_error(EdgeStartupBootPhase::Admin, error, false, false))?;
        let evidence = ports.initialize_evidence();
        let revision_archive = ports.initialize_revision_archive().map_err(|error| {
            boot_error(EdgeStartupBootPhase::RevisionArchive, error, false, false)
        })?;
        // initialize only an explicitly enabled archive transport
        let archive = if configuration.archive_transfer_enabled {
            // contain optional archive refusal without changing the public serving path
            match ports.initialize_archive() {
                Ok(archive) => EdgeStartupArchive::Ready(archive),
                Err(_) => EdgeStartupArchive::Unavailable,
            }
        } else {
            EdgeStartupArchive::Disabled
        };
        let scheduler = ports.initialize_scheduler(configuration.scheduler);
        let assets = ports
            .load_assets()
            .map_err(|error| boot_error(EdgeStartupBootPhase::Assets, error, false, false))?;
        let public_listener = ports
            .request_public_listener(EdgeStartupPublicOwners {
                admin: &admin,
                evidence: &evidence,
                archive: archive.binding(),
                scheduler: &scheduler,
                assets: &assets,
            })
            .map_err(|error| {
                boot_error(EdgeStartupBootPhase::PublicListener, error, false, false)
            })?;
        // request only the configured compose-private listener
        let private_listener = if configuration.private_listener_enabled {
            // issue the private listen without awaiting the public readiness callback
            Some(
                ports
                    .request_private_listener(&revision_archive)
                    .map_err(|error| {
                        boot_error(EdgeStartupBootPhase::PrivateListener, error, true, false)
                    })?,
            )
        } else {
            None
        };
        Ok(Self {
            ports,
            site,
            admin,
            evidence,
            revision_archive,
            archive,
            scheduler,
            assets,
            public_listener,
            private_listener,
            scheduler_readiness: EdgeStartupSchedulerReadiness::AwaitingPublicListener,
        })
    }

    /// handle the one public-listener readiness callback after both listen requests return.
    pub fn public_listener_ready(&mut self) -> Result<bool, Ports::Error> {
        // ignore a duplicate callback without installing a second scheduler timer
        if self.scheduler_readiness != EdgeStartupSchedulerReadiness::AwaitingPublicListener {
            return Ok(self.scheduler_readiness == EdgeStartupSchedulerReadiness::Active);
        }
        // retain the scheduler's readiness result after its one admitted start
        match self.ports.start_scheduler(&mut self.scheduler) {
            Ok(true) => {
                self.scheduler_readiness = EdgeStartupSchedulerReadiness::Active;
                Ok(true)
            }
            Ok(false) => {
                self.scheduler_readiness = EdgeStartupSchedulerReadiness::Inactive;
                Ok(false)
            }
            Err(error) => {
                self.scheduler_readiness = EdgeStartupSchedulerReadiness::Failed;
                Err(error)
            }
        }
    }

    /// close both requested listeners without stopping the source's independent scheduler timer.
    pub fn shutdown(&mut self) -> Result<(), EdgeStartupShutdownError<Ports::Error>> {
        let public = self.ports.close_public_listener(&mut self.public_listener);
        let private = self
            .private_listener
            .as_mut()
            .map(|listener| self.ports.close_private_listener(listener))
            .unwrap_or(Ok(()));
        // surface either close failure after both requests have run
        match (public, private) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), Ok(())) => Err(EdgeStartupShutdownError::Public(error)),
            (Ok(()), Err(error)) => Err(EdgeStartupShutdownError::Private(error)),
            (Err(public), Err(private)) => Err(EdgeStartupShutdownError::Both { public, private }),
        }
    }

    /// borrow optional site preload metadata retained by the admin owner.
    pub const fn site(&self) -> Option<&Ports::Site> {
        self.site.as_ref()
    }

    /// borrow the initialized admin owner.
    pub const fn admin(&self) -> &Ports::Admin {
        &self.admin
    }

    /// borrow the initialized fail-open evidence owner.
    pub const fn evidence(&self) -> &Ports::Evidence {
        &self.evidence
    }

    /// borrow the revision archive owner used by the optional private listener.
    pub const fn revision_archive(&self) -> &Ports::RevisionArchive {
        &self.revision_archive
    }

    /// inspect disabled, unavailable, or ready archive transfer state.
    pub const fn archive(&self) -> &EdgeStartupArchive<Ports::Archive> {
        &self.archive
    }

    /// borrow the initialized scheduler including its contained readiness status.
    pub const fn scheduler(&self) -> &Ports::Scheduler {
        &self.scheduler
    }

    /// borrow the fixed startup asset inventory.
    pub const fn assets(&self) -> &Ports::Assets {
        &self.assets
    }

    /// inspect scheduler activation relative to public listener readiness.
    pub const fn scheduler_readiness(&self) -> EdgeStartupSchedulerReadiness {
        self.scheduler_readiness
    }

    /// borrow the public listener handle retained for shutdown.
    pub const fn public_listener(&self) -> &Ports::PublicListener {
        &self.public_listener
    }

    /// borrow the optional private listener handle retained for shutdown.
    pub const fn private_listener(&self) -> Option<&Ports::PrivateListener> {
        self.private_listener.as_ref()
    }

    /// release every initialized owner after readiness into its concrete runtime.
    pub fn into_parts(self) -> EdgeStartupLifecycleParts<Ports> {
        EdgeStartupLifecycleParts {
            ports: self.ports,
            site: self.site,
            admin: self.admin,
            evidence: self.evidence,
            revision_archive: self.revision_archive,
            archive: self.archive,
            scheduler: self.scheduler,
            assets: self.assets,
            public_listener: self.public_listener,
            private_listener: self.private_listener,
            scheduler_readiness: self.scheduler_readiness,
        }
    }
}

/// retain one process-failing phase without performing cleanup absent from the source.
fn boot_error<Error>(
    phase: EdgeStartupBootPhase,
    error: Error,
    public_listener_requested: bool,
    private_listener_requested: bool,
) -> EdgeStartupBootError<Error> {
    EdgeStartupBootError {
        phase,
        error,
        public_listener_requested,
        private_listener_requested,
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Failure {
        Archive,
        Private,
        PublicClose,
        PrivateClose,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct Listener(&'static str);

    struct Ports {
        events: Rc<RefCell<Vec<&'static str>>>,
        archive_fails: bool,
        private_fails: bool,
        public_close_fails: bool,
        private_close_fails: bool,
        scheduler_active: bool,
    }

    impl Ports {
        /// create one ordinary lifecycle fixture with every optional owner active.
        fn new(events: Rc<RefCell<Vec<&'static str>>>) -> Self {
            Self {
                events,
                archive_fails: false,
                private_fails: false,
                public_close_fails: false,
                private_close_fails: false,
                scheduler_active: true,
            }
        }

        /// retain one exact observed lifecycle event.
        fn event(&self, value: &'static str) {
            self.events.borrow_mut().push(value);
        }
    }

    impl EdgeStartupLifecyclePorts for Ports {
        type Site = ();
        type Admin = ();
        type Evidence = ();
        type RevisionArchive = ();
        type Archive = ();
        type Scheduler = ();
        type Assets = ();
        type PublicListener = Listener;
        type PrivateListener = Listener;
        type Error = Failure;

        /// load one valid optional site.
        fn load_site(&mut self) -> Option<Self::Site> {
            self.event("site");
            Some(())
        }

        /// initialize admin state after the site result.
        fn initialize_admin(
            &mut self,
            site: Option<&Self::Site>,
        ) -> Result<Self::Admin, Self::Error> {
            assert!(site.is_some());
            self.event("admin");
            Ok(())
        }

        /// initialize the fail-open evidence owner.
        fn initialize_evidence(&mut self) -> Self::Evidence {
            self.event("evidence");
        }

        /// construct the revision archive owner.
        fn initialize_revision_archive(&mut self) -> Result<Self::RevisionArchive, Self::Error> {
            self.event("revision");
            Ok(())
        }

        /// initialize or refuse only the optional online archive.
        fn initialize_archive(&mut self) -> Result<Self::Archive, Self::Error> {
            self.event("archive");
            // exercise optional archive refusal independently of serving
            if self.archive_fails {
                Err(Failure::Archive)
            } else {
                Ok(())
            }
        }

        /// bind the exact scheduler controller configuration.
        fn initialize_scheduler(
            &mut self,
            configuration: ForecastSchedulerConfiguration,
        ) -> Self::Scheduler {
            assert!(configuration.enabled);
            assert!(configuration.migration_ready);
            self.event("scheduler_init");
        }

        /// load public assets before listener requests.
        fn load_assets(&mut self) -> Result<Self::Assets, Self::Error> {
            self.event("assets");
            Ok(())
        }

        /// request public listen without starting the scheduler callback.
        fn request_public_listener(
            &mut self,
            _owners: EdgeStartupPublicOwners<
                '_,
                Self::Admin,
                Self::Evidence,
                Self::Archive,
                Self::Scheduler,
                Self::Assets,
            >,
        ) -> Result<Self::PublicListener, Self::Error> {
            self.event("public_request");
            Ok(Listener("public"))
        }

        /// request private listen after the public request returns.
        fn request_private_listener(
            &mut self,
            _revision: &Self::RevisionArchive,
        ) -> Result<Self::PrivateListener, Self::Error> {
            self.event("private_request");
            // model a process-failing private listen request after public submission
            if self.private_fails {
                Err(Failure::Private)
            } else {
                Ok(Listener("private"))
            }
        }

        /// start the scheduler only from an explicit readiness event.
        fn start_scheduler(
            &mut self,
            _scheduler: &mut Self::Scheduler,
        ) -> Result<bool, Self::Error> {
            self.event("scheduler_start");
            Ok(self.scheduler_active)
        }

        /// request public listener closure.
        fn close_public_listener(
            &mut self,
            listener: &mut Self::PublicListener,
        ) -> Result<(), Self::Error> {
            assert_eq!(listener.0, "public");
            self.event("public_close");
            // retain an ordinary public close completion failure
            if self.public_close_fails {
                Err(Failure::PublicClose)
            } else {
                Ok(())
            }
        }

        /// request private listener closure.
        fn close_private_listener(
            &mut self,
            listener: &mut Self::PrivateListener,
        ) -> Result<(), Self::Error> {
            assert_eq!(listener.0, "private");
            self.event("private_close");
            // retain an ordinary private close completion failure
            if self.private_close_fails {
                Err(Failure::PrivateClose)
            } else {
                Ok(())
            }
        }
    }

    /// preserve owner initialization and asynchronous public readiness ordering.
    #[test]
    fn initializes_then_requests_both_listeners_before_scheduler_start() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut ports = Ports::new(Rc::clone(&events));
        ports.archive_fails = true;
        let mut lifecycle = EdgeStartupLifecycle::initialize(ports, configuration()).unwrap();
        assert_eq!(
            events.borrow().as_slice(),
            [
                "site",
                "admin",
                "evidence",
                "revision",
                "archive",
                "scheduler_init",
                "assets",
                "public_request",
                "private_request",
            ]
        );
        assert!(matches!(
            lifecycle.archive(),
            EdgeStartupArchive::Unavailable
        ));
        assert_eq!(
            lifecycle.scheduler_readiness(),
            EdgeStartupSchedulerReadiness::AwaitingPublicListener
        );
        assert!(lifecycle.public_listener_ready().unwrap());
        assert_eq!(events.borrow().last(), Some(&"scheduler_start"));
        assert!(lifecycle.public_listener_ready().unwrap());
        assert_eq!(
            events
                .borrow()
                .iter()
                .filter(|event| **event == "scheduler_start")
                .count(),
            1
        );
    }

    /// retain the already requested public listener when private listen fails.
    #[test]
    fn reports_partial_startup_without_invented_cleanup() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut ports = Ports::new(Rc::clone(&events));
        ports.private_fails = true;
        // avoid requiring the successful owner aggregate to implement debug
        let error = match EdgeStartupLifecycle::initialize(ports, configuration()) {
            Ok(_) => panic!("private listen unexpectedly succeeded"),
            Err(error) => error,
        };
        assert_eq!(error.phase, EdgeStartupBootPhase::PrivateListener);
        assert!(error.public_listener_requested);
        assert!(!error.private_listener_requested);
        assert_eq!(events.borrow().last(), Some(&"private_request"));
        assert!(!events.borrow().contains(&"public_close"));
    }

    /// request both listener closes even when each completion fails.
    #[test]
    fn shutdown_attempts_both_listener_handles() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut ports = Ports::new(Rc::clone(&events));
        ports.public_close_fails = true;
        ports.private_close_fails = true;
        let mut lifecycle = EdgeStartupLifecycle::initialize(ports, configuration()).unwrap();
        let error = lifecycle.shutdown().unwrap_err();
        assert_eq!(
            error,
            EdgeStartupShutdownError::Both {
                public: Failure::PublicClose,
                private: Failure::PrivateClose,
            }
        );
        assert_eq!(
            events.borrow().as_slice().get(9..),
            Some(["public_close", "private_close"].as_slice())
        );
    }

    /// return the exact enabled controller fixture used by normal startup.
    fn configuration() -> EdgeStartupLifecycleConfiguration {
        EdgeStartupLifecycleConfiguration {
            archive_transfer_enabled: true,
            private_listener_enabled: true,
            scheduler: ForecastSchedulerConfiguration {
                enabled: true,
                migration_ready: true,
            },
        }
    }
}
