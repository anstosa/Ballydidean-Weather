import hashlib
import importlib.util
import json
import os
import subprocess
import tempfile
import unittest
from contextlib import ExitStack
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

SCRIPT = Path(__file__).with_name("native-benchmark-node-api.py")
SPEC = importlib.util.spec_from_file_location("native_benchmark_node_api", SCRIPT)
fixture = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(fixture)
FIXTURE_ID = "rust-rewrite-0123456789abcdef"
DIGEST = "1" * 64
IMAGE_ID = f"sha256:{'2' * 64}"


# build one completed subprocess result
def result(stdout=b"", stderr=b"", returncode=0):
    return subprocess.CompletedProcess([], returncode, stdout=stdout, stderr=stderr)


# encode one Docker inspection record
def docker_record(value):
    return result(json.dumps([value]).encode("utf-8"))


class NativeNodeApiFixtureRegression(unittest.TestCase):
    # create one isolated filesystem fixture
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.base = Path(self.temporary.name)
        self.fixture_root = self.base / FIXTURE_ID
        self.mountpoint = self.fixture_root / "mnt"
        self.volume_root = self.mountpoint / "docker-volumes"
        self.volume_root.mkdir(parents=True)
        self.marker = {
            "schemaVersion": 1,
            "kind": "weather-native-rust-rewrite-fixture",
            "fixtureId": FIXTURE_ID,
            "fixtureRoot": str(self.fixture_root),
            "backingFile": str(self.fixture_root / "fixture.ext4"),
            "mountpoint": str(self.mountpoint),
            "volumeRoot": str(self.volume_root),
            "volumeNamespacePrefix": f"weather-native-benchmark-{FIXTURE_ID}-",
        }
        self.target = f"weather-native-benchmark-{FIXTURE_ID}-slot-01"
        self.handoff = {
            "volumeName": self.target,
            "volumeDevice": str(self.volume_root / self.target),
            "databaseName": "weather",
        }

    # remove the isolated filesystem fixture
    def tearDown(self):
        self.temporary.cleanup()

    # accept the official short repository digest and bind arm64
    def test_image_inspection_accepts_short_digest_and_rejects_tag(self):
        image = f"postgres@sha256:{DIGEST}"

        # return one exact image inspection
        def command(argv, **_kwargs):
            self.assertEqual(argv, [fixture.DOCKER, "image", "inspect", image])
            return docker_record(
                {
                    "Architecture": "arm64",
                    "Id": IMAGE_ID,
                    "Os": "linux",
                    "RepoDigests": [image],
                }
            )

        self.assertEqual(fixture.inspect_image(image, command)["Id"], IMAGE_ID)
        with self.assertRaisesRegex(fixture.FixtureError, "digest"):
            fixture.inspect_image("postgres:17", command)

    # parse only simple exact deployment assignments
    def test_deployment_environment_parses_runtime_identity_and_rejects_duplicate(self):
        image = f"registry.example/weather-server@sha256:{DIGEST}"
        postgres = f"postgres@sha256:{'3' * 64}"
        raw = (
            f"WEATHER_RELEASE=2026.11.04-6\nWEATHER_SERVER_IMAGE={image}\n"
            f"POSTGRES_IMAGE={postgres}\n"
        ).encode()
        with patch.object(fixture, "read_regular_bytes", return_value=raw):
            values, observed = fixture.read_deployment_environment(Path("/ignored"))
        self.assertEqual(values["WEATHER_RELEASE"], "2026.11.04-6")
        self.assertEqual(observed, hashlib.sha256(raw).hexdigest())
        with (
            patch.object(
                fixture,
                "read_regular_bytes",
                return_value=raw + b"WEATHER_RELEASE=duplicate\n",
            ),
            self.assertRaisesRegex(fixture.FixtureError, "syntax"),
        ):
            fixture.read_deployment_environment(Path("/ignored"))

    # derive the exact active environment from current-release and active.env
    def test_deployment_root_uses_installed_release_state_layout(self):
        root = self.base / "installed"
        state = root / "deploy" / "state"
        releases = root / "deploy" / "releases"
        state.mkdir(parents=True)
        releases.mkdir(parents=True)
        release = "2026.11.04-6"
        environment_path = releases / f"{release}.env"
        environment_path.write_text("placeholder\n")
        (state / "active.env").symlink_to(f"../releases/{release}.env")
        environment = {
            "POSTGRES_IMAGE": f"postgres@sha256:{'3' * 64}",
            "WEATHER_RELEASE": release,
            "WEATHER_SERVER_IMAGE": f"registry.example/server@sha256:{DIGEST}",
        }
        payloads = {
            state / "current-release": f"{release}\n".encode(),
            state / "adjustment-candidate-catalog.json": b"{}\n",
            state / "adjustment-capture-epoch.json": b"{}\n",
        }

        # return the fixed installed state bytes by canonical path
        def read_bytes(path, *_args, **_kwargs):
            return payloads[path]

        with (
            patch.object(fixture, "require_path"),
            patch.object(fixture, "require_canonical"),
            patch.object(fixture, "read_regular_bytes", side_effect=read_bytes),
            patch.object(
                fixture,
                "read_deployment_environment",
                return_value=(environment, "4" * 64),
            ),
        ):
            actual, environment_sha, installed = fixture.validate_deployment_root(
                root, installed_root=root
            )
        self.assertEqual(actual, environment)
        self.assertEqual(environment_sha, "4" * 64)
        self.assertEqual(
            installed["activeEnvironment"]["target"], f"../releases/{release}.env"
        )

    # create PostgreSQL without publishing a port or running project entrypoints
    def test_postgres_command_uses_only_restored_volume_and_default_entrypoint(self):
        image = f"postgres@sha256:{DIGEST}"
        command = fixture.build_postgres_command(
            self.marker,
            self.handoff,
            image,
            "fixture-network",
            "fixture-postgres",
        )
        joined = "\n".join(command)
        self.assertIn("/usr/local/bin/docker-entrypoint.sh", command)
        self.assertIn(
            f"type=volume,src={self.target},dst=/var/lib/postgresql/data,volume-nocopy",
            command,
        )
        self.assertNotIn("--publish", command)
        self.assertNotIn("migrate", joined)
        self.assertNotIn("weather-postgres-entrypoint", joined)
        self.assertEqual(command[-4:], [image, "postgres", "-c", "listen_addresses=*"])

    # publish only the API on loopback and omit every write-side service
    def test_api_command_is_read_only_loopback_and_has_no_maintenance_listener(self):
        environment = {
            "WEATHER_RELEASE": "2026.11.04-6",
            "WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH": "1",
            "WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH": "0",
        }
        command = fixture.build_api_command(
            self.marker,
            self.handoff,
            environment,
            f"registry.example/weather-server@sha256:{DIGEST}",
            "fixture-network",
            "fixture-api",
            13001,
            Path("/runtime/api-password"),
            Path("/runtime/catalog.json"),
            Path("/runtime/capture.json"),
        )
        joined = "\n".join(map(str, command))
        self.assertIn("127.0.0.1:13001:3001", command)
        self.assertIn("api", command)
        self.assertIn("--read-only", command)
        self.assertIn("--init", command)
        self.assertIn("node", command)
        self.assertIn("apps/api/dist/main.js", command)
        self.assertNotIn("WEATHER_ADJUSTMENT_MAINTENANCE_INTERNAL_PORT", joined)
        self.assertNotIn("WEATHER_ADJUSTMENT_MAINTENANCE_ARCHIVE_ORIGIN", joined)
        self.assertNotIn("worker", joined)
        self.assertNotIn("deploy/scripts/migrate.mjs", joined)

    # create the optional web edge without production data credentials or egress
    def test_web_command_is_tmpfs_only_and_credential_free(self):
        image = f"registry.example/weather-web@sha256:{DIGEST}"
        command = fixture.build_web_command(
            self.marker,
            {"WEATHER_RELEASE": "2026.11.04-6"},
            image,
            "fixture-network",
            "fixture-web",
        )
        joined = "\n".join(command)
        self.assertEqual(command[-3:], [image, "node", "deploy/scripts/web-server.mjs"])
        self.assertIn("384m", command)
        self.assertIn("64m", command)
        self.assertIn("0.25", command)
        self.assertIn("64", command)
        self.assertIn("/tmp:size=16m,mode=1777", command)
        self.assertIn(
            "/run/weather-web-state:size=16m,mode=0700,uid=10002,gid=10002",
            command,
        )
        self.assertIn("WEATHER_API_ORIGIN=http://api:3001", command)
        self.assertNotIn("--publish", command)
        self.assertNotIn("/var/lib/weather", joined)
        self.assertNotIn("/run/secrets", joined)
        self.assertNotIn("WEATHER_ADJUSTMENT_MAINTENANCE_INTERNAL_PORT", joined)
        self.assertNotIn("WEATHER_ADJUSTMENT_SCHEDULED_CAPTURE_ENABLED", joined)

    # bind fixture web command and inherited entrypoint to production identity
    def test_web_launch_identity_rejects_production_command_mismatch(self):
        image = f"registry.example/weather-web@sha256:{DIGEST}"
        expected_command = ["node", "deploy/scripts/web-server.mjs"]
        image_record = {
            "Config": {
                "Cmd": expected_command,
                "Entrypoint": ["docker-entrypoint.sh"],
            }
        }
        production_record = {
            "Config": {
                "Cmd": expected_command,
                "Entrypoint": ["docker-entrypoint.sh"],
            }
        }
        command = fixture.build_web_command(
            self.marker,
            {"WEATHER_RELEASE": "2026.11.04-6"},
            image,
            "fixture-network",
            "fixture-web",
        )
        self.assertEqual(
            fixture.validate_web_launch_identity(
                image_record, production_record, command, image
            ),
            ["docker-entrypoint.sh"],
        )
        production_record["Config"]["Cmd"] = ["node", "unexpected.mjs"]
        with self.assertRaises(fixture.FixtureError) as raised:
            fixture.validate_web_launch_identity(
                image_record, production_record, command, image
            )
        differences = json.loads(str(raised.exception).split(": ", 1)[1])
        self.assertEqual(set(differences), {"Production.Config.Cmd"})

    # require a final PostgreSQL pid one before accepting readiness
    def test_postgres_readiness_checks_final_pid_one(self):
        calls = []

        # become ready on the second final-postmaster probe
        def command(argv, **_kwargs):
            calls.append(argv)
            return result(returncode=1 if len(calls) == 1 else 0)

        ticks = iter([0, 0, 0])
        fixture.wait_postgres(
            "a" * 64,
            10,
            command=command,
            monotonic=lambda: next(ticks),
            sleeper=lambda _seconds: None,
        )
        self.assertEqual(len(calls), 2)
        self.assertIn('test "$(cat /proc/1/comm)" = postgres', calls[0][5])

    # require database and migration readiness from the API probe
    def test_api_readiness_checks_database_contract(self):
        calls = []

        # expose one successful in-container health probe
        def command(argv, **_kwargs):
            calls.append(argv)
            return result()

        fixture.wait_api(
            "b" * 64,
            10,
            command=command,
            monotonic=lambda: 0,
            sleeper=lambda _seconds: None,
        )
        probe = calls[0][-1]
        self.assertIn("database!=='ready'", probe)
        self.assertIn("migration?.status!=='current'", probe)

    # bind the running API to one owned bridge address and IPAM subnet
    def test_api_internal_endpoint_proves_network_identity(self):
        api_id = "a" * 64
        postgres_id = "b" * 64
        endpoint_id = "c" * 64
        network_id = "d" * 64
        network_name = "fixture-network"
        api_name = "fixture-api"
        postgres_name = "fixture-postgres"
        container_record = {
            "Config": {
                "Labels": fixture.fixture_labels(self.marker, "node-baseline-api")
            },
            "Id": api_id,
            "Image": IMAGE_ID,
            "Name": f"/{api_name}",
            "NetworkSettings": {
                "Networks": {
                    network_name: {
                        "Aliases": ["api"],
                        "EndpointID": endpoint_id,
                        "IPAddress": "172.30.0.3",
                        "IPPrefixLen": 24,
                        "NetworkID": network_id,
                    }
                }
            },
            "State": {"Running": True},
        }
        network_record = {
            "Containers": {
                api_id: {"IPv4Address": "172.30.0.3/24", "Name": api_name},
                postgres_id: {
                    "IPv4Address": "172.30.0.2/24",
                    "Name": postgres_name,
                },
            },
            "Driver": "bridge",
            "IPAM": {"Config": [{"Subnet": "172.30.0.0/24"}]},
            "Id": network_id,
            "Internal": True,
            "Labels": fixture.fixture_labels(self.marker, "node-baseline-network"),
            "Name": network_name,
            "Scope": "local",
        }

        # return the two immutable Docker inspection records
        def command(argv, **_kwargs):
            # return the owned running API record
            if argv[1:3] == ["container", "inspect"]:
                return docker_record(container_record)
            # return the owned internal bridge record
            if argv[1:3] == ["network", "inspect"]:
                return docker_record(network_record)
            self.fail(f"unexpected inspection command: {argv}")

        endpoint = fixture.inspect_api_internal_endpoint(
            api_id,
            api_name,
            postgres_id,
            postgres_name,
            self.marker,
            IMAGE_ID,
            network_id,
            network_name,
            command,
        )
        self.assertEqual(
            endpoint,
            {
                "endpointId": endpoint_id,
                "ipv4": "172.30.0.3",
                "port": 3001,
                "subnet": "172.30.0.0/24",
            },
        )
        container_record["NetworkSettings"]["Networks"][network_name]["IPAddress"] = (
            "172.31.0.3"
        )
        with self.assertRaisesRegex(fixture.FixtureError, "outside exact bridge"):
            fixture.inspect_api_internal_endpoint(
                api_id,
                api_name,
                postgres_id,
                postgres_name,
                self.marker,
                IMAGE_ID,
                network_id,
                network_name,
                command,
            )

    # prove the optional web endpoint and API alias share only the fixture bridge
    def test_web_internal_endpoint_requires_three_owned_members(self):
        api_id = "a" * 64
        postgres_id = "b" * 64
        web_id = "c" * 64
        network_id = "d" * 64
        api_name = "fixture-api"
        postgres_name = "fixture-postgres"
        web_name = "fixture-web"
        network_name = "fixture-network"

        # build one running endpoint inspection record
        def container_record(address, endpoint, aliases=None):
            return {
                "NetworkSettings": {
                    "Networks": {
                        network_name: {
                            "Aliases": aliases,
                            "EndpointID": endpoint,
                            "IPAddress": address,
                            "IPPrefixLen": 24,
                            "NetworkID": network_id,
                        }
                    }
                },
                "State": {"Running": True},
            }

        web_record = container_record("172.30.0.4", "e" * 64)
        api_record = container_record("172.30.0.3", "f" * 64, ["api"])
        network_record = {
            "Containers": {
                api_id: {"IPv4Address": "172.30.0.3/24", "Name": api_name},
                postgres_id: {
                    "IPv4Address": "172.30.0.2/24",
                    "Name": postgres_name,
                },
                web_id: {"IPv4Address": "172.30.0.4/24", "Name": web_name},
            },
            "IPAM": {"Config": [{"Subnet": "172.30.0.0/24"}]},
        }
        with (
            patch.object(
                fixture,
                "inspect_container",
                side_effect=[web_record, api_record],
            ),
            patch.object(fixture, "inspect_network", return_value=network_record),
        ):
            endpoint = fixture.inspect_web_internal_endpoint(
                web_id,
                web_name,
                IMAGE_ID,
                api_id,
                api_name,
                IMAGE_ID,
                postgres_id,
                postgres_name,
                self.marker,
                network_id,
                network_name,
            )
        self.assertEqual(endpoint["ipv4"], "172.30.0.4")
        self.assertEqual(endpoint["port"], 3000)

    # require the direct host probe to return database-backed readiness
    def test_internal_host_probe_uses_bridge_ipv4_health(self):
        requests = []
        closed = []
        response = SimpleNamespace(
            status=200,
            read=lambda _limit: json.dumps(
                {"data": {"database": "ready", "migration": {"status": "current"}}}
            ).encode(),
        )

        # record the bounded read-only health request
        def request(method, path, headers):
            requests.append((method, path, headers))

        # return one bounded healthy response
        def getresponse():
            return response

        # record deterministic connection cleanup
        def close():
            closed.append(True)

        connection = SimpleNamespace(
            close=close,
            getresponse=getresponse,
            request=request,
        )

        # bind the probe directly to the exact IPv4 and API port
        def connection_factory(address, port, timeout):
            self.assertEqual((address, port, timeout), ("172.30.0.3", 3001, 5))
            return connection

        proof = fixture.probe_internal_api_from_host("172.30.0.3", connection_factory)
        self.assertEqual(proof["status"], 200)
        self.assertEqual(requests[0][0:2], ("GET", "/api/v1/health"))
        self.assertEqual(closed, [True])

    # require only the API edge GET without warming an ordinary page
    def test_internal_web_probe_avoids_page_prefetch(self):
        requests = []
        responses = iter(
            [
                (
                    200,
                    "application/json; charset=utf-8",
                    json.dumps(
                        {
                            "data": {
                                "database": "ready",
                                "migration": {"status": "current"},
                            }
                        }
                    ).encode(),
                ),
            ]
        )

        # create one closed fake HTTP connection for each GET
        def connection_factory(address, port, timeout):
            self.assertEqual((address, port, timeout), ("172.30.0.4", 3000, 5))
            status, content_type, body = next(responses)

            # retain the exact read-only request path
            def request(method, path, headers):
                requests.append((method, path, headers))

            # return one bounded public response
            def getresponse():
                return SimpleNamespace(
                    getheader=lambda name: (
                        content_type if name == "Content-Type" else None
                    ),
                    read=lambda _limit: body,
                    status=status,
                )

            return SimpleNamespace(
                close=lambda: None,
                getresponse=getresponse,
                request=request,
            )

        proof = fixture.probe_internal_web_from_host("172.30.0.4", connection_factory)
        self.assertEqual(proof["apiEdgeStatus"], 200)
        self.assertEqual(
            [request[0:2] for request in requests],
            [("GET", "/api/v1/health")],
        )

    # validate exactly five detached bind volumes and one stopped cluster
    def test_volume_set_requires_five_detached_slots_and_stopped_target(self):
        names = [
            f"{self.marker['volumeNamespacePrefix']}slot-{index:02d}"
            for index in range(1, 6)
        ]
        # create one initialized target and four empty slots
        for name in names:
            path = self.volume_root / name
            path.mkdir()
            # mark only the selected slot initialized
            if name == self.target:
                (path / "PG_VERSION").write_text("17\n")

        # return exact Docker volume metadata and no attachments
        def command(argv, **_kwargs):
            # list the fixed labeled namespace
            if argv[1:3] == ["volume", "ls"]:
                return result(("\n".join(names) + "\n").encode())
            # inspect one bind volume
            if argv[1:3] == ["volume", "inspect"]:
                name = argv[-1]
                device = self.volume_root / name
                return docker_record(
                    {
                        "Driver": "local",
                        "Labels": {
                            "weather.native-benchmark.fixture-id": FIXTURE_ID,
                            "weather.native-benchmark.kind": fixture.VOLUME_KIND,
                        },
                        "Name": name,
                        "Options": {"device": str(device), "o": "bind", "type": "none"},
                        "Scope": "local",
                    }
                )
            # prove no container attaches any slot
            if argv[1:3] == ["container", "ls"]:
                return result()
            self.fail(f"unexpected command: {argv}")

        self.assertEqual(
            fixture.inspect_volume_set(self.marker, self.handoff, command),
            self.volume_root / self.target,
        )
        (self.volume_root / names[1] / "unexpected").write_text("data")
        with self.assertRaisesRegex(fixture.FixtureError, "not empty"):
            fixture.inspect_volume_set(self.marker, self.handoff, command)

    # reject an external or relabeled network before any lifecycle action
    def test_network_identity_requires_internal_exact_labels(self):
        network_id = "c" * 64
        record = {
            "Driver": "bridge",
            "Id": network_id,
            "Internal": True,
            "Labels": {
                "weather.native-benchmark.fixture-id": FIXTURE_ID,
                "weather.native-benchmark.kind": fixture.KIND,
                "weather.native-benchmark.role": "node-baseline-network",
            },
            "Name": "fixture-network",
            "Scope": "local",
        }
        command = lambda *_args, **_kwargs: docker_record(record)
        self.assertTrue(
            fixture.inspect_network(
                network_id,
                "fixture-network",
                FIXTURE_ID,
                command,
            )["Internal"]
        )
        record["Internal"] = False
        with self.assertRaisesRegex(fixture.FixtureError, "network ownership"):
            fixture.inspect_network(network_id, "fixture-network", FIXTURE_ID, command)

    # allow inherited OCI labels while closing the fixture label namespace
    def test_container_identity_accepts_oci_label_and_rejects_extra_fixture_label(self):
        container_id = "d" * 64
        labels = {
            "org.opencontainers.image.revision": "source-revision",
            "weather.native-benchmark.fixture-id": FIXTURE_ID,
            "weather.native-benchmark.kind": fixture.KIND,
            "weather.native-benchmark.role": "node-baseline-api",
        }
        record = {
            "Config": {"Labels": labels},
            "Id": container_id,
            "Image": IMAGE_ID,
            "Name": "/fixture-api",
        }
        command = lambda *_args, **_kwargs: docker_record(record)
        self.assertEqual(
            fixture.inspect_container(
                container_id,
                "fixture-api",
                self.marker,
                IMAGE_ID,
                "node-baseline-api",
                command,
            )["Id"],
            container_id,
        )
        labels["weather.native-benchmark.unexpected"] = "foreign"
        with self.assertRaisesRegex(fixture.FixtureError, "ownership proof"):
            fixture.inspect_container(
                container_id,
                "fixture-api",
                self.marker,
                IMAGE_ID,
                "node-baseline-api",
                command,
            )

    # accept documented no-new-privileges forms and name only failed controls
    def test_host_constraints_accept_security_equivalents_and_diagnose_fields(self):
        host = {
            "CapDrop": ["ALL"],
            "LogConfig": {
                "Config": {"max-file": "3", "max-size": "10m"},
                "Type": "json-file",
            },
            "Memory": 256 * 1024**2,
            "NanoCpus": 500_000_000,
            "NetworkMode": "fixture-network",
            "PidsLimit": 96,
            "ReadonlyRootfs": True,
            "RestartPolicy": {"MaximumRetryCount": 0, "Name": "no"},
        }
        # accept only Docker's three documented equivalent spellings
        for security_option in (
            "no-new-privileges",
            "no-new-privileges=true",
            "no-new-privileges:true",
        ):
            host["SecurityOpt"] = [security_option]
            fixture.validate_host_constraints(
                {"HostConfig": host},
                "fixture-network",
                256 * 1024**2,
                500_000_000,
                96,
            )
        host["Memory"] = 1
        host["SecurityOpt"] = ["no-new-privileges=false"]
        with self.assertRaises(fixture.FixtureError) as raised:
            fixture.validate_host_constraints(
                {"HostConfig": host},
                "fixture-network",
                256 * 1024**2,
                500_000_000,
                96,
            )
        differences = json.loads(str(raised.exception).split(": ", 1)[1])
        self.assertEqual(
            set(differences),
            {"HostConfig.Memory", "HostConfig.SecurityOpt"},
        )
        self.assertEqual(differences["HostConfig.Memory"]["observed"], 1)
        self.assertEqual(
            differences["HostConfig.SecurityOpt"]["observed"],
            ["no-new-privileges=false"],
        )

    # identify exact PostgreSQL mount and command fields without record dumping
    def test_postgres_validation_diagnoses_command_field(self):
        volume_name = f"weather-native-benchmark-{FIXTURE_ID}-slot-01"
        record = {
            "Args": ["postgres", "-c", "listen_addresses=*"],
            "Config": {"User": "999:999"},
            "HostConfig": {
                "CapDrop": ["ALL"],
                "LogConfig": {
                    "Config": {"max-file": "3", "max-size": "10m"},
                    "Type": "json-file",
                },
                "Memory": 512 * 1024**2,
                "NanoCpus": 1_000_000_000,
                "NetworkMode": "fixture-network",
                "PidsLimit": 128,
                "PortBindings": {},
                "ReadonlyRootfs": True,
                "RestartPolicy": {"MaximumRetryCount": 0, "Name": "no"},
                "SecurityOpt": ["no-new-privileges"],
            },
            "Mounts": [
                {
                    "Destination": "/var/lib/postgresql/data",
                    "Name": volume_name,
                    "RW": True,
                    "Type": "volume",
                }
            ],
            "Path": "docker-entrypoint.sh",
        }
        with self.assertRaises(fixture.FixtureError) as raised:
            fixture.validate_postgres_container(record, "fixture-network", volume_name)
        differences = json.loads(str(raised.exception).split(": ", 1)[1])
        self.assertEqual(set(differences), {"Path"})
        self.assertEqual(differences["Path"]["observed"], "docker-entrypoint.sh")

    # bind API runtime limits loopback and the exact three snapshot mounts
    def test_api_container_validation_rejects_non_loopback_publication(self):
        secret = Path("/runtime/password")
        catalog = Path("/runtime/catalog")
        capture = Path("/runtime/capture")
        record = {
            "Config": {
                "Cmd": ["node", "apps/api/dist/main.js"],
                "Env": ["WEATHER_API_PORT=3001"],
                "User": "10002:10002",
            },
            "HostConfig": {
                "CapDrop": ["ALL"],
                "Init": True,
                "LogConfig": {
                    "Config": {"max-file": "3", "max-size": "10m"},
                    "Type": "json-file",
                },
                "Memory": 256 * 1024**2,
                "NanoCpus": 500_000_000,
                "NetworkMode": "fixture-network",
                "PidsLimit": 96,
                "PortBindings": {
                    "3001/tcp": [{"HostIp": "127.0.0.1", "HostPort": "13001"}],
                },
                "ReadonlyRootfs": True,
                "RestartPolicy": {"MaximumRetryCount": 0, "Name": "no"},
                "SecurityOpt": ["no-new-privileges:true"],
            },
            "Mounts": [
                {
                    "Destination": "/run/secrets/weather_api_password",
                    "RW": False,
                    "Source": str(secret),
                    "Type": "bind",
                },
                {
                    "Destination": "/run/weather/adjustment-candidate-catalog.json",
                    "RW": False,
                    "Source": str(catalog),
                    "Type": "bind",
                },
                {
                    "Destination": "/run/weather/adjustment-capture-epoch.json",
                    "RW": False,
                    "Source": str(capture),
                    "Type": "bind",
                },
            ],
        }
        fixture.validate_api_container(
            record, "fixture-network", 13001, secret, catalog, capture
        )
        record["HostConfig"]["PortBindings"]["3001/tcp"][0]["HostIp"] = "0.0.0.0"
        with self.assertRaisesRegex(fixture.FixtureError, "publication") as raised:
            fixture.validate_api_container(
                record, "fixture-network", 13001, secret, catalog, capture
            )
        differences = json.loads(str(raised.exception).split(": ", 1)[1])
        self.assertEqual(set(differences), {"HostConfig.PortBindings"})

    # validate the optional web runtime's closed tmpfs and environment surface
    def test_web_container_validation_rejects_production_mount(self):
        environment = {"WEATHER_RELEASE": "2026.11.04-6"}
        fixture_environment = {
            "WEATHER_ADJUSTMENT_EVIDENCE_ROOT": (
                "/run/weather-web-state/adjustment-evidence"
            ),
            "WEATHER_ADMIN_AUTH_PATH": "/run/weather-web-state/admin-auth.json",
            "WEATHER_ADMIN_BOOTSTRAP_TOKEN_PATH": (
                "/run/weather-web-state/admin-bootstrap-disabled"
            ),
            "WEATHER_API_ORIGIN": "http://api:3001",
            "WEATHER_PROPERTY_SENSOR_LAYOUT_PATH": (
                "/run/weather-web-state/property-sensor-layout.json"
            ),
            "WEATHER_RELEASE": environment["WEATHER_RELEASE"],
            "WEATHER_SITE_CONFIG_PATH": "/opt/weather/config/sites/ballydidean.json",
            "WEATHER_XWEATHER_CLIENT_ID_FILE": (
                "/run/weather-web-state/xweather-client-id-disabled"
            ),
            "WEATHER_XWEATHER_CLIENT_SECRET_FILE": (
                "/run/weather-web-state/xweather-client-secret-disabled"
            ),
            "WEATHER_XWEATHER_MAP_ORIGIN": "http://127.0.0.1:9",
            "WEATHER_XWEATHER_USAGE_PATH": (
                "/run/weather-web-state/xweather-usage.json"
            ),
        }
        # serialize the closed nonsecret Weather environment
        fixture_assignments = [
            f"{name}={value}" for name, value in fixture_environment.items()
        ]
        record = {
            "Config": {
                "Cmd": ["node", "deploy/scripts/web-server.mjs"],
                "Env": [
                    "HOST=0.0.0.0",
                    "NODE_ENV=production",
                    "PORT=3000",
                    "WEATHER_WEB_REVISION=" + "1" * 40,
                    *fixture_assignments,
                ],
                "User": "10002:10002",
            },
            "HostConfig": {
                "CapDrop": ["ALL"],
                "Init": True,
                "LogConfig": {
                    "Config": {"max-file": "3", "max-size": "10m"},
                    "Type": "json-file",
                },
                "Memory": 384 * 1024**2,
                "MemoryReservation": 64 * 1024**2,
                "NanoCpus": 250_000_000,
                "NetworkMode": "fixture-network",
                "PidsLimit": 64,
                "PortBindings": {},
                "ReadonlyRootfs": True,
                "RestartPolicy": {"MaximumRetryCount": 0, "Name": "no"},
                "SecurityOpt": ["no-new-privileges"],
                "Tmpfs": {
                    "/run/weather-web-state": (
                        "size=16m,mode=0700,uid=10002,gid=10002"
                    ),
                    "/tmp": "size=16m,mode=1777",
                },
            },
            "Mounts": [],
        }
        fixture.validate_web_container(record, "fixture-network", environment)
        record["Mounts"] = [
            {
                "Destination": "/var/lib/weather/xweather",
                "Source": "/var/lib/weather/xweather",
                "Type": "bind",
            }
        ]
        with self.assertRaises(fixture.FixtureError) as raised:
            fixture.validate_web_container(record, "fixture-network", environment)
        differences = json.loads(str(raised.exception).split(": ", 1)[1])
        self.assertEqual(set(differences), {"Mounts"})

    # remove the web edge before the API PostgreSQL and network
    def test_cleanup_uses_dependency_order_and_exact_identities(self):
        calls = []
        web_name = f"weather-native-{FIXTURE_ID}-web"
        api_name = f"weather-native-{FIXTURE_ID}-api"
        postgres_name = f"weather-native-{FIXTURE_ID}-postgres"
        network_name = f"weather-native-benchmark-{FIXTURE_ID}-node-api"
        journal = {
            "resources": {
                "web": {
                    "id": "d" * 64,
                    "imageId": IMAGE_ID,
                    "labels": fixture.fixture_labels(self.marker, "node-baseline-web"),
                    "name": web_name,
                    "role": "node-baseline-web",
                    "state": "created",
                },
                "api": {
                    "id": "a" * 64,
                    "imageId": IMAGE_ID,
                    "labels": fixture.fixture_labels(self.marker, "node-baseline-api"),
                    "name": api_name,
                    "role": "node-baseline-api",
                    "state": "created",
                },
                "postgres": {
                    "id": "b" * 64,
                    "imageId": IMAGE_ID,
                    "labels": fixture.fixture_labels(
                        self.marker, "node-baseline-postgres"
                    ),
                    "name": postgres_name,
                    "role": "node-baseline-postgres",
                    "state": "created",
                },
                "network": {
                    "id": "c" * 64,
                    "labels": fixture.fixture_labels(
                        self.marker, "node-baseline-network"
                    ),
                    "name": network_name,
                    "role": "node-baseline-network",
                    "state": "created",
                },
            },
        }

        # record one proven container cleanup
        def remove_container(*args, **_kwargs):
            calls.append(("container", args[1]))

        # record one immutable container inspection
        def inspect_container(*args, **_kwargs):
            calls.append(("container-inspect", args[1]))

        # record one proven network inspection
        def inspect_network(*args, **_kwargs):
            calls.append(("network-inspect", args[1]))

        # record one network removal
        def command(argv, **_kwargs):
            calls.append(("command", argv[1]))
            return result()

        with (
            patch.object(fixture, "remove_container", side_effect=remove_container),
            patch.object(fixture, "inspect_container", side_effect=inspect_container),
            patch.object(fixture, "inspect_network", side_effect=inspect_network),
        ):
            self.assertEqual(fixture.cleanup_runtime(journal, self.marker, command), [])
        self.assertEqual(
            calls,
            [
                ("container-inspect", web_name),
                ("container", web_name),
                ("container-inspect", api_name),
                ("container", api_name),
                ("container-inspect", postgres_name),
                ("container", postgres_name),
                ("network-inspect", network_name),
                ("command", "network"),
            ],
        )

    # retain a planned container record when image reconciliation is uncertain
    def test_planned_container_reconciliation_rejects_wrong_image(self):
        container_id = "d" * 64
        api_name = f"weather-native-{FIXTURE_ID}-api"
        journal = {
            "resources": {
                "api": {
                    "imageId": IMAGE_ID,
                    "labels": fixture.fixture_labels(self.marker, "node-baseline-api"),
                    "name": api_name,
                    "role": "node-baseline-api",
                    "state": "planned",
                }
            }
        }
        journal_before = json.loads(json.dumps(journal))
        calls = []

        # expose one exact-name object with a mismatched immutable image
        def command(argv, **_kwargs):
            calls.append(argv[1:3])
            # return the only deterministic-name container match
            if argv[1:3] == ["container", "ls"]:
                self.assertEqual(argv[-1], f"name=^/{api_name}$")
                return result(f"{container_id}\n".encode())
            # preserve unrelated OCI labels while mismatching the image id
            if argv[1:3] == ["container", "inspect"]:
                return docker_record(
                    {
                        "Config": {
                            "Labels": {
                                **fixture.fixture_labels(
                                    self.marker, "node-baseline-api"
                                ),
                                "org.opencontainers.image.revision": "deployed",
                            }
                        },
                        "Id": container_id,
                        "Image": f"sha256:{'3' * 64}",
                        "Name": f"/{api_name}",
                    }
                )
            self.fail(f"unexpected cleanup command: {argv}")

        errors = fixture.cleanup_runtime(journal, self.marker, command)
        self.assertEqual(len(errors), 1)
        self.assertIn("ownership proof differs", errors[0])
        self.assertEqual(journal, journal_before)
        self.assertEqual(calls, [["container", "ls"], ["container", "inspect"]])

    # journal every created immutable id before its first inspection can fail
    def test_start_journals_each_resource_before_inspection(self):
        postgres_reference = f"postgres@sha256:{'3' * 64}"
        server_reference = f"registry.example/server@sha256:{DIGEST}"
        web_reference = f"registry.example/web@sha256:{'2' * 64}"
        postgres_id = "e" * 64
        server_id = "f" * 64
        web_image_id = "1" * 64
        resource_ids = {
            "network": "a" * 64,
            "postgres": "b" * 64,
            "api": "c" * 64,
            "web": "d" * 64,
        }
        # inject failures after creates and before one create returns its id
        for boundary in (
            "network",
            "network-timeout",
            "postgres",
            "api",
            "web-timeout",
        ):
            with (
                self.subTest(boundary=boundary),
                tempfile.TemporaryDirectory() as runtime,
            ):
                runtime_base = Path(runtime)
                runtime_root = runtime_base / f"weather-native-benchmark-{FIXTURE_ID}"
                runtime_root.mkdir(mode=0o700)
                marker = dict(self.marker)
                marker["fixtureRoot"] = str(self.fixture_root)
                handoff = {
                    "databaseName": "weather",
                    "imageId": postgres_id,
                    "imageRepoDigest": postgres_reference,
                    "volumeName": self.target,
                }
                environment = {
                    "POSTGRES_IMAGE": postgres_reference,
                    "WEATHER_RELEASE": "2026.11.04-6",
                    "WEATHER_SERVER_IMAGE": server_reference,
                    "WEATHER_WEB_IMAGE": web_reference,
                }
                installed = {
                    "activeEnvironment": {
                        "path": Path("/unused/active.env"),
                        "target": "../releases/2026.11.04-6.env",
                    },
                    "candidateCatalog": {
                        "bytes": b"{}\n",
                        "path": Path("/unused/catalog"),
                        "sha256": "4" * 64,
                    },
                    "captureEpoch": {
                        "bytes": b"{}\n",
                        "path": Path("/unused/capture"),
                        "sha256": "5" * 64,
                    },
                    "currentRelease": {
                        "bytes": b"2026.11.04-6\n",
                        "path": Path("/unused/current-release"),
                        "sha256": "6" * 64,
                    },
                }
                args = SimpleNamespace(
                    deployment_root="/opt/weather/current",
                    fixture_root=str(self.fixture_root),
                    listen_port=13001,
                    profile=(
                        fixture.PROFILE_API_WEB
                        if boundary == "web-timeout"
                        else fixture.PROFILE_API_ONLY
                    ),
                    ready_timeout_seconds=180,
                )
                observed_journals = []
                removed_networks = []
                original_cleanup_runtime = fixture.cleanup_runtime
                original_inspect_network = fixture.inspect_network

                # return immutable ids for each create and accept starts
                def command(
                    argv,
                    selected_boundary=boundary,
                    selected_marker=marker,
                    selected_removed_networks=removed_networks,
                    **_kwargs,
                ):
                    # create one fixture network identity
                    if argv[1:3] == ["network", "create"]:
                        # simulate daemon success hidden by one client timeout
                        if selected_boundary == "network-timeout":
                            raise subprocess.TimeoutExpired(
                                "injected network create", 60
                            )
                        return result(f"{resource_ids['network']}\n".encode())
                    # create one fixture container identity
                    if argv[1] == "create":
                        name = argv[argv.index("--name") + 1]
                        identity = (
                            "postgres"
                            if name.endswith("-postgres")
                            else "web"
                            if name.endswith("-web")
                            else "api"
                        )
                        # simulate one web object hidden by a client timeout
                        if selected_boundary == "web-timeout" and identity == "web":
                            raise subprocess.TimeoutExpired("injected web create", 120)
                        return result(f"{resource_ids[identity]}\n".encode())
                    # accept one owned container start
                    if argv[1] == "start":
                        return result()
                    # reconcile the deterministic network after the timeout
                    if argv[1:3] == ["network", "ls"]:
                        return result(f"{resource_ids['network']}\n".encode())
                    # prove the reconciled network's exact identity
                    if argv[1:3] == ["network", "inspect"]:
                        return docker_record(
                            {
                                "Driver": "bridge",
                                "Id": resource_ids["network"],
                                "Internal": True,
                                "Labels": fixture.fixture_labels(
                                    selected_marker, "node-baseline-network"
                                ),
                                "Name": (
                                    f"weather-native-benchmark-{FIXTURE_ID}-node-api"
                                ),
                                "Scope": "local",
                            }
                        )
                    # record removal of only the reconciled immutable id
                    if argv[1:3] == ["network", "rm"]:
                        selected_removed_networks.append(argv[-1])
                        return result()
                    self.fail(f"unexpected command: {argv}")

                # fail only the selected network ownership inspection
                def inspect_network(
                    *_args,
                    selected_boundary=boundary,
                    selected_inspector=original_inspect_network,
                    **_kwargs,
                ):
                    # use the real proof path for timeout reconciliation
                    if selected_boundary == "network-timeout":
                        return selected_inspector(*_args, **_kwargs)
                    # inject the selected network boundary failure
                    if selected_boundary == "network":
                        raise fixture.FixtureError(
                            "injected network inspection failure"
                        )
                    return {}

                # fail only the selected container ownership inspection
                def inspect_container(
                    _identifier,
                    _name,
                    _marker,
                    _image,
                    role,
                    _command,
                    selected_boundary=boundary,
                ):
                    selected = (
                        "postgres"
                        if role.endswith("postgres")
                        else "web"
                        if role.endswith("web")
                        else "api"
                    )
                    # inject the selected container boundary failure
                    if selected_boundary == selected:
                        raise fixture.FixtureError(
                            f"injected {selected} inspection failure"
                        )
                    return {}

                # capture the fsynced journal observed by failure cleanup
                def cleanup_runtime(
                    _journal,
                    _marker,
                    _command,
                    observations=observed_journals,
                    selected_runtime_root=runtime_root,
                    selected_boundary=boundary,
                    selected_cleanup=original_cleanup_runtime,
                ):
                    observations.append(
                        json.loads(
                            (
                                selected_runtime_root
                                / fixture.SESSION_DIRECTORY_NAME
                                / fixture.JOURNAL_NAME
                            ).read_text()
                        )
                    )
                    # exercise real planned-resource reconciliation after timeout
                    if selected_boundary == "network-timeout":
                        return selected_cleanup(_journal, _marker, _command)
                    return []

                patchers = (
                    patch.object(fixture.os, "geteuid", return_value=0),
                    patch.object(fixture, "RUNTIME_BASE", runtime_base),
                    patch.object(fixture, "require_path"),
                    patch.object(fixture, "require_canonical"),
                    patch.object(
                        fixture, "validate_marker", return_value=(marker, "7" * 64)
                    ),
                    patch.object(
                        fixture,
                        "validate_restore_handoff",
                        return_value=(handoff, "8" * 64),
                    ),
                    patch.object(
                        fixture,
                        "validate_credentials",
                        return_value=self.fixture_root
                        / ".restore-secrets-0123456789abcdef",
                    ),
                    patch.object(
                        fixture,
                        "inspect_volume_set",
                        return_value=self.volume_root / self.target,
                    ),
                    patch.object(
                        fixture,
                        "validate_deployment_root",
                        return_value=(environment, "9" * 64, installed),
                    ),
                    patch.object(
                        fixture,
                        "inspect_image",
                        side_effect=[
                            {"Id": postgres_id},
                            {"Id": server_id},
                            *(
                                [
                                    {
                                        "Config": {
                                            "Cmd": [
                                                "node",
                                                "deploy/scripts/web-server.mjs",
                                            ],
                                            "Entrypoint": ["docker-entrypoint.sh"],
                                        },
                                        "Id": web_image_id,
                                    }
                                ]
                                if boundary == "web-timeout"
                                else []
                            ),
                        ],
                    ),
                    patch.object(
                        fixture,
                        "inspect_production_container",
                        return_value={
                            "Config": {
                                "Cmd": ["node", "deploy/scripts/web-server.mjs"],
                                "Entrypoint": ["docker-entrypoint.sh"],
                            }
                        },
                    ),
                    patch.object(fixture, "require_empty_runtime"),
                    patch.object(fixture, "write_snapshot"),
                    patch.object(
                        fixture, "read_regular_bytes", return_value=b"fixture-secret\n"
                    ),
                    patch.object(
                        fixture, "inspect_network", side_effect=inspect_network
                    ),
                    patch.object(
                        fixture, "inspect_container", side_effect=inspect_container
                    ),
                    patch.object(fixture, "validate_postgres_container"),
                    patch.object(fixture, "validate_api_container"),
                    patch.object(fixture, "wait_postgres"),
                    patch.object(fixture, "wait_api"),
                    patch.object(
                        fixture,
                        "inspect_api_internal_endpoint",
                        return_value={
                            "endpointId": "2" * 64,
                            "ipv4": "172.30.0.3",
                            "port": 3001,
                            "subnet": "172.30.0.0/24",
                        },
                    ),
                    patch.object(
                        fixture, "probe_internal_api_from_host", return_value={}
                    ),
                    patch.object(
                        fixture, "cleanup_runtime", side_effect=cleanup_runtime
                    ),
                    patch.object(fixture, "remove_session_directory"),
                )
                with ExitStack() as stack:
                    # activate the bounded start seams for this injected boundary
                    for patcher in patchers:
                        stack.enter_context(patcher)
                    stack.enter_context(
                        self.assertRaisesRegex(fixture.FixtureError, "injected")
                    )
                    fixture.start(args, command)
                journal = observed_journals[0]
                resource_name = boundary.removesuffix("-timeout")
                resource = journal["resources"][resource_name]
                # retain the pre-create plan when the client returns no id
                if boundary.endswith("-timeout"):
                    self.assertEqual(resource["state"], "planned")
                    self.assertNotIn("id", resource)
                    self.assertEqual(
                        removed_networks,
                        [resource_ids["network"]]
                        if boundary == "network-timeout"
                        else [],
                    )
                else:
                    self.assertEqual(resource["state"], "created")
                    self.assertEqual(resource["id"], resource_ids[resource_name])
                # bind container cleanup to the immutable image and name too
                if resource_name != "network":
                    expected_image = {
                        "api": server_id,
                        "postgres": postgres_id,
                        "web": web_image_id,
                    }[resource_name]
                    self.assertEqual(resource["imageId"], expected_image)
                    self.assertTrue(resource["name"].endswith(resource_name))

    # enforce loopback port and readiness bounds before dispatch
    def test_main_rejects_unsupported_start_bounds(self):
        with self.assertRaisesRegex(fixture.FixtureError, "outside supported bounds"):
            fixture.main(
                [
                    "start",
                    "--fixture-root",
                    "/fixture",
                    "--deployment-root",
                    "/release",
                    "--listen-port",
                    "80",
                ]
            )

    # keep API-only as default and admit only the explicit web profile
    def test_start_profile_defaults_api_only(self):
        base = [
            "start",
            "--fixture-root",
            "/fixture",
            "--deployment-root",
            "/release",
            "--listen-port",
            "13001",
        ]
        self.assertEqual(fixture.parse_args(base).profile, fixture.PROFILE_API_ONLY)
        self.assertEqual(
            fixture.parse_args([*base, "--profile", fixture.PROFILE_API_WEB]).profile,
            fixture.PROFILE_API_WEB,
        )

    # validate restored fixture-only credential custody and manifest
    def test_credentials_require_restore_manifest(self):
        directory = self.fixture_root / ".restore-secrets-0123456789abcdef"
        directory.mkdir()
        digests = {}
        # create the complete fixture-only secret set
        for name in fixture.SECRET_NAMES:
            value = f"{name}-secret\n".encode()
            path = directory / name
            path.write_bytes(value)
            path.chmod(0o400)
            digests[name] = hashlib.sha256(value).hexdigest()
        manifest = "".join(f"{name}|{digests[name]}\n" for name in sorted(digests))
        handoff = {
            "credentialDirectory": str(directory),
            "credentialManifestSha256": hashlib.sha256(manifest.encode()).hexdigest(),
        }
        owner = (os.geteuid(), os.getegid())
        with (
            patch.object(fixture, "require_path") as require_path,
            patch.object(fixture, "read_regular_bytes") as read_regular,
        ):
            require_path.return_value = directory.stat()

            # return one exact secret by its fixed filename
            def secret_bytes(path, *_args, **_kwargs):
                return f"{path.name}-secret\n".encode()

            read_regular.side_effect = secret_bytes
            with patch.object(fixture, "require_canonical", return_value=directory):
                # retain the real file mode assertions
                self.assertEqual(
                    fixture.validate_credentials(handoff, self.fixture_root),
                    directory,
                )
        self.assertEqual(owner, (os.geteuid(), os.getegid()))


# run the focused suite only on direct invocation
if __name__ == "__main__":
    unittest.main()
