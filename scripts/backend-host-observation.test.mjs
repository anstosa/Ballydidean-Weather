import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import {
  chmod,
  copyFile,
  mkdir,
  mkdtemp,
  readFile,
  readdir,
  readlink,
  rename,
  rm,
  stat,
  symlink,
  truncate,
  unlink,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";
import test from "node:test";
import { pathToFileURL } from "node:url";

import * as hostObserver from "./backend-host-observation.mjs";

import {
  HOST_EVIDENCE_CONTRACT_VERSION,
  HOST_EVIDENCE_REQUEST_VERSION,
  HOST_OBSERVATION_VERSION,
  HOST_PROBE_VERSION,
  HOST_PRODUCER,
  buildBackendHostObservation,
  canonicalHostJson,
  collectBackendHostEvidence,
  hostEvidenceSha256,
} from "./backend-host-observation.mjs";

const REPOSITORY_ROOT = new URL("..", import.meta.url);
const INVENTORY_PATH = new URL("../config/backend-runtime-inventory.json", import.meta.url);
const RUNTIME_MAP_PATH = new URL("../config/backend-runtime-live-map.json", import.meta.url);
const PRODUCER_PATH = new URL("./backend-host-observation.mjs", import.meta.url);
const COMMON_DEPLOY_PATH = new URL("../deploy/scripts/common.sh", import.meta.url);
const HASH_A = "a".repeat(64);
const SERVICES = ["api", "cloudflared", "migration", "postgres", "web", "worker"];
const IMAGE_ROLE = {
  api: "server",
  cloudflared: "cloudflared",
  migration: "server",
  postgres: "postgres",
  web: "web",
  worker: "server",
};
const DOCKER_CONTAINER_FORMAT = [
  "{\"args\":{{json .Args}}",
  "\"cgroupParent\":{{json .HostConfig.CgroupParent}}",
  "\"containerNumber\":{{json (index .Config.Labels \"com.docker.compose.container-number\")}}",
  "\"exitCode\":{{json .State.ExitCode}}",
  "\"health\":{{if .State.Health}}{{json .State.Health.Status}}{{else}}\"none\"{{end}}",
  "\"id\":{{json .Id}}",
  "\"imageId\":{{json .Image}}",
  "\"imageReference\":{{json .Config.Image}}",
  "\"memory\":{{json .HostConfig.Memory}}",
  "\"name\":{{json .Name}}",
  "\"nanoCpus\":{{json .HostConfig.NanoCpus}}",
  "\"oneOff\":{{json (index .Config.Labels \"com.docker.compose.oneoff\")}}",
  "\"path\":{{json .Path}}",
  "\"pid\":{{json .State.Pid}}",
  "\"pidsLimit\":{{json .HostConfig.PidsLimit}}",
  "\"project\":{{json (index .Config.Labels \"com.docker.compose.project\")}}",
  "\"readOnlyRootfs\":{{json .HostConfig.ReadonlyRootfs}}",
  "\"running\":{{json .State.Running}}",
  "\"service\":{{json (index .Config.Labels \"com.docker.compose.service\")}}",
  "\"startedAt\":{{json .State.StartedAt}}",
  "\"status\":{{json .State.Status}}}",
].join(",");
const DOCKER_IMAGE_FORMAT = [
  "{\"architecture\":{{json .Architecture}}",
  "\"cmd\":{{json .Config.Cmd}}",
  "\"entrypoint\":{{json .Config.Entrypoint}}",
  "\"imageId\":{{json .Id}}",
  "\"os\":{{json .Os}}",
  "\"repoDigests\":{{json .RepoDigests}}",
  "\"revision\":{{json (index .Config.Labels \"org.opencontainers.image.revision\")}}}",
].join(",");
const RUNNER_UNITS = [
  "weather-adjustment-archive.service",
  "weather-adjustment-daily.service",
  "weather-adjustment-daily.timer",
  "weather-adjustment-monthly.service",
  "weather-adjustment-monthly.timer",
];

// hash one exact byte sequence
function sha256Bytes(value) {
  return createHash("sha256").update(value).digest("hex");
}

// derive one Docker-shaped fixture container identity
function containerIdForService(service) {
  return sha256Bytes(Buffer.from(`weather-fixture-container:${service}`));
}

// render one selected service and container list row
function dockerContainerListRow(service) {
  return `${service}\t${containerIdForService(service)}`;
}

// encode one retained raw byte sequence
function rawBytes(value) {
  const bytes = Buffer.from(value);
  return {
    byteLength: bytes.length,
    dataBase64: bytes.toString("base64"),
    encoding: "base64",
    sha256: sha256Bytes(bytes),
    truncated: false,
  };
}

// build one typed retained probe
function probe(id, stdout, {
  args = [],
  exitCode = undefined,
  kind = "command",
  operation = null,
  path = null,
  program = "/usr/bin/uname",
  sourceScope = "host",
  status = "ok",
} = {}) {
  return {
    completedAt: "2026-11-13T08:00:01.000Z",
    contractVersion: HOST_PROBE_VERSION,
    id,
    kind,
    operation: operation ?? (path === null ? { args, program } : { path }),
    outcome: { exitCode: exitCode ?? (kind === "command" ? 0 : null), status },
    raw: {
      stderr: rawBytes(""),
      stdout: rawBytes(stdout),
    },
    sourceScope,
    startedAt: "2026-11-13T08:00:00.000Z",
  };
}

// build common typed host ABI probes
function commonHostProbes() {
  return [
    probe("host.uname-machine", "aarch64\n", { args: ["-m"] }),
    probe("host.uname-kernel", "6.12.47+rpt-rpi-v8\n", { args: ["-r"] }),
    probe("host.cpuinfo", "model name\t: Cortex-A76\n", {
      kind: "file",
      path: "/proc/cpuinfo",
    }),
    probe("host.os-release", "ID=debian\nVERSION_ID=\"12\"\n", {
      kind: "file",
      path: "/etc/os-release",
    }),
    probe("host.libc", "glibc 2.36\n", {
      args: ["GNU_LIBC_VERSION"],
      program: "/usr/bin/getconf",
    }),
    probe(
      "host.mountinfo",
      "36 25 0:32 / /sys/fs/cgroup rw,nosuid,nodev,noexec,relatime - cgroup2 cgroup rw\n",
      { kind: "file", path: "/proc/self/mountinfo" },
    ),
    probe("host.filesystems", "nodev\tcgroup2\n", {
      kind: "file",
      path: "/proc/filesystems",
    }),
    probe("host.self-cgroup", "0::/system.slice/weather-compose.service\n", {
      kind: "file",
      path: "/proc/self/cgroup",
    }),
    probe("host.cgroup-namespace", "cgroup:[4026531835]\n", {
      kind: "readlink",
      path: "/proc/self/ns/cgroup",
    }),
    probe("host.cgroup-statfs", canonicalHostJson({ bsize: 4096, type: 1667723888 }), {
      kind: "statfs",
      path: "/sys/fs/cgroup",
    }),
  ];
}

// build one minimal source-owned Compose map
function blueberrySourceFixture() {
  const entries = [];
  const artifacts = [];
  const bindings = [];
  const contexts = [];

  // retain every exact production Compose service
  for (const service of SERVICES) {
    const rootEntryId = `compose-command:deploy/compose.yaml#${service}`;
    const contextId = `runtime-context:blueberry-compose:${rootEntryId}#service=${service}`;
    const artifactId = `artifact:container-file:${rootEntryId}#0`;
    const imageTarget = IMAGE_ROLE[service];
    entries.push({
      accountingScope: `blueberry:compose:${service}`,
      id: rootEntryId,
      installedPaths: [`/opt/weather/${service}/main`],
      kind: "compose-command",
      parents: [],
    });
    artifacts.push({
      authority: { kind: "dockerfile-copy", sourcePath: "Dockerfile" },
      contentRole: "runtime-bytes",
      expectedMode: "0644",
      expectedType: "regular-file",
      id: artifactId,
      imageTarget,
      kind: "container-file",
      ownerPolicy: "runtime-user",
      path: `/opt/weather/${service}/main`,
      sourceEntryId: rootEntryId,
    });
    contexts.push({
      accountPolicy: null,
      accountingScopes: [`blueberry:compose:${service}`],
      activationSource: "container-inspect",
      artifactIds: [artifactId],
      hostRole: "blueberry",
      id: contextId,
      imageTarget,
      profile: "blueberry-compose",
      rootEntryId,
      rootKind: "compose-service",
      service,
      unit: null,
    });
    bindings.push({
      artifactIds: [artifactId],
      contextId,
      entryId: rootEntryId,
      id: `runtime-binding:${contextId}:${rootEntryId}`,
      rootEntryId,
    });
  }
  return {
    inventory: { entries, schemaVersion: "backend-runtime-inventory/v1" },
    runtimeMap: {
      artifacts,
      bindings,
      contexts,
      contractVersion: "weather-backend-runtime-live-map/v1",
      inventorySchemaVersion: "backend-runtime-inventory/v1",
    },
  };
}

// build one parseable Linux proc stat record
function procStatText(pid, ppid, startTimeTicks) {
  const fields = [
    "S",
    String(ppid),
    ...Array(17).fill("0"),
    String(startTimeTicks),
    "0",
  ];
  return `${String(pid)} (fixture process) ${fields.join(" ")}\n`;
}

// map one service to retained physical process identities
function serviceProcess(service, initPid, mainPid) {
  const role = IMAGE_ROLE[service];
  const mainProgram = service === "cloudflared"
    ? "/usr/local/bin/cloudflared"
    : service === "postgres"
      ? "/usr/lib/postgresql/17/bin/postgres"
      : "/usr/local/bin/node";
  const processInputs = [
    {
      argv: ["/usr/local/bin/docker-entrypoint.sh", mainProgram],
      exePath: "/usr/local/bin/docker-entrypoint.sh",
      pid: initPid,
      ppid: 0,
      startTimeTicks: 1000,
    },
    {
      argv: [mainProgram, `${role}-main`],
      exePath: mainProgram,
      pid: mainPid,
      ppid: initPid,
      startTimeTicks: 1001,
    },
  ];
  return processInputs.map(
    // retain each raw proc source beside its derived fields
    (process) => {
      const cmdline = Buffer.from(`${process.argv.join("\0")}\0`);
      const cgroup = Buffer.from(`0::/weather/${service}\n`);
      const stat = Buffer.from(procStatText(
        process.pid,
        process.ppid,
        process.startTimeTicks,
      ));
      const maps = Buffer.from(
        "7f000000-7f100000 r-xp 00000000 00:00 0 /usr/lib/aarch64-linux-gnu/libc.so.6\n",
      );
      const exeLink = Buffer.from(process.exePath);
      const rootOsRelease = Buffer.from("ID=debian\nVERSION_ID=\"12\"\n");
      const containerId = containerIdForService(service);
      const imageId = `sha256:image-${role}`;
      const procBinding = {
        cgroupAfter: rawBytes(cgroup),
        cgroupBefore: rawBytes(cgroup),
        containerId,
        imageId,
        mountNamespace: secureDirectoryIdentity("5001"),
        mountinfoAfter: rawBytes("fixture mountinfo\n"),
        mountinfoBefore: rawBytes("fixture mountinfo\n"),
        pid: process.pid,
        pidfd: true,
        pidfdInfo: rawBytes(`Pid:\t${String(process.pid)}\n`),
        root: secureDirectoryIdentity("5002"),
        startTimeAfter: String(process.startTimeTicks),
        startTimeBefore: String(process.startTimeTicks),
      };
      // build one secure root subject for this exact process
      const proofSubject = (id, relativeComponents, content, readMode) => {
        const expected = {
          containerId,
          expectedGid: 0,
          expectedMode: "0644",
          expectedUid: 0,
          id,
          imageId,
          pid: process.pid,
          readMode,
        };
        const identity = secureFileIdentity(expected);
        return {
          contentSha256: sha256Bytes(content),
          expected,
          identity: { post: structuredClone(identity), pre: identity },
          procBinding: structuredClone(procBinding),
          relativeComponents,
        };
      };
      return {
        argv: process.argv,
        cgroupPath: `/weather/${service}`,
        exePath: process.exePath,
        exeSha256: sha256Bytes(Buffer.from("fixture-executable")),
        mapsSha256: sha256Bytes(maps),
        mappedLibcPath: "/usr/lib/aarch64-linux-gnu/libc.so.6",
        mappedLibcSha256: sha256Bytes(Buffer.from("fixture-libc")),
        pid: process.pid,
        ppid: process.ppid,
        raw: {
          cgroup: rawBytes(cgroup),
          cmdline: rawBytes(cmdline),
          exeByteLength: Buffer.byteLength("fixture-executable"),
          exeLink: rawBytes(exeLink),
          maps: rawBytes(maps),
          rootOsRelease: rawBytes(rootOsRelease),
          stat: rawBytes(stat),
        },
        rootOsReleaseSha256: sha256Bytes(rootOsRelease),
        secureRootProof: {
          contractVersion: "weather-backend-host-secure-root-proof/v1",
          helperSha256: hostObserver.HOST_SECURE_READER_SHA256,
          subjects: [
            proofSubject(
              `${service}-${String(process.pid)}-os-release`,
              ["etc", "os-release"],
              rootOsRelease,
              "bytes",
            ),
            proofSubject(
              `${service}-${String(process.pid)}-libc`,
              ["usr", "lib", "aarch64-linux-gnu", "libc.so.6"],
              Buffer.from("fixture-libc"),
              "sha256",
            ),
          ],
        },
        startTimeTicks: process.startTimeTicks,
      };
    },
  );
}

// render one native Docker top table
function nativeDockerTop(service, initPid, mainPid) {
  const processes = serviceProcess(service, initPid, mainPid);
  return [
    "PID PPID COMMAND",
    ...processes.map(
      // retain native space-delimited top output
      (process) => `${String(process.pid)} ${String(process.ppid)} ${process.argv.join(" ")}`,
    ),
    "",
  ].join("\n");
}

// build one complete Blueberry evidence fixture
function blueberryEvidenceFixture({ includeOperator = true } = {}) {
  const { inventory, runtimeMap } = blueberrySourceFixture();
  const probes = commonHostProbes();
  const containerIds = SERVICES.filter(
    // exclude the completed ephemeral migration one-shot
    (service) => service !== "migration",
  ).map(
    // retain every long-lived container identity
    (service) => dockerContainerListRow(service),
  );
  probes.push(probe("docker.containers.list", `${containerIds.join("\n")}\n`, {
    args: [
      "ps",
      "--all",
      "--filter",
      "label=com.docker.compose.project=weather",
      "--no-trunc",
      "--format",
      '{{.Label "com.docker.compose.service"}}\t{{.ID}}',
    ],
    program: "/usr/bin/docker",
  }));

  // retain exact container and process evidence per service
  for (let index = 0; index < SERVICES.length; index += 1) {
    const service = SERVICES[index];

    // omit historical migration state from current container evidence
    if (service === "migration") {
      continue;
    }
    const role = IMAGE_ROLE[service];
    const initPid = 2000 + index * 10;
    const mainPid = initPid + 1;
    const path = service === "cloudflared"
      ? "/usr/local/bin/cloudflared"
      : service === "postgres"
        ? "/usr/local/bin/docker-entrypoint.sh"
        : "node";
    const args = service === "web"
      ? ["deploy/scripts/web-server.mjs"]
      : [`${service}/main.js`];
    const inspect = [
      {
        args,
        cgroupParent: "/weather",
        containerNumber: 1,
        exitCode: 0,
        health: "healthy",
        id: containerIdForService(service),
        imageId: `sha256:image-${role}`,
        imageReference: `weather-${role}:fixture`,
        memory: 536870912,
        name: `weather-${service}-1`,
        nanoCpus: 1000000000,
        oneOff: false,
        path,
        pid: initPid,
        pidsLimit: 256,
        project: "weather",
        readOnlyRootfs: true,
        running: true,
        service,
        startedAt: "2026-11-13T07:00:00.000Z",
        status: "running",
      },
    ];
    probes.push(probe(
      `docker.container.${service}.inspect`,
      canonicalHostJson(inspect),
      {
        args: ["inspect", "--format", DOCKER_CONTAINER_FORMAT, containerIdForService(service)],
        program: "/usr/bin/docker",
        sourceScope: service,
      },
    ));
    probes.push(probe(
      `docker.container.${service}.top`,
      nativeDockerTop(service, initPid, mainPid),
      {
        args: ["top", containerIdForService(service), "-eo", "pid,ppid,args"],
        program: "/usr/bin/docker",
        sourceScope: service,
      },
    ));
    probes.push(probe(
      `docker.container.${service}.processes`,
      canonicalHostJson({
        containerId: containerIdForService(service),
        dockerInitPid: initPid,
        processes: serviceProcess(service, initPid, mainPid),
      }),
      {
        kind: "derived",
        operation: {
          inputProbeIds: [
            `docker.container.${service}.inspect`,
            `docker.container.${service}.top`,
          ],
          method: "docker-top-procfs-v2",
        },
        sourceScope: service,
      },
    ));
    probes.push(probe(
      `cgroup.blueberry:compose:${service}`,
      canonicalHostJson({
        controllers: ["cpu", "memory", "pids"],
        cpuMax: "100000 100000",
        memoryHigh: "max",
        memoryMax: "536870912",
        path: `/weather/${service}`,
        pid: initPid,
        pidsMax: "256",
        raw: {
          controllers: rawBytes("cpu memory pids\n"),
          cpuMax: rawBytes("100000 100000\n"),
          memoryHigh: rawBytes("max\n"),
          memoryMax: rawBytes("536870912\n"),
          pidsMax: rawBytes("256\n"),
        },
        scopeId: `blueberry:compose:${service}`,
      }),
      {
        kind: "derived",
        operation: {
          inputProbeIds: [
            `docker.container.${service}.inspect`,
            `docker.container.${service}.processes`,
          ],
          method: "derived-canonical-json-v1",
        },
        sourceScope: service,
      },
    ));
  }

  // retain exact selected image configuration per role
  for (const role of ["cloudflared", "postgres", "server", "web"]) {
    const cmd = role === "web"
      ? ["node", "deploy/scripts/web-server.mjs"]
      : role === "server"
        ? ["node", "apps/api/dist/main.js"]
        : null;
    probes.push(probe(
      `docker.image.${role}.inspect`,
      canonicalHostJson([{
        architecture: "arm64",
        cmd,
        entrypoint: role === "server" || role === "web"
          ? ["docker-entrypoint.sh"]
          : null,
        imageId: `sha256:image-${role}`,
        os: "linux",
        repoDigests: [`weather-${role}@sha256:${"b".repeat(64)}`],
        revision: "fixture-revision",
      }]),
      {
        args: [
          "image",
          "inspect",
          "--format",
          DOCKER_IMAGE_FORMAT,
          `sha256:image-${role}`,
        ],
        program: "/usr/bin/docker",
        sourceScope: role,
      },
    ));
  }
  const unitShow = [
    "Id=weather-compose.service",
    "LoadState=loaded",
    "ActiveState=failed",
    "SubState=failed",
    "UnitFileState=enabled",
    "Result=exit-code",
    "ExecMainStatus=1",
    "FragmentPath=/etc/systemd/system/weather-compose.service",
    "DropInPaths=/etc/systemd/system/weather-compose.service.d/override.conf",
    "ExecStart={ path=/usr/bin/docker-compose ; argv[]=/usr/bin/docker-compose up ; }",
    "User=root",
    "Group=root",
    "",
  ].join("\n");
  probes.push(probe("systemd.weather-compose.service.show", unitShow, {
    args: [
      "show",
      "weather-compose.service",
      "--no-pager",
      "--property=Id,LoadState,ActiveState,SubState,UnitFileState,Result,ExecMainStatus,FragmentPath,DropInPaths,ExecStart,ExecReload,ExecStop,TimeoutStartUSec,User,Group",
    ],
    program: "/usr/bin/systemctl",
  }));
  probes.push(probe(
    "systemd.weather-compose.service.files",
    canonicalHostJson([{
      byteLength: Buffer.byteLength("[Service]\n"),
      dataBase64: Buffer.from("[Service]\n").toString("base64"),
      gid: 0,
      mode: "0644",
      nlink: 1,
      path: "/etc/systemd/system/weather-compose.service",
      sha256: sha256Bytes(Buffer.from("[Service]\n")),
      uid: 0,
    }]),
    {
      kind: "derived",
      operation: {
        inputProbeIds: ["systemd.weather-compose.service.show"],
        method: "derived-canonical-json-v1",
      },
    },
  ));
  probes.push(probe("release.current", "2026.11.13-1\n", {
    kind: "file",
    path: "/opt/weather/current/deploy/state/current-release",
  }));
  probes.push(probe("release.previous", "2026.11.12-1\n", {
    kind: "file",
    path: "/opt/weather/current/deploy/state/previous-release",
  }));
  probes.push(probe("release.schema", "2026.11.13-schema\n", {
    kind: "file",
    path: "/opt/weather/current/deploy/state/schema-release",
  }));

  // retain every exact installed artifact observation
  for (const context of runtimeMap.contexts) {
    const artifactId = context.artifactIds[0];
    const artifact = runtimeMap.artifacts.find(
      // select the context artifact
      (candidate) => candidate.id === artifactId,
    );
    const service = context.service;
    const artifactProbeId = `artifact.${sha256Bytes(`${context.id}\0${artifactId}`)}`;
    const migration = service === "migration";
    probes.push(probe(artifactProbeId, canonicalHostJson({
      artifactId,
      containerId: migration ? null : containerIdForService(service),
      declaredPath: artifact.path,
      gid: 10002,
      imageId: `sha256:image-${IMAGE_ROLE[service]}`,
      mode: "0644",
      present: true,
      resolvedPath: artifact.path,
      scope: migration ? "image" : "container",
      sha256: HASH_A,
      symlinkTarget: null,
      type: "regular-file",
      uid: 10002,
    }), {
      kind: "derived",
      operation: {
        inputProbeIds: [
          `docker.container.${migration ? "api" : service}.inspect`,
        ],
        method: "derived-canonical-json-v1",
      },
      sourceScope: context.id,
    }));
  }
  const evidence = {
    authority: {
      canonicalEligible: false,
      inventorySha256: hostEvidenceSha256(inventory),
      kind: "injected-test",
      liveEligible: false,
      producerSha256: HASH_A,
      runtimeMapSha256: hostEvidenceSha256(runtimeMap),
    },
    complete: true,
    completedAt: "2026-11-13T08:00:02.000Z",
    contractVersion: HOST_EVIDENCE_CONTRACT_VERSION,
    errors: [],
    hostRole: "blueberry",
    probes: probes.sort(
      // retain deterministic probe order
      (left, right) => left.id.localeCompare(right.id),
    ),
    producer: HOST_PRODUCER,
    profile: "blueberry-compose",
    startedAt: "2026-11-13T07:59:59.000Z",
    unresolved: [],
  };

  // retain the fixed Blueberry operator family by default
  if (includeOperator) {
    addOperatorFixture(evidence);
  }
  return { evidence, inventory, runtimeMap };
}

// build the complete current v17 control-plane tree proof
async function buildControlPlaneTreeFixture() {
  const directories = [];
  const files = [];

  // walk one fixed predecessor-protected directory
  async function walk(relativeDirectory) {
    const entries = await readdir(
      new URL(`../${relativeDirectory}`, import.meta.url),
      { withFileTypes: true },
    );
    const retainedEntries = entries.map(
      // reject links and special nodes from the fixture oracle
      (entry) => {
        assert.ok(entry.isDirectory() || entry.isFile(), `${relativeDirectory}/${entry.name}`);
        return {
          name: entry.name,
          type: entry.isDirectory() ? "directory" : "regular-file",
        };
      },
    ).sort(
      // retain canonical directory order
      (left, right) => left.name.localeCompare(right.name),
    );
    directories.push({ entries: retainedEntries, path: relativeDirectory });

    // retain every recursive regular file exactly once
    for (const entry of retainedEntries) {
      const relativePath = `${relativeDirectory}/${entry.name}`;

      // recurse only through the four fixed protected roots
      if (entry.type === "directory") {
        await walk(relativePath);
        continue;
      }
      const bytes = await readFile(new URL(`../${relativePath}`, import.meta.url));
      const expected = { expectedGid: 0, expectedMode: "0644", expectedUid: 0 };
      const identity = secureFileIdentity(expected);
      identity.ino = String(8000 + files.length);
      identity.size = String(bytes.length);
      files.push({ identity, path: relativePath, sha256: sha256Bytes(bytes) });
    }
  }

  // retain all four predecessor-protected roots
  for (const relativeDirectory of [
    "deploy/postgres",
    "deploy/scripts",
    "deploy/sudoers",
    "deploy/systemd",
  ]) {
    await walk(relativeDirectory);
  }
  files.sort(
    // retain the predecessor's protected-root order before Compose
    (left, right) => left.path.localeCompare(right.path),
  );
  const composeBytes = await readFile(new URL("../deploy/compose.yaml", import.meta.url));
  const composeIdentity = secureFileIdentity({
    expectedGid: 0,
    expectedMode: "0644",
    expectedUid: 0,
  });
  composeIdentity.ino = String(8000 + files.length);
  composeIdentity.size = String(composeBytes.length);
  files.push({
    identity: composeIdentity,
    path: "deploy/compose.yaml",
    sha256: sha256Bytes(composeBytes),
  });
  directories.sort(
    // retain canonical recursive directory order
    (left, right) => left.path.localeCompare(right.path),
  );
  const controlPlaneSha256 = controlPlaneDigest(files);
  return {
    contractVersion: "weather-schema-preserving-control-plane-tree/v1",
    controlPlaneSha256,
    directories,
    files,
  };
}

const CONTROL_PLANE_TREE_FIXTURE = await buildControlPlaneTreeFixture();

// hash one exact predecessor-framed protected file set
function controlPlaneDigest(files) {
  return sha256Bytes(Buffer.from(files.map(
    // retain the predecessor path-digest framing exactly
    (file) => `${file.path}\0${file.sha256}\n`,
  ).join("")));
}

// clone one complete schema-preserving control-plane tree proof
function controlPlaneTreeFixture() {
  return structuredClone(CONTROL_PLANE_TREE_FIXTURE);
}

// build one reusable current-v1 operator material set
function buildOperatorMaterialFixture() {
  const baseUnitBytes = Buffer.from(`[Unit]
Description=Weather ARM64 Compose stack

[Service]
ExecStart=/opt/weather/current/deploy/scripts/update.sh recover
ExecReload=/opt/weather/current/deploy/scripts/update.sh recover
ExecStop=/usr/bin/docker compose --project-name weather --env-file /opt/weather/current/deploy/state/active.env --file /opt/weather/current/deploy/compose.yaml stop --timeout 45
TimeoutStartSec=300
`);
  const moduleBytes = Buffer.from("export const helper = true;\n");
  const scriptBytes = Buffer.from("#!/usr/bin/env bash\nexec node helper.mjs \"$@\"\n");
  const moduleSha256 = sha256Bytes(moduleBytes);
  const scriptSha256 = sha256Bytes(scriptBytes);
  const bundleManifest = `${moduleSha256}  schema-preserving-release.mjs\n` +
    `${scriptSha256}  schema-preserving-release.sh\n`;
  const bundleSha256 = sha256Bytes(bundleManifest);
  const helperDirectory = `/opt/weather/operators/schema-preserving-release/${bundleSha256}`;
  const helperScript = `${helperDirectory}/schema-preserving-release.sh`;
  const dropInBytes = Buffer.from(`[Service]
ExecStart=
ExecStart=${helperScript} system-recover
ExecReload=
ExecReload=${helperScript} system-recover
TimeoutStartSec=600
`);
  const execStart = `${helperScript}|${helperScript} system-recover`;
  const execStop = "/usr/bin/docker|/usr/bin/docker compose --project-name weather --env-file /opt/weather/current/deploy/state/active.env --file /opt/weather/current/deploy/compose.yaml stop --timeout 45";
  const controlPlane = controlPlaneTreeFixture();
  const receipt = {
    baseUnitSha256: sha256Bytes(baseUnitBytes),
    bundleSha256,
    contractVersion: "weather-schema-preserving-release-installation/v1",
    controlPlaneSha256: controlPlane.controlPlaneSha256,
    controlPlaneVersion: 17,
    dropInSha256: sha256Bytes(dropInBytes),
    execReload: execStart,
    execStart,
    execStop,
    helperDirectory,
    helperScript,
    timeoutStartSec: 600,
  };
  const receiptBytes = Buffer.from(`${canonicalHostJson(receipt)}\n`);
  const receiptPath = "/opt/weather/current/deploy/state/schema-preserving-release-v1/" +
    `installations/${bundleSha256}.json`;
  const dropInPath = "/etc/systemd/system/weather-compose.service.d/" +
    "10-schema-preserving-release.conf";
  const showText = [
    "Id=weather-compose.service",
    "LoadState=loaded",
    "ActiveState=failed",
    "SubState=failed",
    "UnitFileState=enabled",
    "Result=exit-code",
    "ExecMainStatus=1",
    "FragmentPath=/etc/systemd/system/weather-compose.service",
    `DropInPaths=${dropInPath}`,
    `ExecStart={ path=${helperScript} ; argv[]=${helperScript} system-recover ; }`,
    `ExecReload={ path=${helperScript} ; argv[]=${helperScript} system-recover ; }`,
    "ExecStop={ path=/usr/bin/docker ; argv[]=/usr/bin/docker compose --project-name weather --env-file /opt/weather/current/deploy/state/active.env --file /opt/weather/current/deploy/compose.yaml stop --timeout 45 ; }",
    "TimeoutStartUSec=10min",
    "User=root",
    "Group=root",
    "",
  ].join("\n");
  return {
    baseUnitBytes,
    bundleSha256,
    controlPlane,
    dropInBytes,
    dropInPath,
    helperDirectory,
    helperScript,
    moduleBytes,
    receipt,
    receiptBytes,
    receiptPath,
    scriptBytes,
    showText,
  };
}

const OPERATOR_FIXTURE = buildOperatorMaterialFixture();

// add one complete current-v1 operator evidence closure
function addOperatorFixture(evidence) {
  const {
    baseUnitBytes,
    bundleSha256,
    controlPlane,
    dropInBytes,
    dropInPath,
    helperDirectory,
    helperScript,
    moduleBytes,
    receipt,
    receiptBytes,
    receiptPath,
    scriptBytes,
    showText,
  } = OPERATOR_FIXTURE;
  // build one closed operator file identity
  const file = (path, bytes, mode) => ({
    byteLength: bytes.length,
    gid: 0,
    mode,
    nlink: 1,
    path,
    sha256: sha256Bytes(bytes),
    uid: 0,
  });
  const showProbe = evidence.probes.find(
    // select the exact supervising unit state
    (candidate) => candidate.id === "systemd.weather-compose.service.show",
  );
  showProbe.raw.stdout = rawBytes(showText);
  const filesProbe = evidence.probes.find(
    // select the exact ordered unit source list
    (candidate) => candidate.id === "systemd.weather-compose.service.files",
  );
  filesProbe.raw.stdout = rawBytes(canonicalHostJson([
    {
      dataBase64: baseUnitBytes.toString("base64"),
      ...file("/etc/systemd/system/weather-compose.service", baseUnitBytes, "0644"),
    },
    {
      dataBase64: dropInBytes.toString("base64"),
      ...file(dropInPath, dropInBytes, "0644"),
    },
  ]));
  // build one derived operator probe
  const derived = (
    id,
    value,
    prerequisiteIds = [],
    method = "derived-canonical-json-v1",
  ) => probe(
    id,
    canonicalHostJson(value),
    {
      kind: "derived",
      operation: { inputProbeIds: prerequisiteIds, method },
      sourceScope: "operator",
    },
  );
  evidence.probes.push(
    derived("operator.forced.dispatcher", {
      file: file(
        "/usr/local/bin/weather-ssh-dispatch",
        Buffer.from("fixture dispatcher"),
        "0755",
      ),
    }),
    derived("operator.forced.remote-ops", {
      file: file(
        "/usr/local/sbin/weather-remote-ops",
        Buffer.from("fixture remote ops"),
        "0755",
      ),
    }),
    derived("operator.installation-receipt", {
      file: file(receiptPath, receiptBytes, "0600"),
      raw: { bytes: rawBytes(receiptBytes) },
      selection: {
        bootstrapPath: "/opt/weather/current/deploy/state/" +
          "schema-preserving-release-v1/installation.json",
        immutablePath: receiptPath,
        immutableStatus: "present",
        selectedPath: receiptPath,
      },
    }, ["systemd.weather-compose.service.show"]),
    derived(
      "operator.control-plane.files",
      controlPlane,
      ["operator.installation-receipt"],
      "secure-control-plane-tree-v1",
    ),
    derived("operator.bundle.schema-preserving-release.mjs", {
      file: file(`${helperDirectory}/schema-preserving-release.mjs`, moduleBytes, "0644"),
    }, ["operator.installation-receipt"]),
    derived("operator.bundle.schema-preserving-release.sh", {
      file: file(helperScript, scriptBytes, "0755"),
    }, ["operator.installation-receipt"]),
    derived("operator.weather-compose.dropin", {
      file: file(dropInPath, dropInBytes, "0644"),
      raw: { bytes: rawBytes(dropInBytes) },
    }, ["systemd.weather-compose.service.show"]),
  );
  evidence.probes.sort(
    // retain deterministic probe ordering
    (left, right) => left.id.localeCompare(right.id),
  );
  return { bundleSha256, dropInBytes, receipt, receiptBytes };
}

// build one minimal runner source map
function runnerSourceFixture() {
  const rootEntryId =
    "installer-allowlist:scripts/install-adjustment-maintenance-runner.sh#RUNNER_FILES:scripts/runner.mjs";
  const contextId = `runtime-context:workstation-runner:${rootEntryId}#account=ubuntu`;
  const artifactId = `artifact:runner-file:${rootEntryId}#0`;
  const inventory = {
    entries: [{
      accountingScope: "runner-cgroup",
      id: rootEntryId,
      installedPaths: ["${HOME}/.weather/adjustment-maintenance/runner/current/scripts/runner.mjs"],
      kind: "installer-allowlist",
      parents: [],
    }],
    schemaVersion: "backend-runtime-inventory/v1",
  };
  const accountPolicy = {
    accountName: "ubuntu",
    homeTemplate: "${HOME}",
    resolution: "nss-passwd-exact",
  };
  const runtimeMap = {
    artifacts: [{
      accountPolicy,
      authority: {
        kind: "runner-installer",
        sourcePath: "scripts/install-adjustment-maintenance-runner.sh",
      },
      declaredPath: "${HOME}/.weather/adjustment-maintenance/runner/current/scripts/runner.mjs",
      expectedMode: "0644",
      expectedType: "regular-file",
      id: artifactId,
      kind: "runner-file",
      ownerPolicy: "nss-account",
      resolvedRootPolicy: "nss-account-home",
      sourceEntryId: rootEntryId,
    }],
    bindings: [{
      artifactIds: [artifactId],
      contextId,
      entryId: rootEntryId,
      id: `runtime-binding:${contextId}:${rootEntryId}`,
      rootEntryId,
    }],
    contexts: [{
      accountPolicy,
      accountingScopes: ["runner-cgroup"],
      activationSource: "systemd-show",
      artifactIds: [artifactId],
      hostRole: "workstation",
      id: contextId,
      imageTarget: null,
      profile: "workstation-runner",
      rootEntryId,
      rootKind: "runner-package",
      service: null,
      unit: null,
    }],
    contractVersion: "weather-backend-runtime-live-map/v1",
    inventorySchemaVersion: "backend-runtime-inventory/v1",
  };
  return { artifactId, contextId, inventory, runtimeMap };
}

// build one complete workstation runner evidence fixture
function runnerEvidenceFixture() {
  const { artifactId, contextId, inventory, runtimeMap } = runnerSourceFixture();
  const probes = commonHostProbes();
  probes.push(probe("nss.ubuntu.passwd", "ubuntu:x:1000:1000:Ubuntu:/home/ubuntu:/bin/bash\n", {
    args: ["passwd", "ubuntu"],
    program: "/usr/bin/getent",
  }));
  const runnerRoots = [
    {
      id: "runner.root.private",
      path: "/home/ubuntu/.weather/adjustment-maintenance/runner",
    },
    {
      id: "runner.root.library",
      path: "/home/ubuntu/.local/lib/weather-adjustment-maintenance",
    },
  ];

  // retain both exact runner root absence facts
  for (const root of runnerRoots) {
    probes.push(probe(root.id, canonicalHostJson({
      identity: null,
      path: root.path,
      present: false,
    }), {
      kind: "derived",
      operation: {
        inputProbeIds: ["nss.ubuntu.passwd"],
        method: "secure-runner-root-status-v1",
      },
      sourceScope: "runner",
    }));
  }

  // retain all five explicit absent unit states
  for (const unit of RUNNER_UNITS) {
    probes.push(probe(`systemd.${unit}.show`, [
      `Id=${unit}`,
      "LoadState=not-found",
      "ActiveState=inactive",
      "SubState=dead",
      "UnitFileState=disabled",
      "Result=success",
      "ExecMainStatus=0",
      "FragmentPath=",
      "DropInPaths=",
      "ExecStart=",
      "User=ubuntu",
      "Group=ubuntu",
      "",
    ].join("\n"), {
      args: [
        "show",
        unit,
        "--no-pager",
        "--property=Id,LoadState,ActiveState,SubState,UnitFileState,Result,ExecMainStatus,FragmentPath,DropInPaths,ExecStart,ExecReload,ExecStop,TimeoutStartUSec,User,Group",
      ],
      program: "/usr/bin/systemctl",
    }));
    probes.push(probe(`systemd.${unit}.enabled`, "not-found\n", {
      args: ["is-enabled", unit],
      exitCode: 1,
      program: "/usr/bin/systemctl",
    }));
  }
  probes.push(probe("runner.processes", "[]", {
    kind: "directory",
    path: "/proc",
    sourceScope: "runner",
  }));
  probes.push(probe(
    `artifact.${sha256Bytes(`${contextId}\0${artifactId}`)}`,
    canonicalHostJson({
      artifactId,
      containerId: null,
      declaredPath: "${HOME}/.weather/adjustment-maintenance/runner/current/scripts/runner.mjs",
      gid: null,
      imageId: null,
      mode: null,
      present: false,
      resolvedPath: "/home/ubuntu/.weather/adjustment-maintenance/runner/current/scripts/runner.mjs",
      scope: "host",
      sha256: null,
      symlinkTarget: null,
      type: "missing",
      uid: null,
    }),
    {
      kind: "derived",
      operation: {
        inputProbeIds: ["nss.ubuntu.passwd"],
        method: "derived-canonical-json-v1",
      },
      sourceScope: contextId,
    },
  ));
  return {
    evidence: {
      authority: {
        canonicalEligible: false,
        inventorySha256: hostEvidenceSha256(inventory),
        kind: "injected-test",
        liveEligible: false,
        producerSha256: HASH_A,
        runtimeMapSha256: hostEvidenceSha256(runtimeMap),
      },
      complete: true,
      completedAt: "2026-11-13T08:00:02.000Z",
      contractVersion: HOST_EVIDENCE_CONTRACT_VERSION,
      errors: [],
      hostRole: "workstation",
      probes: probes.sort(
        // retain deterministic probe order
        (left, right) => left.id.localeCompare(right.id),
      ),
      producer: HOST_PRODUCER,
      profile: "workstation-runner",
      startedAt: "2026-11-13T07:59:59.000Z",
      unresolved: [],
    },
    inventory,
    runtimeMap,
  };
}

// build one full retained-v1 raw request and evidence pair
function retainedV1Fixture(fixture, profile, phase, observerSourceBytes) {
  const inventoryJson = canonicalHostJson(fixture.inventory);
  const runtimeMapJson = canonicalHostJson(fixture.runtimeMap);
  const producerSha256 = sha256Bytes(observerSourceBytes);
  const evidence = structuredClone(fixture.evidence);
  evidence.authority = {
    ...evidence.authority,
    canonicalEligible: false,
    kind: "physical-host",
    liveEligible: true,
    producerSha256,
  };
  const request = {
    contractVersion: HOST_EVIDENCE_REQUEST_VERSION,
    expectedHostRole: profile === "blueberry-compose" ? "blueberry" : "workstation",
    expectedReleaseId: "2026.11.13-1",
    expectedSchemaVersion: "2026.11.13-schema",
    inventory: { json: inventoryJson, sha256: sha256Bytes(inventoryJson) },
    limits: {
      deadlineMs: 5000,
      maxFiles: 100,
      maxProbeBytes: 65536,
      maxProcesses: 100,
      maxTotalBytes: 16777216,
    },
    producerSha256,
    profile,
    runtimeMap: { json: runtimeMapJson, sha256: sha256Bytes(runtimeMapJson) },
  };
  return {
    evidence,
    evidenceBytes: Buffer.from(canonicalHostJson(evidence)),
    inventoryBytes: Buffer.from(inventoryJson),
    observerSourceBytes,
    phase,
    request,
    requestBytes: Buffer.from(canonicalHostJson(request)),
    runtimeMapBytes: Buffer.from(runtimeMapJson),
  };
}

// build one exact production request from checked bytes
async function checkedRequest(profile = "blueberry-compose") {
  const inventoryJson = await readFile(INVENTORY_PATH, "utf8");
  const runtimeMapJson = await readFile(RUNTIME_MAP_PATH, "utf8");
  const producerBytes = await readFile(PRODUCER_PATH);
  return {
    contractVersion: HOST_EVIDENCE_REQUEST_VERSION,
    expectedHostRole: profile === "blueberry-compose" ? "blueberry" : "workstation",
    expectedReleaseId: "2026.11.13-1",
    expectedSchemaVersion: "2026.11.13-schema",
    inventory: { json: inventoryJson, sha256: sha256Bytes(inventoryJson) },
    limits: {
      deadlineMs: 5000,
      maxFiles: 100,
      maxProbeBytes: 65536,
      maxProcesses: 100,
      maxTotalBytes: 16777216,
    },
    producerSha256: sha256Bytes(producerBytes),
    profile,
    runtimeMap: { json: runtimeMapJson, sha256: sha256Bytes(runtimeMapJson) },
  };
}

// build one local-only injected adapter set
async function injectedAdapters({ secretOutput = null } = {}) {
  const inventoryJson = await readFile(INVENTORY_PATH);
  const runtimeMapJson = await readFile(RUNTIME_MAP_PATH);
  const calls = { commands: [], paths: [] };
  let tick = 0;
  const missing = () => Object.assign(new Error("fixture absent"), { code: "ENOENT" });
  return {
    adapters: {
      // retain every producer-owned command request
      async executor(program, args, options) {
        calls.commands.push(structuredClone({ args, program, ...options, signal: undefined }));
        return {
          exitCode: 1,
          stderr: Buffer.from("fixture unavailable\n"),
          stdout: Buffer.from(secretOutput ?? ""),
        };
      },
      filesystem: {
        // open only fixed checked config bytes
        async open(path, flags) {
          calls.paths.push(String(path));
          assert.equal(flags, "r");
          const name = basename(String(path));
          let bytes;

          // return the exact staged inventory input
          if (name === "backend-runtime-inventory.json") {
            bytes = inventoryJson;
          }

          // return the exact staged runtime-map input
          if (name === "backend-runtime-live-map.json") {
            bytes = runtimeMapJson;
          }

          // reject every unconfigured fixture file
          if (bytes === undefined) {
            throw missing();
          }
          return {
            // close one local fixture descriptor
            async close() {},
            // read one bounded fixture byte range
            async read(buffer, offset, length, position) {
              const count = Math.max(0, Math.min(length, bytes.length - position));
              bytes.copy(buffer, offset, position, position + count);
              return { buffer, bytesRead: count };
            },
            // report size before any read
            async stat() {
              return { size: bytes.length };
            },
          };
        },
        async lstat(path) {
          calls.paths.push(String(path));
          throw missing();
        },
        async readlink(path) {
          calls.paths.push(String(path));
          throw missing();
        },
        async opendir(path) {
          calls.paths.push(String(path));
          return {
            // close one local fixture directory
            async close() {},
            // end one empty local fixture directory
            async read() {
              return null;
            },
          };
        },
        async realpath(path) {
          calls.paths.push(String(path));
          return String(path);
        },
        async statfs(path) {
          calls.paths.push(String(path));
          return { bsize: 4096, type: 1667723888 };
        },
      },
      clock: {
        // advance one deterministic wall-clock instant
        now() {
          tick += 1;
          return new Date(Date.UTC(2026, 10, 13, 8, 0, 0, tick));
        },
        // advance one deterministic monotonic instant
        monotonicMs() {
          tick += 1;
          return tick;
        },
      },
    },
    calls,
  };
}

// build one successful local-only profile adapter set
async function scriptedProfileAdapters(profile, { includeMigration = false } = {}) {
  const base = await injectedAdapters();
  const baseOpen = base.adapters.filesystem.open;
  base.adapters.secureExecutor = capturingSecureExecutor([]);
  const safeFiles = new Map([
    ["/proc/cpuinfo", Buffer.from("model name\t: Cortex-A76\n")],
    ["/etc/os-release", Buffer.from("ID=debian\nVERSION_ID=\"12\"\n")],
    ["/proc/self/mountinfo", Buffer.from("36 25 0:32 / /sys/fs/cgroup rw - cgroup2 cgroup rw\n")],
    ["/proc/filesystems", Buffer.from("nodev\tcgroup2\n")],
    ["/proc/self/cgroup", Buffer.from("0::/system.slice/weather-compose.service\n")],
  ]);

  // open one bounded scripted file source
  base.adapters.filesystem.open = async (path, flags) => {
    const sourcePath = String(path);

    // preserve exact checked config inputs
    if (basename(sourcePath) === "backend-runtime-inventory.json" ||
      basename(sourcePath) === "backend-runtime-live-map.json") {
      return await baseOpen(path, flags);
    }
    base.calls.paths.push(sourcePath);
    const procMatch = sourcePath.match(/^\/proc\/([1-9][0-9]*)\/(.+)$/u);
    let bytes = safeFiles.get(sourcePath);

    // expose exact retained proc sources for native Docker top identities
    if (bytes === undefined && procMatch !== null) {
      const pid = Number.parseInt(procMatch[1], 10);
      const relativePath = procMatch[2];
      const serviceIndex = Math.floor((pid - 2000) / 10);
      const service = SERVICES[serviceIndex];
      const initPid = 2000 + serviceIndex * 10;
      const process = serviceProcess(service, initPid, initPid + 1).find(
        // select the exact proc identity
        (candidate) => candidate.pid === pid,
      );

      // map each fixed proc file to its retained raw bytes
      if (process !== undefined) {
        if (["cmdline", "cgroup", "stat", "maps"].includes(relativePath)) {
          bytes = Buffer.from(process.raw[relativePath].dataBase64, "base64");
        } else if (relativePath === "exe") {
          bytes = Buffer.from("fixture-executable");
        } else if (relativePath === "root/etc/os-release") {
          bytes = Buffer.from(process.raw.rootOsRelease.dataBase64, "base64");
        } else if (relativePath.endsWith("/libc.so.6")) {
          bytes = Buffer.from("fixture-libc");
        }
      }
    }
    const cgroupMatch = sourcePath.match(
      /^\/sys\/fs\/cgroup\/weather\/([^/]+)\/(cgroup\.controllers|cpu\.max|memory\.high|memory\.max|pids\.max)$/u,
    );

    // expose exact direct cgroup controller bytes
    if (bytes === undefined && cgroupMatch !== null) {
      const values = {
        "cgroup.controllers": "cpu memory pids\n",
        "cpu.max": "100000 100000\n",
        "memory.high": "max\n",
        "memory.max": "536870912\n",
        "pids.max": "256\n",
      };
      bytes = Buffer.from(values[cgroupMatch[2]]);
    }
    bytes ??= Buffer.from("fixture-runtime-bytes\n");
    return {
      // close one scripted descriptor
      async close() {},
      // read one bounded scripted range
      async read(buffer, offset, length, position) {
        const count = Math.max(0, Math.min(length, bytes.length - position));
        bytes.copy(buffer, offset, position, position + count);
        return { buffer, bytesRead: count };
      },
      // report size before reading
      async stat() {
        return { gid: 0, mode: 0o100644, size: bytes.length, uid: 0 };
      },
    };
  };
  base.adapters.filesystem.lstat = async (path) => {
    base.calls.paths.push(String(path));
    return {
      gid: String(path).includes("/home/ubuntu/") ? 1000 : 0,
      isFile: () => true,
      isSymbolicLink: () => false,
      mode: 0o100644,
      size: 22,
      uid: String(path).includes("/home/ubuntu/") ? 1000 : 0,
    };
  };
  base.adapters.filesystem.readlink = async (path) => {
    base.calls.paths.push(String(path));
    const sourcePath = String(path);

    // retain the host cgroup namespace identity
    if (sourcePath === "/proc/self/ns/cgroup") {
      return "cgroup:[4026531835]";
    }
    const exeMatch = sourcePath.match(/^\/proc\/([1-9][0-9]*)\/exe$/u);

    // retain one exact process executable link
    if (exeMatch !== null) {
      const pid = Number.parseInt(exeMatch[1], 10);
      const serviceIndex = Math.floor((pid - 2000) / 10);
      const initPid = 2000 + serviceIndex * 10;
      return serviceProcess(
        SERVICES[serviceIndex],
        initPid,
        initPid + 1,
      ).find(
        // select the exact proc identity
        (candidate) => candidate.pid === pid,
      )?.exePath ?? null;
    }
    return null;
  };
  base.adapters.executor = async (program, args, options) => {
    base.calls.commands.push({
      args: [...args],
      cwd: options.cwd,
      deadlineMs: options.deadlineMs,
      env: structuredClone(options.env),
      maxBytes: options.maxBytes,
      program,
    });
    let stdout = "";
    let exitCode = 0;

    // return exact common command output
    if (program === "/usr/bin/uname") {
      stdout = args[0] === "-m" ? "aarch64\n" : "6.12.47+rpt-rpi-v8\n";
    } else if (program === "/usr/bin/getconf") {
      stdout = "glibc 2.36\n";
    } else if (program === "/usr/bin/getent") {
      stdout = "ubuntu:x:1000:1000:Ubuntu:/home/ubuntu:/bin/bash\n";
    } else if (program === "/usr/bin/systemctl") {
      const unit = args.find((value) => value.startsWith("weather-"));

      // return one exact loaded unit state
      if (args[0] === "show") {
        stdout = profile === "blueberry-compose" && unit === "weather-compose.service"
          ? OPERATOR_FIXTURE.showText
          : [
              `Id=${unit}`,
              "LoadState=loaded",
              "ActiveState=inactive",
              "SubState=dead",
              "UnitFileState=disabled",
              "Result=success",
              "ExecMainStatus=0",
              `FragmentPath=/etc/systemd/system/${unit}`,
              "DropInPaths=",
              "ExecStart=",
              "User=ubuntu",
              "Group=ubuntu",
              "",
            ].join("\n");
      } else {
        stdout = "disabled\n";
        exitCode = 1;
      }
    } else if (program === "/usr/bin/docker") {
      const target = args.at(-1);

      // enumerate exactly the five long-lived containers
      if (args[0] === "ps") {
        stdout = SERVICES.filter(
          // include the optional completed migration only when requested
          (service) => service !== "migration" || includeMigration,
        ).map(
          // retain each exact selected service and container identity
          (service) => dockerContainerListRow(service),
        ).join("\n") + "\n";
      } else if (args[0] === "top") {
        const service = SERVICES.find(
          // recover the fixed service from its exact selected identity
          (candidate) => containerIdForService(candidate) === args[1],
        );
        const index = SERVICES.indexOf(service);
        stdout = nativeDockerTop(service, 2000 + index * 10, 2001 + index * 10);
      } else if (args[0] === "image") {
        const role = target.replace("sha256:image-", "");
        stdout = canonicalHostJson([{
          architecture: "arm64",
          cmd: role === "web"
            ? ["node", "deploy/scripts/web-server.mjs"]
            : role === "server"
              ? ["node", "apps/api/dist/main.js"]
              : null,
          entrypoint: ["docker-entrypoint.sh"],
          imageId: target,
          os: "linux",
          repoDigests: [`weather-${role}@sha256:${"b".repeat(64)}`],
          revision: "fixture-revision",
        }]);
      } else if (args[0] === "inspect") {
        const service = SERVICES.find(
          // recover the fixed service from its exact selected identity
          (candidate) => containerIdForService(candidate) === target,
        );
        const role = IMAGE_ROLE[service];
        const index = SERVICES.indexOf(service);
        const migration = service === "migration";
        stdout = canonicalHostJson([{
          args: [`${service}/main.js`],
          cgroupParent: "/weather",
          containerNumber: 1,
          exitCode: 0,
          health: migration ? "none" : "healthy",
          id: target,
          imageId: `sha256:image-${role}`,
          imageReference: `weather-${role}:fixture`,
          memory: 536870912,
          name: `weather-${service}-1`,
          nanoCpus: 1000000000,
          oneOff: migration,
          path: "node",
          pid: migration ? 0 : 2000 + index * 10,
          pidsLimit: 256,
          project: "weather",
          readOnlyRootfs: true,
          running: !migration,
          service,
          startedAt: "2026-11-13T07:00:00.000Z",
          status: migration ? "exited" : "running",
        }]);
      }
    }
    return {
      exitCode,
      stderr: Buffer.from(""),
      stdout: Buffer.from(stdout),
    };
  };
  return base;
}

// collect one exact selected API inspect failure through the real collector
async function collectApiInspectFailure({ malformed = false } = {}) {
  const request = await checkedRequest("blueberry-compose");
  request.limits.maxFiles = 2048;
  const fixture = await scriptedProfileAdapters("blueberry-compose");
  const baseExecutor = fixture.adapters.executor;

  // intercept only the selected API inspect command
  fixture.adapters.executor = async (program, args, options) => {
    const selectedApiInspect = program === "/usr/bin/docker" &&
      args[0] === "inspect" && args.at(-1) === containerIdForService("api");

    // return one observed failed or malformed selected inspect
    if (selectedApiInspect) {
      fixture.calls.commands.push({
        args: [...args],
        cwd: options.cwd,
        deadlineMs: options.deadlineMs,
        env: structuredClone(options.env),
        maxBytes: options.maxBytes,
        program,
      });
      return malformed
        ? {
          exitCode: 0,
          stderr: Buffer.alloc(0),
          stdout: Buffer.from("{malformed selected inspect"),
        }
        : {
          exitCode: 7,
          stderr: Buffer.from("selected inspect failed\n"),
          stdout: Buffer.alloc(0),
        };
    }
    return await baseExecutor(program, args, options);
  };
  const evidence = await collectBackendHostEvidence(request, fixture.adapters);
  return {
    evidence,
    inventory: JSON.parse(request.inventory.json),
    runtimeMap: JSON.parse(request.runtimeMap.json),
  };
}

// require one stable diagnostic code
function assertDiagnostic(result, code) {
  assert.equal(result.ok, false);
  assert.ok(result.diagnostics.some(
    // match the requested stable code
    (diagnostic) => diagnostic.code === code,
  ), JSON.stringify(result.diagnostics));
}

// require one stable diagnostic code at one evidence path
function assertDiagnosticPath(result, code, path) {
  assert.equal(result.ok, false);
  assert.ok(result.diagnostics.some(
    // match the requested code and evidence path
    (diagnostic) => diagnostic.code === code && diagnostic.path === path,
  ), JSON.stringify(result.diagnostics));
}

// require one of several stable diagnostic codes
function assertAnyDiagnostic(result, codes) {
  assert.equal(result.ok, false);
  assert.ok(result.diagnostics.some(
    // match one accepted failure boundary
    (diagnostic) => codes.includes(diagnostic.code),
  ), JSON.stringify(result.diagnostics));
}

// require one coded asynchronous rejection
async function assertRejectCode(run, code) {
  await assert.rejects(run, (error) => error?.code === code);
}

// build one controlled confined-read request
function secureReaderRequest(rootPath, overrides = {}) {
  return {
    contractVersion: hostObserver.HOST_SECURE_READER_REQUEST_VERSION,
    maximumBytes: 4096,
    operation: "confined-files",
    profile: "synthetic-test",
    subjects: [{
      expectedGid: process.getgid(),
      expectedMode: "0644",
      expectedUid: process.getuid(),
      id: "fixture-payload",
      readMode: "bytes",
      relativeComponents: ["nested", "payload.txt"],
      rootPath,
      rootSelector: "controlled-test-root",
      ...overrides,
    }],
  };
}

// execute the reviewed helper as an injected child
async function executeSecureReader(program, args, options) {
  const child = spawn(program, args, {
    cwd: options.cwd,
    env: options.env,
    stdio: ["pipe", "pipe", "pipe"],
  });
  const stdout = [];
  const stderr = [];
  let aborted = false;

  // stop one injected helper child
  const abort = () => {
    aborted = true;
    child.kill("SIGTERM");
  };
  options.signal.addEventListener("abort", abort, { once: true });

  // retain bounded helper output
  child.stdout.on("data", (chunk) => {
    stdout.push(chunk);
  });

  // retain bounded helper diagnostics
  child.stderr.on("data", (chunk) => {
    stderr.push(chunk);
  });
  child.stdin.end(options.stdin);
  const exitCode = await new Promise((resolve, reject) => {
    child.once("error", reject);
    child.once("close", resolve);
  });
  options.signal.removeEventListener("abort", abort);
  return {
    exitCode: aborted ? 143 : exitCode,
    stderr: Buffer.concat(stderr),
    stdout: Buffer.concat(stdout),
  };
}

// execute the fixed helper and mutate one path after its descriptor is pinned
async function executeSecureReaderAfterPin(request, pinnedPath, mutate) {
  const child = spawn(
    hostObserver.HOST_SECURE_READER_PROGRAM,
    ["-I", "-B", "-S", "-c", hostObserver.HOST_SECURE_READER_SOURCE],
    {
      cwd: "/",
      env: { LANG: "C" },
      stdio: ["pipe", "pipe", "pipe"],
    },
  );
  const stdout = [];
  const stderr = [];

  // retain fixed-helper stdout bytes
  child.stdout.on("data", (chunk) => {
    stdout.push(chunk);
  });

  // retain fixed-helper stderr bytes
  child.stderr.on("data", (chunk) => {
    stderr.push(chunk);
  });
  child.stdin.end(Buffer.from(`${canonicalHostJson(request)}\n`));
  let pinned = false;
  const observer = (async () => {
    const deadline = Date.now() + 5000;

    // poll only this controlled child descriptor table
    while (!pinned && Date.now() < deadline) {
      let descriptors = [];

      // tolerate the child exiting between proc directory operations
      try {
        descriptors = await readdir(`/proc/${String(child.pid)}/fd`);
      } catch {
        break;
      }

      // detect the exact pinned source-owned path
      for (const descriptor of descriptors) {
        let target;

        // tolerate one descriptor closing during inspection
        try {
          target = await readlink(`/proc/${String(child.pid)}/fd/${descriptor}`);
        } catch {
          continue;
        }

        // mutate only after the exact descriptor is visible
        if (target === pinnedPath) {
          pinned = true;
          await mutate();
          break;
        }
      }

      // yield without timing sleeps while the child advances
      if (!pinned) {
        await new Promise((resolve) => setImmediate(resolve));
      }
    }
  })();
  const exitCode = await new Promise((resolve, reject) => {
    child.once("error", reject);
    child.once("close", resolve);
  });
  await observer;
  return {
    exitCode,
    pinned,
    stderr: Buffer.concat(stderr),
    stdout: Buffer.concat(stdout),
  };
}

// execute the fixed helper and mutate after one file closes during later traversal
async function executeSecureReaderAfterCloseAndPin(
  request,
  closedPath,
  laterPinnedPath,
  mutate,
) {
  const child = spawn(
    hostObserver.HOST_SECURE_READER_PROGRAM,
    ["-I", "-B", "-S", "-c", hostObserver.HOST_SECURE_READER_SOURCE],
    {
      cwd: "/",
      env: { LANG: "C" },
      stdio: ["pipe", "pipe", "pipe"],
    },
  );
  const stdout = [];
  const stderr = [];

  // retain fixed-helper stdout bytes
  child.stdout.on("data", (chunk) => {
    stdout.push(chunk);
  });

  // retain fixed-helper stderr bytes
  child.stderr.on("data", (chunk) => {
    stderr.push(chunk);
  });
  child.stdin.end(Buffer.from(`${canonicalHostJson(request)}\n`));
  let sawClosedPath = false;
  let mutated = false;
  const observer = (async () => {
    const deadline = Date.now() + 5000;

    // poll only this controlled child descriptor table
    while (!mutated && Date.now() < deadline) {
      let descriptors = [];

      // tolerate the child exiting between proc directory operations
      try {
        descriptors = await readdir(`/proc/${String(child.pid)}/fd`);
      } catch {
        break;
      }
      const targets = [];

      // retain each descriptor target observed in this polling instant
      for (const descriptor of descriptors) {
        try {
          targets.push(await readlink(`/proc/${String(child.pid)}/fd/${descriptor}`));
        } catch {
          // ignore one descriptor that closed during inspection
        }
      }

      // remember the initial file descriptor before waiting for its close
      if (targets.includes(closedPath)) {
        sawClosedPath = true;
      }

      // mutate only after the file closes and later traversal is pinned
      if (sawClosedPath && !targets.includes(closedPath) && targets.includes(laterPinnedPath)) {
        mutated = true;
        await mutate();
        break;
      }

      // yield without timing sleeps while the child advances
      if (!mutated) {
        await new Promise((resolve) => setImmediate(resolve));
      }
    }
  })();
  const exitCode = await new Promise((resolve, reject) => {
    child.once("error", reject);
    child.once("close", resolve);
  });
  await observer;
  return {
    exitCode,
    mutated,
    stderr: Buffer.concat(stderr),
    stdout: Buffer.concat(stdout),
  };
}

// write one bounded physical control-plane fixture tree
async function writeControlledControlPlaneTree(rootPath, { large = false } = {}) {
  const files = new Map([
    ["deploy/postgres/roles.sql", Buffer.from("select 1;\n")],
    ["deploy/scripts/a-seen.txt", Buffer.from("seen before mutation\n")],
    ["deploy/scripts/z-script.sh", Buffer.from("#!/bin/sh\n")],
    ["deploy/sudoers/weather", Buffer.from("weather ALL=(root) /bin/true\n")],
    ["deploy/systemd/weather.service", Buffer.from("[Service]\n")],
    ["deploy/compose.yaml", Buffer.from("services: {}\n")],
  ]);

  // add one bounded slow digest source after directory enumeration
  if (large) {
    files.set("deploy/scripts/m-large.bin", Buffer.alloc(0));
  }

  // create every fixed protected root
  for (const directory of [
    "deploy/postgres",
    "deploy/scripts",
    "deploy/sudoers",
    "deploy/systemd",
  ]) {
    await mkdir(join(rootPath, directory), { recursive: true });
  }
  await chmod(join(rootPath, "deploy"), 0o755);

  // normalize every protected fixture directory despite the test umask
  for (const directory of [
    "deploy/postgres",
    "deploy/scripts",
    "deploy/sudoers",
    "deploy/systemd",
  ]) {
    await chmod(join(rootPath, directory), 0o755);
  }

  // retain every exact fixture file
  for (const [relativePath, bytes] of files) {
    const path = join(rootPath, relativePath);
    await writeFile(path, bytes, { mode: 0o644 });

    // expand only the designated bounded race file
    if (large && relativePath.endsWith("m-large.bin")) {
      await truncate(path, 16 * 1024 * 1024);
    }
    await chmod(path, 0o644);
  }
  return files;
}

// widen one protected directory to keep closing traversal observable
async function writeWideControlPlaneDirectory(rootPath, count = 256) {
  const postgresPath = join(rootPath, "deploy", "postgres");

  // create one bounded set of empty protected directories
  for (let index = 0; index < count; index += 1) {
    const directory = join(postgresPath, `fixture-${String(index).padStart(3, "0")}`);
    await mkdir(directory);
    await chmod(directory, 0o755);
  }
}

// build one controlled physical tree request
function controlledControlPlaneRequest(rootPath) {
  return {
    contractVersion: hostObserver.HOST_SECURE_READER_REQUEST_VERSION,
    maximumBytes: 20 * 1024 * 1024,
    operation: "confined-files",
    profile: "synthetic-test",
    subjects: [{
      expectedGid: process.getgid(),
      expectedMode: "0755",
      expectedType: "directory",
      expectedUid: process.getuid(),
      id: "operator.control-plane.files",
      readMode: "tree",
      relativeComponents: ["deploy"],
      rootPath,
      rootSelector: "controlled-test-root",
    }],
  };
}

// hash one independent v17 fixture file sequence
function independentV17Digest(files) {
  const protectedPaths = [...files.keys()].filter(
    // exclude the separately appended Compose file
    (path) => path !== "deploy/compose.yaml",
  ).sort();
  return sha256Bytes(Buffer.from([...protectedPaths, "deploy/compose.yaml"].map(
    // retain the predecessor path-digest framing exactly
    (path) => `${path}\0${sha256Bytes(files.get(path))}\n`,
  ).join("")));
}

// create one fifo without a shell
async function createFifo(path) {
  const child = spawn("/usr/bin/mkfifo", [path], {
    env: { LANG: "C" },
    stdio: ["ignore", "pipe", "pipe"],
  });
  let stderr = "";
  child.stderr.setEncoding("utf8");

  // retain one fixture command error
  child.stderr.on("data", (chunk) => {
    stderr += chunk;
  });
  const exitCode = await new Promise((resolve, reject) => {
    child.once("error", reject);
    child.once("close", resolve);
  });
  assert.equal(exitCode, 0, stderr);
}

// build one portable fixed file identity
function secureFileIdentity(subject) {
  return {
    ctimeNs: "1000000000",
    dev: "2049",
    gid: subject.expectedGid,
    ino: "4001",
    mode: subject.expectedMode,
    mtimeNs: "1000000000",
    nlink: "1",
    size: "22",
    type: "regular-file",
    uid: subject.expectedUid,
  };
}

// build one portable fixed directory identity
function secureDirectoryIdentity(ino) {
  return {
    ctimeNs: "1000000000",
    dev: "2049",
    gid: 0,
    ino,
    mode: "0755",
    mtimeNs: "1000000000",
    nlink: "2",
    size: "4096",
    type: "directory",
    uid: 0,
  };
}

// select deterministic physical bytes for one secure subject
function secureSubjectBytes(subject) {
  const path = subject.relativeComponents.join("/");

  // retain the complete current control-plane tree proof
  if (subject.id === "operator.control-plane.files") {
    return Buffer.from(canonicalHostJson(OPERATOR_FIXTURE.controlPlane));
  }

  // retain the exact immutable or bootstrap installation receipt
  if (path.endsWith("schema-preserving-release-v1/installation.json") ||
    path.includes("schema-preserving-release-v1/installations/")) {
    return OPERATOR_FIXTURE.receiptBytes;
  }

  // retain the exact supervising base unit bytes
  if (path.endsWith("weather-compose.service")) {
    return OPERATOR_FIXTURE.baseUnitBytes;
  }

  // retain the exact effective schema-preserving drop-in bytes
  if (path.endsWith("10-schema-preserving-release.conf")) {
    return OPERATOR_FIXTURE.dropInBytes;
  }

  // retain the exact immutable operator module bytes
  if (path.endsWith("schema-preserving-release.mjs")) {
    return OPERATOR_FIXTURE.moduleBytes;
  }

  // retain the exact immutable operator launcher bytes
  if (path.endsWith("schema-preserving-release.sh")) {
    return OPERATOR_FIXTURE.scriptBytes;
  }

  // retain exact release marker contents
  if (path.endsWith("current-release")) {
    return Buffer.from("2026.11.13-1\n");
  }

  // retain exact previous release marker contents
  if (path.endsWith("previous-release")) {
    return Buffer.from("2026.11.12-1\n");
  }

  // retain exact schema release marker contents
  if (path.endsWith("schema-release")) {
    return Buffer.from("2026.11.13-schema\n");
  }

  // retain one container root ABI identity
  if (path.endsWith("etc/os-release")) {
    return Buffer.from("ID=debian\nVERSION_ID=\"12\"\n");
  }

  // retain one container libc identity
  if (path.endsWith("libc.so.6")) {
    return Buffer.from("fixture-libc");
  }

  // retain one container executable identity
  if (path.endsWith("node") || path.endsWith("postgres") ||
    path.endsWith("cloudflared") || path.endsWith("docker-entrypoint.sh")) {
    return Buffer.from("fixture-executable");
  }
  return Buffer.from("fixture-runtime-bytes\n");
}

// build one exact release snapshot binding
function secureReleaseBinding() {
  const markers = [
    { id: "current-release", bytes: rawBytes("2026.11.13-1\n") },
    { id: "previous-release", bytes: rawBytes("2026.11.12-1\n") },
    { id: "schema-release", bytes: rawBytes("2026.11.13-schema\n") },
  ];
  return {
    anchor: secureDirectoryIdentity("3001"),
    currentTarget: null,
    markersAfter: structuredClone(markers),
    markersBefore: structuredClone(markers),
    releaseRoot: secureDirectoryIdentity("3002"),
  };
}

// build one exact runner current-alias binding
function secureRunnerReleaseBinding() {
  return {
    anchor: secureDirectoryIdentity("6001"),
    currentTarget: ["releases", "a".repeat(40)],
    markersAfter: [],
    markersBefore: [],
    releaseRoot: secureDirectoryIdentity("6002"),
  };
}

// build one exact synthetic proc-root binding
function secureProcBinding(subject) {
  const service = SERVICES.find(
    // bind the selected Docker identity back to its fixed service
    (candidate) => containerIdForService(candidate) === subject.containerId,
  ) ?? "fixture";
  const startTime = subject.pid % 10 === 1 ? "1001" : "1000";
  return {
    cgroupAfter: rawBytes(`0::/weather/${service}\n`),
    cgroupBefore: rawBytes(`0::/weather/${service}\n`),
    containerId: subject.containerId,
    imageId: subject.imageId,
    mountNamespace: secureDirectoryIdentity("5001"),
    mountinfoAfter: rawBytes("fixture mountinfo\n"),
    mountinfoBefore: rawBytes("fixture mountinfo\n"),
    pid: subject.pid,
    pidfd: true,
    pidfdInfo: rawBytes(`Pid:\t${String(subject.pid)}\n`),
    root: secureDirectoryIdentity("5002"),
    startTimeAfter: startTime,
    startTimeBefore: startTime,
  };
}

// return one physical-shape secure helper execution
function secureExecutionForRequest(request, mutate = null) {
  const subjects = request.subjects.map(
    // bind each result to its exact request subject
    (subject) => {
      const bytes = secureSubjectBytes(subject);
      const retained = ["bytes", "tree"].includes(subject.readMode)
        ? rawBytes(bytes)
        : null;
      const directory = subject.expectedType === "directory";
      const identity = directory
        ? secureDirectoryIdentity("4001")
        : secureFileIdentity(subject);

      // bind the portable identity to the requested owner mode and size
      if (directory) {
        identity.gid = subject.expectedGid;
        identity.mode = subject.expectedMode;
        identity.uid = subject.expectedUid;
      } else {
        identity.size = String(bytes.length);
      }
      const runnerCurrent = ["runner-home", "controlled-test-root"].includes(
        subject.rootSelector,
      ) &&
        subject.relativeComponents.includes("current");
      return {
        bytes: retained,
        contentSha256: subject.readMode === "metadata" ? null : sha256Bytes(bytes),
        errorCode: null,
        executionIdentity: {
          dropped: subject.rootSelector === "runner-home",
          gid: subject.rootSelector === "runner-home" ? subject.expectedGid : 0,
          supplementaryGroups: [],
          uid: subject.rootSelector === "runner-home" ? subject.expectedUid : 0,
        },
        id: subject.id,
        identity: { post: structuredClone(identity), pre: identity },
        procBinding: request.operation === "container-proc-root"
          ? secureProcBinding(subject)
          : null,
        releaseBinding: subject.rootSelector === "release-root"
          ? secureReleaseBinding()
          : runnerCurrent
            ? secureRunnerReleaseBinding()
            : null,
        status: "ok",
      };
    },
  );
  const result = {
    authority: { kind: "python-helper", liveEligible: false },
    completedAt: "2026-11-13T08:00:01.000Z",
    startedAt: "2026-11-13T08:00:00.000Z",
    subjects,
  };

  // apply one explicit adversarial result mutation
  if (mutate !== null) {
    mutate(result);
  }
  return {
    exitCode: 0,
    stderr: Buffer.alloc(0),
    stdout: Buffer.from(`${canonicalHostJson(result)}\n`),
  };
}

// build one capturing injected secure executor
function capturingSecureExecutor(calls, mutate = null) {
  return async (program, args, options) => {
    const request = JSON.parse(options.stdin.toString("utf8"));
    calls.push({ args, options, program, request });
    return secureExecutionForRequest(request, mutate);
  };
}

// run one invalid CLI request without reaching physical probes
async function runInvalidCli(args, input) {
  const child = spawn(process.execPath, [PRODUCER_PATH.pathname, ...args], {
    cwd: REPOSITORY_ROOT.pathname,
    env: { LANG: "C", PATH: process.env.PATH },
    stdio: ["pipe", "pipe", "pipe"],
  });
  let stdout = "";
  let stderr = "";
  child.stdout.setEncoding("utf8");
  child.stderr.setEncoding("utf8");
  child.stdout.on("data", (chunk) => {
    stdout += chunk;
  });
  child.stderr.on("data", (chunk) => {
    stderr += chunk;
  });
  child.stdin.end(input);
  const exitCode = await new Promise((resolve, reject) => {
    child.once("error", reject);
    child.once("close", resolve);
  });
  return { exitCode, stderr, stdout };
}

// prove the observer module has no package or TypeScript runtime closure
test("host observer imports standalone with Node builtins only", async () => {
  const rootDir = await mkdtemp(join(tmpdir(), "weather-host-observer-import-"));
  const copiedPath = join(rootDir, "backend-host-observation.mjs");
  try {
    await copyFile(PRODUCER_PATH, copiedPath);
    const copied = await import(`${pathToFileURL(copiedPath).href}?fixture=1`);
    assert.equal(copied.HOST_EVIDENCE_CONTRACT_VERSION, "weather-backend-host-evidence/v1");
    assert.equal(copied.HOST_OBSERVATION_VERSION, "weather-backend-host-observation/v2");
    assert.equal(copied.HOST_PRODUCER, "weather-backend-read-only-host-observer/v2");
  } finally {
    await rm(rootDir, { force: true, recursive: true });
  }
});

// reject canonical values with ambiguous or effectful identities
test("canonical host JSON rejects collisions unsafe numbers and hidden structure", async (testContext) => {
  const cases = [
    ["sparse array", () => Array(1)],
    ["array property", () => Object.assign([1], { extra: true })],
    ["magic prototype key", () => JSON.parse('{"__proto__":{"admin":true}}')],
    ["unsafe integer", () => ({ value: Number.MAX_SAFE_INTEGER + 1 })],
    ["negative zero", () => ({ value: -0 })],
    ["symbol key", () => ({ value: 1, [Symbol("hidden")]: 2 })],
    ["hidden property", () => {
      const value = { visible: true };
      Object.defineProperty(value, "hidden", { enumerable: false, value: true });
      return value;
    }],
  ];

  // reject every non-injective canonical value class
  for (const [name, createValue] of cases) {
    // isolate one canonical rejection
    await testContext.test(name, () => {
      assert.throws(
        () => canonicalHostJson(createValue()),
        (error) => error?.code === "host.canonical",
      );
    });
  }

  // reject accessors without invoking caller code
  await testContext.test("accessor property", () => {
    let getterCalls = 0;
    const value = {};
    Object.defineProperty(value, "secret", {
      enumerable: true,
      get() {
        getterCalls += 1;
        return "do-not-read";
      },
    });
    assert.throws(
      () => canonicalHostJson(value),
      (error) => error?.code === "host.canonical",
    );
    assert.equal(getterCalls, 0);
  });
});

// keep CLI request JSON on stdin without credential or arbitrary-path channels
test("host observer CLI fails invalid stdin requests before physical probes", async () => {
  const secret = "do-not-echo-cli-secret";
  const invalidStdin = await runInvalidCli(
    ["collect", "--request", "/dev/stdin"],
    `${JSON.stringify({ sudoPassword: secret })}\n`,
  );
  assert.notEqual(invalidStdin.exitCode, 0);
  assert.match(invalidStdin.stderr, /host\.request/u);
  assert.equal(`${invalidStdin.stdout}${invalidStdin.stderr}`.includes(secret), false);
  const arbitraryPath = await runInvalidCli(
    ["collect", "--request", "/tmp/caller-request.json"],
    "{}\n",
  );
  assert.notEqual(arbitraryPath.exitCode, 0);
  assert.match(arbitraryPath.stderr, /host\.request/u);
});

// reject caller-selected commands paths identities and credentials
test("host collection request is closed and source inputs are digest bound", async () => {
  const request = await checkedRequest();
  const { adapters } = await injectedAdapters();
  const mutations = [
    ["argv", ["/bin/sh", "-c", "cat /etc/shadow"]],
    ["path", "/etc/shadow"],
    ["environment", { WEATHER_SECRET: "secret" }],
    ["sudoPassword", "secret"],
    ["containerId", "caller-selected"],
    ["uid", 0],
    ["home", "/root"],
  ];

  // reject every caller-selected authority field
  for (const [field, value] of mutations) {
    const candidate = structuredClone(request);
    candidate[field] = value;
    await assertRejectCode(
      // collect only through local injected adapters
      () => collectBackendHostEvidence(candidate, adapters),
      "host.request",
    );
  }
  const wrongProfile = structuredClone(request);
  wrongProfile.profile = "arbitrary-host";
  await assertRejectCode(
    // reject an unknown host profile
    () => collectBackendHostEvidence(wrongProfile, adapters),
    "host.request",
  );
  const wrongRole = structuredClone(request);
  wrongRole.expectedHostRole = "workstation";
  await assertRejectCode(
    // reject a profile and host-role substitution
    () => collectBackendHostEvidence(wrongRole, adapters),
    "host.request",
  );
  const wrongDigest = structuredClone(request);
  wrongDigest.inventory.sha256 = HASH_A;
  await assertRejectCode(
    // reject checked bytes with a forged digest
    () => collectBackendHostEvidence(wrongDigest, adapters),
    "host.request",
  );
});

// keep workstation home artifacts out of every Blueberry runtime context
test("checked Blueberry contexts exclude workstation home artifact bindings", async () => {
  const runtimeMap = JSON.parse(await readFile(RUNTIME_MAP_PATH, "utf8"));
  const artifacts = new Map(runtimeMap.artifacts.map(
    // index every checked artifact declaration
    (artifact) => [artifact.id, artifact],
  ));
  const invalid = [];

  // inspect only exact Blueberry context ownership
  for (const context of runtimeMap.contexts) {
    // ignore workstation-owned runtime instances
    if (context.profile !== "blueberry-compose") {
      continue;
    }

    // reject every literal or templated workstation-home artifact
    for (const artifactId of context.artifactIds) {
      const artifact = artifacts.get(artifactId);
      const path = artifact?.declaredPath ?? artifact?.path ?? "";

      // retain the exact invalid ownership tuple for diagnostics
      if (path.startsWith("/home/ubuntu/") || path.startsWith("${HOME}/")) {
        invalid.push({ artifactId, contextId: context.id, path });
      }
    }
  }
  assert.deepEqual(invalid, []);
});

// keep injected collection bounded read only and explicitly live-ineligible
test("injected collection uses only fixed bounded read-only probes", async () => {
  const request = await checkedRequest();
  const { adapters, calls } = await injectedAdapters();
  const evidence = await collectBackendHostEvidence(request, adapters);
  assert.equal(evidence.authority.kind, "injected-test");
  assert.equal(evidence.authority.liveEligible, false);
  assert.equal(evidence.authority.canonicalEligible, false);
  assert.equal(evidence.complete, false);
  assert.ok(evidence.probes.length > 0);
  assert.ok(new Date(evidence.startedAt) <= new Date(evidence.completedAt));

  // verify every command stays in the absolute read-only allowlist
  for (const call of calls.commands) {
    assert.ok([
      "/usr/bin/docker",
      "/usr/bin/getconf",
      "/usr/bin/getent",
      "/usr/bin/systemctl",
      "/usr/bin/uname",
    ].includes(call.program), call.program);
    assert.deepEqual(call.env, { LANG: "C" });
    assert.equal(call.cwd, "/");
    assert.ok(call.deadlineMs > 0 && call.deadlineMs <= 30000);
    assert.ok(call.maxBytes > 0 && call.maxBytes <= 4194304);
    assert.equal(call.args.some((value) => /Env|environ|secret|password/iu.test(value)), false);

    // require selected Docker output instead of full secret-bearing inspect
    if (call.program === "/usr/bin/docker" && call.args.includes("inspect")) {
      assert.ok(call.args.includes("--format"));
    }
  }

  // forbid request-selected or secret filesystem reads
  for (const path of calls.paths) {
    assert.equal(/shadow|environ|\.env(?:$|\/)/u.test(path), false, path);
  }
});

// attempt every fixed profile probe even when local fake sources are absent
test("collector emits the complete profile-specific probe catalog", async (testContext) => {
  const cases = [
    { profile: "blueberry-compose", units: ["weather-compose.service"] },
    { profile: "workstation-runner", units: RUNNER_UNITS },
  ];

  // verify both fixed host profiles independently
  for (const fixtureCase of cases) {
    // isolate one fixed profile catalog
    await testContext.test(fixtureCase.profile, async () => {
      const request = await checkedRequest(fixtureCase.profile);
      request.limits.maxFiles = 2048;
      const runtimeMap = JSON.parse(request.runtimeMap.json);
      const { adapters, calls } = await injectedAdapters();
      const evidence = await collectBackendHostEvidence(request, adapters);
      assert.ok(Array.isArray(evidence.errors));
      assert.ok(evidence.errors.length > 0);
      assert.ok(evidence.probes.every(
        // keep failed attempts outside semantic probe inputs
        (candidate) => candidate.outcome.status === "ok" &&
          (candidate.outcome.exitCode === 0 || candidate.outcome.exitCode === null ||
            (candidate.id.endsWith(".enabled") &&
              [1, 3, 4].includes(candidate.outcome.exitCode))),
      ));
      assert.ok(evidence.errors.every(
        // retain failed attempts as diagnostics only
        (candidate) => candidate.outcome.status !== "ok" || candidate.outcome.exitCode !== 0,
      ));
      const attemptedIds = evidence.probes.map(
        // retain each attempted probe identity
        (candidate) => candidate.id,
      );
      const unresolvedIds = evidence.unresolved.map(
        // retain each safely blocked planned identity
        (candidate) => candidate.id,
      );
      const actualIds = new Set([...attemptedIds, ...unresolvedIds]);
      assert.equal(actualIds.size, attemptedIds.length + unresolvedIds.length, "duplicate or overlapping probe identities");
      assert.ok(evidence.unresolved.every(
        // require explicit nonempty prerequisite backlinks
        (unresolved) => unresolved.code === "blocked-by-prerequisite" &&
          unresolved.prerequisiteIds.length > 0,
      ));
      assert.ok(evidence.errors.every(
        // bind every failed attempt to one unresolved planned fact
        (candidate) => unresolvedIds.includes(candidate.id),
      ));
      const expectedIds = new Set([
        "host.cgroup-namespace",
        "host.cgroup-statfs",
        "host.cpuinfo",
        "host.filesystems",
        "host.libc",
        "host.mountinfo",
        "host.os-release",
        "host.self-cgroup",
        "host.uname-kernel",
        "host.uname-machine",
      ]);

      // retain profile-specific Docker and release probes
      if (fixtureCase.profile === "blueberry-compose") {
        expectedIds.add("docker.containers.list");
        expectedIds.add("release.current");
        expectedIds.add("release.previous");
        expectedIds.add("release.schema");

        // retain every fixed Compose service probe family
        for (const service of SERVICES) {
          expectedIds.add(`docker.container.${service}.inspect`);
          expectedIds.add(`docker.container.${service}.top`);
          expectedIds.add(`docker.container.${service}.processes`);
          expectedIds.add(`cgroup.blueberry:compose:${service}`);
        }

        // retain every selected runtime image identity
        for (const role of ["cloudflared", "postgres", "server", "web"]) {
          expectedIds.add(`docker.image.${role}.inspect`);
        }
      } else {
        expectedIds.add("nss.ubuntu.passwd");
        expectedIds.add("runner.processes");
        expectedIds.add("runner.root.library");
        expectedIds.add("runner.root.private");
      }

      // retain exact unit raw bytes and selected state
      for (const unit of fixtureCase.units) {
        expectedIds.add(`systemd.${unit}.show`);
        expectedIds.add(`systemd.${unit}.files`);

        // retain workstation enablement separately
        if (fixtureCase.profile === "workstation-runner") {
          expectedIds.add(`systemd.${unit}.enabled`);
        }
      }
      const contexts = runtimeMap.contexts.filter(
        // select physical runtime instances for this profile
        (context) => context.profile === fixtureCase.profile &&
          context.activationSource !== "never-active-alternative",
      );

      // retain every exact context and artifact probe identity
      for (const context of contexts) {
        // retain every artifact installation in this instance
        for (const artifactId of context.artifactIds) {
          expectedIds.add(`artifact.${sha256Bytes(`${context.id}\0${artifactId}`)}`);
        }
      }

      // require every fixed probe to be represented even on collection error
      for (const expectedId of expectedIds) {
        assert.ok(actualIds.has(expectedId), `missing probe ${expectedId}`);
      }
      assert.equal(evidence.complete, false);

      // do not touch dependent runtime paths after failed identity prerequisites
      if (fixtureCase.profile === "workstation-runner") {
        assert.equal(calls.paths.some(
          // reject caller-home or runner-root reads before valid NSS
          (path) => path.includes("/.weather/") || path.includes("/.local/lib/"),
        ), false);
      }
    });
  }
});

// execute dependent safe probes only after their typed prerequisites validate
test("collector executes dependent profile probes after valid prerequisites", async (testContext) => {
  const cases = ["blueberry-compose", "workstation-runner"];

  // verify both prerequisite chains independently
  for (const profile of cases) {
    // isolate one successful local prerequisite chain
    await testContext.test(profile, async () => {
      const request = await checkedRequest(profile);
      request.limits.maxFiles = 2048;
      const runtimeMap = JSON.parse(request.runtimeMap.json);
      const { adapters, calls } = await scriptedProfileAdapters(profile);
      const evidence = await collectBackendHostEvidence(request, adapters);
      const probeIds = new Set(evidence.probes.map(
        // retain every actually executed probe
        (candidate) => candidate.id,
      ));

      // require selected Docker and cgroup collection after exact container identity
      if (profile === "blueberry-compose") {
        assert.equal(calls.commands.some(
          // forbid workstation NSS authority on Blueberry
          (call) => call.program === "/usr/bin/getent",
        ), false);
        const listCall = calls.commands.find(
          // select the fixed Compose container enumeration
          (call) => call.program === "/usr/bin/docker" && call.args[0] === "ps",
        );
        assert.deepEqual(listCall.args, [
          "ps",
          "--all",
          "--filter",
          "label=com.docker.compose.project=weather",
          "--no-trunc",
          "--format",
          '{{.Label "com.docker.compose.service"}}\t{{.ID}}',
        ]);

        for (const service of SERVICES.filter(
          // omit the source-proven absent migration one-shot
          (candidate) => candidate !== "migration",
        )) {
          assert.ok(probeIds.has(`docker.container.${service}.inspect`), service);
          assert.ok(probeIds.has(`docker.container.${service}.top`), service);
          assert.ok(probeIds.has(`docker.container.${service}.processes`), service);
          assert.ok(probeIds.has(`cgroup.blueberry:compose:${service}`), service);
        }

        // require every exact image selected from validated containers
        for (const role of ["cloudflared", "postgres", "server", "web"]) {
          assert.ok(probeIds.has(`docker.image.${role}.inspect`), role);
        }
        assert.ok(probeIds.has("systemd.weather-compose.service.files"));
        const topProbe = evidence.probes.find(
          // select the retained native API process table
          (candidate) => candidate.id === "docker.container.api.top",
        );
        assert.match(
          Buffer.from(topProbe.raw.stdout.dataBase64, "base64").toString("utf8"),
          /^PID PPID COMMAND\n/mu,
        );
        const processProbe = evidence.probes.find(
          // select the API proc-derived identity
          (candidate) => candidate.id === "docker.container.api.processes",
        );
        const processValue = JSON.parse(
          Buffer.from(processProbe.raw.stdout.dataBase64, "base64").toString("utf8"),
        );
        assert.equal(processProbe.kind, "derived");
        assert.equal(processProbe.operation.method, "docker-top-procfs-v2");
        assert.deepEqual(processProbe.operation.inputProbeIds, [
          "docker.container.api.inspect",
          "docker.container.api.top",
        ]);
        assert.equal(processValue.containerId, containerIdForService("api"));
        assert.equal(processValue.dockerInitPid, 2000);
        assert.ok(processValue.processes.length >= 2);
        assert.ok(processValue.processes.every(
          // require every retained physical process source
          (process) => process.raw.cmdline !== undefined &&
            process.raw.cgroup !== undefined &&
            process.raw.stat !== undefined &&
            process.raw.maps !== undefined &&
            process.raw.exeLink !== undefined &&
            process.raw.rootOsRelease !== undefined &&
            process.secureRootProof !== undefined,
        ));
        const controlProbe = evidence.probes.find(
          // select the complete installed control-plane tree
          (candidate) => candidate.id === "operator.control-plane.files",
        );
        assert.ok(controlProbe);
        const controlPlane = JSON.parse(
          Buffer.from(controlProbe.raw.stdout.dataBase64, "base64").toString("utf8"),
        );
        assert.equal(controlPlane.files.length, 57);
        assert.equal(
          controlPlane.controlPlaneSha256,
          OPERATOR_FIXTURE.receipt.controlPlaneSha256,
        );
      } else {
        // require unit bytes only after loaded unit state
        for (const unit of RUNNER_UNITS) {
          assert.ok(probeIds.has(`systemd.${unit}.files`), unit);
        }
        assert.ok(probeIds.has("runner.processes"));
      }
      assert.equal(calls.commands.some(
        // forbid the redundant systemctl cat transport
        (call) => call.program === "/usr/bin/systemctl" && call.args[0] === "cat",
      ), false);
      const profileContexts = runtimeMap.contexts.filter(
        // select physical runtime instances for this profile
        (context) => context.profile === profile &&
          context.activationSource !== "never-active-alternative",
      );
      const expectedArtifactProbe = profileContexts.flatMap(
        // retain every context-specific artifact identity
        (context) => context.artifactIds.map(
          // derive the fixed artifact probe identity
          (artifactId) => `artifact.${sha256Bytes(`${context.id}\0${artifactId}`)}`,
        ),
      ).find(
        // choose one required dependent artifact probe
        (probeId) => probeIds.has(probeId),
      );
      assert.ok(expectedArtifactProbe, `missing executed ${profile} artifact probe`);
      assert.equal(evidence.complete, true, canonicalHostJson({
        errors: evidence.errors,
        unresolved: evidence.unresolved,
      }));
      const result = buildBackendHostObservation({
        evidence,
        inventory: JSON.parse(request.inventory.json),
        phase: "baseline",
        runtimeMap,
      });
      assert.equal(result.ok, true, JSON.stringify(result.diagnostics));

      // keep phase-selected native alternatives outside legacy proof authority
      if (profile === "blueberry-compose") {
        const selectedNative = buildBackendHostObservation({
          evidence,
          inventory: JSON.parse(request.inventory.json),
          phase: "post-cutover",
          runtimeMap,
        });
        assert.equal(selectedNative.ok, false);
        assert.ok(selectedNative.diagnostics.some(
          // require the explicit native receipt boundary
          (diagnostic) => diagnostic.code === "host.binding" &&
            diagnostic.path === "$.phase",
        ));
      }
      assert.equal(evidence.authority.liveEligible, false);
    });
  }
});

// retain every observed selected inspect failure as typed raw evidence
test("collector retains failed and malformed selected Docker inspect attempts", async (testContext) => {
  const cases = [
    {
      exitCode: 7,
      label: "nonzero exit",
      malformed: false,
      stderr: "selected inspect failed\n",
      stdout: "",
    },
    {
      exitCode: 0,
      label: "malformed success bytes",
      malformed: true,
      stderr: "",
      stdout: "{malformed selected inspect",
    },
  ];

  // isolate failed and malformed selected observations
  for (const fixtureCase of cases) {
    await testContext.test(fixtureCase.label, async () => {
      const fixture = await collectApiInspectFailure({ malformed: fixtureCase.malformed });
      const attempt = fixture.evidence.errors.find(
        // select the exact retained API inspect attempt
        (candidate) => candidate.id === "docker.container.api.inspect",
      );
      assert.ok(attempt, canonicalHostJson({
        errors: fixture.evidence.errors,
        unresolved: fixture.evidence.unresolved.filter(
          // retain only the selected inspect family for failure output
          (candidate) => candidate.id.startsWith("docker.container.api."),
        ),
      }));
      assert.equal(attempt.kind, "command");
      assert.deepEqual(attempt.operation, {
        args: [
          "inspect",
          "--format",
          DOCKER_CONTAINER_FORMAT,
          containerIdForService("api"),
        ],
        program: "/usr/bin/docker",
      });
      assert.equal(attempt.sourceScope, "api");
      assert.deepEqual(attempt.outcome, {
        exitCode: fixtureCase.exitCode,
        status: "error",
      });
      assert.equal(
        Buffer.from(attempt.raw.stdout.dataBase64, "base64").toString("utf8"),
        fixtureCase.stdout,
      );
      assert.equal(
        Buffer.from(attempt.raw.stderr.dataBase64, "base64").toString("utf8"),
        fixtureCase.stderr,
      );
      assert.equal(fixture.evidence.probes.some(
        // keep diagnostic attempts outside semantic probe inputs
        (candidate) => candidate.id === attempt.id,
      ), false);
      const inspectUnresolved = fixture.evidence.unresolved.find(
        // select the exact unresolved failed fact
        (candidate) => candidate.id === attempt.id,
      );
      assert.deepEqual(inspectUnresolved?.prerequisiteIds, ["docker.containers.list"]);
      assert.deepEqual(fixture.evidence.unresolved.find(
        // bind the first dependent fact to the failed inspect
        (candidate) => candidate.id === "docker.container.api.top",
      )?.prerequisiteIds, ["docker.container.api.inspect"]);
    });
  }
});

// reject every substituted selected inspect error descriptor field
test("observation validates failed Docker inspect provenance and outcome", async (testContext) => {
  const fixture = await collectApiInspectFailure();
  const errorIndex = fixture.evidence.errors.findIndex(
    // select the exact retained API inspect attempt
    (candidate) => candidate.id === "docker.container.api.inspect",
  );
  assert.notEqual(errorIndex, -1, canonicalHostJson({
    errors: fixture.evidence.errors,
    unresolved: fixture.evidence.unresolved.filter(
      // retain only the selected inspect family for failure output
      (candidate) => candidate.id.startsWith("docker.container.api."),
    ),
  }));
  const cases = [
    {
      label: "kind",
      // substitute the attempted operation kind
      mutate(error) {
        error.kind = "file";
      },
    },
    {
      label: "program",
      // substitute the fixed executable
      mutate(error) {
        error.operation.program = "/usr/bin/false";
      },
    },
    {
      label: "arguments",
      // remove the selected output format
      mutate(error) {
        error.operation.args = ["inspect", error.operation.args.at(-1)];
      },
    },
    {
      label: "target",
      // swap the selected container identity
      mutate(error) {
        error.operation.args[3] = containerIdForService("worker");
      },
    },
    {
      label: "scope",
      // swap the selected service scope
      mutate(error) {
        error.sourceScope = "worker";
      },
    },
    {
      label: "status",
      // promote the failed attempt into a semantic success
      mutate(error) {
        error.outcome.status = "ok";
      },
    },
    {
      label: "exit type",
      // replace the numeric child exit status
      mutate(error) {
        error.outcome.exitCode = "7";
      },
    },
    {
      label: "missing error exit",
      // remove the required command failure status
      mutate(error) {
        error.outcome.exitCode = null;
      },
    },
    {
      label: "negative exit",
      // move the child exit outside its closed range
      mutate(error) {
        error.outcome.exitCode = -1;
      },
    },
    {
      label: "oversized exit",
      // move the child exit outside its closed range
      mutate(error) {
        error.outcome.exitCode = 256;
      },
    },
    {
      label: "raw shape",
      // add one unsupported raw envelope field
      mutate(error) {
        error.raw.stdout.unexpected = true;
      },
    },
  ];

  // isolate every failed-attempt descriptor substitution
  for (const fixtureCase of cases) {
    await testContext.test(fixtureCase.label, () => {
      const evidence = structuredClone(fixture.evidence);
      fixtureCase.mutate(evidence.errors[errorIndex]);
      const result = buildBackendHostObservation({
        evidence,
        inventory: fixture.inventory,
        phase: "baseline",
        runtimeMap: fixture.runtimeMap,
      });
      const indexPath = `$.evidence.errors[${String(errorIndex)}]`;
      assert.ok(result.diagnostics.some(
        // require a diagnostic tied to the substituted attempt
        (diagnostic) => ["host.bounds", "host.digest", "host.probe", "host.schema"].includes(
          diagnostic.code,
        ) && (
          diagnostic.path.startsWith(indexPath) ||
          diagnostic.path === "$.evidence.errors.docker.container.api.inspect"
        ),
      ), JSON.stringify(result.diagnostics));
    });
  }

  // reject a caller-selected failed-attempt prerequisite
  await testContext.test("prerequisites", () => {
    const evidence = structuredClone(fixture.evidence);
    const unresolvedIndex = evidence.unresolved.findIndex(
      // select the exact failed inspect blocker
      (candidate) => candidate.id === "docker.container.api.inspect",
    );
    assert.notEqual(unresolvedIndex, -1);
    evidence.unresolved[unresolvedIndex].prerequisiteIds = ["host.uname-machine"];
    const result = buildBackendHostObservation({
      evidence,
      inventory: fixture.inventory,
      phase: "baseline",
      runtimeMap: fixture.runtimeMap,
    });
    assertDiagnosticPath(
      result,
      "host.probe",
      `$.evidence.unresolved[${String(unresolvedIndex)}]`,
    );
  });
});

// reject oversized Docker project sets before the first selected inspect
test("collector gates Compose cardinality before container inspection", async () => {
  const request = await checkedRequest();
  const { adapters, calls } = await injectedAdapters();
  const baseExecutor = adapters.executor;
  adapters.executor = async (program, args, options) => {
    // return seven syntactically valid project container IDs
    if (program === "/usr/bin/docker" && args[0] === "ps") {
      calls.commands.push({ args: [...args], program });
      return {
        exitCode: 0,
        stderr: Buffer.alloc(0),
        stdout: Buffer.from(`${Array.from(
          { length: 7 },
          // retain one exact Docker-sized hex identity
          (_, index) => (index + 1).toString(16).padStart(64, "0"),
        ).join("\n")}\n`),
      };
    }
    return await baseExecutor(program, args, options);
  };
  const evidence = await collectBackendHostEvidence(request, adapters);
  assert.equal(calls.commands.some(
    // forbid selected inspect after invalid cardinality
    (call) => call.program === "/usr/bin/docker" && call.args[0] === "inspect",
  ), false);
  const result = buildBackendHostObservation({
    evidence,
    inventory: JSON.parse(request.inventory.json),
    phase: "baseline",
    runtimeMap: JSON.parse(request.runtimeMap.json),
  });
  assertDiagnostic(result, "host.cardinality");
});

// require a complete numeric NSS identity before runner-dependent reads
test("collector blocks runner enumeration after malformed NSS identity", async () => {
  const request = await checkedRequest("workstation-runner");
  const { adapters, calls } = await injectedAdapters();
  const baseExecutor = adapters.executor;
  let procOpened = false;
  const baseOpendir = adapters.filesystem.opendir;
  adapters.executor = async (program, args, options) => {
    // return a seven-field record with unsafe numeric identities
    if (program === "/usr/bin/getent") {
      calls.commands.push({ args: [...args], program });
      return {
        exitCode: 0,
        stderr: Buffer.alloc(0),
        stdout: Buffer.from("ubuntu:x:not-a-uid:-1:Ubuntu:/home/ubuntu:/bin/bash\n"),
      };
    }
    return await baseExecutor(program, args, options);
  };
  adapters.filesystem.opendir = async (path) => {
    // detect forbidden dependent proc enumeration
    if (String(path) === "/proc") {
      procOpened = true;
    }
    return await baseOpendir(path);
  };
  const evidence = await collectBackendHostEvidence(request, adapters);
  assert.equal(procOpened, false);
  assert.equal(calls.paths.some(
    // reject home-root reads from malformed caller identity
    (path) => path.includes("/.weather/") || path.includes("/.local/lib/"),
  ), false);
  const result = buildBackendHostObservation({
    evidence,
    inventory: JSON.parse(request.inventory.json),
    phase: "baseline",
    runtimeMap: JSON.parse(request.runtimeMap.json),
  });
  assertAnyDiagnostic(result, ["host.identity", "host.probe"]);
});

// source completed migration artifacts from an active same-image representative
test("collector never reads completed migration artifacts through PID zero", async () => {
  const request = await checkedRequest();
  request.limits.maxFiles = 2048;
  const runtimeMap = JSON.parse(request.runtimeMap.json);
  const { adapters, calls } = await scriptedProfileAdapters(
    "blueberry-compose",
    { includeMigration: true },
  );
  const evidence = await collectBackendHostEvidence(request, adapters);
  assert.equal(calls.paths.some(
    // forbid the nonexistent exited-container proc root
    (path) => path.startsWith("/proc/0/"),
  ), false);
  const migrationContext = runtimeMap.contexts.find(
    // select the exact migration service context
    (context) => context.rootEntryId === "compose-command:deploy/compose.yaml#migration",
  );
  assert.ok(migrationContext);
  const probeIds = new Set(evidence.probes.map(
    // retain every physically attempted fact
    (candidate) => candidate.id,
  ));

  // require all migration artifacts through the active server-image authority
  for (const artifactId of migrationContext.artifactIds) {
    const probeId = `artifact.${sha256Bytes(`${migrationContext.id}\0${artifactId}`)}`;
    assert.ok(
      probeIds.has(probeId),
      `${probeId}:${canonicalHostJson(evidence.unresolved.filter(
        // retain only this missing artifact explanation
        (item) => item.id === probeId,
      ))}`,
    );
    const artifactProbe = evidence.probes.find(
      // select the exact migration artifact observation
      (candidate) => candidate.id === probeId,
    );
    assert.ok(
      artifactProbe.operation.inputProbeIds.includes("docker.container.api.inspect") ||
        artifactProbe.operation.inputProbeIds.includes("docker.image.server.inspect"),
      `${probeId}:${canonicalHostJson(artifactProbe.operation)}`,
    );
    assert.equal(
      artifactProbe.operation.inputProbeIds.includes("docker.container.migration.inspect"),
      false,
      probeId,
    );
  }
});

// settle every sibling descriptor after one parallel filesystem failure
test("collector does not return before delayed sibling descriptors close", { timeout: 5000 }, async () => {
  const request = await checkedRequest();
  request.limits.maxFiles = 2048;
  const { adapters } = await scriptedProfileAdapters("blueberry-compose");
  const baseOpen = adapters.filesystem.open;
  let closeStarted = false;
  let closeSettled = false;
  let releaseClose;
  let released = false;
  adapters.filesystem.open = async (path, flags) => {
    const sourcePath = String(path);

    // fail one cgroup sibling immediately
    if (sourcePath.endsWith("/cgroup.controllers")) {
      throw new Error("fixture cgroup sibling failure");
    }

    // delay another sibling descriptor close
    if (sourcePath.endsWith("/cpu.max")) {
      const bytes = Buffer.from("100000 100000\n");
      return {
        // hold cleanup until the controlled release
        async close() {
          closeStarted = true;
          await new Promise((resolve) => {
            releaseClose = resolve;
          });
          closeSettled = true;
        },
        // read one exact cgroup value
        async read(buffer, offset, length, position) {
          const count = Math.max(0, Math.min(length, bytes.length - position));
          bytes.copy(buffer, offset, position, position + count);
          return { buffer, bytesRead: count };
        },
        // report size before reading
        async stat() {
          return { size: bytes.length };
        },
      };
    }
    return await baseOpen(path, flags);
  };
  const releaseTimer = setTimeout(() => {
    released = true;
    releaseClose?.();
  }, 1000);
  const evidence = await collectBackendHostEvidence(request, adapters);
  const returnedBeforeRelease = !released;

  // settle any broken early-return sibling before ending the test
  if (!released) {
    released = true;
    releaseClose?.();
  }
  clearTimeout(releaseTimer);
  await Promise.resolve();
  assert.equal(closeStarted, true);
  assert.equal(closeSettled, true);
  assert.equal(returnedBeforeRelease, false);
  assert.equal(evidence.complete, false);
});

// bound file and process discovery before materializing untrusted host data
test("collector bounds files and process directories before reading them", async () => {
  const request = await checkedRequest("workstation-runner");
  request.limits.maxProbeBytes = 1024;
  request.limits.maxProcesses = 2;
  const { adapters } = await injectedAdapters();
  const baseExecutor = adapters.executor;
  const baseOpen = adapters.filesystem.open;
  const baseOpendir = adapters.filesystem.opendir;
  let oversizedRead = false;
  let procClosed = false;
  let procReads = 0;
  adapters.executor = async (program, args, options) => {
    // provide only the exact NSS prerequisite for proc enumeration
    if (program === "/usr/bin/getent" && args.join("\0") === "passwd\0ubuntu") {
      return {
        exitCode: 0,
        stderr: Buffer.from(""),
        stdout: Buffer.from("ubuntu:x:1000:1000:Ubuntu:/home/ubuntu:/bin/bash\n"),
      };
    }
    return await baseExecutor(program, args, options);
  };

  // expose one oversized fixed host file without readable contents
  adapters.filesystem.open = async (path, flags) => {
    // preserve fixed checked config sources
    if (String(path) !== "/proc/cpuinfo") {
      return await baseOpen(path, flags);
    }
    return {
      // close one oversized fixture descriptor
      async close() {},
      // forbid a read before the size gate
      async read() {
        oversizedRead = true;
        throw new Error("oversized file was read");
      },
      // report a size above the request bound
      async stat() {
        return { size: 1025 };
      },
    };
  };
  adapters.filesystem.opendir = async (path) => {
    // preserve every non-proc fixture directory
    if (String(path) !== "/proc") {
      return await baseOpendir(path);
    }
    return {
      // close the bounded proc iterator
      async close() {
        procClosed = true;
      },
      // stream one proc identity at a time
      async read() {
        procReads += 1;
        return {
          isDirectory: () => true,
          isFile: () => false,
          name: String(3000 + procReads),
        };
      },
    };
  };
  const evidence = await collectBackendHostEvidence(request, adapters);
  assert.equal(evidence.complete, false);
  assert.equal(oversizedRead, false);
  assert.equal(procClosed, true);
  assert.ok(procReads <= request.limits.maxProcesses + 1, String(procReads));
  const result = buildBackendHostObservation({
    evidence,
    inventory: JSON.parse(request.inventory.json),
    phase: "baseline",
    runtimeMap: JSON.parse(request.runtimeMap.json),
  });
  assertDiagnostic(result, "host.bounds");
});

// abort globally late operations and settle cleanup before returning evidence
test("collector awaits late operation settlement after its global deadline", { timeout: 2000 }, async () => {
  const request = await checkedRequest();
  request.limits.deadlineMs = 1;
  const { adapters, calls } = await injectedAdapters();
  let resolveOperation;
  let resolveAbort;
  let returned = false;
  const aborted = new Promise((resolve) => {
    resolveAbort = resolve;
  });
  adapters.executor = async (program, args, options) => {
    calls.commands.push({ args, program });
    options.signal.addEventListener("abort", resolveAbort, { once: true });
    return await new Promise((resolve) => {
      resolveOperation = resolve;
    });
  };
  const collecting = collectBackendHostEvidence(request, adapters).then((evidence) => {
    returned = true;
    return evidence;
  });
  await aborted;
  assert.equal(returned, false);
  resolveOperation({
    exitCode: 0,
    stderr: Buffer.from(""),
    stdout: Buffer.from("late-result-must-not-append\n"),
  });
  const evidence = await collecting;
  assert.equal(returned, true);
  assert.equal(evidence.complete, false);
  assert.equal(calls.commands.length, 1);
  assert.equal(canonicalHostJson(evidence).includes("late-result-must-not-append"), false);
});

// prevent recomputed malicious map bytes from widening fixed probe scope
test("rehashed malicious runtime maps cannot select new host paths", async () => {
  const request = await checkedRequest();
  const malicious = JSON.parse(request.runtimeMap.json);
  malicious.artifacts.push({
    authority: { kind: "inventory-installed-path", sourcePath: "/etc/shadow" },
    contentRole: "operator-control",
    expectedMode: "0600",
    expectedType: "regular-file",
    id: "artifact:host-file:malicious#0",
    kind: "host-file",
    ownerPolicy: "root",
    path: "/etc/shadow",
    sourceEntryId: malicious.contexts[0].rootEntryId,
  });
  request.runtimeMap.json = `${canonicalHostJson(malicious)}\n`;
  request.runtimeMap.sha256 = sha256Bytes(request.runtimeMap.json);
  const { adapters, calls } = await injectedAdapters();
  await assertRejectCode(
    // reject bytes that differ from the fixed staged source input
    () => collectBackendHostEvidence(request, adapters),
    "host.request",
  );
  assert.equal(calls.paths.some((path) => path === "/etc/shadow"), false);
});

// scrub secret-like output instead of retaining it as raw evidence
test("secret-like probe output is incomplete and never retained", async () => {
  const values = [
    "--password do-not-retain\n",
    '["--token","do-not-retain"]\n',
    "Authorization: Bearer do-not-retain\n",
    "{\"accessToken\":\"do-not-retain\"}\n",
  ];

  // scrub every common structured credential form
  for (const secretOutput of values) {
    const request = await checkedRequest();
    const { adapters } = await injectedAdapters({ secretOutput });
    const evidence = await collectBackendHostEvidence(request, adapters);
    assert.equal(evidence.complete, false);
    assert.equal(
      canonicalHostJson(evidence).includes(Buffer.from("do-not-retain").toString("base64")),
      false,
    );
    const fixture = blueberrySourceFixture();
    const result = buildBackendHostObservation({
      evidence,
      inventory: fixture.inventory,
      phase: "baseline",
      runtimeMap: fixture.runtimeMap,
    });
    assertDiagnostic(result, "host.secret");
  }
});

// reject credentials inside retained systemd fragment bytes before encoding
test("systemd fragment secrets are scrubbed before base64 retention", () => {
  const fixture = blueberryEvidenceFixture();
  const filesProbe = fixture.evidence.probes.find(
    // select the retained unit-file record
    (candidate) => candidate.id === "systemd.weather-compose.service.files",
  );
  const secretBytes = Buffer.from("[Service]\nEnvironment=API_TOKEN=do-not-retain\n");
  const files = JSON.parse(Buffer.from(filesProbe.raw.stdout.dataBase64, "base64"));
  files[0].dataBase64 = secretBytes.toString("base64");
  files[0].sha256 = sha256Bytes(secretBytes);
  filesProbe.raw.stdout = rawBytes(canonicalHostJson(files));
  const result = buildBackendHostObservation({ ...fixture, phase: "baseline" });
  assertDiagnostic(result, "host.secret");
  assert.equal(
    canonicalHostJson(result).includes(secretBytes.toString("base64")),
    false,
  );
});

// enforce the serialized evidence bound before base64 expansion is retained
test("collector bounds canonical serialized output rather than decoded bytes only", async () => {
  const request = await checkedRequest();
  const source = blueberrySourceFixture();
  request.inventory.json = `${canonicalHostJson(source.inventory)}\n`;
  request.inventory.sha256 = sha256Bytes(request.inventory.json);
  request.runtimeMap.json = `${canonicalHostJson(source.runtimeMap)}\n`;
  request.runtimeMap.sha256 = sha256Bytes(request.runtimeMap.json);
  request.limits.maxProbeBytes = 65536;
  request.limits.maxTotalBytes = 65536;
  const { adapters } = await injectedAdapters();
  const baseOpen = adapters.filesystem.open;
  adapters.filesystem.open = async (path, flags) => {
    const name = basename(String(path));
    const selected = name === "backend-runtime-inventory.json"
      ? Buffer.from(request.inventory.json)
      : name === "backend-runtime-live-map.json"
        ? Buffer.from(request.runtimeMap.json)
        : null;

    // preserve every non-source fixture path
    if (selected === null) {
      return await baseOpen(path, flags);
    }
    return {
      // close one small checked-source descriptor
      async close() {},
      // read one bounded checked-source range
      async read(buffer, offset, length, position) {
        const count = Math.max(0, Math.min(length, selected.length - position));
        selected.copy(buffer, offset, position, position + count);
        return { buffer, bytesRead: count };
      },
      // report checked-source size before reading
      async stat() {
        return { size: selected.length };
      },
    };
  };
  let emitted = false;
  adapters.executor = async () => {
    const stdout = emitted ? Buffer.alloc(0) : Buffer.alloc(50000, 65);
    emitted = true;
    return { exitCode: 0, stderr: Buffer.alloc(0), stdout };
  };
  const evidence = await collectBackendHostEvidence(request, adapters);
  assert.ok(Buffer.byteLength(canonicalHostJson(evidence)) <= request.limits.maxTotalBytes);
  assert.equal(evidence.complete, false);
});

// derive typed Blueberry facts from retained raw probes only
test("Blueberry observation separates containers init processes units and bindings", () => {
  const fixture = blueberryEvidenceFixture();
  const result = buildBackendHostObservation({ ...fixture, phase: "baseline" });
  assert.equal(result.ok, true, JSON.stringify(result.diagnostics));
  assert.equal(result.observation.contractVersion, HOST_OBSERVATION_VERSION);
  assert.equal(result.observation.authority.liveEligible, false);
  assert.equal(result.observation.docker.containers.length, 5);
  const api = result.observation.docker.containers.find(
    // select the API container
    (container) => container.service === "api",
  );
  assert.ok(api);
  assert.notEqual(api.initPid, api.mainPid);
  assert.deepEqual(api.configuredArgv, ["node", "api/main.js"]);
  const serverImage = result.observation.docker.images.find(
    // select the server image
    (image) => image.role === "server",
  );
  assert.deepEqual(serverImage.declaredEntrypoint, ["docker-entrypoint.sh"]);
  assert.deepEqual(serverImage.declaredCmd, ["node", "apps/api/dist/main.js"]);
  const composeUnit = result.observation.systemd.find(
    // select the supervising Compose unit
    (unit) => unit.unit === "weather-compose.service",
  );
  assert.equal(composeUnit.activeState, "failed");
  assert.equal(composeUnit.result, "exit-code");
  assert.equal(composeUnit.execMainStatus, 1);
  assert.ok(result.observation.docker.containers.every(
    // retain independently healthy running application containers
    (container) => container.running && container.health === "healthy",
  ));
  assert.equal(result.observation.artifacts.length, 6);
  assert.ok(result.observation.artifacts.every(
    // require exact binding backlinks
    (artifact) => artifact.bindingIds.length === 1 && artifact.probeIds.length === 1,
  ));
});

// reject ambiguous raw encoding failed outcomes and provenance substitutions
test("semantic probes require canonical raw bytes and exact successful provenance", () => {
  const cases = [
    {
      code: "host.binding",
      // substitute the bootstrap path without immutable ENOENT proof
      mutate(evidence) {
        const selected = evidence.probes.find(
          // select the immutable installation receipt
          (candidate) => candidate.id === "operator.installation-receipt",
        );
        const envelope = JSON.parse(
          Buffer.from(selected.raw.stdout.dataBase64, "base64").toString("utf8"),
        );
        envelope.file.path = envelope.selection.bootstrapPath;
        envelope.selection.selectedPath = envelope.selection.bootstrapPath;
        selected.raw.stdout = rawBytes(canonicalHostJson(envelope));
      },
    },
    {
      code: "host.digest",
      // replace valid base64 with a lenient-decoder alias
      mutate(evidence) {
        const selected = evidence.probes.find(
          // select one required ABI fact
          (probeValue) => probeValue.id === "host.uname-machine",
        );
        selected.raw.stdout = {
          byteLength: 0,
          dataBase64: "!!!!",
          encoding: "base64",
          sha256: sha256Bytes(Buffer.alloc(0)),
          truncated: false,
        };
      },
    },
    {
      code: "host.bounds",
      // forbid semantic consumption of a truncated field
      mutate(evidence) {
        const selected = evidence.probes.find(
          // select one required ABI fact
          (probeValue) => probeValue.id === "host.uname-machine",
        );
        selected.raw.stdout.truncated = true;
        selected.raw.stdout.byteLength = 0;
        selected.raw.stdout.sha256 = HASH_A;
      },
    },
    {
      code: "host.probe",
      // retain plausible bytes under a failed outcome
      mutate(evidence) {
        const selected = evidence.probes.find(
          // select one required ABI fact
          (probeValue) => probeValue.id === "host.uname-machine",
        );
        selected.outcome = { exitCode: 1, status: "error" };
      },
    },
    {
      code: "host.probe",
      // retain a nonzero exit under a false success label
      mutate(evidence) {
        const selected = evidence.probes.find(
          // select one required ABI fact
          (probeValue) => probeValue.id === "host.uname-machine",
        );
        selected.outcome = { exitCode: 9, status: "ok" };
      },
    },
    {
      code: "host.probe",
      // substitute file provenance for the fixed uname command
      mutate(evidence) {
        const selected = evidence.probes.find(
          // select one required ABI fact
          (probeValue) => probeValue.id === "host.uname-machine",
        );
        selected.kind = "file";
        selected.operation = { path: "/tmp/forged-machine" };
      },
    },
    {
      code: "host.probe",
      // substitute one command argument
      mutate(evidence) {
        const selected = evidence.probes.find(
          // select one required ABI fact
          (probeValue) => probeValue.id === "host.uname-machine",
        );
        selected.operation.args = ["-r"];
      },
    },
    {
      code: "host.probe",
      // cross-link one host fact to an unrelated service scope
      mutate(evidence) {
        const selected = evidence.probes.find(
          // select one required ABI fact
          (probeValue) => probeValue.id === "host.uname-machine",
        );
        selected.sourceScope = "worker";
      },
    },
    {
      code: "host.probe",
      // inject invalid UTF-8 into a semantic text field
      mutate(evidence) {
        const selected = evidence.probes.find(
          // select one required ABI fact
          (probeValue) => probeValue.id === "host.uname-machine",
        );
        selected.raw.stdout = rawBytes(Buffer.from([0xff]));
      },
    },
    {
      code: "host.probe",
      // inject a duplicate key whose last value masks the first
      mutate(evidence) {
        const selected = evidence.probes.find(
          // select one cgroup semantic record
          (probeValue) => probeValue.id === "cgroup.blueberry:compose:api",
        );
        const current = Buffer.from(selected.raw.stdout.dataBase64, "base64").toString("utf8");
        selected.raw.stdout = rawBytes(`{"scopeId":"forged",${current.slice(1)}`);
      },
    },
  ];

  // isolate every raw evidence ambiguity
  for (const fixtureCase of cases) {
    const fixture = blueberryEvidenceFixture();
    fixtureCase.mutate(fixture.evidence);
    assertDiagnostic(buildBackendHostObservation({
      evidence: fixture.evidence,
      inventory: fixture.inventory,
      phase: "baseline",
      runtimeMap: fixture.runtimeMap,
    }), fixtureCase.code);
  }
});

// reject typed Docker-top shortcuts and incomplete proc source records
test("container processes require native top and retained proc raw bytes", () => {
  const typedTop = blueberryEvidenceFixture();
  const topProbe = typedTop.evidence.probes.find(
    // select the native API top probe
    (candidate) => candidate.id === "docker.container.api.top",
  );
  topProbe.raw.stdout = rawBytes(canonicalHostJson(serviceProcess("api", 2000, 2001)));
  assertDiagnostic(buildBackendHostObservation({ ...typedTop, phase: "baseline" }), "host.probe");

  const missingExeLink = blueberryEvidenceFixture();
  const processProbe = missingExeLink.evidence.probes.find(
    // select the proc-derived API process probe
    (candidate) => candidate.id === "docker.container.api.processes",
  );
  const value = JSON.parse(
    Buffer.from(processProbe.raw.stdout.dataBase64, "base64").toString("utf8"),
  );
  delete value.processes[0].raw.exeLink;
  processProbe.raw.stdout = rawBytes(canonicalHostJson(value));
  assertAnyDiagnostic(
    buildBackendHostObservation({ ...missingExeLink, phase: "baseline" }),
    ["host.probe", "host.schema"],
  );

  const reusedPid = blueberryEvidenceFixture();
  const reusedProbe = reusedPid.evidence.probes.find(
    // select the proc-derived API process probe
    (candidate) => candidate.id === "docker.container.api.processes",
  );
  const reusedValue = JSON.parse(
    Buffer.from(reusedProbe.raw.stdout.dataBase64, "base64").toString("utf8"),
  );
  reusedValue.processes[0].startTimeTicks += 1;
  reusedProbe.raw.stdout = rawBytes(canonicalHostJson(reusedValue));
  assertAnyDiagnostic(
    buildBackendHostObservation({ ...reusedPid, phase: "baseline" }),
    ["host.binding", "host.identity", "host.probe"],
  );
});

// bind every container ABI fact to the retained secure proc-root proof
test("container processes reject substituted secure-root proof identities", () => {
  const cases = [
    {
      code: "host.digest",
      // substitute one retained ABI content digest
      mutate(process) {
        process.secureRootProof.subjects[0].contentSha256 = HASH_A;
      },
    },
    {
      code: "host.binding",
      // substitute the maps-derived libc subject path
      mutate(process) {
        process.secureRootProof.subjects[1].relativeComponents = [
          "usr",
          "lib",
          "aarch64-linux-gnu",
          "libm.so.6",
        ];
      },
    },
    {
      code: "host.identity",
      // substitute the exact proc-root process identity
      mutate(process) {
        process.secureRootProof.subjects[0].procBinding.pid += 1;
      },
    },
    {
      code: "host.probe",
      // substitute the reviewed fixed-helper identity
      mutate(process) {
        process.secureRootProof.helperSha256 = HASH_A;
      },
    },
  ];

  // isolate each retained secure-root proof substitution
  for (const fixtureCase of cases) {
    const fixture = blueberryEvidenceFixture();
    const processProbe = fixture.evidence.probes.find(
      // select the proc-derived API process probe
      (candidate) => candidate.id === "docker.container.api.processes",
    );
    const value = JSON.parse(
      Buffer.from(processProbe.raw.stdout.dataBase64, "base64").toString("utf8"),
    );
    fixtureCase.mutate(value.processes[1]);
    processProbe.raw.stdout = rawBytes(canonicalHostJson(value));
    assertDiagnostic(
      buildBackendHostObservation({ ...fixture, phase: "baseline" }),
      fixtureCase.code,
    );
  }
});

// require every profile fact to match its source-owned probe descriptor
test("Blueberry evidence rejects probe-family provenance substitutions", () => {
  const cases = [
    {
      id: "docker.containers.list",
      label: "Docker list arguments",
      // remove the fixed untruncated Docker identity projection
      mutate(selected) {
        selected.operation.args = selected.operation.args.filter(
          // remove only the required Docker option
          (argument) => argument !== "--no-trunc",
        );
      },
    },
    {
      id: "docker.container.api.inspect",
      label: "container inspect kind",
      // substitute one probe operation kind
      mutate(selected) {
        selected.kind = "file";
        selected.operation = { path: "/tmp/api-inspect" };
      },
    },
    {
      id: "docker.container.api.inspect",
      label: "container inspect scope",
      // swap one service-scoped fact into another service
      mutate(selected) {
        selected.sourceScope = "worker";
      },
    },
    {
      id: "docker.image.server.inspect",
      label: "image inspect outcome",
      // retain bytes under a disallowed command outcome
      mutate(selected) {
        selected.outcome = { exitCode: 1, status: "ok" };
      },
    },
    {
      id: "cgroup.blueberry:compose:api",
      label: "cgroup prerequisites",
      // remove one physical process prerequisite
      mutate(selected) {
        selected.operation.inputProbeIds = ["docker.container.api.inspect"];
      },
    },
    {
      id: "systemd.weather-compose.service.files",
      label: "unit file prerequisites",
      // link secure unit bytes to an obsolete cat probe
      mutate(selected) {
        selected.operation.inputProbeIds = [
          "systemd.weather-compose.service.show",
          "systemd.weather-compose.service.cat",
        ];
      },
    },
    {
      label: "artifact prerequisites",
      match(candidate) {
        return candidate.id.startsWith("artifact.") &&
          candidate.sourceScope.includes("#service=api");
      },
      // swap one API artifact onto the worker execution context
      mutate(selected) {
        selected.operation.inputProbeIds = ["docker.container.worker.inspect"];
      },
    },
    {
      id: "operator.control-plane.files",
      label: "control-plane method",
      // substitute the secure tree collection method
      mutate(selected) {
        selected.operation.method = "derived-canonical-json-v1";
      },
    },
  ];

  // isolate each exact descriptor substitution
  for (const fixtureCase of cases) {
    const fixture = blueberryEvidenceFixture();
    const selected = fixture.evidence.probes.find(
      // select the exact descriptor under test
      (candidate) => fixtureCase.id === undefined
        ? fixtureCase.match(candidate)
        : candidate.id === fixtureCase.id,
    );
    assert.ok(selected, fixtureCase.id ?? "artifact probe");
    fixtureCase.mutate(selected);
    const result = buildBackendHostObservation({ ...fixture, phase: "baseline" });
    assert.equal(result.ok, false, fixtureCase.label);
    assertDiagnostic(result, "host.probe");
  }
});

// reject obsolete systemctl-cat evidence outside the fixed catalog
test("Blueberry evidence rejects obsolete systemd cat probes", () => {
  const fixture = blueberryEvidenceFixture();
  fixture.evidence.probes.push(probe(
    "systemd.weather-compose.service.cat",
    "[Service]\n",
    {
      args: ["cat", "weather-compose.service", "--no-pager"],
      program: "/usr/bin/systemctl",
    },
  ));
  fixture.evidence.probes.sort(
    // retain deterministic probe ordering
    (left, right) => left.id.localeCompare(right.id),
  );
  assertDiagnostic(
    buildBackendHostObservation({ ...fixture, phase: "baseline" }),
    "host.probe",
  );
});

// distinguish absent completed and invalid migration one-shot evidence
test("Blueberry migration cardinality permits absence without inventing exit history", () => {
  const fixture = blueberryEvidenceFixture();
  const absent = buildBackendHostObservation({ ...fixture, phase: "baseline" });
  assert.equal(absent.ok, true, JSON.stringify(absent.diagnostics));
  assert.equal(absent.observation.docker.containers.some(
    // reject fabricated migration history when the one-shot is absent
    (container) => container.service === "migration",
  ), false);

  // add one selected migration container observation
  function addMigration(evidence, { exitCode, listed, running, status }) {
    const listProbe = evidence.probes.find(
      // select the complete container inventory
      (candidate) => candidate.id === "docker.containers.list",
    );

    // retain the migration ID only when currently enumerated
    if (listed) {
      const current = Buffer.from(listProbe.raw.stdout.dataBase64, "base64").toString("utf8");
      listProbe.raw.stdout = rawBytes(`${current}${dockerContainerListRow("migration")}\n`);
    }
    evidence.probes.push(probe(
      "docker.container.migration.inspect",
      canonicalHostJson([{
        args: ["apps/worker/dist/migrate.js"],
        cgroupParent: "/weather",
        containerNumber: 1,
        exitCode,
        health: "none",
        id: containerIdForService("migration"),
        imageId: "sha256:image-server",
        imageReference: "weather-server:fixture",
        memory: 536870912,
        name: "weather-migration-run-1",
        nanoCpus: 1000000000,
        oneOff: true,
        path: "node",
        pid: running ? 2900 : 0,
        pidsLimit: 256,
        project: "weather",
        readOnlyRootfs: true,
        running,
        service: "migration",
        startedAt: "2026-11-13T07:30:00.000Z",
        status,
      }]),
      {
        args: [
          "inspect",
          "--format",
          DOCKER_CONTAINER_FORMAT,
          containerIdForService("migration"),
        ],
        program: "/usr/bin/docker",
        sourceScope: "migration",
      },
    ));

    // retain process authority only for a currently running one-shot
    if (running) {
      evidence.probes.push(probe(
        "docker.container.migration.top",
        nativeDockerTop("migration", 2900, 2901),
        {
          args: [
            "top",
            containerIdForService("migration"),
            "-eo",
            "pid,ppid,args",
          ],
          program: "/usr/bin/docker",
          sourceScope: "migration",
        },
      ));
      evidence.probes.push(probe(
        "docker.container.migration.processes",
        canonicalHostJson({
          containerId: containerIdForService("migration"),
          dockerInitPid: 2900,
          processes: serviceProcess("migration", 2900, 2901),
        }),
        {
          kind: "derived",
          operation: {
            inputProbeIds: [
              "docker.container.migration.inspect",
              "docker.container.migration.top",
            ],
            method: "docker-top-procfs-v2",
          },
          sourceScope: "migration",
        },
      ));
    }
    evidence.probes.sort(
      // restore deterministic probe order
      (left, right) => left.id.localeCompare(right.id),
    );
  }

  const forgedExit = structuredClone(fixture.evidence);
  addMigration(forgedExit, {
    exitCode: 0,
    listed: false,
    running: false,
    status: "exited",
  });
  assertAnyDiagnostic(buildBackendHostObservation({
    evidence: forgedExit,
    inventory: fixture.inventory,
    phase: "baseline",
    runtimeMap: fixture.runtimeMap,
  }), ["host.cardinality", "host.identity"]);

  const completed = structuredClone(fixture.evidence);
  addMigration(completed, {
    exitCode: 0,
    listed: true,
    running: false,
    status: "exited",
  });
  const completedResult = buildBackendHostObservation({
    evidence: completed,
    inventory: fixture.inventory,
    phase: "baseline",
    runtimeMap: fixture.runtimeMap,
  });
  assert.equal(completedResult.ok, true, JSON.stringify(completedResult.diagnostics));

  // reject failed and still-running one-shot states
  for (const state of [
    { exitCode: 1, listed: true, running: false, status: "exited" },
    { exitCode: 0, listed: true, running: true, status: "running" },
  ]) {
    const invalid = structuredClone(fixture.evidence);
    addMigration(invalid, state);
    assertAnyDiagnostic(buildBackendHostObservation({
      evidence: invalid,
      inventory: fixture.inventory,
      phase: "baseline",
      runtimeMap: fixture.runtimeMap,
    }), ["host.cardinality", "host.identity"]);
  }
});

// reject raw digest cardinality identity and binding substitutions
test("Blueberry observation fails closed on malformed or cross-context raw evidence", () => {
  const fixture = blueberryEvidenceFixture();
  const mutations = [
    {
      code: "host.digest",
      // break one retained raw-byte digest
      mutate(evidence) {
        evidence.probes[0].raw.stdout.sha256 = HASH_A;
      },
    },
    {
      code: "host.cardinality",
      // duplicate one selected Compose container
      mutate(evidence) {
        const selected = evidence.probes.find(
          // select the complete container inventory probe
          (candidate) => candidate.id === "docker.containers.list",
        );
        const row = dockerContainerListRow("api");
        selected.raw.stdout = rawBytes(`${row}\n${row}\n`);
      },
    },
    {
      code: "host.identity",
      // substitute an API container as the worker service
      mutate(evidence) {
        const selected = evidence.probes.find(
          // select the worker inspect probe
          (candidate) => candidate.id === "docker.container.worker.inspect",
        );
        const raw = JSON.parse(Buffer.from(selected.raw.stdout.dataBase64, "base64"));
        raw[0].service = "api";
        selected.raw.stdout = rawBytes(canonicalHostJson(raw));
      },
    },
    {
      code: "host.binding",
      // substitute one artifact's container instance
      mutate(evidence) {
        const selected = evidence.probes.find(
          // select one API artifact probe
          (candidate) => candidate.sourceScope.includes("#api#service=api") &&
            candidate.id.startsWith("artifact."),
        );
        const raw = JSON.parse(Buffer.from(selected.raw.stdout.dataBase64, "base64"));
        raw.containerId = containerIdForService("worker");
        selected.raw.stdout = rawBytes(canonicalHostJson(raw));
      },
    },
    {
      code: "host.bounds",
      // move one probe outside the enclosing collection interval
      mutate(evidence) {
        evidence.probes[0].completedAt = "2026-11-13T09:00:00.000Z";
      },
    },
  ];

  // isolate every reviewed raw-evidence failure
  for (const mutation of mutations) {
    const evidence = structuredClone(fixture.evidence);
    mutation.mutate(evidence);
    assertDiagnostic(buildBackendHostObservation({
      evidence,
      inventory: fixture.inventory,
      phase: "baseline",
      runtimeMap: fixture.runtimeMap,
    }), mutation.code);
  }
});

// require active service artifacts
test("Blueberry observation rejects missing active code", () => {
  const missingCode = blueberryEvidenceFixture();
  const apiArtifact = missingCode.evidence.probes.find(
    // select one active API artifact
    (candidate) => candidate.sourceScope.includes("#api#service=api") &&
      candidate.id.startsWith("artifact."),
  );
  const missingValue = JSON.parse(
    Buffer.from(apiArtifact.raw.stdout.dataBase64, "base64").toString("utf8"),
  );
  Object.assign(missingValue, {
    gid: null,
    mode: null,
    present: false,
    sha256: null,
    type: "missing",
    uid: null,
  });
  apiArtifact.raw.stdout = rawBytes(canonicalHostJson(missingValue));
  assertDiagnostic(
    buildBackendHostObservation({ ...missingCode, phase: "baseline" }),
    "host.binding",
  );
});

// require exact cgroup scope backlinks
test("Blueberry observation rejects relabeled cgroup scope", () => {
  const relabeledCgroup = blueberryEvidenceFixture();
  const cgroupProbe = relabeledCgroup.evidence.probes.find(
    // select the API cgroup observation
    (candidate) => candidate.id === "cgroup.blueberry:compose:api",
  );
  const cgroupValue = JSON.parse(
    Buffer.from(cgroupProbe.raw.stdout.dataBase64, "base64").toString("utf8"),
  );
  cgroupValue.scopeId = "blueberry:compose:worker";
  cgroupProbe.raw.stdout = rawBytes(canonicalHostJson(cgroupValue));
  assertDiagnostic(
    buildBackendHostObservation({ ...relabeledCgroup, phase: "baseline" }),
    "host.binding",
  );
});

// require the existing immutable current-v1 operator protocol
test("Blueberry observation validates the current-v1 operator receipt closure", () => {
  const fixture = blueberryEvidenceFixture({ includeOperator: false });
  const operator = addOperatorFixture(fixture.evidence);
  const result = buildBackendHostObservation({ ...fixture, phase: "baseline" });
  assert.equal(result.ok, true, JSON.stringify(result.diagnostics));
  assert.equal(result.observation.release.operator.bundleSha256, operator.bundleSha256);
  assert.equal(
    result.observation.release.operator.receiptSha256,
    sha256Bytes(operator.receiptBytes),
  );
  assert.equal(
    result.observation.release.operator.dropInSha256,
    sha256Bytes(operator.dropInBytes),
  );
});

// require operator facts from the fixed Blueberry profile catalog
test("Blueberry observation rejects an omitted operator evidence family", () => {
  const fixture = blueberryEvidenceFixture({ includeOperator: false });
  assertDiagnostic(
    buildBackendHostObservation({ ...fixture, phase: "baseline" }),
    "host.probe",
  );
});

// bind operator receipt semantics metadata and ordered systemd sources
test("Blueberry observation rejects substituted operator receipt and unit facts", () => {
  const cases = [
    {
      code: "host.binding",
      // substitute one self-consistent receipt vector
      mutate(evidence) {
        const selected = evidence.probes.find(
          // select the immutable installation receipt
          (candidate) => candidate.id === "operator.installation-receipt",
        );
        const envelope = JSON.parse(
          Buffer.from(selected.raw.stdout.dataBase64, "base64").toString("utf8"),
        );
        const receipt = JSON.parse(
          Buffer.from(envelope.raw.bytes.dataBase64, "base64").toString("utf8"),
        );
        receipt.execStart = "/bin/false|/bin/false";
        const bytes = Buffer.from(`${canonicalHostJson(receipt)}\n`);
        envelope.file.byteLength = bytes.length;
        envelope.file.sha256 = sha256Bytes(bytes);
        envelope.raw.bytes = rawBytes(bytes);
        selected.raw.stdout = rawBytes(canonicalHostJson(envelope));
      },
    },
    {
      code: "host.binding",
      // weaken private receipt metadata
      mutate(evidence) {
        const selected = evidence.probes.find(
          // select the immutable installation receipt
          (candidate) => candidate.id === "operator.installation-receipt",
        );
        const envelope = JSON.parse(
          Buffer.from(selected.raw.stdout.dataBase64, "base64").toString("utf8"),
        );
        envelope.file.mode = "0644";
        envelope.file.nlink = 2;
        selected.raw.stdout = rawBytes(canonicalHostJson(envelope));
      },
    },
    {
      code: "host.binding",
      // reverse the selected base and drop-in order
      mutate(evidence) {
        const selected = evidence.probes.find(
          // select the exact ordered unit source list
          (candidate) => candidate.id === "systemd.weather-compose.service.files",
        );
        const files = JSON.parse(
          Buffer.from(selected.raw.stdout.dataBase64, "base64").toString("utf8"),
        );
        files.reverse();
        selected.raw.stdout = rawBytes(canonicalHostJson(files));
      },
    },
    {
      code: "host.binding",
      // substitute the effective timeout independently of receipt bytes
      mutate(evidence) {
        const selected = evidence.probes.find(
          // select the effective systemd show record
          (candidate) => candidate.id === "systemd.weather-compose.service.show",
        );
        const text = Buffer.from(selected.raw.stdout.dataBase64, "base64")
          .toString("utf8").replace("TimeoutStartUSec=10min", "TimeoutStartUSec=9min");
        selected.raw.stdout = rawBytes(text);
      },
    },
  ];

  // isolate each operator closure substitution
  for (const fixtureCase of cases) {
    const fixture = blueberryEvidenceFixture({ includeOperator: false });
    addOperatorFixture(fixture.evidence);
    fixtureCase.mutate(fixture.evidence);
    assertDiagnostic(
      buildBackendHostObservation({ ...fixture, phase: "baseline" }),
      fixtureCase.code,
    );
  }
});

// bind the current-v1 receipt to the complete installed control-plane tree
test("Blueberry observation rejects incomplete or drifted v17 control-plane trees", () => {
  assert.equal(CONTROL_PLANE_TREE_FIXTURE.files.length, 57);
  assert.equal(
    CONTROL_PLANE_TREE_FIXTURE.controlPlaneSha256,
    "b2a211e5936fe0063f552a5e0d9ccf9a89b61a308ab9accf58e95fa54325822d",
  );
  assert.equal(CONTROL_PLANE_TREE_FIXTURE.files.at(-1).path, "deploy/compose.yaml");
  assert.deepEqual(
    CONTROL_PLANE_TREE_FIXTURE.files.slice(0, -1).map(
      // retain only the four recursively sorted protected roots
      (file) => file.path,
    ),
    CONTROL_PLANE_TREE_FIXTURE.files.slice(0, -1).map(
      // compare against an independently sorted protected path list
      (file) => file.path,
    ).toSorted(),
  );
  assert.deepEqual(
    CONTROL_PLANE_TREE_FIXTURE.directories.map(
      // retain every fixed predecessor-protected root
      (directory) => directory.path,
    ),
    [
      "deploy/postgres",
      "deploy/scripts",
      "deploy/sudoers",
      "deploy/systemd",
    ],
  );
  const cases = [
    {
      // omit one directory-declared installed file
      mutate(tree) {
        tree.files = tree.files.filter(
          // retain every file except the fixed update script
          (file) => file.path !== "deploy/scripts/update.sh",
        );
        tree.controlPlaneSha256 = controlPlaneDigest(tree.files);
      },
    },
    {
      // add one complete but receipt-unauthorized control file
      mutate(tree) {
        const directory = tree.directories.find(
          // select the exact scripts directory proof
          (candidate) => candidate.path === "deploy/scripts",
        );
        directory.entries.push({ name: "unexpected.sh", type: "regular-file" });
        directory.entries.sort(
          // retain canonical entry order after the adversarial addition
          (left, right) => left.name.localeCompare(right.name),
        );
        const identity = secureFileIdentity({
          expectedGid: 0,
          expectedMode: "0644",
          expectedUid: 0,
        });
        identity.ino = "9999";
        tree.files.push({
          identity,
          path: "deploy/scripts/unexpected.sh",
          sha256: HASH_A,
        });
        const compose = tree.files.find(
          // retain the separately appended Compose file
          (file) => file.path === "deploy/compose.yaml",
        );
        tree.files = tree.files.filter(
          // sort only the four recursively protected roots
          (file) => file.path !== "deploy/compose.yaml",
        ).sort(
          // retain canonical protected-root order after the addition
          (left, right) => left.path.localeCompare(right.path),
        );
        tree.files.push(compose);
        tree.controlPlaneSha256 = controlPlaneDigest(tree.files);
      },
    },
    {
      // replace one installed byte digest self-consistently inside the tree
      mutate(tree) {
        const file = tree.files.find(
          // select the fixed update script identity
          (candidate) => candidate.path === "deploy/scripts/update.sh",
        );
        file.sha256 = HASH_A;
        tree.controlPlaneSha256 = controlPlaneDigest(tree.files);
      },
    },
    {
      // reverse a canonically sorted installed file set
      mutate(tree) {
        tree.files.reverse();
      },
    },
  ];

  // isolate every complete-tree integrity failure
  for (const fixtureCase of cases) {
    const fixture = blueberryEvidenceFixture({ includeOperator: false });
    addOperatorFixture(fixture.evidence);
    const positive = buildBackendHostObservation({ ...fixture, phase: "baseline" });
    assert.equal(positive.ok, true, JSON.stringify(positive.diagnostics));
    const selected = fixture.evidence.probes.find(
      // select the fixed control-plane tree probe
      (candidate) => candidate.id === "operator.control-plane.files",
    );
    const tree = JSON.parse(
      Buffer.from(selected.raw.stdout.dataBase64, "base64").toString("utf8"),
    );
    fixtureCase.mutate(tree);
    selected.raw.stdout = rawBytes(canonicalHostJson(tree));
    assertAnyDiagnostic(
      buildBackendHostObservation({ ...fixture, phase: "baseline" }),
      ["host.binding", "host.digest", "host.probe", "host.schema"],
    );
  }
});

// rederive historical observations from full retained-v1 raw inputs
test("retained-v1 pure builder rederives both physical host profiles", async (testContext) => {
  assert.equal(typeof hostObserver.buildRetainedBackendHostObservationV1, "function");
  const observerSourceBytes = await readFile(PRODUCER_PATH);
  const cases = [
    ["blueberry-compose", "baseline", blueberryEvidenceFixture()],
    ["workstation-runner", "pre-cutover", runnerEvidenceFixture()],
  ];

  // validate both source-owned raw profile dialects
  for (const [profile, phase, source] of cases) {
    await testContext.test(profile, () => {
      const fixture = retainedV1Fixture(source, profile, phase, observerSourceBytes);
      const expected = buildBackendHostObservation({
        evidence: fixture.evidence,
        inventory: source.inventory,
        phase,
        runtimeMap: source.runtimeMap,
      });
      assert.equal(expected.ok, true, JSON.stringify(expected.diagnostics));
      const result = hostObserver.buildRetainedBackendHostObservationV1({
        evidenceBytes: fixture.evidenceBytes,
        inventoryBytes: fixture.inventoryBytes,
        observerSourceBytes: fixture.observerSourceBytes,
        phase: fixture.phase,
        requestBytes: fixture.requestBytes,
        runtimeMapBytes: fixture.runtimeMapBytes,
      });
      assert.equal(result.ok, true, JSON.stringify(result.diagnostics));
      assert.deepEqual(result.observation, expected.observation);
    });
  }
});

// preserve retained-v1 semantics beside later active host contracts
test("retained-v1 pure builder ignores later active protocol and Docker revisions", async () => {
  const observerSourceBytes = await readFile(PRODUCER_PATH);
  const source = blueberryEvidenceFixture();
  const fixture = retainedV1Fixture(
    source,
    "blueberry-compose",
    "baseline",
    observerSourceBytes,
  );
  const expected = hostObserver.buildRetainedBackendHostObservationV1({
    evidenceBytes: fixture.evidenceBytes,
    inventoryBytes: fixture.inventoryBytes,
    observerSourceBytes,
    phase: fixture.phase,
    requestBytes: fixture.requestBytes,
    runtimeMapBytes: fixture.runtimeMapBytes,
  });
  assert.equal(expected.ok, true, JSON.stringify(expected.diagnostics));
  const activeModulePath = join(
    new URL(".", PRODUCER_PATH).pathname,
    `.backend-host-observation-active-${String(process.pid)}-${String(Date.now())}.mjs`,
  );
  const originalSource = observerSourceBytes.toString("utf8");
  const retainedBoundary = originalSource.indexOf(
    "// retain the complete immutable v1 interpretation independently of later active protocol changes",
  );
  assert.ok(retainedBoundary > 0);
  let activePrefix = originalSource.slice(0, retainedBoundary);
  const retainedSuffix = originalSource.slice(retainedBoundary);
  const replacements = [
    [
      'export const HOST_EVIDENCE_REQUEST_VERSION = "weather-backend-host-evidence-request/v1";',
      'export const HOST_EVIDENCE_REQUEST_VERSION = "weather-backend-host-evidence-request/v2";',
    ],
    [
      'export const HOST_EVIDENCE_CONTRACT_VERSION = "weather-backend-host-evidence/v1";',
      'export const HOST_EVIDENCE_CONTRACT_VERSION = "weather-backend-host-evidence/v2";',
    ],
    [
      'export const HOST_PROBE_VERSION = "weather-backend-host-probe/v1";',
      'export const HOST_PROBE_VERSION = "weather-backend-host-probe/v2";',
    ],
    [
      'export const HOST_OBSERVATION_VERSION = "weather-backend-host-observation/v2";',
      'export const HOST_OBSERVATION_VERSION = "weather-backend-host-observation/v3";',
    ],
    [
      'export const HOST_PRODUCER = "weather-backend-read-only-host-observer/v2";',
      'export const HOST_PRODUCER = "weather-backend-read-only-host-observer/v3";',
    ],
    [
      '  "{\\"args\\":{{json .Args}}",',
      '  "{\\"activeArgs\\":{{json .Args}}",',
    ],
    [
      '  "{\\"architecture\\":{{json .Architecture}}",',
      '  "{\\"activeArchitecture\\":{{json .Architecture}}",',
    ],
    [
      "const DOCKER_CONTAINER_LIST_FORMAT = '{{.Label \"com.docker.compose.service\"}}\\t{{.ID}}';",
      "const DOCKER_CONTAINER_LIST_FORMAT = '{{.Label \"com.docker.compose.service\"}}\\t{{.ID}}\\tactive-v3';",
    ],
  ];

  // revise only active declarations before the immutable v1 boundary
  for (const [from, to] of replacements) {
    assert.equal(activePrefix.split(from).length, 2, from);
    activePrefix = activePrefix.replace(from, to);
  }
  const activeSource = `${activePrefix}${retainedSuffix}`;

  try {
    await writeFile(activeModulePath, activeSource, { flag: "wx", mode: 0o600 });
    const active = await import(`${pathToFileURL(activeModulePath).href}?active-v3`);
    assert.equal(active.HOST_PRODUCER, "weather-backend-read-only-host-observer/v3");
    assert.equal(active.HOST_OBSERVATION_VERSION, "weather-backend-host-observation/v3");
    const result = active.buildRetainedBackendHostObservationV1({
      evidenceBytes: fixture.evidenceBytes,
      inventoryBytes: fixture.inventoryBytes,
      observerSourceBytes,
      phase: fixture.phase,
      requestBytes: fixture.requestBytes,
      runtimeMapBytes: fixture.runtimeMapBytes,
    });
    assert.equal(result.ok, true, JSON.stringify(result.diagnostics));
    assert.deepEqual(result.observation, expected.observation);
  } finally {
    await rm(activeModulePath, { force: true });
  }
});

// reject later source and raw dialects at the retained-v1 boundary
test("retained-v1 pure builder rejects coherent later protocol source and raw inputs", async () => {
  const observerSourceBytes = await readFile(PRODUCER_PATH);
  let laterSource = observerSourceBytes.toString("utf8");
  const sourceReplacements = [
    [
      'export const HOST_EVIDENCE_REQUEST_VERSION = "weather-backend-host-evidence-request/v1";',
      'export const HOST_EVIDENCE_REQUEST_VERSION = "weather-backend-host-evidence-request/v2";',
    ],
    [
      'export const HOST_EVIDENCE_CONTRACT_VERSION = "weather-backend-host-evidence/v1";',
      'export const HOST_EVIDENCE_CONTRACT_VERSION = "weather-backend-host-evidence/v2";',
    ],
    [
      'export const HOST_PROBE_VERSION = "weather-backend-host-probe/v1";',
      'export const HOST_PROBE_VERSION = "weather-backend-host-probe/v2";',
    ],
    [
      'export const HOST_OBSERVATION_VERSION = "weather-backend-host-observation/v2";',
      'export const HOST_OBSERVATION_VERSION = "weather-backend-host-observation/v3";',
    ],
    [
      'export const HOST_PRODUCER = "weather-backend-read-only-host-observer/v2";',
      'export const HOST_PRODUCER = "weather-backend-read-only-host-observer/v3";',
    ],
  ];

  // revise every retained source export coherently
  for (const [from, to] of sourceReplacements) {
    assert.equal(laterSource.split(from).length, 2, from);
    laterSource = laterSource.replace(from, to);
  }
  const laterSourceBytes = Buffer.from(laterSource);
  const fixture = retainedV1Fixture(
    blueberryEvidenceFixture(),
    "blueberry-compose",
    "baseline",
    laterSourceBytes,
  );
  fixture.request.contractVersion = "weather-backend-host-evidence-request/v2";
  fixture.evidence.contractVersion = "weather-backend-host-evidence/v2";
  fixture.evidence.producer = "weather-backend-read-only-host-observer/v3";

  // revise every retained probe coherently
  for (const retainedProbe of fixture.evidence.probes) {
    retainedProbe.contractVersion = "weather-backend-host-probe/v2";
  }
  const result = hostObserver.buildRetainedBackendHostObservationV1({
    evidenceBytes: Buffer.from(canonicalHostJson(fixture.evidence)),
    inventoryBytes: fixture.inventoryBytes,
    observerSourceBytes: laterSourceBytes,
    phase: fixture.phase,
    requestBytes: Buffer.from(canonicalHostJson(fixture.request)),
    runtimeMapBytes: fixture.runtimeMapBytes,
  });
  assert.equal(result.ok, false);
  assert.equal(result.observation, null);
  assertAnyDiagnostic(result, ["host.digest", "host.schema"]);
});

// reject incomplete raw dialects semantic tampering and retained helper drift
test("retained-v1 pure builder rejects impossible compact and cross-proof inputs", async (testContext) => {
  const observerSourceBytes = await readFile(PRODUCER_PATH);
  const source = blueberryEvidenceFixture();

  await testContext.test("compact request and evidence", () => {
    const result = hostObserver.buildRetainedBackendHostObservationV1({
      evidenceBytes: Buffer.from(canonicalHostJson({ profile: "blueberry-compose" })),
      inventoryBytes: Buffer.from(canonicalHostJson(source.inventory)),
      observerSourceBytes,
      phase: "baseline",
      requestBytes: Buffer.from(canonicalHostJson({ producerSha256: sha256Bytes(observerSourceBytes) })),
      runtimeMapBytes: Buffer.from(canonicalHostJson(source.runtimeMap)),
    });
    assert.equal(result.ok, false);
    assertAnyDiagnostic(result, ["host.schema", "host.probe"]);
  });

  await testContext.test("semantic probe failure", () => {
    const fixture = retainedV1Fixture(source, "blueberry-compose", "baseline", observerSourceBytes);
    const evidence = structuredClone(fixture.evidence);
    const selected = evidence.probes.find(
      // select one mandatory semantic identity probe
      (probeValue) => probeValue.id === "host.uname-machine",
    );
    selected.outcome = { exitCode: 1, status: "error" };
    const result = hostObserver.buildRetainedBackendHostObservationV1({
      evidenceBytes: Buffer.from(canonicalHostJson(evidence)),
      inventoryBytes: fixture.inventoryBytes,
      observerSourceBytes: fixture.observerSourceBytes,
      phase: fixture.phase,
      requestBytes: fixture.requestBytes,
      runtimeMapBytes: fixture.runtimeMapBytes,
    });
    assert.equal(result.ok, false);
    assertAnyDiagnostic(result, ["host.identity", "host.probe", "host.schema"]);
  });

  await testContext.test("retained secure-reader helper drift", () => {
    const changedSource = Buffer.from(
      observerSourceBytes.toString("utf8").replace("import base64\n", "import base64\n# retained mutation\n"),
    );
    const fixture = retainedV1Fixture(source, "blueberry-compose", "baseline", changedSource);
    const result = hostObserver.buildRetainedBackendHostObservationV1({
      evidenceBytes: fixture.evidenceBytes,
      inventoryBytes: fixture.inventoryBytes,
      observerSourceBytes: changedSource,
      phase: fixture.phase,
      requestBytes: fixture.requestBytes,
      runtimeMapBytes: fixture.runtimeMapBytes,
    });
    assert.equal(result.ok, false);
    assertAnyDiagnostic(result, ["host.digest", "host.identity", "host.probe"]);
  });
});

// derive runner account home and unit state from retained NSS evidence
test("workstation observation derives NSS home and five runner unit states", () => {
  const fixture = runnerEvidenceFixture();
  const result = buildBackendHostObservation({ ...fixture, phase: "pre-cutover" });
  assert.equal(result.ok, true, JSON.stringify(result.diagnostics));
  assert.deepEqual(result.observation.runner.account, {
    gid: 1000,
    home: "/home/ubuntu",
    name: "ubuntu",
    probeIds: ["nss.ubuntu.passwd"],
    uid: 1000,
  });
  assert.equal(result.observation.runner.units.length, 5);
  assert.ok(result.observation.runner.units.every(
    // retain exact not-found inactive states separately
    (unit) => unit.loadState === "not-found" &&
      unit.activeState === "inactive" &&
      unit.enabledState === "not-found",
  ));
  assert.deepEqual(result.observation.runner.roots, [
    "/home/ubuntu/.weather/adjustment-maintenance/runner",
    "/home/ubuntu/.local/lib/weather-adjustment-maintenance",
  ]);
  assert.equal(result.observation.authority.liveEligible, false);
});

// require both source-owned runner roots before deriving installation state
test("workstation observation rejects missing or contradictory runner root facts", () => {
  const missing = runnerEvidenceFixture();
  missing.evidence.probes = missing.evidence.probes.filter(
    // remove one required root identity
    (candidate) => candidate.id !== "runner.root.library",
  );
  assertDiagnostic(
    buildBackendHostObservation({ ...missing, phase: "pre-cutover" }),
    "host.probe",
  );

  const contradictory = runnerEvidenceFixture();
  const selected = contradictory.evidence.probes.find(
    // select one exact runner root state
    (candidate) => candidate.id === "runner.root.private",
  );
  selected.raw.stdout = rawBytes(canonicalHostJson({
    identity: secureDirectoryIdentity("7001"),
    path: "/home/ubuntu/.weather/adjustment-maintenance/runner",
    present: true,
  }));
  assertDiagnostic(
    buildBackendHostObservation({ ...contradictory, phase: "pre-cutover" }),
    "host.binding",
  );

  const relabeled = runnerEvidenceFixture();
  const relabeledProbe = relabeled.evidence.probes.find(
    // select one exact runner root provenance record
    (candidate) => candidate.id === "runner.root.library",
  );
  relabeledProbe.operation.inputProbeIds = ["runner.processes"];
  assertDiagnostic(
    buildBackendHostObservation({ ...relabeled, phase: "pre-cutover" }),
    "host.probe",
  );
});

// distinguish exact missing runner roots from every other secure-read failure
test("collector derives absent runner roots only from exact ENOENT proof", async (testContext) => {
  const cases = [
    { errorCode: "ENOENT", label: "absent", status: "absent" },
    { errorCode: "host.binding", label: "permission-denied", status: "error" },
    { errorCode: "host.binding", label: "symlink", status: "error" },
    { errorCode: "host.binding", label: "wrong-type", status: "error" },
  ];

  // isolate every runner-root secure result class
  for (const fixtureCase of cases) {
    await testContext.test(fixtureCase.label, async () => {
      const request = await checkedRequest("workstation-runner");
      const fixture = await scriptedProfileAdapters("workstation-runner");
      fixture.adapters.secureExecutor = async (program, args, options) => {
        const secureRequest = JSON.parse(options.stdin.toString("utf8"));
        const execution = secureExecutionForRequest(secureRequest);
        const result = JSON.parse(execution.stdout.toString("utf8"));

        // replace only the two fixed root-status subjects
        for (const subject of result.subjects) {
          if (!["runner.root.private", "runner.root.library"].includes(subject.id)) {
            continue;
          }
          Object.assign(subject, {
            bytes: null,
            contentSha256: null,
            errorCode: fixtureCase.errorCode,
            identity: null,
            procBinding: null,
            releaseBinding: null,
            status: fixtureCase.status,
          });
        }
        return {
          exitCode: 0,
          stderr: Buffer.alloc(0),
          stdout: Buffer.from(`${canonicalHostJson(result)}\n`),
        };
      };
      const evidence = await collectBackendHostEvidence(request, fixture.adapters);
      const rootProbes = evidence.probes.filter(
        // select only the two fixed root-status facts
        (candidate) => candidate.id.startsWith("runner.root."),
      );

      // accept only exact ENOENT as an absent root fact
      if (fixtureCase.status === "absent") {
        assert.equal(rootProbes.length, 2);
        assert.ok(rootProbes.every(
          // require an explicit absent value rather than source inference
          (candidate) => JSON.parse(
            Buffer.from(candidate.raw.stdout.dataBase64, "base64").toString("utf8"),
          ).present === false,
        ));
      } else {
        assert.equal(rootProbes.length, 0);
        assert.equal(evidence.complete, false);
        assert.ok(evidence.unresolved.some(
          // require the failed root to remain explicitly unresolved
          (candidate) => candidate.id === "runner.root.private",
        ));
      }
    });
  }
});

// require runner roots artifacts units and processes to agree on installation state
test("workstation observation rejects artifacts hidden behind an absent runner state", () => {
  const fixture = runnerEvidenceFixture();
  const artifactProbe = fixture.evidence.probes.find(
    // select the sole runner artifact observation
    (candidate) => candidate.id.startsWith("artifact."),
  );
  const value = JSON.parse(
    Buffer.from(artifactProbe.raw.stdout.dataBase64, "base64").toString("utf8"),
  );
  Object.assign(value, {
    gid: 1000,
    mode: "0644",
    present: true,
    sha256: HASH_A,
    type: "regular-file",
    uid: 1000,
  });
  artifactProbe.raw.stdout = rawBytes(canonicalHostJson(value));
  assertDiagnostic(
    buildBackendHostObservation({ ...fixture, phase: "pre-cutover" }),
    "host.binding",
  );
});

// require observed runner processes to agree with roots artifacts and units
test("workstation observation rejects a process hidden behind absent runner roots", () => {
  const fixture = runnerEvidenceFixture();
  const processProbe = fixture.evidence.probes.find(
    // select the runner process observation
    (candidate) => candidate.id === "runner.processes",
  );
  processProbe.raw.stdout = rawBytes(canonicalHostJson([
    serviceProcess("api", 3000, 3001)[1],
  ]));
  assertDiagnostic(
    buildBackendHostObservation({ ...fixture, phase: "pre-cutover" }),
    "host.binding",
  );
});

// prevent injected evidence from self-promoting into live authority
test("injected evidence cannot promote itself with labels or a fresh hash", () => {
  const fixture = blueberryEvidenceFixture();
  const promoted = structuredClone(fixture.evidence);
  promoted.authority.liveEligible = true;
  promoted.authority.canonicalEligible = true;
  promoted.evidenceSha256 = hostEvidenceSha256(promoted);
  const result = buildBackendHostObservation({
    evidence: promoted,
    inventory: fixture.inventory,
    phase: "baseline",
    runtimeMap: fixture.runtimeMap,
  });
  assertDiagnostic(result, "host.synthetic");
});

// prove the fixed helper executes the physical grammar in a controlled root
test("secure reader confines actual helper bytes and keeps injected authority synthetic", async () => {
  const rootDir = await mkdtemp(join(tmpdir(), "weather-host-secure-reader-"));

  try {
    const nestedDir = join(rootDir, "nested");
    const payloadPath = join(nestedDir, "payload.txt");
    await mkdir(nestedDir);
    await writeFile(payloadPath, "confined fixture bytes\n", { mode: 0o644 });
    await chmod(payloadPath, 0o644);
    const request = secureReaderRequest(rootDir);
    let invocation = null;
    const result = await hostObserver.runBackendHostSecureReader(request, {
      // execute only the reviewed fixed helper command
      async secureExecutor(program, args, options) {
        invocation = { args, options, program };
        return await executeSecureReader(program, args, options);
      },
    });

    assert.equal(
      hostObserver.HOST_SECURE_READER_SHA256,
      sha256Bytes(hostObserver.HOST_SECURE_READER_SOURCE),
    );
    assert.equal(invocation.program, "/usr/bin/python3");
    assert.deepEqual(invocation.args, [
      "-I",
      "-B",
      "-S",
      "-c",
      hostObserver.HOST_SECURE_READER_SOURCE,
    ]);
    assert.equal(invocation.options.cwd, "/");
    assert.deepEqual(invocation.options.env, { LANG: "C" });
    assert.deepEqual(
      invocation.options.stdin,
      Buffer.from(`${canonicalHostJson(request)}\n`),
    );
    assert.equal(invocation.options.maximumStderrBytes, 65536);
    assert.ok(invocation.options.maximumStdoutBytes > request.maximumBytes);
    assert.ok(invocation.options.deadlineMs > 0);
    assert.ok(invocation.options.signal instanceof AbortSignal);
    assert.deepEqual(result.authority, {
      kind: "injected-test",
      liveEligible: false,
    });
    assert.equal(result.contractVersion, hostObserver.HOST_SECURE_READER_RESULT_VERSION);
    assert.equal(result.helperSha256, hostObserver.HOST_SECURE_READER_SHA256);
    assert.equal(result.subjects.length, 1);
    assert.equal(result.subjects[0].status, "ok");
    assert.equal(
      Buffer.from(result.subjects[0].bytes.dataBase64, "base64").toString(),
      "confined fixture bytes\n",
    );
    assert.deepEqual(
      result.subjects[0].identity.pre,
      result.subjects[0].identity.post,
    );
    assert.equal(result.subjects[0].identity.pre.type, "regular-file");
    assert.equal(result.subjects[0].identity.pre.mode, "0644");
    assert.equal(result.subjects[0].releaseBinding, null);
    assert.equal(result.subjects[0].procBinding, null);
  } finally {
    await rm(rootDir, { force: true, recursive: true });
  }
});

// prove the exact helper enumerates one complete bounded control tree
test("secure reader executes the fixed control-plane tree grammar", async () => {
  const rootDir = await mkdtemp(join(tmpdir(), "weather-host-secure-control-tree-"));

  try {
    const files = await writeControlledControlPlaneTree(rootDir);
    const request = controlledControlPlaneRequest(rootDir);
    const result = await hostObserver.runBackendHostSecureReader(request, {
      secureExecutor: executeSecureReader,
    });
    const subject = result.subjects[0];
    assert.equal(subject.status, "ok");
    const tree = JSON.parse(
      Buffer.from(subject.bytes.dataBase64, "base64").toString("utf8"),
    );
    assert.deepEqual(tree.files.map(
      // retain the exact v17 file sequence
      (file) => file.path,
    ), [
      "deploy/postgres/roles.sql",
      "deploy/scripts/a-seen.txt",
      "deploy/scripts/z-script.sh",
      "deploy/sudoers/weather",
      "deploy/systemd/weather.service",
      "deploy/compose.yaml",
    ]);
    assert.equal(tree.controlPlaneSha256, independentV17Digest(files));
    assert.equal(tree.files.at(-1).path, "deploy/compose.yaml");
  } finally {
    await rm(rootDir, { force: true, recursive: true });
  }
});

// prove close-time enumeration detects additions and removals
test("secure reader rejects control-plane directory drift after enumeration", {
  timeout: 20000,
}, async (testContext) => {
  const cases = [
    {
      label: "addition",
      // add one name after the directory has been enumerated
      async mutate(rootDir) {
        await writeFile(join(rootDir, "deploy", "scripts", "late-added.txt"), "late\n", {
          mode: 0o644,
        });
      },
    },
    {
      label: "removal",
      // remove one already visited name after enumeration
      async mutate(rootDir) {
        await unlink(join(rootDir, "deploy", "scripts", "a-seen.txt"));
      },
    },
    {
      label: "in-place overwrite",
      // overwrite one already hashed file without changing membership
      async mutate(rootDir) {
        await writeFile(
          join(rootDir, "deploy", "scripts", "a-seen.txt"),
          "changed after hashing\n",
          { mode: 0o644 },
        );
      },
    },
    {
      label: "same-name replacement",
      // replace one already hashed file without changing final membership
      async mutate(rootDir) {
        const target = join(rootDir, "deploy", "scripts", "a-seen.txt");
        const replacement = join(rootDir, "deploy", "scripts", "replacement.next");
        await writeFile(replacement, "replacement after hashing\n", { mode: 0o644 });
        await rename(replacement, target);
      },
    },
  ];

  // isolate each close-time directory mutation
  for (const fixtureCase of cases) {
    await testContext.test(fixtureCase.label, async () => {
      const rootDir = await mkdtemp(join(tmpdir(), "weather-host-secure-control-race-"));

      try {
        await writeControlledControlPlaneTree(rootDir, { large: true });
        const request = controlledControlPlaneRequest(rootDir);
        const execution = await executeSecureReaderAfterPin(
          request,
          join(rootDir, "deploy", "scripts", "m-large.bin"),
          async () => fixtureCase.mutate(rootDir),
        );
        assert.equal(execution.pinned, true);
        assert.equal(execution.exitCode, 0, execution.stderr.toString("utf8"));
        assert.equal(execution.stderr.length, 0);
        const result = JSON.parse(execution.stdout.toString("utf8"));
        assert.equal(result.subjects[0].status, "error");
        assert.equal(result.subjects[0].errorCode, "host.binding");
      } finally {
        await rm(rootDir, { force: true, recursive: true });
      }
    });
  }
});

// prove the separately appended Compose file remains bound through closing
test("secure reader rejects Compose overwrite after its initial hash", {
  timeout: 20000,
}, async () => {
  const rootDir = await mkdtemp(join(tmpdir(), "weather-host-secure-compose-close-"));

  try {
    await writeControlledControlPlaneTree(rootDir);
    await writeWideControlPlaneDirectory(rootDir);
    const request = controlledControlPlaneRequest(rootDir);
    const composePath = join(rootDir, "deploy", "compose.yaml");
    await truncate(composePath, 16 * 1024 * 1024);
    const execution = await executeSecureReaderAfterCloseAndPin(
      request,
      composePath,
      join(rootDir, "deploy", "postgres"),
      async () => {
        await writeFile(composePath, "services:\n  changed: {}\n", { mode: 0o644 });
      },
    );
    assert.equal(execution.mutated, true);
    assert.equal(execution.exitCode, 0, execution.stderr.toString("utf8"));
    assert.equal(execution.stderr.length, 0);
    const result = JSON.parse(execution.stdout.toString("utf8"));
    assert.equal(result.subjects[0].status, "error");
    assert.equal(result.subjects[0].errorCode, "host.binding");
  } finally {
    await rm(rootDir, { force: true, recursive: true });
  }
});

// prove caller paths cannot widen the fixed helper domain
test("secure reader rejects traversal and physical controlled-root promotion", async (testContext) => {
  const rootDir = await mkdtemp(join(tmpdir(), "weather-host-secure-domain-"));

  try {
    const unsafeComponents = [
      [""],
      ["."],
      [".."],
      ["nested/payload.txt"],
      ["/etc/passwd"],
      ["nested", "payload\0.txt"],
    ];

    // reject every noncanonical component before helper execution
    for (const relativeComponents of unsafeComponents) {
      await testContext.test(JSON.stringify(relativeComponents), async () => {
        let called = false;
        const request = secureReaderRequest(rootDir, { relativeComponents });
        await assertRejectCode(
          () => hostObserver.runBackendHostSecureReader(request, {
            // record any forbidden helper execution
            async secureExecutor() {
              called = true;
              throw new Error("unexpected secure helper execution");
            },
          }),
          "host.binding",
        );
        assert.equal(called, false);
      });
    }

    await assertRejectCode(
      () => hostObserver.runBackendHostSecureReader(secureReaderRequest(rootDir)),
      "host.binding",
    );
  } finally {
    await rm(rootDir, { force: true, recursive: true });
  }
});

// prove final symlinks and special files are rejected before content reads
test("secure reader rejects final symlinks and fifos without following or blocking", {
  timeout: 5000,
}, async (testContext) => {
  const rootDir = await mkdtemp(join(tmpdir(), "weather-host-secure-types-"));
  const outsideDir = await mkdtemp(join(tmpdir(), "weather-host-secure-outside-"));

  try {
    const nestedDir = join(rootDir, "nested");
    await mkdir(nestedDir);
    const outsidePath = join(outsideDir, "outside.txt");
    await writeFile(outsidePath, "must not be read\n", { mode: 0o644 });
    await symlink(outsidePath, join(nestedDir, "symlink.txt"));
    await createFifo(join(nestedDir, "fifo"));

    const unsafeFinals = ["symlink.txt", "fifo"];

    // reject each unsafe final object through the actual helper
    for (const name of unsafeFinals) {
      await testContext.test(name, async () => {
        const request = secureReaderRequest(rootDir, {
          relativeComponents: ["nested", name],
        });
        await assertRejectCode(
          () => hostObserver.runBackendHostSecureReader(request, {
            secureExecutor: executeSecureReader,
          }),
          "host.binding",
        );
      });
    }
  } finally {
    await rm(rootDir, { force: true, recursive: true });
    await rm(outsideDir, { force: true, recursive: true });
  }
});

// prove forged helper metadata cannot hide a path identity change
test("secure reader rejects changed descriptor identity and authority promotion", async () => {
  const rootDir = await mkdtemp(join(tmpdir(), "weather-host-secure-identity-"));

  try {
    const nestedDir = join(rootDir, "nested");
    const payloadPath = join(nestedDir, "payload.txt");
    await mkdir(nestedDir);
    await writeFile(payloadPath, "identity fixture\n", { mode: 0o644 });
    await chmod(payloadPath, 0o644);
    const request = secureReaderRequest(rootDir);
    await assertRejectCode(
      () => hostObserver.runBackendHostSecureReader(request, {
        // alter one validated result after the actual helper exits
        async secureExecutor(program, args, options) {
          const execution = await executeSecureReader(program, args, options);
          const parsed = JSON.parse(execution.stdout.toString("utf8"));
          parsed.authority = { kind: "physical-python", liveEligible: true };
          parsed.subjects[0].identity.post.ino = String(
            BigInt(parsed.subjects[0].identity.post.ino) + 1n,
          );
          return {
            ...execution,
            stdout: Buffer.from(`${canonicalHostJson(parsed)}\n`),
          };
        },
      }),
      "host.binding",
    );
  } finally {
    await rm(rootDir, { force: true, recursive: true });
  }
});

// prove invalid proc authorities cannot select a host fallback
test("secure reader rejects PID zero before helper execution", async () => {
  const baseSubject = {
    containerId: "container-api",
    expectedGid: 10002,
    expectedMode: "0644",
    expectedUid: 10002,
    id: "container-api-code",
    imageId: "sha256:image-server",
    pid: 0,
    readMode: "sha256",
    relativeComponents: ["opt", "weather", "apps", "api", "dist", "main.js"],
  };
  const request = {
    contractVersion: hostObserver.HOST_SECURE_READER_REQUEST_VERSION,
    maximumBytes: 4096,
    operation: "container-proc-root",
    profile: "synthetic-test",
    subjects: [baseSubject],
  };
  let called = false;
  await assertRejectCode(
    () => hostObserver.runBackendHostSecureReader(request, {
      // reject any PID-zero helper attempt
      async secureExecutor() {
        called = true;
        throw new Error("unexpected secure helper execution");
      },
    }),
    "host.identity",
  );
  assert.equal(called, false);
});

// prove proc-root reads bind the same process before and after the read
test("secure reader binds proc-root reads and rejects process identity reuse", async () => {
  const rootDir = await mkdtemp(join(tmpdir(), "weather-host-secure-proc-"));

  try {
    const payloadPath = join(rootDir, "container.txt");
    await writeFile(payloadPath, "proc-root fixture\n", { mode: 0o644 });
    await chmod(payloadPath, 0o644);
    const request = {
      contractVersion: hostObserver.HOST_SECURE_READER_REQUEST_VERSION,
      maximumBytes: 4096,
      operation: "container-proc-root",
      profile: "synthetic-test",
      subjects: [{
        containerId: "synthetic-container",
        expectedGid: process.getgid(),
        expectedMode: "0644",
        expectedUid: process.getuid(),
        id: "synthetic-container-file",
        imageId: "sha256:synthetic-image",
        pid: process.pid,
        readMode: "bytes",
        relativeComponents: payloadPath.split("/").filter(Boolean),
      }],
    };
    const result = await hostObserver.runBackendHostSecureReader(request, {
      secureExecutor: executeSecureReader,
    });
    const subject = result.subjects[0];
    assert.equal(subject.status, "ok");
    assert.equal(subject.procBinding.pid, process.pid);
    assert.equal(subject.procBinding.pidfd, true);
    assert.equal(
      subject.procBinding.startTimeBefore,
      subject.procBinding.startTimeAfter,
    );
    assert.deepEqual(
      subject.procBinding.cgroupBefore,
      subject.procBinding.cgroupAfter,
    );
    assert.deepEqual(
      subject.procBinding.mountinfoBefore,
      subject.procBinding.mountinfoAfter,
    );
    assert.equal(subject.procBinding.root.type, "directory");
    assert.equal(subject.procBinding.mountNamespace.type, "directory");
    await assertRejectCode(
      () => hostObserver.runBackendHostSecureReader(request, {
        // forge one reused process identity after the actual helper exits
        async secureExecutor(program, args, options) {
          const execution = await executeSecureReader(program, args, options);
          const parsed = JSON.parse(execution.stdout.toString("utf8"));
          parsed.subjects[0].procBinding.startTimeAfter = String(
            BigInt(parsed.subjects[0].procBinding.startTimeAfter) + 1n,
          );
          return {
            ...execution,
            stdout: Buffer.from(`${canonicalHostJson(parsed)}\n`),
          };
        },
      }),
      "host.identity",
    );
  } finally {
    await rm(rootDir, { force: true, recursive: true });
  }
});

// prove a root swap after JavaScript admission cannot redirect helper bytes
test("secure reader rejects a parent root replaced before helper open", async () => {
  const rootDir = await mkdtemp(join(tmpdir(), "weather-host-secure-race-"));
  const originalDir = `${rootDir}-original`;

  try {
    const nestedDir = join(rootDir, "nested");
    await mkdir(nestedDir);
    await writeFile(join(nestedDir, "payload.txt"), "original bytes\n", { mode: 0o644 });
    await chmod(join(nestedDir, "payload.txt"), 0o644);
    const request = secureReaderRequest(rootDir);
    await assertRejectCode(
      () => hostObserver.runBackendHostSecureReader(request, {
        // replace the admitted root before the helper opens it
        async secureExecutor(program, args, options) {
          await rename(rootDir, originalDir);
          await mkdir(join(rootDir, "nested"), { recursive: true });
          await writeFile(join(rootDir, "nested", "payload.txt"), "substituted bytes\n", {
            mode: 0o644,
          });
          await chmod(join(rootDir, "nested", "payload.txt"), 0o644);
          return await executeSecureReader(program, args, options);
        },
      }),
      "host.binding",
    );
  } finally {
    await rm(rootDir, { force: true, recursive: true });
    await rm(originalDir, { force: true, recursive: true });
  }
});

// prove a production root path still names the pinned tree after all reads
test("fixed helper rejects a release root replaced after descriptor pin", {
  timeout: 10000,
}, async () => {
  const rootDir = await mkdtemp(join(tmpdir(), "weather-host-secure-release-close-"));
  const originalDir = `${rootDir}-original`;

  // create one complete controlled release tree
  async function writeReleaseTree(path, marker, large) {
    await mkdir(join(path, "deploy", "state"), { recursive: true });
    await writeFile(join(path, "deploy", "state", "current-release"), `${marker}\n`);
    await writeFile(join(path, "deploy", "state", "previous-release"), `${marker}-previous\n`);
    await writeFile(join(path, "deploy", "state", "schema-release"), `${marker}-schema\n`);
    const payloadPath = join(path, "payload.bin");

    // retain enough bounded work to observe the pinned root descriptor
    if (large) {
      await writeFile(payloadPath, Buffer.alloc(0), { mode: 0o644 });
      await truncate(payloadPath, 16 * 1024 * 1024);
    } else {
      await writeFile(payloadPath, "replacement bytes\n", { mode: 0o644 });
    }
    await chmod(payloadPath, 0o644);
  }

  try {
    await writeReleaseTree(rootDir, "original", true);
    const subjects = Array.from(
      { length: 4 },
      // repeat one digest-only source read while retaining one shared root
      (_, index) => ({
        expectedGid: process.getgid(),
        expectedMode: "0644",
        expectedUid: process.getuid(),
        id: `release-payload-${String(index)}`,
        readMode: "sha256",
        relativeComponents: ["payload.bin"],
        rootPath: rootDir,
        rootSelector: "release-root",
      }),
    );
    const request = {
      contractVersion: hostObserver.HOST_SECURE_READER_REQUEST_VERSION,
      maximumBytes: 16 * 1024 * 1024,
      operation: "confined-files",
      profile: "blueberry-compose",
      subjects,
    };
    const execution = await executeSecureReaderAfterPin(request, rootDir, async () => {
      await rename(rootDir, originalDir);
      await mkdir(rootDir);
      await writeReleaseTree(rootDir, "replacement", false);
    });
    assert.equal(execution.pinned, true);
    assert.equal(execution.exitCode, 0, execution.stderr.toString("utf8"));
    assert.equal(execution.stderr.length, 0);
    const result = JSON.parse(execution.stdout.toString("utf8"));
    assert.ok(result.subjects.every(
      // reject every detached old-root result
      (subject) => subject.status === "error" && subject.errorCode === "host.binding",
    ), canonicalHostJson(result));
  } finally {
    await rm(rootDir, { force: true, recursive: true });
    await rm(originalDir, { force: true, recursive: true });
  }
});

// prove a runner alias still names the pinned release after all reads
test("fixed helper rejects a runner current target replaced after descriptor pin", {
  timeout: 10000,
}, async () => {
  const rootDir = await mkdtemp(join(tmpdir(), "weather-host-secure-runner-close-"));
  const firstRelease = "a".repeat(40);
  const secondRelease = "b".repeat(40);
  const firstPath = join(rootDir, "install", "releases", firstRelease);
  const secondPath = join(rootDir, "install", "releases", secondRelease);
  const currentPath = join(rootDir, "install", "current");

  try {
    await mkdir(firstPath, { recursive: true });
    await mkdir(secondPath, { recursive: true });
    await writeFile(join(firstPath, "payload.bin"), Buffer.alloc(0), { mode: 0o644 });
    await truncate(join(firstPath, "payload.bin"), 16 * 1024 * 1024);
    await writeFile(join(secondPath, "payload.bin"), "replacement bytes\n", { mode: 0o644 });
    await chmod(join(firstPath, "payload.bin"), 0o644);
    await chmod(join(secondPath, "payload.bin"), 0o644);
    await symlink(join("releases", firstRelease), currentPath);
    const request = {
      contractVersion: hostObserver.HOST_SECURE_READER_REQUEST_VERSION,
      maximumBytes: 16 * 1024 * 1024,
      operation: "confined-files",
      profile: "synthetic-test",
      subjects: Array.from(
        { length: 4 },
        // repeat one digest-only source read while retaining one release descriptor
        (_, index) => ({
          expectedGid: process.getgid(),
          expectedMode: "0644",
          expectedUid: process.getuid(),
          id: `runner-current-payload-${String(index)}`,
          readMode: "sha256",
          relativeComponents: ["install", "current", "payload.bin"],
          rootPath: rootDir,
          rootSelector: "controlled-test-root",
        }),
      ),
    };
    const execution = await executeSecureReaderAfterPin(request, firstPath, async () => {
      const replacementLink = join(rootDir, "install", "current.next");
      await symlink(join("releases", secondRelease), replacementLink);
      await rename(replacementLink, currentPath);
    });
    assert.equal(execution.pinned, true);
    assert.equal(execution.exitCode, 0, execution.stderr.toString("utf8"));
    assert.equal(execution.stderr.length, 0);
    const result = JSON.parse(execution.stdout.toString("utf8"));
    assert.ok(result.subjects.every(
      // reject every detached old-current result
      (subject) => subject.status === "error" && subject.errorCode === "host.binding",
    ), canonicalHostJson(result));
  } finally {
    await rm(rootDir, { force: true, recursive: true });
  }
});

// prove the public secure-reader facade cannot select production roots
test("secure reader rejects every direct production selector before execution", async (testContext) => {
  const cases = [
    {
      id: "systemd-unit",
      profile: "blueberry-compose",
      relativeComponents: ["etc", "systemd", "system", "weather-compose.service"],
      rootPath: "/",
      rootSelector: "systemd-root",
      uid: 0,
    },
    {
      id: "operator.forced.dispatcher",
      profile: "blueberry-compose",
      relativeComponents: ["usr", "local", "bin", "weather-ssh-dispatch"],
      rootPath: "/",
      rootSelector: "operator-root",
      uid: 0,
    },
    {
      id: "release.current",
      profile: "blueberry-compose",
      relativeComponents: ["deploy", "state", "current-release"],
      rootPath: "/opt/weather/current",
      rootSelector: "release-root",
      uid: 0,
    },
    {
      id: "runner-file",
      profile: "workstation-runner",
      relativeComponents: [
        ".weather",
        "adjustment-maintenance",
        "runner",
        "current",
        "scripts",
        "runner.mjs",
      ],
      rootPath: "/home/ubuntu",
      rootSelector: "runner-home",
      uid: 1000,
    },
  ];

  // reject each production root outside collector-owned planning
  for (const fixtureCase of cases) {
    await testContext.test(fixtureCase.rootSelector, async () => {
      const request = {
        contractVersion: hostObserver.HOST_SECURE_READER_REQUEST_VERSION,
        maximumBytes: 4096,
        operation: "confined-files",
        profile: fixtureCase.profile,
        subjects: [{
          expectedGid: fixtureCase.uid,
          expectedMode: fixtureCase.rootSelector === "operator-root" ? "0755" : "0644",
          expectedUid: fixtureCase.uid,
          id: fixtureCase.id,
          readMode: "sha256",
          relativeComponents: fixtureCase.relativeComponents,
          rootPath: fixtureCase.rootPath,
          rootSelector: fixtureCase.rootSelector,
        }],
      };
      let called = false;
      await assertRejectCode(
        () => hostObserver.runBackendHostSecureReader(request, {
          // record any forbidden production-root execution
          async secureExecutor() {
            called = true;
            throw new Error("unexpected production-root execution");
          },
        }),
        "host.binding",
      );
      assert.equal(called, false);
    });
  }
});

// prove arbitrary raw container selectors cannot reach any helper executor
test("secure reader rejects arbitrary proc-root authority before execution", async () => {
  const request = {
    contractVersion: hostObserver.HOST_SECURE_READER_REQUEST_VERSION,
    maximumBytes: 4096,
    operation: "container-proc-root",
    profile: "arbitrary-not-a-production-profile",
    subjects: [{
      containerId: "caller-selected-container",
      expectedGid: false,
      expectedMode: { value: "0000" },
      expectedUid: { value: 0 },
      id: "caller-selected-shadow",
      imageId: "caller-selected-image",
      pid: process.pid,
      readMode: "bytes",
      relativeComponents: ["etc", "shadow"],
    }],
  };
  let called = false;
  await assertRejectCode(
    () => hostObserver.runBackendHostSecureReader(request, {
      // record any forbidden arbitrary proc-root execution
      async secureExecutor() {
        called = true;
        throw new Error("unexpected arbitrary proc-root execution");
      },
    }),
    "host.binding",
  );
  assert.equal(called, false);
});

// prove private collection rejects mixed release and runner identities
test("collector rejects mixed release markers and missing runner privilege drop", async (testContext) => {
  const cases = [
    {
      expectedUnresolved: "release.current",
      label: "release-markers",
      profile: "blueberry-compose",
      // substitute one closing release marker snapshot
      mutate(result) {
        const subject = result.subjects.find(
          // select one release-bound secure result
          (candidate) => candidate.releaseBinding?.markersAfter.length > 0,
        );
        subject.releaseBinding.markersAfter[0].bytes = rawBytes("changed\n");
      },
    },
    {
      expectedUnresolved: "release.current",
      label: "release-marker-public-mode",
      profile: "blueberry-compose",
      // substitute the obsolete public marker mode
      mutate(result) {
        const subject = result.subjects.find(
          // select the exact current-release secure result
          (candidate) => candidate.id === "release.current",
        );
        subject.identity.pre.mode = "0644";
        subject.identity.post.mode = "0644";
      },
    },
    {
      expectedUnresolved: "runner.root.private",
      label: "runner-privilege-drop",
      profile: "workstation-runner",
      // remove the required nonroot privilege drop
      mutate(result) {
        const subject = result.subjects.find(
          // select one runner-home secure result
          (candidate) => candidate.executionIdentity.dropped === true,
        );
        subject.executionIdentity.dropped = false;
      },
    },
    {
      expectedUnresolved: "runner.root.private",
      label: "runner-current-target",
      profile: "workstation-runner",
      // substitute one noncanonical runner release target
      mutate(result) {
        const subject = result.subjects.find(
          // select one runner current-alias secure result
          (candidate) => candidate.releaseBinding?.currentTarget !== undefined &&
            candidate.releaseBinding?.currentTarget !== null,
        );
        subject.releaseBinding.currentTarget = ["releases", "A".repeat(40)];
      },
    },
  ];

  // isolate each private collector identity failure
  for (const fixtureCase of cases) {
    await testContext.test(fixtureCase.label, async () => {
      const request = await checkedRequest(fixtureCase.profile);
      const fixture = await scriptedProfileAdapters(fixtureCase.profile);
      fixture.adapters.secureExecutor = capturingSecureExecutor([], fixtureCase.mutate);
      const evidence = await collectBackendHostEvidence(request, fixture.adapters);
      assert.equal(evidence.complete, false);
      assert.ok(evidence.unresolved.some(
        // require the affected physical fact to remain unresolved
        (candidate) => candidate.id === fixtureCase.expectedUnresolved,
      ));
    });
  }
});

// prove host collection routes privileged paths only through the secure reader
test("collector never reopens secured host paths through Node filesystem adapters", async (testContext) => {
  const securedPaths = [];

  // recognize only privileged paths owned by the secure-reader boundary
  const isSecuredPath = (path) => /^\/proc\/[1-9][0-9]*\/root(?:\/|$)/u.test(path) ||
    /^\/opt\/weather\/(?:current|operators)(?:\/|$)/u.test(path) ||
    /^\/usr\/local\/(?:bin|sbin)\/weather/u.test(path) ||
    /^\/etc\/systemd\/system(?:\/|$)/u.test(path) ||
    /^\/home\/ubuntu\/(?:\.weather|\.local\/lib)(?:\/|$)/u.test(path);
  const cases = ["blueberry-compose", "workstation-runner"];

  // verify each host profile through the same collector seam
  for (const profile of cases) {
    await testContext.test(profile, async () => {
      const request = await checkedRequest(profile);
      const fixture = await scriptedProfileAdapters(profile);
      const secureCalls = [];

      // observe every legacy Node filesystem operation
      for (const method of ["open", "lstat", "readlink", "realpath"]) {
        const original = fixture.adapters.filesystem[method];
        fixture.adapters.filesystem[method] = async (path, ...args) => {
          const sourcePath = String(path);

          // record any privileged path that bypasses the secure reader
          if (isSecuredPath(sourcePath)) {
            securedPaths.push({ method, path: sourcePath, profile });
          }
          return await original(path, ...args);
        };
      }
      fixture.adapters.secureExecutor = capturingSecureExecutor(secureCalls);
      await collectBackendHostEvidence(request, fixture.adapters);
      assert.ok(secureCalls.length > 0);

      // require the exact profile-owned secure roots
      if (profile === "blueberry-compose") {
        assert.ok(secureCalls.some(
          // retain one exact container proc-root request
          (call) => call.request.operation === "container-proc-root",
        ));
        assert.ok(secureCalls.some(
          // retain one exact release snapshot request
          (call) => call.request.subjects.some(
            (subject) => subject.rootSelector === "release-root",
          ),
        ));
        assert.ok(secureCalls.some(
          // retain one exact systemd root request
          (call) => call.request.subjects.some(
            (subject) => subject.rootSelector === "systemd-root",
          ),
        ));
      } else {
        assert.ok(secureCalls.some(
          // retain one exact NSS-derived runner home request
          (call) => call.request.subjects.some(
            (subject) => subject.rootSelector === "runner-home" &&
              subject.rootPath === "/home/ubuntu" &&
              subject.expectedUid === 1000 && subject.expectedGid === 1000,
          ),
        ));
      }
    });
  }
  assert.deepEqual(securedPaths, []);
});

// match all private release markers to write_private_state mode
test("Blueberry release marker subjects require root-owned mode 0600", async () => {
  const request = await checkedRequest("blueberry-compose");
  const fixture = await scriptedProfileAdapters("blueberry-compose");
  const secureCalls = [];
  fixture.adapters.secureExecutor = capturingSecureExecutor(secureCalls);
  await collectBackendHostEvidence(request, fixture.adapters);
  const markerSubjects = secureCalls.flatMap(
    // retain only the three source-owned private marker subjects
    (call) => call.request.subjects.filter(
      (subject) => ["release.current", "release.previous", "release.schema"].includes(subject.id),
    ),
  );
  assert.deepEqual(markerSubjects.map(
    // compare one exact marker policy per subject
    (subject) => ({
      expectedGid: subject.expectedGid,
      expectedMode: subject.expectedMode,
      expectedUid: subject.expectedUid,
      id: subject.id,
    }),
  ), [
    { expectedGid: 0, expectedMode: "0600", expectedUid: 0, id: "release.current" },
    { expectedGid: 0, expectedMode: "0600", expectedUid: 0, id: "release.previous" },
    { expectedGid: 0, expectedMode: "0600", expectedUid: 0, id: "release.schema" },
  ]);
});

// prove the actual marker producer writes the observer's private mode
test("write_private_state publishes all release markers as mode 0600", async () => {
  const rootDir = await mkdtemp(join(tmpdir(), "weather-private-markers-"));

  try {
    for (const [name, value] of [
      ["current-release", "2026.11.13-1"],
      ["previous-release", "2026.11.12-1"],
      ["schema-release", "2026.11.13-schema"],
    ]) {
      const path = join(rootDir, name);
      const child = spawn("/usr/bin/bash", [
        "-c",
        'source "$1"; write_private_state "$2" "$3"',
        "marker-fixture",
        COMMON_DEPLOY_PATH.pathname,
        path,
        value,
      ], {
        cwd: rootDir,
        env: { LANG: "C", PATH: process.env.PATH },
        stdio: ["ignore", "pipe", "pipe"],
      });
      const stderr = [];
      child.stderr.on("data", (chunk) => {
        stderr.push(Buffer.from(chunk));
      });
      const exitCode = await new Promise((resolve, reject) => {
        child.once("error", reject);
        child.once("close", resolve);
      });
      assert.equal(exitCode, 0, Buffer.concat(stderr).toString("utf8"));
      const details = await stat(path);
      assert.equal(details.mode & 0o777, 0o600);
      assert.equal(details.nlink, 1);
      assert.equal((await readFile(path)).toString("utf8"), `${value}\n`);
    }
  } finally {
    await rm(rootDir, { force: true, recursive: true });
  }
});
