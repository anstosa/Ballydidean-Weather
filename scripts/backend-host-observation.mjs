import { createHash } from "node:crypto";
import { execFile } from "node:child_process";
import {
  open,
  opendir,
  lstat,
  readlink,
  realpath,
  statfs,
} from "node:fs/promises";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

export const HOST_EVIDENCE_REQUEST_VERSION = "weather-backend-host-evidence-request/v1";
export const HOST_EVIDENCE_CONTRACT_VERSION = "weather-backend-host-evidence/v1";
export const HOST_PROBE_VERSION = "weather-backend-host-probe/v1";
export const HOST_OBSERVATION_VERSION = "weather-backend-host-observation/v2";
export const HOST_PRODUCER = "weather-backend-read-only-host-observer/v2";
export const HOST_SECURE_READER_REQUEST_VERSION = "weather-backend-host-secure-reader-request/v1";
export const HOST_SECURE_READER_RESULT_VERSION = "weather-backend-host-secure-reader-result/v1";
export const HOST_SECURE_READER_PROGRAM = "/usr/bin/python3";
// weather-retained-v1-secure-reader-source:start
export const HOST_SECURE_READER_SOURCE = String.raw`import base64
import datetime
import hashlib
import json
import os
import stat
import sys

REQUEST_VERSION = "weather-backend-host-secure-reader-request/v1"
PROC_SUPER_MAGIC = 0x9FA0

# encode one exact byte field
def raw_bytes(value):
    return {
        "byteLength": len(value),
        "dataBase64": base64.b64encode(value).decode("ascii"),
        "encoding": "base64",
        "sha256": hashlib.sha256(value).hexdigest(),
        "truncated": False,
    }

# encode one stable descriptor identity
def identity(value, forced_kind=None):
    mode = stat.S_IMODE(value.st_mode)
    kind = forced_kind if forced_kind is not None else "regular-file" if stat.S_ISREG(value.st_mode) else "directory" if stat.S_ISDIR(value.st_mode) else "other"
    return {
        "ctimeNs": str(value.st_ctime_ns),
        "dev": str(value.st_dev),
        "gid": value.st_gid,
        "ino": str(value.st_ino),
        "mode": format(mode, "04o"),
        "mtimeNs": str(value.st_mtime_ns),
        "nlink": str(value.st_nlink),
        "size": str(value.st_size),
        "type": kind,
        "uid": value.st_uid,
    }

# close every descriptor in reverse acquisition order
def close_all(descriptors):
    for descriptor in reversed(descriptors):
        try:
            os.close(descriptor)
        except OSError:
            pass

# read one small regular file beneath a retained directory descriptor
def read_relative_bytes(root_fd, components, maximum_bytes):
    descriptors = []
    try:
        parent_fd = root_fd
        # walk every retained relative parent
        for component in components[:-1]:
            child_fd = os.open(component, os.O_PATH | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=parent_fd)
            descriptors.append(child_fd)
            if not stat.S_ISDIR(os.fstat(child_fd).st_mode):
                raise ValueError("relative parent is not a directory")
            parent_fd = child_fd
        object_fd = os.open(components[-1], os.O_PATH | os.O_NOFOLLOW, dir_fd=parent_fd)
        descriptors.append(object_fd)
        if not stat.S_ISREG(os.fstat(object_fd).st_mode):
            raise ValueError("relative object is not a regular file")
        readable_fd = os.open("/proc/self/fd/" + str(object_fd), os.O_RDONLY | os.O_NONBLOCK | os.O_NOCTTY)
        descriptors.append(readable_fd)
        chunks = []
        total = 0
        while total <= maximum_bytes:
            chunk = os.read(readable_fd, min(65536, maximum_bytes + 1 - total))
            if not chunk:
                break
            chunks.append(chunk)
            total += len(chunk)
        if total > maximum_bytes:
            raise ValueError("relative file exceeds bound")
        return b"".join(chunks)
    finally:
        close_all(descriptors)

# read one already pinned regular file without reopening its pathname
def read_pinned_bytes(object_fd, maximum_bytes):
    readable_fd = os.open("/proc/self/fd/" + str(object_fd), os.O_RDONLY | os.O_NONBLOCK | os.O_NOCTTY)
    try:
        before = identity(os.fstat(object_fd))
        if before["type"] != "regular-file" or before != identity(os.fstat(readable_fd)):
            raise ValueError("pinned object identity differs")
        chunks = []
        total = 0
        while total <= maximum_bytes:
            chunk = os.read(readable_fd, min(65536, maximum_bytes + 1 - total))
            if not chunk:
                break
            chunks.append(chunk)
            total += len(chunk)
        if total > maximum_bytes or before != identity(os.fstat(object_fd)) or before != identity(os.fstat(readable_fd)):
            raise ValueError("pinned object changed")
        return b"".join(chunks), before
    finally:
        os.close(readable_fd)

# snapshot the three exact release state markers
def release_markers(root_fd, maximum_bytes):
    markers = []
    # retain the exact three source-owned marker files
    for marker_id in ("current-release", "previous-release", "schema-release"):
        value = read_relative_bytes(root_fd, ["deploy", "state", marker_id], maximum_bytes)
        markers.append({"bytes": raw_bytes(value), "id": marker_id})
    return markers

# open one descriptor-confined directory for enumeration
def open_confined_directory(root_fd, components):
    descriptors = []
    parent_fd = root_fd
    try:
        # pin and open every path component relative to its retained parent
        for component in components:
            pinned_fd = os.open(component, os.O_PATH | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=parent_fd)
            descriptors.append(pinned_fd)
            readable_fd = os.open(component, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=parent_fd)
            descriptors.append(readable_fd)
            # require matching pinned and readable directory identities
            if identity(os.fstat(pinned_fd), "directory") != identity(os.fstat(readable_fd), "directory"):
                raise ValueError("control-plane directory identity differs")
            parent_fd = pinned_fd
        return descriptors, parent_fd, readable_fd
    except Exception:
        close_all(descriptors)
        raise

# list one bounded readable directory descriptor
def list_directory_names(readable_fd):
    names = []
    with os.scandir(readable_fd) as iterator:
        # bound entries before retaining the complete directory list
        for entry in iterator:
            names.append(entry.name)
            # stop before one oversized directory list is retained
            if len(names) > 2048:
                raise ValueError("control-plane directory exceeds bound")
    return sorted(names)

# compare one descriptor-relative closing file identity
def closing_file_matches(root_fd, components, expected_identity):
    descriptors = []
    parent_fd = root_fd
    try:
        # re-walk every parent without following links
        for component in components[:-1]:
            child_fd = os.open(component, os.O_PATH | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=parent_fd)
            descriptors.append(child_fd)
            # require every closing parent to remain a directory
            if not stat.S_ISDIR(os.fstat(child_fd).st_mode):
                return False
            parent_fd = child_fd
        object_fd = os.open(components[-1], os.O_PATH | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=parent_fd)
        descriptors.append(object_fd)
        current = identity(os.fstat(object_fd))
        return current["type"] == "regular-file" and current == expected_identity
    except (KeyError, OSError, TypeError, ValueError):
        return False
    finally:
        close_all(descriptors)

# enumerate one complete predecessor-protected control-plane tree
def control_plane_tree(deploy_fd, maximum_bytes):
    directories = []
    file_snapshots = []
    files = []
    retained = []
    snapshots = []

    # walk one fixed protected directory without following links
    def walk(relative_path, components):
        descriptors = []
        try:
            descriptors, parent_fd, readable_fd = open_confined_directory(deploy_fd, components)
            names = list_directory_names(readable_fd)
            snapshots.append({
                "components": list(components),
                "identity": identity(os.fstat(parent_fd), "directory"),
                "names": names,
            })
            entries = []
            # inspect every bounded directory entry relative to its pinned parent
            for name in names:
                if name in (".", "..") or "/" in name or "\x00" in name:
                    raise ValueError("invalid control-plane entry")
                object_fd = os.open(name, os.O_PATH | os.O_NOFOLLOW, dir_fd=parent_fd)
                descriptors.append(object_fd)
                details = os.fstat(object_fd)
                if stat.S_ISDIR(details.st_mode):
                    entries.append({"name": name, "type": "directory"})
                elif stat.S_ISREG(details.st_mode):
                    entries.append({"name": name, "type": "regular-file"})
                    value, file_identity = read_pinned_bytes(object_fd, maximum_bytes)
                    files.append({
                        "identity": file_identity,
                        "path": relative_path + "/" + name,
                        "sha256": hashlib.sha256(value).hexdigest(),
                    })
                    file_snapshots.append({
                        "components": list(components) + [name],
                        "identity": file_identity,
                    })
                else:
                    raise ValueError("unsupported control-plane entry")
                if len(files) + len(directories) > 2048:
                    raise ValueError("control-plane tree exceeds bound")
            directories.append({"entries": entries, "path": relative_path})
            for entry in entries:
                if entry["type"] == "directory":
                    walk(relative_path + "/" + entry["name"], components + [entry["name"]])
        finally:
            close_all(descriptors)

    for root_name in ("postgres", "scripts", "sudoers", "systemd"):
        walk("deploy/" + root_name, [root_name])
    directories.sort(key=lambda item: item["path"])
    files.sort(key=lambda item: item["path"])
    compose_fd = os.open("compose.yaml", os.O_PATH | os.O_NOFOLLOW, dir_fd=deploy_fd)
    retained.append(compose_fd)
    try:
        compose, compose_identity = read_pinned_bytes(compose_fd, maximum_bytes)
        files.append({
            "identity": compose_identity,
            "path": "deploy/compose.yaml",
            "sha256": hashlib.sha256(compose).hexdigest(),
        })
        file_snapshots.append({
            "components": ["compose.yaml"],
            "identity": compose_identity,
        })
    finally:
        close_all(retained)

    # re-open every directory and reject post-enumeration identity or listing drift
    for snapshot in snapshots:
        closing = []
        try:
            closing, pinned_fd, readable_fd = open_confined_directory(deploy_fd, snapshot["components"])
            # reject directory identity or membership changes after collection
            if identity(os.fstat(pinned_fd), "directory") != snapshot["identity"] or list_directory_names(readable_fd) != snapshot["names"]:
                raise ValueError("control-plane directory changed")
        finally:
            close_all(closing)

    # re-open every regular file and reject post-hash identity drift
    for snapshot in file_snapshots:
        # bind the closing path entry to the identity captured with its digest
        if not closing_file_matches(deploy_fd, snapshot["components"], snapshot["identity"]):
            raise ValueError("control-plane file changed")
    framed = b"".join((item["path"] + "\x00" + item["sha256"] + "\n").encode("utf-8") for item in files)
    return {
        "contractVersion": "weather-schema-preserving-control-plane-tree/v1",
        "controlPlaneSha256": hashlib.sha256(framed).hexdigest(),
        "directories": directories,
        "files": files,
    }

# reopen the source-owned root entry and compare it with the pinned directory
def closing_root_matches(shared):
    descriptors = []
    try:
        closing_fd = os.open(shared["rootEntry"], os.O_PATH | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=shared["anchorFd"])
        descriptors.append(closing_fd)
        if identity(os.fstat(closing_fd), "directory") != identity(os.fstat(shared["rootFd"]), "directory"):
            return False
        if "currentParentFd" in shared:
            closing_target = os.readlink("current", dir_fd=shared["currentParentFd"]).split("/")
            if closing_target != shared["currentTarget"]:
                return False
            closing_releases = os.open("releases", os.O_PATH | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=shared["currentParentFd"])
            descriptors.append(closing_releases)
            if identity(os.fstat(closing_releases), "directory") != identity(os.fstat(shared["releasesFd"]), "directory"):
                return False
            closing_release = os.open(closing_target[1], os.O_PATH | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=closing_releases)
            descriptors.append(closing_release)
            if identity(os.fstat(closing_release), "directory") != identity(os.fstat(shared["releaseFd"]), "directory"):
                return False
        return True
    except (KeyError, OSError, TypeError, ValueError):
        return False
    finally:
        close_all(descriptors)

# reject one invalid confined subject
def fail(subject_id, code):
    return {
        "bytes": None,
        "contentSha256": None,
        "errorCode": code,
        "executionIdentity": {
            "dropped": False,
            "gid": os.getegid(),
            "supplementaryGroups": sorted(os.getgroups()),
            "uid": os.geteuid(),
        },
        "id": subject_id,
        "identity": None,
        "procBinding": None,
        "releaseBinding": None,
        "status": "error",
    }

# retain one exact missing-path outcome without inventing file identity
def absent(subject_id):
    return {
        "bytes": None,
        "contentSha256": None,
        "errorCode": "ENOENT",
        "executionIdentity": {
            "dropped": False,
            "gid": os.getegid(),
            "supplementaryGroups": sorted(os.getgroups()),
            "uid": os.geteuid(),
        },
        "id": subject_id,
        "identity": None,
        "procBinding": None,
        "releaseBinding": None,
        "status": "absent",
    }

# read one file through retained directory descriptors
def read_confined(subject, maximum_bytes, shared):
    descriptors = []
    try:
        # drop to the proven runner account before opening its home
        dropped = subject["rootSelector"] == "runner-home"
        if dropped:
            if subject["expectedUid"] <= 0 or subject["expectedGid"] <= 0:
                return fail(subject["id"], "host.identity")
            # drop once before the first runner subject in this helper batch
            if os.geteuid() != subject["expectedUid"] or os.getegid() != subject["expectedGid"]:
                os.setgroups([])
                os.setgid(subject["expectedGid"])
                os.setuid(subject["expectedUid"])
            if os.geteuid() != subject["expectedUid"] or os.getegid() != subject["expectedGid"] or os.getgroups():
                return fail(subject["id"], "host.identity")
        root_flags = os.O_PATH | os.O_DIRECTORY | os.O_NOFOLLOW
        if "rootFd" not in shared:
            root_path = subject["rootPath"]
            if root_path == "/":
                anchor_path = "/"
                root_entry = "."
            else:
                anchor_path, root_entry = os.path.split(root_path.rstrip("/"))
            shared["anchorFd"] = os.open(anchor_path, root_flags)
            shared["rootEntry"] = root_entry
            shared["rootFd"] = os.open(root_entry, root_flags, dir_fd=shared["anchorFd"])
            shared["rootPath"] = subject["rootPath"]
            shared["rootSelector"] = subject["rootSelector"]
            shared["descriptors"].append(shared["anchorFd"])
            shared["descriptors"].append(shared["rootFd"])
        if shared["rootPath"] != subject["rootPath"] or shared["rootSelector"] != subject["rootSelector"]:
            return fail(subject["id"], "host.binding")
        root_fd = shared["rootFd"]
        root_identity = os.fstat(root_fd)

        # require one nonwritable root owned by the active helper identity
        if not stat.S_ISDIR(root_identity.st_mode) or root_identity.st_uid != os.geteuid() or root_identity.st_gid != os.getegid() or stat.S_IMODE(root_identity.st_mode) & 0o022:
            return fail(subject["id"], "host.binding")
        relative_components = subject["relativeComponents"]
        parent_fd = root_fd
        release_binding = None

        # pin the complete Blueberry release marker snapshot around this read
        if subject["rootSelector"] == "release-root":
            if "markersBefore" not in shared:
                shared["markersBefore"] = release_markers(root_fd, maximum_bytes)
            release_binding = {
                "anchor": identity(root_identity, "directory"),
                "currentTarget": None,
                "markersAfter": None,
                "markersBefore": shared["markersBefore"],
                "releaseRoot": identity(root_identity, "directory"),
            }

        # resolve only the reviewed runner or synthetic current alias
        alias_current = subject["rootSelector"] in ("runner-home", "controlled-test-root") and "current" in relative_components
        if alias_current:
            current_index = relative_components.index("current")
            # walk to the retained runner installation root
            for component in relative_components[:current_index]:
                child_fd = os.open(component, os.O_PATH | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=parent_fd)
                descriptors.append(child_fd)
                if not stat.S_ISDIR(os.fstat(child_fd).st_mode):
                    return fail(subject["id"], "host.binding")
                parent_fd = child_fd
            if "currentParentFd" not in shared:
                shared["currentParentFd"] = os.dup(parent_fd)
                shared["descriptors"].append(shared["currentParentFd"])
            current_target = os.readlink("current", dir_fd=parent_fd).split("/")
            if len(current_target) != 2 or current_target[0] != "releases" or len(current_target[1]) != 40 or any(character not in "0123456789abcdef" for character in current_target[1]):
                return fail(subject["id"], "host.binding")
            if "releaseFd" not in shared:
                releases_fd = os.open("releases", os.O_PATH | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=parent_fd)
                shared["releasesFd"] = releases_fd
                shared["descriptors"].append(shared["releasesFd"])
                shared["releaseFd"] = os.open(current_target[1], os.O_PATH | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=releases_fd)
                shared["currentTarget"] = current_target
                shared["descriptors"].append(shared["releaseFd"])
            if shared["currentTarget"] != current_target:
                return fail(subject["id"], "host.binding")
            release_fd = shared["releaseFd"]
            release_identity = os.fstat(release_fd)
            if not stat.S_ISDIR(release_identity.st_mode):
                return fail(subject["id"], "host.binding")
            parent_fd = release_fd
            relative_components = relative_components[current_index + 1:]
            # expose release authority only for the physical runner profile
            if subject["rootSelector"] == "runner-home":
                release_binding = {
                    "anchor": identity(root_identity, "directory"),
                    "currentTarget": current_target,
                    "markersAfter": [],
                    "markersBefore": [],
                    "releaseRoot": identity(release_identity, "directory"),
                }

        # walk every parent component without following links
        for component in relative_components[:-1]:
            child_fd = os.open(component, root_flags, dir_fd=parent_fd)
            descriptors.append(child_fd)
            child_identity = os.fstat(child_fd)
            if not stat.S_ISDIR(child_identity.st_mode):
                return fail(subject["id"], "host.binding")
            parent_fd = child_fd
        final_name = relative_components[-1]
        object_fd = os.open(final_name, os.O_PATH | os.O_NOFOLLOW, dir_fd=parent_fd)
        descriptors.append(object_fd)
        before_stat = os.fstat(object_fd)

        expected_directory = subject.get("expectedType") == "directory"

        # reject links devices fifos and substituted ownership before reading
        if (expected_directory and not stat.S_ISDIR(before_stat.st_mode)) or (not expected_directory and not stat.S_ISREG(before_stat.st_mode)) or before_stat.st_uid != subject["expectedUid"] or before_stat.st_gid != subject["expectedGid"] or format(stat.S_IMODE(before_stat.st_mode), "04o") != subject["expectedMode"]:
            return fail(subject["id"], "host.binding")
        if expected_directory and subject["readMode"] == "tree":
            tree = control_plane_tree(object_fd, maximum_bytes)
            content = json.dumps(tree, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")
            if len(content) > maximum_bytes:
                return fail(subject["id"], "host.bounds")
            before = identity(before_stat, "directory")
            after = identity(os.fstat(object_fd), "directory")
            if before != after:
                return fail(subject["id"], "host.binding")
            return {
                "bytes": raw_bytes(content),
                "contentSha256": hashlib.sha256(content).hexdigest(),
                "errorCode": None,
                "executionIdentity": {
                    "dropped": dropped,
                    "gid": os.getegid(),
                    "supplementaryGroups": sorted(os.getgroups()),
                    "uid": os.geteuid(),
                },
                "id": subject["id"],
                "identity": {"post": after, "pre": before},
                "procBinding": None,
                "releaseBinding": release_binding,
                "status": "ok",
            }
        if expected_directory:
            before = identity(before_stat, "directory")
            after_stat = os.fstat(object_fd)
            after = identity(after_stat, "directory")
            if before != after:
                return fail(subject["id"], "host.binding")
            return {
                "bytes": None,
                "contentSha256": None,
                "errorCode": None,
                "executionIdentity": {
                    "dropped": dropped,
                    "gid": os.getegid(),
                    "supplementaryGroups": sorted(os.getgroups()),
                    "uid": os.geteuid(),
                },
                "id": subject["id"],
                "identity": {"post": after, "pre": before},
                "procBinding": None,
                "releaseBinding": release_binding,
                "status": "ok",
            }
        readable_fd = os.open("/proc/self/fd/" + str(object_fd), os.O_RDONLY | os.O_NONBLOCK | os.O_NOCTTY)
        descriptors.append(readable_fd)
        chunks = []
        total = 0

        # read one byte beyond the fixed cap before retaining content
        while total <= maximum_bytes:
            chunk = os.read(readable_fd, min(65536, maximum_bytes + 1 - total))
            if not chunk:
                break
            chunks.append(chunk)
            total += len(chunk)
        if total > maximum_bytes:
            return fail(subject["id"], "host.bounds")
        after_stat = os.fstat(object_fd)
        readable_after = os.fstat(readable_fd)
        before = identity(before_stat)
        after = identity(after_stat)

        # require one unchanged pinned object through the complete read
        if before != after or before != identity(readable_after):
            return fail(subject["id"], "host.binding")
        content = b"".join(chunks)
        retained = raw_bytes(content) if subject["readMode"] == "bytes" else None
        return {
            "bytes": retained,
            "contentSha256": hashlib.sha256(content).hexdigest(),
            "errorCode": None,
            "executionIdentity": {
                "dropped": dropped,
                "gid": os.getegid(),
                "supplementaryGroups": sorted(os.getgroups()),
                "uid": os.geteuid(),
            },
            "id": subject["id"],
            "identity": {"post": after, "pre": before},
            "procBinding": None,
            "releaseBinding": release_binding,
            "status": "ok",
        }
    except FileNotFoundError:
        if subject.get("allowAbsent") is True:
            return absent(subject.get("id", "invalid"))
        return fail(subject.get("id", "invalid"), "host.binding")
    except (KeyError, OSError, TypeError, ValueError):
        return fail(subject.get("id", "invalid"), "host.binding")
    finally:
        close_all(descriptors)

# parse one proc stat start-time identity
def proc_start_time(value):
    close = value.rfind(b")")
    if close < 0:
        raise ValueError("invalid proc stat")
    fields = value[close + 2:].split()
    return fields[19].decode("ascii")

# read one bounded proc metadata file
def read_proc_file(path, maximum_bytes):
    descriptor = os.open(path, os.O_RDONLY | os.O_NONBLOCK | os.O_NOCTTY)
    try:
        chunks = []
        total = 0
        while total <= maximum_bytes:
            chunk = os.read(descriptor, min(65536, maximum_bytes + 1 - total))
            if not chunk:
                break
            chunks.append(chunk)
            total += len(chunk)
        if total > maximum_bytes:
            raise ValueError("proc metadata exceeds bound")
        return b"".join(chunks)
    finally:
        os.close(descriptor)

# read one file through a pinned process root
def read_container_root(subject, maximum_bytes):
    descriptors = []
    try:
        pid = subject["pid"]
        if not isinstance(pid, int) or isinstance(pid, bool) or pid <= 0:
            return fail(subject["id"], "host.identity")
        proc_root = "/proc/" + str(pid)
        metadata_maximum = 4 * 1024 * 1024
        pidfd = os.pidfd_open(pid, 0)
        descriptors.append(pidfd)
        pidfd_info = read_proc_file("/proc/self/fdinfo/" + str(pidfd), metadata_maximum)
        stat_before = read_proc_file(proc_root + "/stat", metadata_maximum)
        cgroup_before = read_proc_file(proc_root + "/cgroup", metadata_maximum)
        mountinfo_before = read_proc_file(proc_root + "/mountinfo", metadata_maximum)
        namespace_fd = os.open(proc_root + "/ns/mnt", os.O_PATH)
        descriptors.append(namespace_fd)
        root_fd = os.open(proc_root + "/root", os.O_PATH | os.O_DIRECTORY)
        descriptors.append(root_fd)
        namespace_identity = identity(os.fstat(namespace_fd), "directory")
        root_identity = identity(os.fstat(root_fd), "directory")
        parent_fd = root_fd

        # walk every container path component beneath the pinned root
        for component in subject["relativeComponents"][:-1]:
            child_fd = os.open(component, os.O_PATH | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=parent_fd)
            descriptors.append(child_fd)
            if not stat.S_ISDIR(os.fstat(child_fd).st_mode):
                return fail(subject["id"], "host.binding")
            parent_fd = child_fd
        object_fd = os.open(subject["relativeComponents"][-1], os.O_PATH | os.O_NOFOLLOW, dir_fd=parent_fd)
        descriptors.append(object_fd)
        before_stat = os.fstat(object_fd)
        if not stat.S_ISREG(before_stat.st_mode) or before_stat.st_uid != subject["expectedUid"] or before_stat.st_gid != subject["expectedGid"] or format(stat.S_IMODE(before_stat.st_mode), "04o") != subject["expectedMode"]:
            return fail(subject["id"], "host.binding")
        readable_fd = os.open("/proc/self/fd/" + str(object_fd), os.O_RDONLY | os.O_NONBLOCK | os.O_NOCTTY)
        descriptors.append(readable_fd)
        chunks = []
        total = 0

        # read one byte beyond the fixed content cap
        while total <= maximum_bytes:
            chunk = os.read(readable_fd, min(65536, maximum_bytes + 1 - total))
            if not chunk:
                break
            chunks.append(chunk)
            total += len(chunk)
        if total > maximum_bytes:
            return fail(subject["id"], "host.bounds")
        after_stat = os.fstat(object_fd)
        readable_after = os.fstat(readable_fd)
        before = identity(before_stat)
        after = identity(after_stat)
        if before != after or before != identity(readable_after):
            return fail(subject["id"], "host.binding")
        stat_after = read_proc_file(proc_root + "/stat", metadata_maximum)
        cgroup_after = read_proc_file(proc_root + "/cgroup", metadata_maximum)
        mountinfo_after = read_proc_file(proc_root + "/mountinfo", metadata_maximum)
        start_before = proc_start_time(stat_before)
        start_after = proc_start_time(stat_after)
        if start_before != start_after or cgroup_before != cgroup_after or mountinfo_before != mountinfo_after or namespace_identity != identity(os.fstat(namespace_fd), "directory") or root_identity != identity(os.fstat(root_fd), "directory"):
            return fail(subject["id"], "host.identity")
        content = b"".join(chunks)
        return {
            "bytes": raw_bytes(content) if subject["readMode"] == "bytes" else None,
            "contentSha256": hashlib.sha256(content).hexdigest(),
            "errorCode": None,
            "executionIdentity": {
                "dropped": False,
                "gid": os.getegid(),
                "supplementaryGroups": sorted(os.getgroups()),
                "uid": os.geteuid(),
            },
            "id": subject["id"],
            "identity": {"post": after, "pre": before},
            "procBinding": {
                "cgroupAfter": raw_bytes(cgroup_after),
                "cgroupBefore": raw_bytes(cgroup_before),
                "containerId": subject["containerId"],
                "imageId": subject["imageId"],
                "mountNamespace": namespace_identity,
                "mountinfoAfter": raw_bytes(mountinfo_after),
                "mountinfoBefore": raw_bytes(mountinfo_before),
                "pid": pid,
                "pidfd": True,
                "pidfdInfo": raw_bytes(pidfd_info),
                "root": root_identity,
                "startTimeAfter": start_after,
                "startTimeBefore": start_before,
            },
            "releaseBinding": None,
            "status": "ok",
        }
    except (KeyError, OSError, TypeError, ValueError):
        return fail(subject.get("id", "invalid"), "host.identity")
    finally:
        close_all(descriptors)

# execute one closed helper request
def main():
    request = json.loads(sys.stdin.buffer.read().decode("utf-8"))
    subjects = []
    shared = {"descriptors": []}

    # execute only the two closed secure reader operations
    if request.get("contractVersion") != REQUEST_VERSION or request.get("operation") not in ("confined-files", "container-proc-root"):
        raise ValueError("unsupported secure reader request")
    try:
        # execute every subject through one pinned confined-root batch
        for subject in request["subjects"]:
            if request["operation"] == "confined-files":
                subjects.append(read_confined(subject, request["maximumBytes"], shared))
            else:
                subjects.append(read_container_root(subject, request["maximumBytes"]))
        if shared.get("rootSelector") == "release-root":
            markers_after = release_markers(shared["rootFd"], request["maximumBytes"])
            # attach one common closing marker snapshot to every successful release read
            for result in subjects:
                if result.get("releaseBinding") is not None:
                    result["releaseBinding"]["markersAfter"] = markers_after
        # fail the whole confined batch when the current root entry changed
        if request["operation"] == "confined-files" and not closing_root_matches(shared):
            subjects = [fail(subject["id"], "host.binding") for subject in request["subjects"]]
    finally:
        close_all(shared["descriptors"])
    now = datetime.datetime.now(datetime.timezone.utc).isoformat().replace("+00:00", "Z")
    result = {
        "authority": {"kind": "python-helper", "liveEligible": False},
        "completedAt": now,
        "startedAt": now,
        "subjects": subjects,
    }
    sys.stdout.write(json.dumps(result, ensure_ascii=False, separators=(",", ":"), sort_keys=True) + "\n")

# run only as the isolated helper program
if __name__ == "__main__":
    main()
`;
// weather-retained-v1-secure-reader-source:end
export const HOST_SECURE_READER_SHA256 = createHash("sha256")
  .update(HOST_SECURE_READER_SOURCE)
  .digest("hex");

const INVENTORY_SCHEMA_VERSION = "backend-runtime-inventory/v1";
const RUNTIME_MAP_SCHEMA_VERSION = "weather-backend-runtime-live-map/v1";
const PHASES = new Set(["baseline", "pre-cutover", "post-cutover", "rollback-drill"]);
const GENERATION_BY_PHASE = Object.freeze({
  baseline: "node-v1",
  "post-cutover": "rust-v1",
  "pre-cutover": "mixed",
  "rollback-drill": "node-v1-rollback",
});
const SERVICES = Object.freeze(["api", "cloudflared", "migration", "postgres", "web", "worker"]);
const IMAGE_ROLES = Object.freeze(["cloudflared", "postgres", "server", "web"]);
const IMAGE_ROLE_BY_SERVICE = Object.freeze({
  api: "server",
  cloudflared: "cloudflared",
  migration: "server",
  postgres: "postgres",
  web: "web",
  worker: "server",
});
const RUNNER_UNITS = Object.freeze([
  "weather-adjustment-archive.service",
  "weather-adjustment-daily.service",
  "weather-adjustment-daily.timer",
  "weather-adjustment-monthly.service",
  "weather-adjustment-monthly.timer",
]);
const REQUEST_KEYS = new Set([
  "contractVersion",
  "expectedHostRole",
  "expectedReleaseId",
  "expectedSchemaVersion",
  "inventory",
  "limits",
  "producerSha256",
  "profile",
  "runtimeMap",
]);
const MAX_LIMITS = Object.freeze({
  deadlineMs: 30_000,
  maxFiles: 2_048,
  maxProbeBytes: 4 * 1024 * 1024,
  maxProcesses: 4_096,
  maxTotalBytes: 64 * 1024 * 1024,
});
const MAX_OPAQUE_FILE_BYTES = 256 * 1024 * 1024;
const MAX_SOURCE_INPUT_BYTES = 32 * 1024 * 1024;
const SECRET_KEY_PATTERN = /(?:password|passwd|secret|token|authorization|private[_-]?key)/iu;
const DIGEST_PATTERN = /^[a-f0-9]{64}$/u;
const FIXED_ENVIRONMENT = Object.freeze({ LANG: "C" });
const TRUSTED_INVENTORY_PATH = join(import.meta.dirname, "..", "config", "backend-runtime-inventory.json");
const TRUSTED_RUNTIME_MAP_PATH = join(import.meta.dirname, "..", "config", "backend-runtime-live-map.json");
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
const DOCKER_CONTAINER_LIST_FORMAT = '{{.Label "com.docker.compose.service"}}\t{{.ID}}';

// retain the complete immutable v1 interpretation independently of later active protocol changes
const RETAINED_HOST_SEMANTIC_CONTEXT_V1 = Object.freeze({
  dockerContainerFormat: [
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
  ].join(","),
  dockerContainerListFormat: '{{.Label "com.docker.compose.service"}}\t{{.ID}}',
  dockerImageFormat: [
    "{\"architecture\":{{json .Architecture}}",
    "\"cmd\":{{json .Config.Cmd}}",
    "\"entrypoint\":{{json .Config.Entrypoint}}",
    "\"imageId\":{{json .Id}}",
    "\"os\":{{json .Os}}",
    "\"repoDigests\":{{json .RepoDigests}}",
    "\"revision\":{{json (index .Config.Labels \"org.opencontainers.image.revision\")}}}",
  ].join(","),
  evidenceVersion: "weather-backend-host-evidence/v1",
  imageRoleByService: Object.freeze({
    api: "server",
    cloudflared: "cloudflared",
    migration: "server",
    postgres: "postgres",
    web: "web",
    worker: "server",
  }),
  imageRoles: Object.freeze(["cloudflared", "postgres", "server", "web"]),
  inventoryVersion: "backend-runtime-inventory/v1",
  limits: Object.freeze({
    deadlineMs: 30_000,
    maxFiles: 2_048,
    maxProbeBytes: 4 * 1024 * 1024,
    maxProcesses: 4_096,
    maxTotalBytes: 64 * 1024 * 1024,
  }),
  observationVersion: "weather-backend-host-observation/v2",
  phases: new Set(["baseline", "pre-cutover", "post-cutover", "rollback-drill"]),
  probeVersion: "weather-backend-host-probe/v1",
  requestVersion: "weather-backend-host-evidence-request/v1",
  runnerUnits: Object.freeze([
    "weather-adjustment-archive.service",
    "weather-adjustment-daily.service",
    "weather-adjustment-daily.timer",
    "weather-adjustment-monthly.service",
    "weather-adjustment-monthly.timer",
  ]),
  runtimeMapVersion: "weather-backend-runtime-live-map/v1",
  services: Object.freeze(["api", "cloudflared", "migration", "postgres", "web", "worker"]),
});

// bind the active collector to the currently exported protocol constants
const ACTIVE_HOST_SEMANTIC_CONTEXT = Object.freeze({
  ...RETAINED_HOST_SEMANTIC_CONTEXT_V1,
  dockerContainerFormat: DOCKER_CONTAINER_FORMAT,
  dockerContainerListFormat: DOCKER_CONTAINER_LIST_FORMAT,
  dockerImageFormat: DOCKER_IMAGE_FORMAT,
  evidenceVersion: HOST_EVIDENCE_CONTRACT_VERSION,
  imageRoleByService: IMAGE_ROLE_BY_SERVICE,
  imageRoles: IMAGE_ROLES,
  inventoryVersion: INVENTORY_SCHEMA_VERSION,
  limits: MAX_LIMITS,
  observationVersion: HOST_OBSERVATION_VERSION,
  phases: PHASES,
  probeVersion: HOST_PROBE_VERSION,
  producer: HOST_PRODUCER,
  requestVersion: HOST_EVIDENCE_REQUEST_VERSION,
  runnerUnits: RUNNER_UNITS,
  runtimeMapVersion: RUNTIME_MAP_SCHEMA_VERSION,
  secureReaderSha256: HOST_SECURE_READER_SHA256,
  services: SERVICES,
});

// keep declarative alternatives outside physical host evidence
function contextRequiresPhysicalEvidence(context) {
  return context.activationSource !== "never-active-alternative";
}

// detect a phase that selects evidence outside the legacy observer
function phaseSelectsDeclarativeContext(context, phase) {
  const generation = GENERATION_BY_PHASE[phase];
  const selected = context.activationSourceByGeneration?.[generation];
  return !contextRequiresPhysicalEvidence(context) && selected !== undefined &&
    selected !== "never-active-alternative";
}

// build the finite planned fact catalog from the reviewed source map
function plannedProbeCatalog(source, profile, includeOperator = false, context = ACTIVE_HOST_SEMANTIC_CONTEXT) {
  const catalog = new Map();
  const add = (id, prerequisiteIds = []) => catalog.set(id, [...prerequisiteIds].sort());

  // retain the common host identity probes
  for (const id of [
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
  ]) {
    add(id);
  }

  // retain the fixed Blueberry fact families
  if (profile === "blueberry-compose") {
    add("docker.containers.list");
    add("release.current");
    add("release.previous");
    add("release.schema");
    add("systemd.weather-compose.service.show");
    add("systemd.weather-compose.service.files", ["systemd.weather-compose.service.show"]);

    // retain installed operator identities only for physical collection catalogs
    if (includeOperator) {
      add("operator.installation-receipt", ["systemd.weather-compose.service.show"]);
      add("operator.control-plane.files", ["operator.installation-receipt"]);
      add("operator.bundle.schema-preserving-release.mjs", ["operator.installation-receipt"]);
      add("operator.bundle.schema-preserving-release.sh", ["operator.installation-receipt"]);
      add("operator.weather-compose.dropin", ["systemd.weather-compose.service.show"]);
      add("operator.forced.dispatcher");
      add("operator.forced.remote-ops");
    }

    // retain every exact Compose service family
    for (const service of context.services) {
      const inspectId = `docker.container.${service}.inspect`;
      const topId = `docker.container.${service}.top`;
      add(inspectId, ["docker.containers.list"]);
      add(topId, [inspectId]);
      add(`docker.container.${service}.processes`, [inspectId, topId]);
      add(`cgroup.blueberry:compose:${service}`, [inspectId]);
    }

    // retain every exact selected runtime image family
    for (const role of context.imageRoles) {
      const representative = role === "server" ? "api" : role;
      add(`docker.image.${role}.inspect`, [`docker.container.${representative}.inspect`]);
    }
  } else {
    add("nss.ubuntu.passwd");
    add("runner.root.library", ["nss.ubuntu.passwd"]);
    add("runner.root.private", ["nss.ubuntu.passwd"]);
    add("runner.processes", ["nss.ubuntu.passwd"]);

    // retain every exact conditional runner unit family
    for (const unit of context.runnerUnits) {
      const showId = `systemd.${unit}.show`;
      add(showId);
      add(`systemd.${unit}.enabled`);
      add(`systemd.${unit}.files`, [showId]);
    }
  }

  // retain every exact per-context artifact installation
  for (const context of source.runtimeMap.contexts) {
    // skip other hosts and declarative-only alternatives
    if (context.profile !== profile || !contextRequiresPhysicalEvidence(context)) {
      continue;
    }

    // bind each artifact attempt to its activation prerequisite
    for (const artifactId of context.artifactIds) {
      const id = `artifact.${sha256Bytes(`${context.id}\0${artifactId}`)}`;
      const artifact = source.runtimeMap.artifacts.find(
        // select the exact source-owned artifact declaration
        (candidate) => candidate.id === artifactId,
      );
      const roleService = artifact?.imageTarget === "server" ? "api" : artifact?.imageTarget;
      const prerequisiteIds = profile === "workstation-runner"
        ? ["nss.ubuntu.passwd"]
        : artifact?.kind === "image-config"
          ? [`docker.image.${artifact.imageTarget}.inspect`]
          : ["container-file", "package-metadata"].includes(artifact?.kind)
            ? [`docker.container.${context.service === "migration"
              ? roleService
              : context.service ?? roleService}.inspect`]
          : context.service === "migration"
            ? ["docker.container.api.inspect"]
            : context.service === null
              ? ["docker.containers.list"]
              : [`docker.container.${context.service}.inspect`];
      add(id, prerequisiteIds);
    }
  }
  return catalog;
}

// construct one stable coded failure
function hostError(code, message) {
  const error = new Error(message);
  error.code = code;
  return error;
}

// reject ambiguous values before deterministic encoding
function canonicalizeHostValue(value, ancestors = new Set(), path = "$") {
  // retain portable primitive values
  if (value === null || typeof value === "string" || typeof value === "boolean") {
    return value;
  }
  // retain portable numbers only
  if (typeof value === "number") {
    // reject non-injective numeric encodings
    if (!Number.isFinite(value) || Object.is(value, -0) ||
      (Number.isInteger(value) && !Number.isSafeInteger(value))) {
      throw hostError("host.canonical", `ambiguous number at ${path}`);
    }
    return value;
  }
  // retain arrays without sparse or hidden values
  if (Array.isArray(value)) {
    // reject cycles before descending
    if (ancestors.has(value)) {
      throw hostError("host.canonical", `cyclic value at ${path}`);
    }
    const ownKeys = Reflect.ownKeys(value);

    // reject sparse hidden symbol accessor or extra array structure
    if (ownKeys.length !== value.length + 1 || ownKeys.some(
      // accept only dense numeric indexes and the intrinsic length
      (key) => key !== "length" && (typeof key !== "string" || !/^(?:0|[1-9][0-9]*)$/u.test(key) ||
        Number(key) >= value.length),
    )) {
      throw hostError("host.canonical", `ambiguous array structure at ${path}`);
    }
    // validate every dense array descriptor without invoking accessors
    for (let index = 0; index < value.length; index += 1) {
      const descriptor = Object.getOwnPropertyDescriptor(value, String(index));

      // require one enumerable data property per index
      if (descriptor === undefined || descriptor.enumerable !== true ||
        !("value" in descriptor) || "get" in descriptor || "set" in descriptor) {
        throw hostError("host.canonical", `ambiguous array member at ${path}[${String(index)}]`);
      }
    }
    const nextAncestors = new Set(ancestors);
    nextAncestors.add(value);
    const result = [];

    // canonicalize every ordered array member
    for (let index = 0; index < value.length; index += 1) {
      const descriptor = Object.getOwnPropertyDescriptor(value, String(index));
      result.push(canonicalizeHostValue(
        descriptor.value,
        nextAncestors,
        `${path}[${String(index)}]`,
      ));
    }
    return result;
  }
  // retain plain records with sorted keys
  if (value !== null && typeof value === "object" &&
    [Object.prototype, null].includes(Object.getPrototypeOf(value))) {
    // reject cycles before descending
    if (ancestors.has(value)) {
      throw hostError("host.canonical", `cyclic value at ${path}`);
    }
    const nextAncestors = new Set(ancestors);
    nextAncestors.add(value);
    const result = Object.create(null);
    const ownKeys = Reflect.ownKeys(value);

    // reject symbols magic keys hidden properties and accessors
    for (const key of ownKeys) {
      const descriptor = Object.getOwnPropertyDescriptor(value, key);

      // require one ordinary enumerable data property
      if (typeof key !== "string" || key === "__proto__" || descriptor === undefined ||
        descriptor.enumerable !== true || !("value" in descriptor) ||
        "get" in descriptor || "set" in descriptor) {
        throw hostError("host.canonical", `ambiguous object property at ${path}`);
      }
    }

    // canonicalize fields in lexical order
    for (const key of ownKeys.sort()) {
      const descriptor = Object.getOwnPropertyDescriptor(value, key);
      result[key] = canonicalizeHostValue(descriptor.value, nextAncestors, `${path}.${key}`);
    }
    return result;
  }
  throw hostError("host.canonical", `unsupported value at ${path}`);
}

// encode one deterministic host-evidence document
export function canonicalHostJson(value) {
  return JSON.stringify(canonicalizeHostValue(value));
}

// hash one canonical host-evidence document
export function hostEvidenceSha256(value) {
  return createHash("sha256").update(canonicalHostJson(value)).digest("hex");
}

// decode one byte sequence without replacement characters
function decodeUtf8Strict(value, code = "host.probe") {
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(Buffer.from(value));
  } catch {
    throw hostError(code, "bytes are not valid UTF-8");
  }
}

// skip insignificant JSON whitespace
function skipJsonWhitespace(text, offset) {
  let cursor = offset;

  // advance across JSON whitespace only
  while (cursor < text.length && /[\u0009\u000a\u000d\u0020]/u.test(text[cursor])) {
    cursor += 1;
  }
  return cursor;
}

// locate the end of one JSON string token
function jsonStringEnd(text, offset) {
  let cursor = offset + 1;

  // scan escaped code units without interpreting caller data
  while (cursor < text.length) {
    // close at the first unescaped quote
    if (text[cursor] === '"') {
      return cursor + 1;
    }
    // skip one escaped unit or unicode escape
    if (text[cursor] === "\\") {
      cursor += text[cursor + 1] === "u" ? 6 : 2;
    } else {
      cursor += 1;
    }
  }
  throw hostError("host.probe", "unterminated JSON string");
}

// scan one valid JSON value for duplicate object keys
function scanJsonValue(text, offset) {
  let cursor = skipJsonWhitespace(text, offset);

  // scan one object and reject decoded duplicate keys
  if (text[cursor] === "{") {
    cursor = skipJsonWhitespace(text, cursor + 1);
    const keys = new Set();

    // accept one empty object
    if (text[cursor] === "}") {
      return cursor + 1;
    }

    // scan every validated object member
    while (cursor < text.length) {
      const end = jsonStringEnd(text, cursor);
      const key = JSON.parse(text.slice(cursor, end));

      // reject duplicate decoded property identities
      if (keys.has(key)) {
        throw hostError("host.probe", "JSON contains a duplicate object key");
      }
      keys.add(key);
      cursor = skipJsonWhitespace(text, end);

      // require the already-validated member separator
      if (text[cursor] !== ":") {
        throw hostError("host.probe", "JSON object separator is invalid");
      }
      cursor = scanJsonValue(text, cursor + 1);
      cursor = skipJsonWhitespace(text, cursor);

      // finish or advance to the next member
      if (text[cursor] === "}") {
        return cursor + 1;
      }
      cursor = skipJsonWhitespace(text, cursor + 1);
    }
  }

  // scan one array recursively
  if (text[cursor] === "[") {
    cursor = skipJsonWhitespace(text, cursor + 1);

    // accept one empty array
    if (text[cursor] === "]") {
      return cursor + 1;
    }

    // scan every validated array member
    while (cursor < text.length) {
      cursor = scanJsonValue(text, cursor);
      cursor = skipJsonWhitespace(text, cursor);

      // finish or advance to the next member
      if (text[cursor] === "]") {
        return cursor + 1;
      }
      cursor = skipJsonWhitespace(text, cursor + 1);
    }
  }

  // skip one validated string primitive
  if (text[cursor] === '"') {
    return jsonStringEnd(text, cursor);
  }

  // skip one validated number or keyword primitive
  while (cursor < text.length && !/[\u0009\u000a\u000d\u0020,}\]]/u.test(text[cursor])) {
    cursor += 1;
  }
  return cursor;
}

// parse strict UTF-8 JSON and reject duplicate keys
function parseStrictJson(value, code = "host.probe") {
  const text = typeof value === "string" ? value : decodeUtf8Strict(value, code);
  let parsed;

  // use the platform parser as the authoritative JSON grammar
  try {
    parsed = JSON.parse(text);
  } catch {
    throw hostError(code, "JSON bytes are invalid");
  }

  // run the bounded duplicate-key detector after grammar validation
  try {
    const completed = skipJsonWhitespace(text, scanJsonValue(text, 0));

    // require the scanner to consume the exact parsed document
    if (completed !== text.length) {
      throw hostError(code, "JSON trailing bytes are invalid");
    }
  } catch (error) {
    throw hostError(code, error?.message ?? "JSON structure is ambiguous");
  }
  return parsed;
}

// identify plain records only
function isRecord(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

// hash one exact byte sequence
function sha256Bytes(value) {
  return createHash("sha256").update(value).digest("hex");
}

// require one exact closed key set
function requireExactKeys(value, keys, code, path) {
  // reject non-records before key comparison
  if (!isRecord(value)) {
    throw hostError(code, `${path} must be an object`);
  }
  const actual = Object.keys(value).sort();
  const expected = [...keys].sort();

  // reject omitted or hidden request authority
  if (canonicalHostJson(actual) !== canonicalHostJson(expected)) {
    throw hostError(code, `${path} has unsupported or missing fields`);
  }
}

// compare one record against an exact closed key set
function hasExactKeys(value, keys) {
  return isRecord(value) && canonicalHostJson(Object.keys(value).sort()) ===
    canonicalHostJson([...keys].sort());
}

// recognize one bounded positive integer
function isBoundedInteger(value, maximum) {
  return Number.isSafeInteger(value) && value > 0 && value <= maximum;
}

// validate one canonical relative path component list
function validateSecureComponents(components) {
  return Array.isArray(components) && components.length > 0 && components.every(
    // reject traversal separators NUL and noncanonical component names
    (component) => typeof component === "string" && component.length > 0 &&
      component !== "." && component !== ".." && !component.includes("/") &&
      !component.includes("\0"),
  );
}

// validate one secure reader identity record
function validateSecureIdentity(value, expected, path) {
  const decimal = /^(?:0|[1-9][0-9]*)$/u;

  // require one closed portable stat identity
  if (!hasExactKeys(value, [
    "ctimeNs",
    "dev",
    "gid",
    "ino",
    "mode",
    "mtimeNs",
    "nlink",
    "size",
    "type",
    "uid",
  ]) || ![value.ctimeNs, value.dev, value.ino, value.mtimeNs, value.nlink, value.size].every(
    // require one unsigned decimal scalar
    (item) => typeof item === "string" && decimal.test(item),
  ) || !Number.isSafeInteger(value.uid) || !Number.isSafeInteger(value.gid) ||
    value.type !== "regular-file" || value.uid !== expected.expectedUid ||
    value.gid !== expected.expectedGid || value.mode !== expected.expectedMode) {
    throw hostError("host.binding", `${path} identity is invalid`);
  }
}

// validate one portable pinned directory identity
function validateSecureDirectoryIdentity(value, path) {
  const decimal = /^(?:0|[1-9][0-9]*)$/u;

  // require one closed directory stat identity
  if (!hasExactKeys(value, [
    "ctimeNs",
    "dev",
    "gid",
    "ino",
    "mode",
    "mtimeNs",
    "nlink",
    "size",
    "type",
    "uid",
  ]) || ![value.ctimeNs, value.dev, value.ino, value.mtimeNs, value.nlink, value.size].every(
    // require one unsigned decimal scalar
    (item) => typeof item === "string" && decimal.test(item),
  ) || !Number.isSafeInteger(value.uid) || !Number.isSafeInteger(value.gid) ||
    value.type !== "directory" || !/^[0-7]{4}$/u.test(value.mode ?? "")) {
    throw hostError("host.identity", `${path} directory identity is invalid`);
  }
}

// validate one complete process-root authority binding
function validateSecureProcBinding(binding, expected) {
  // require the fixed proc-root result shape
  if (!hasExactKeys(binding, [
    "cgroupAfter",
    "cgroupBefore",
    "containerId",
    "imageId",
    "mountNamespace",
    "mountinfoAfter",
    "mountinfoBefore",
    "pid",
    "pidfd",
    "pidfdInfo",
    "root",
    "startTimeAfter",
    "startTimeBefore",
  ]) || binding.containerId !== expected.containerId ||
    binding.imageId !== expected.imageId || binding.pid !== expected.pid ||
    binding.pidfd !== true || !/^(?:0|[1-9][0-9]*)$/u.test(binding.startTimeBefore ?? "") ||
    binding.startTimeBefore !== binding.startTimeAfter) {
    throw hostError("host.identity", "secure reader process identity differs");
  }
  validateSecureDirectoryIdentity(binding.mountNamespace, "mount namespace");
  validateSecureDirectoryIdentity(binding.root, "process root");
  const diagnostics = [];
  const cgroupBefore = decodeRawField(binding.cgroupBefore, "$.proc.cgroupBefore", diagnostics);
  const cgroupAfter = decodeRawField(binding.cgroupAfter, "$.proc.cgroupAfter", diagnostics);
  const mountinfoBefore = decodeRawField(binding.mountinfoBefore, "$.proc.mountinfoBefore", diagnostics);
  const mountinfoAfter = decodeRawField(binding.mountinfoAfter, "$.proc.mountinfoAfter", diagnostics);
  const pidfdInfo = decodeRawField(binding.pidfdInfo, "$.proc.pidfdInfo", diagnostics);

  // bind every proc semantic source to unchanged exact bytes
  if (diagnostics.length > 0 || [
    cgroupBefore,
    cgroupAfter,
    mountinfoBefore,
    mountinfoAfter,
    pidfdInfo,
  ].some(
    // reject one unavailable proc source
    (bytes) => bytes === null,
  ) || !cgroupBefore.equals(cgroupAfter) || !mountinfoBefore.equals(mountinfoAfter)) {
    throw hostError("host.identity", "secure reader process sources changed");
  }
}

// recognize one exact production confined-reader domain
function validateProductionSecureSubject(profile, subject) {
  const relativePath = subject.relativeComponents.join("/");

  // admit only reviewed systemd unit locations
  if (subject.rootSelector === "systemd-root") {
    return ["blueberry-compose", "workstation-runner"].includes(profile) &&
      subject.rootPath === "/" &&
      /^(?:etc|usr\/lib|lib)\/systemd\/system\/weather-[A-Za-z0-9.-]+(?:\.service|\.timer|\.service\.d\/10-schema-preserving-release\.conf)$/u.test(relativePath);
  }

  // admit only the installed operator mirrors and immutable bundle members
  if (subject.rootSelector === "operator-root") {
    return profile === "blueberry-compose" && subject.rootPath === "/" && (
      relativePath === "usr/local/bin/weather-ssh-dispatch" ||
      relativePath === "usr/local/sbin/weather-remote-ops" ||
      /^opt\/weather\/operators\/schema-preserving-release\/[a-f0-9]{64}\/schema-preserving-release\.(?:mjs|sh)$/u.test(relativePath)
    );
  }

  // admit only reviewed release-tree state and deployed runtime paths
  if (subject.rootSelector === "release-root") {
    return ["blueberry-compose", "workstation-runner"].includes(profile) &&
      subject.rootPath === "/opt/weather/current" && (
        (subject.id === "operator.control-plane.files" && relativePath === "deploy" &&
          subject.expectedType === "directory" && subject.readMode === "tree") ||
        /^deploy\/state\/(?:current-release|previous-release|schema-release)$/u.test(relativePath) ||
        /^deploy\/state\/schema-preserving-release-v1\/(?:installation\.json|installations\/[a-f0-9]{64}\.json)$/u.test(relativePath) ||
        ["deploy/compose.local.yaml", "deploy/compose.yaml"].includes(relativePath) ||
        /^deploy\/scripts\/[A-Za-z0-9._-]+$/u.test(relativePath) ||
        /^apps\/(?:api|worker)\/dist\/[A-Za-z0-9._-]+\.js$/u.test(relativePath)
      );
  }

  // admit only exact runner installer trees beneath the proven NSS home
  if (subject.rootSelector === "runner-home") {
    return profile === "workstation-runner" &&
      subject.rootPath === "/home/ubuntu" && (
      relativePath === ".weather/adjustment-maintenance/runner" ||
      relativePath === ".local/lib/weather-adjustment-maintenance" ||
      relativePath.startsWith(".weather/adjustment-maintenance/runner/current/") ||
      relativePath.startsWith(".local/lib/weather-adjustment-maintenance/current/")
    );
  }
  return false;
}

// validate one helper execution identity against the selected privilege domain
function validateSecureExecutionIdentity(identity, expected) {
  if (!hasExactKeys(identity, ["dropped", "gid", "supplementaryGroups", "uid"]) ||
    !Number.isSafeInteger(identity.uid) || !Number.isSafeInteger(identity.gid) ||
    !Array.isArray(identity.supplementaryGroups) || identity.supplementaryGroups.some(
      // require numeric supplementary group identities
      (group) => !Number.isSafeInteger(group),
    )) {
    throw hostError("host.identity", "secure reader execution identity is invalid");
  }

  // require an exact nonroot privilege drop for runner-home content
  if (expected.rootSelector === "runner-home") {
    if (identity.dropped !== true || expected.expectedUid <= 0 || expected.expectedGid <= 0 ||
      identity.uid !== expected.expectedUid || identity.gid !== expected.expectedGid ||
      identity.supplementaryGroups.length !== 0) {
      throw hostError("host.identity", "runner secure reader privilege identity differs");
    }
    return;
  }

  // privileged and synthetic readers must report their actual unchanged helper identity
  if (identity.dropped !== false) {
    throw hostError("host.identity", "secure reader reported an unexpected privilege drop");
  }
}

// validate one retained release snapshot authority
function validateSecureReleaseBinding(binding, expected) {
  const needsRelease = expected.rootSelector === "release-root" ||
    (expected.rootSelector === "runner-home" && expected.relativeComponents.includes("current"));

  // reject missing or unrelated release authority
  if (!needsRelease) {
    if (binding !== null) {
      throw hostError("host.binding", "secure reader returned an unrelated release binding");
    }
    return;
  }
  if (!hasExactKeys(binding, [
    "anchor",
    "currentTarget",
    "markersAfter",
    "markersBefore",
    "releaseRoot",
  ])) {
    throw hostError("host.binding", "secure reader release binding is invalid");
  }
  validateSecureDirectoryIdentity(binding.anchor, "release anchor");
  validateSecureDirectoryIdentity(binding.releaseRoot, "release root");

  // validate the three immutable Blueberry release markers
  if (expected.rootSelector === "release-root") {
    const expectedIds = ["current-release", "previous-release", "schema-release"];
    for (const markerSet of [binding.markersBefore, binding.markersAfter]) {
      if (!Array.isArray(markerSet) || canonicalHostJson(markerSet.map(
        // retain exact marker ordering
        (marker) => marker?.id,
      )) !== canonicalHostJson(expectedIds)) {
        throw hostError("host.binding", "secure reader release markers are incomplete");
      }
      for (const marker of markerSet) {
        const diagnostics = [];
        const bytes = decodeRawField(marker.bytes, "$.release.marker", diagnostics);

        // reject malformed marker bytes before semantic use
        if (bytes === null || diagnostics.length > 0 || bytes.length === 0) {
          throw hostError("host.binding", "secure reader release marker bytes are invalid");
        }
      }
    }
    if (binding.currentTarget !== null ||
      canonicalHostJson(binding.markersBefore) !== canonicalHostJson(binding.markersAfter)) {
      throw hostError("host.binding", "secure reader release markers changed");
    }
    return;
  }

  // bind runner current only to the immutable forty-hex release target
  if (!Array.isArray(binding.currentTarget) || binding.currentTarget.length !== 2 ||
    binding.currentTarget[0] !== "releases" || !/^[a-f0-9]{40}$/u.test(binding.currentTarget[1] ?? "") ||
    !Array.isArray(binding.markersBefore) || binding.markersBefore.length !== 0 ||
    !Array.isArray(binding.markersAfter) || binding.markersAfter.length !== 0) {
    throw hostError("host.binding", "runner current release binding is invalid");
  }
}

// validate one helper request before any child execution
async function validateSecureReaderRequest(request, injected, allowProduction) {
  const admittedRoots = new Map();
  requireExactKeys(
    request,
    new Set(["contractVersion", "maximumBytes", "operation", "profile", "subjects"]),
    "host.binding",
    "secure reader request",
  );

  // require one bounded closed operation and subject set
  if (request.contractVersion !== HOST_SECURE_READER_REQUEST_VERSION ||
    !["confined-files", "container-proc-root"].includes(request.operation) ||
    !isBoundedInteger(request.maximumBytes, MAX_OPAQUE_FILE_BYTES) ||
    !Array.isArray(request.subjects) || request.subjects.length === 0 ||
    request.subjects.length > 2048) {
    throw hostError("host.binding", "secure reader request is invalid");
  }

  // validate every operation-specific subject without selecting new paths
  for (const subject of request.subjects) {
    // validate one confined file subject
    if (request.operation === "confined-files") {
      const subjectKeys = new Set([
        "expectedGid",
        "expectedMode",
        "expectedUid",
        "id",
        "readMode",
        "relativeComponents",
        "rootPath",
        "rootSelector",
      ]);

      // admit absence only for the immutable receipt or fixed runner roots
      if (subject.allowAbsent === true) {
        subjectKeys.add("allowAbsent");
      }
      // admit an explicit directory type only for fixed root-status probes
      if (subject.expectedType !== undefined) {
        subjectKeys.add("expectedType");
      }
      requireExactKeys(subject, subjectKeys, "host.binding", "secure reader subject");

      const controlled = injected && request.profile === "synthetic-test" &&
        subject.rootSelector === "controlled-test-root";
      const production = allowProduction && validateProductionSecureSubject(request.profile, subject);

      // permit only one controlled test root or one fixed production domain
      if ((!controlled && !production) ||
        typeof subject.rootPath !== "string" ||
        !["bytes", "metadata", "sha256", "tree"].includes(subject.readMode) ||
        ![undefined, "directory", "regular-file"].includes(subject.expectedType) ||
        !validateSecureComponents(subject.relativeComponents) ||
        !Number.isSafeInteger(subject.expectedUid) ||
        !Number.isSafeInteger(subject.expectedGid) ||
        !/^[0-7]{4}$/u.test(subject.expectedMode ?? "") ||
        (subject.allowAbsent === true && !(
          (subject.id === "operator.installation-receipt.immutable" &&
            /^deploy\/state\/schema-preserving-release-v1\/installations\/[a-f0-9]{64}\.json$/u.test(
              subject.relativeComponents.join("/"),
            )) ||
          (["runner.root.private", "runner.root.library"].includes(subject.id) &&
            subject.expectedType === "directory" && subject.readMode === "metadata")
        ))) {
        throw hostError("host.binding", "secure reader subject escaped its fixed domain");
      }
      // bind synthetic roots in JavaScript because production roots are Python-confined
      if (controlled) {
        if (!subject.rootPath.startsWith("/tmp/weather-host-secure-")) {
          throw hostError("host.binding", "controlled secure reader root is invalid");
        }
        const rootDetails = await lstat(subject.rootPath);
        const resolvedRoot = await realpath(subject.rootPath);

        // bind the synthetic root to the current nonwritable test account directory
        if (!rootDetails.isDirectory() || rootDetails.isSymbolicLink() ||
          rootDetails.uid !== process.getuid() || rootDetails.gid !== process.getgid() ||
          (rootDetails.mode & 0o022) !== 0 || resolvedRoot !== subject.rootPath) {
          throw hostError("host.binding", "controlled secure reader root is invalid");
        }
        admittedRoots.set(subject.rootPath, {
          dev: String(rootDetails.dev),
          ino: String(rootDetails.ino),
        });
      }
    } else {
      requireExactKeys(subject, new Set([
        "containerId",
        "expectedGid",
        "expectedMode",
        "expectedUid",
        "id",
        "imageId",
        "pid",
        "readMode",
        "relativeComponents",
      ]), "host.identity", "container secure reader subject");

      const syntheticContainer = injected && !allowProduction && request.profile === "synthetic-test";
      const plannedContainer = allowProduction && request.profile === "blueberry-compose";

      // reject PID zero and every caller-selected physical container root
      if ((!syntheticContainer && !plannedContainer) ||
        !Number.isSafeInteger(subject.pid) || subject.pid <= 0 ||
        !Number.isSafeInteger(subject.expectedUid) || subject.expectedUid < 0 ||
        !Number.isSafeInteger(subject.expectedGid) || subject.expectedGid < 0 ||
        !/^[0-7]{4}$/u.test(subject.expectedMode ?? "") ||
        typeof subject.id !== "string" || subject.id.length === 0 ||
        typeof subject.containerId !== "string" || subject.containerId.length === 0 ||
        typeof subject.imageId !== "string" || subject.imageId.length === 0 ||
        !validateSecureComponents(subject.relativeComponents) ||
        !["bytes", "sha256"].includes(subject.readMode)) {
        throw hostError("host.identity", "container secure reader identity is invalid");
      }
    }
  }
  return admittedRoots;
}

// revalidate every admitted synthetic root after helper settlement
async function revalidateSecureReaderRoots(admittedRoots) {
  // compare exact directory identity after the child has fully exited
  for (const [rootPath, expected] of admittedRoots) {
    let details;

    // treat disappearance or substitution as one binding failure
    try {
      details = await lstat(rootPath);
    } catch {
      throw hostError("host.binding", "secure reader root changed during execution");
    }

    // require the same non-link directory inode after collection
    if (!details.isDirectory() || details.isSymbolicLink() ||
      String(details.dev) !== expected.dev || String(details.ino) !== expected.ino) {
      throw hostError("host.binding", "secure reader root changed during execution");
    }
  }
}

// validate and normalize one successful helper result
function validateSecureReaderResult(rawResult, request, injected) {
  // require one closed raw helper envelope
  if (!hasExactKeys(rawResult, ["authority", "completedAt", "startedAt", "subjects"]) ||
    !Array.isArray(rawResult.subjects) || rawResult.subjects.length !== request.subjects.length) {
    throw hostError("host.probe", "secure reader result is invalid");
  }
  const normalized = [];

  // validate every returned subject against its source-owned request
  for (let index = 0; index < request.subjects.length; index += 1) {
    const expected = request.subjects[index];
    const subject = rawResult.subjects[index];

    // require one closed helper subject envelope
    if (!hasExactKeys(subject, [
      "bytes",
      "contentSha256",
      "errorCode",
      "executionIdentity",
      "id",
      "identity",
      "procBinding",
      "releaseBinding",
      "status",
    ]) || subject.id !== expected.id) {
      throw hostError("host.probe", "secure reader subject is invalid");
    }

    // retain only the explicitly admitted immutable-receipt absence outcome
    if (subject.status === "absent" && expected.allowAbsent === true) {
      if (subject.errorCode !== "ENOENT" || subject.bytes !== null ||
        subject.contentSha256 !== null || subject.identity !== null ||
        subject.procBinding !== null || subject.releaseBinding !== null) {
        throw hostError("host.binding", "secure reader absence result is invalid");
      }
      validateSecureExecutionIdentity(subject.executionIdentity, expected);
      normalized.push({
        bytes: null,
        contentSha256: null,
        executionIdentity: subject.executionIdentity,
        id: subject.id,
        identity: null,
        procBinding: null,
        releaseBinding: null,
        status: "absent",
      });
      continue;
    }

    // propagate only a fixed sanitized helper failure code
    if (subject.status !== "ok") {
      throw hostError(
        ["host.binding", "host.bounds", "host.identity"].includes(subject.errorCode)
          ? subject.errorCode
          : "host.probe",
        "secure reader rejected one subject",
      );
    }

    // require unchanged descriptor identity before accepting content
    if (!hasExactKeys(subject.identity, ["post", "pre"]) ||
      canonicalHostJson(subject.identity.pre) !== canonicalHostJson(subject.identity.post)) {
      throw hostError("host.binding", "secure reader descriptor identity changed");
    }
    // validate one regular file or fixed directory identity
    if (expected.expectedType === "directory") {
      validateSecureDirectoryIdentity(subject.identity.pre, "secure reader pre");
      validateSecureDirectoryIdentity(subject.identity.post, "secure reader post");
      const directoryBytes = expected.readMode === "tree"
        ? decodeRawField(subject.bytes, "$.secure.directory.bytes", [])
        : null;
      if (subject.identity.pre.uid !== expected.expectedUid ||
        subject.identity.pre.gid !== expected.expectedGid ||
        subject.identity.pre.mode !== expected.expectedMode ||
        (expected.readMode !== "tree" && (subject.bytes !== null || subject.contentSha256 !== null)) ||
        (expected.readMode === "tree" && (directoryBytes === null ||
          sha256Bytes(directoryBytes) !== subject.contentSha256))) {
        throw hostError("host.binding", "secure reader directory identity differs");
      }
    } else {
      validateSecureIdentity(subject.identity.pre, expected, "secure reader pre");
      validateSecureIdentity(subject.identity.post, expected, "secure reader post");
    }
    validateSecureExecutionIdentity(subject.executionIdentity, expected);

    // bind operation-specific authority rather than accepting caller labels
    if (request.operation === "container-proc-root") {
      validateSecureProcBinding(subject.procBinding, expected);
      if (subject.releaseBinding !== null) {
        throw hostError("host.identity", "container reader returned a release binding");
      }
    } else {
      if (subject.procBinding !== null) {
        throw hostError("host.binding", "confined reader returned process authority");
      }
      validateSecureReleaseBinding(subject.releaseBinding, expected);
    }
    let bytes = null;

    // validate every semantic nested raw byte field before outer retention
    if (["bytes", "tree"].includes(expected.readMode)) {
      const diagnostics = [];
      bytes = decodeRawField(subject.bytes, "$.secureReader.bytes", diagnostics);

      // reject malformed secret-bearing or truncated helper content
      if (bytes === null || diagnostics.length > 0 || containsSecretBytes(bytes)) {
        throw hostError(containsSecretBytes(bytes ?? Buffer.alloc(0)) ? "host.secret" : "host.probe", "secure reader bytes are invalid");
      }
    } else if (subject.bytes !== null) {
      throw hostError("host.probe", "digest-only secure reader retained content bytes");
    }

    // bind the opaque content digest independently of retention mode
    if ((expected.readMode === "metadata" && subject.contentSha256 !== null) ||
      (expected.readMode !== "metadata" && (!DIGEST_PATTERN.test(subject.contentSha256 ?? "") ||
        (bytes !== null && sha256Bytes(bytes) !== subject.contentSha256)))) {
      throw hostError("host.digest", "secure reader content digest differs");
    }
    normalized.push({
      bytes: subject.bytes,
      contentSha256: subject.contentSha256,
      executionIdentity: subject.executionIdentity,
      id: subject.id,
      identity: subject.identity,
      procBinding: subject.procBinding,
      releaseBinding: subject.releaseBinding,
      status: subject.status,
    });
  }

  // reject helper-side authority promotion after validating descriptor integrity
  if (!hasExactKeys(rawResult.authority, ["kind", "liveEligible"]) ||
    rawResult.authority.kind !== "python-helper" || rawResult.authority.liveEligible !== false) {
    throw hostError("host.probe", "secure reader helper authority is invalid");
  }
  return {
    authority: {
      kind: injected ? "injected-test" : "physical-python",
      liveEligible: !injected,
    },
    completedAt: rawResult.completedAt,
    contractVersion: HOST_SECURE_READER_RESULT_VERSION,
    helperSha256: HOST_SECURE_READER_SHA256,
    operation: request.operation,
    startedAt: rawResult.startedAt,
    subjects: normalized,
  };
}

// execute one fixed secure reader request
async function executeBackendHostSecureReader(request, adapters, allowProduction) {
  const injected = adapters !== undefined;
  const admittedRoots = await validateSecureReaderRequest(request, injected, allowProduction);
  const secureExecutor = adapters?.secureExecutor ?? productionSecureExecutor;
  const input = Buffer.from(`${canonicalHostJson(request)}\n`);
  const maximumStdoutBytes = Math.min(
    32 * 1024 * 1024,
    request.maximumBytes * request.subjects.length + 1024 * 1024,
  );
  const abortController = new AbortController();
  const deadlineMs = 10_000;
  const deadline = setTimeout(
    // abort the fixed helper after the closed deadline
    () => abortController.abort(),
    deadlineMs,
  );
  deadline.unref?.();
  let execution;

  // execute and settle the exact reviewed helper child
  try {
    execution = await secureExecutor(
      HOST_SECURE_READER_PROGRAM,
      ["-I", "-B", "-S", "-c", HOST_SECURE_READER_SOURCE],
      {
        cwd: "/",
        deadlineMs,
        env: { LANG: "C" },
        maximumStderrBytes: 64 * 1024,
        maximumStdoutBytes,
        signal: abortController.signal,
        stdin: input,
      },
    );
  } catch {
    throw hostError("host.probe", "secure reader execution failed");
  } finally {
    clearTimeout(deadline);
  }
  await revalidateSecureReaderRoots(admittedRoots);

  // reject failed noisy or oversized helper results
  if (!isRecord(execution) || execution.exitCode !== 0 ||
    !Buffer.isBuffer(execution.stdout) || !Buffer.isBuffer(execution.stderr) ||
    execution.stderr.length !== 0 || execution.stderr.length > 64 * 1024 ||
    execution.stdout.length > maximumStdoutBytes) {
    throw hostError("host.probe", "secure reader process result is invalid");
  }
  const text = decodeUtf8Strict(execution.stdout, "host.probe");
  const rawResult = parseStrictJson(text, "host.probe");

  // require one exact canonical helper result line
  if (`${canonicalHostJson(rawResult)}\n` !== text) {
    throw hostError("host.probe", "secure reader output is not canonical");
  }
  return validateSecureReaderResult(rawResult, request, injected);
}

// expose only the explicitly injected synthetic secure-reader test facade
export async function runBackendHostSecureReader(request, adapters) {
  // prohibit every direct physical or production-selector invocation
  if (adapters === undefined || typeof adapters?.secureExecutor !== "function" ||
    request?.profile !== "synthetic-test") {
    throw hostError("host.binding", "secure reader facade is synthetic-only");
  }
  return await executeBackendHostSecureReader(request, adapters, false);
}

// create the physical command adapter
function productionExecutor(program, args, options) {
  return new Promise((resolveResult) => {
    let settled = false;
    let child;
    let killTimer;

    // terminate the bounded child on the global collection abort
    const abortChild = () => {
      // avoid signaling an already reaped process
      if (!settled) {
        child.kill("SIGTERM");
        killTimer = setTimeout(
          // force cleanup of a child that ignored the bounded termination
          () => child.kill("SIGKILL"),
          250,
        );
      }
    };

    // execute one fixed absolute program without a shell
    child = execFile(program, args, {
      cwd: options.cwd,
      encoding: "buffer",
      env: options.env,
      maxBuffer: options.maxBytes,
      timeout: options.deadlineMs,
    }, (error, stdout, stderr) => {
      settled = true;
      clearTimeout(killTimer);
      options.signal.removeEventListener("abort", abortChild);
      resolveResult({
        exitCode: error === null ? 0 : Number.isInteger(error.code) ? error.code : 1,
        stderr: Buffer.isBuffer(stderr) ? stderr : Buffer.from(stderr ?? ""),
        stdout: Buffer.isBuffer(stdout) ? stdout : Buffer.from(stdout ?? ""),
      });
    });
    options.signal.addEventListener("abort", abortChild, { once: true });

    // close the registration race with an already aborted signal
    if (options.signal.aborted) {
      abortChild();
    }
  });
}

// execute the fixed isolated helper with bounded standard streams
function productionSecureExecutor(program, args, options) {
  return new Promise((resolveResult) => {
    let settled = false;
    let child;
    let killTimer;

    // terminate the helper on the bounded abort signal
    const abortChild = () => {
      // avoid signaling an already reaped helper
      if (!settled) {
        child.kill("SIGTERM");
        killTimer = setTimeout(
          // force cleanup when the helper ignores termination
          () => child.kill("SIGKILL"),
          250,
        );
      }
    };

    // start only the fixed no-shell helper invocation
    child = execFile(program, args, {
      cwd: options.cwd,
      encoding: "buffer",
      env: options.env,
      maxBuffer: Math.max(options.maximumStdoutBytes, options.maximumStderrBytes),
      timeout: options.deadlineMs,
    }, (error, stdout, stderr) => {
      settled = true;
      clearTimeout(killTimer);
      options.signal.removeEventListener("abort", abortChild);
      resolveResult({
        exitCode: error === null ? 0 : Number.isInteger(error.code) ? error.code : 1,
        stderr: Buffer.isBuffer(stderr) ? stderr : Buffer.from(stderr ?? ""),
        stdout: Buffer.isBuffer(stdout) ? stdout : Buffer.from(stdout ?? ""),
      });
    });
    options.signal.addEventListener("abort", abortChild, { once: true });
    child.stdin.end(options.stdin);

    // close the registration race with an already aborted signal
    if (options.signal.aborted) {
      abortChild();
    }
  });
}

// create the physical built-in adapter set
function productionAdapters() {
  return {
    executor: productionExecutor,
    filesystem: { lstat, open, opendir, readlink, realpath, statfs },
    secureExecutor: productionSecureExecutor,
    clock: {
      // read one wall-clock instant
      now() {
        return new Date();
      },
      // read one monotonic instant
      monotonicMs() {
        return Number(process.hrtime.bigint() / 1_000_000n);
      },
    },
  };
}

// execute one source-owned secure-reader batch through the collection authority
async function collectSecureReaderBatch(state, request) {
  const adapters = state.injected
    ? typeof state.adapters.secureExecutor === "function"
      ? { secureExecutor: state.adapters.secureExecutor }
      : null
    : undefined;

  // never fall back from an injected fixture into physical privileged I/O
  if (adapters === null) {
    state.complete = false;
    return null;
  }
  try {
    const result = await executeBackendHostSecureReader(request, adapters, true);
    enforceDeadline(state);
    return result;
  } catch {
    state.complete = false;
    return null;
  }
}

// map one fixed host path to its confined-reader source domain
function secureHostSubject(state, id, path, options = {}) {
  const runnerHome = state.runnerAccount?.home ?? null;
  let rootPath;
  let rootSelector;
  let relativePath;

  // select the exact systemd root domain
  if (/^\/(?:etc|usr\/lib|lib)\/systemd\/system\//u.test(path)) {
    rootPath = "/";
    rootSelector = "systemd-root";
    relativePath = path.slice(1);
  } else if (/^\/usr\/local\/(?:bin|sbin)\/weather/u.test(path) ||
    /^\/opt\/weather\/operators\/schema-preserving-release\/[a-f0-9]{64}\//u.test(path)) {
    rootPath = "/";
    rootSelector = "operator-root";
    relativePath = path.slice(1);
  } else if (path.startsWith("/opt/weather/current/")) {
    rootPath = "/opt/weather/current";
    rootSelector = "release-root";
    relativePath = path.slice("/opt/weather/current/".length);
  } else if (runnerHome !== null && path.startsWith(`${runnerHome}/`)) {
    rootPath = runnerHome;
    rootSelector = "runner-home";
    relativePath = path.slice(`${runnerHome}/`.length);
  } else {
    return null;
  }
  const expectedUid = rootSelector === "runner-home" ? state.runnerAccount.uid : options.uid ?? 0;
  const expectedGid = rootSelector === "runner-home" ? state.runnerAccount.gid : options.gid ?? 0;
  const subject = {
    expectedGid,
    expectedMode: options.mode ?? "0644",
    expectedUid,
    id,
    readMode: options.readMode ?? "sha256",
    relativeComponents: relativePath.split("/"),
    rootPath,
    rootSelector,
  };

  // request directory metadata only for fixed root-status probes
  if (options.expectedType === "directory") {
    subject.expectedType = "directory";
  }

  // enable only an explicitly selected immutable-receipt absence outcome
  if (options.allowAbsent === true) {
    subject.allowAbsent = true;
  }
  return subject;
}

// decode one already validated secure-reader byte subject
function secureSubjectBytes(subject) {
  const diagnostics = [];
  const bytes = decodeRawField(subject.bytes, "$.secure.subject.bytes", diagnostics);

  // accept only complete bytes from the validated helper boundary
  return bytes !== null && diagnostics.length === 0 ? bytes : null;
}

// expose one validated secure-reader file identity as a typed fact
function secureSubjectFileValue(subject, path) {
  return {
    byteLength: Number.parseInt(subject.identity.pre.size, 10),
    gid: subject.identity.pre.gid,
    mode: subject.identity.pre.mode,
    nlink: Number.parseInt(subject.identity.pre.nlink, 10),
    path,
    sha256: subject.contentSha256,
    uid: subject.identity.pre.uid,
  };
}

// collect both fixed runner installation root states
async function collectRunnerRootProbes(state) {
  const entries = [
    {
      id: "runner.root.private",
      mode: "0700",
      path: "/home/ubuntu/.weather/adjustment-maintenance/runner",
    },
    {
      id: "runner.root.library",
      mode: "0755",
      path: "/home/ubuntu/.local/lib/weather-adjustment-maintenance",
    },
  ];
  const subjects = entries.map(
    // bind each exact root beneath the proven NSS home
    (entry) => secureHostSubject(state, entry.id, entry.path, {
      allowAbsent: true,
      expectedType: "directory",
      mode: entry.mode,
      readMode: "metadata",
    }),
  );

  // reject any root that escapes the fixed runner-home profile
  if (subjects.some(
    // identify one unavailable fixed subject
    (subject) => subject === null,
  )) {
    state.complete = false;
    return;
  }
  const result = await collectSecureReaderBatch(state, {
    contractVersion: HOST_SECURE_READER_REQUEST_VERSION,
    maximumBytes: state.request.limits.maxProbeBytes,
    operation: "confined-files",
    profile: state.request.profile,
    subjects,
  });

  // retain only exact present-directory or ENOENT outcomes
  if (result === null) {
    return;
  }
  for (let index = 0; index < entries.length; index += 1) {
    const subject = result.subjects[index];
    const present = subject.status === "ok";
    appendDerivedProbe(state, entries[index].id, {
      identity: present ? subject.identity.pre : null,
      path: entries[index].path,
      present,
    }, ["nss.ubuntu.passwd"], "runner", "secure-runner-root-status-v1");
  }
}

// append one validated secure-reader byte result as a physical file probe
function appendSecureFileProbe(state, id, path, subject, sourceScope = "host") {
  const observedAt = state.adapters.clock.now().toISOString();
  const probe = {
    completedAt: observedAt,
    contractVersion: HOST_PROBE_VERSION,
    id,
    kind: "file",
    operation: { path },
    outcome: { exitCode: null, status: "ok" },
    raw: {
      stderr: rawByteField(Buffer.alloc(0), state.request.limits.maxProbeBytes).field,
      stdout: subject.bytes,
    },
    sourceScope,
    startedAt: observedAt,
  };
  return appendProbeAttempt(state, probe);
}

// collect one batch of fixed host files through the isolated secure reader
async function collectSecureHostFiles(state, entries) {
  const subjects = entries.map(
    // derive only fixed profile-owned secure subjects
    (entry) => secureHostSubject(state, entry.id, entry.path, entry),
  );

  // reject one entry outside the fixed source-owned secure domains
  if (subjects.some(
    // detect one unsupported source path
    (subject) => subject === null,
  )) {
    state.complete = false;
    return null;
  }
  const result = await collectSecureReaderBatch(state, {
    contractVersion: HOST_SECURE_READER_REQUEST_VERSION,
    maximumBytes: state.request.limits.maxProbeBytes,
    operation: "confined-files",
    profile: state.request.profile,
    subjects,
  });
  return result?.subjects ?? null;
}

// reject secret and traversal path domains independently of the checked map
function validateArtifactPathDomain(artifact) {
  const value = artifact.declaredPath ?? artifact.path ?? null;

  // artifact kinds without filesystem paths remain fixed image metadata
  if (value === null) {
    return;
  }
  // reject normalized traversal and secret-bearing domains
  if (
    typeof value !== "string" ||
    value.includes("\0") ||
    value.split("/").includes("..") ||
    /(?:^|\/)(?:\.env|shadow|environ)(?:$|\/)/u.test(value)
  ) {
    throw hostError("host.request", "runtime map contains a forbidden artifact path");
  }
  const allowed = value.startsWith("/opt/weather/") ||
    value.startsWith("/etc/systemd/system/") ||
    value.startsWith("/usr/local/bin/") ||
    value === "/usr/local/sbin/weather-remote-ops" ||
    value.startsWith("/home/ubuntu/.weather/adjustment-maintenance/runner/current/") ||
    value.startsWith("${HOME}/");

  // reject arbitrary absolute or template roots
  if (!allowed) {
    throw hostError("host.request", "runtime map artifact path is outside fixed domains");
  }
}

// read one file without materializing bytes beyond the fixed bound
async function readBoundedFile(filesystem, path, maximum, state = null) {
  let handle;

  // count each distinct physical file before opening it
  registerFilePath(state, path);

  // open the exact fixed path read only
  try {
    handle = await filesystem.open(path, "r");
    enforceDeadline(state);
    const details = await handle.stat();
    enforceDeadline(state);

    // reject known oversized files before the first data read
    if (Number.isSafeInteger(details.size) && details.size > maximum) {
      throw hostError("host.bounds", `fixed file exceeds ${String(maximum)} bytes`);
    }
    const chunks = [];
    let position = 0;

    // stream at most one bounded chunk beyond the retained range
    while (position <= maximum) {
      const remaining = maximum + 1 - position;
      const buffer = Buffer.alloc(Math.min(64 * 1024, remaining));
      const result = await handle.read(buffer, 0, buffer.length, position);
      enforceDeadline(state);

      // stop at the exact file boundary
      if (result.bytesRead === 0) {
        break;
      }
      chunks.push(buffer.subarray(0, result.bytesRead));
      position += result.bytesRead;
    }

    // reject virtual or changed files that crossed the bound while reading
    if (position > maximum) {
      throw hostError("host.bounds", `fixed file exceeds ${String(maximum)} bytes`);
    }
    return Buffer.concat(chunks, position);
  } finally {
    // close every opened descriptor before returning or rejecting
    if (handle !== undefined) {
      await handle.close();
      enforceDeadline(state);
    }
  }
}

// hash one opaque file through bounded streaming without retaining its contents
async function hashBoundedFile(filesystem, path, maximum, state) {
  let handle;

  // count each distinct opaque file before opening it
  registerFilePath(state, path);

  // retain only the digest and fixed read count
  try {
    handle = await filesystem.open(path, "r");
    enforceDeadline(state);
    const details = await handle.stat();
    enforceDeadline(state);

    // reject known oversized opaque files before reading
    if (Number.isSafeInteger(details.size) && details.size > maximum) {
      throw hostError("host.bounds", "opaque file exceeds fixed hash bound");
    }
    const hash = createHash("sha256");
    let position = 0;

    // stream fixed chunks through the digest only
    while (position <= maximum) {
      const buffer = Buffer.alloc(Math.min(64 * 1024, maximum + 1 - position));
      const result = await handle.read(buffer, 0, buffer.length, position);
      enforceDeadline(state);

      // stop at the exact opaque file boundary
      if (result.bytesRead === 0) {
        break;
      }
      hash.update(buffer.subarray(0, result.bytesRead));
      position += result.bytesRead;
    }

    // reject virtual or changed files that crossed the fixed bound
    if (position > maximum) {
      throw hostError("host.bounds", "opaque file exceeded fixed hash bound while reading");
    }
    return { byteLength: position, sha256: hash.digest("hex") };
  } finally {
    // close every opaque file descriptor on all paths
    if (handle !== undefined) {
      await handle.close();
      enforceDeadline(state);
    }
  }
}

// mark one global collection deadline without returning early from in-flight work
function markDeadline(state) {
  // skip deadline accounting during staged-source validation
  if (state === null) {
    return false;
  }
  const elapsed = state.adapters.clock.monotonicMs() - state.startedMonotonicMs;

  // abort once and retain no later operation result
  if (state.abortController.signal.aborted || elapsed > state.request.limits.deadlineMs) {
    state.complete = false;
    state.deadlineExceeded = true;

    // notify the current bounded adapter operation
    if (!state.abortController.signal.aborted) {
      state.abortController.abort();
    }
    return true;
  }
  return false;
}

// stop semantic derivation after a completed operation crossed the deadline
function enforceDeadline(state) {
  // reject late operation bytes after marking the global collection incomplete
  if (markDeadline(state)) {
    throw hostError("host.bounds", "host collection deadline exceeded");
  }
}

// reserve one distinct physical path before filesystem access
function registerFilePath(state, path) {
  // staged source validation has no collection state yet
  if (state === null) {
    return;
  }
  const value = String(path);

  // reuse an already counted fixed path
  if (state.filePaths.has(value)) {
    return;
  }

  // stop before opening a new path beyond the reviewed cardinality bound
  if (state.filePaths.size >= state.request.limits.maxFiles) {
    state.boundsExceeded = true;
    state.complete = false;
    throw hostError("host.bounds", "filesystem path count exceeds fixed bound");
  }
  state.filePaths.add(value);
}

// validate one request against fixed staged source bytes
async function validateCollectionRequest(request, adapterSet, injected) {
  requireExactKeys(request, REQUEST_KEYS, "host.request", "request");
  requireExactKeys(request.inventory, new Set(["json", "sha256"]), "host.request", "request.inventory");
  requireExactKeys(request.runtimeMap, new Set(["json", "sha256"]), "host.request", "request.runtimeMap");
  requireExactKeys(request.limits, new Set(Object.keys(MAX_LIMITS)), "host.request", "request.limits");
  const expectedRole = request.profile === "blueberry-compose"
    ? "blueberry"
    : request.profile === "workstation-runner"
      ? "workstation"
      : null;

  // require one fixed profile and host role
  if (
    request.contractVersion !== HOST_EVIDENCE_REQUEST_VERSION ||
    expectedRole === null ||
    request.expectedHostRole !== expectedRole ||
    typeof request.expectedReleaseId !== "string" ||
    request.expectedReleaseId.length === 0 ||
    typeof request.expectedSchemaVersion !== "string" ||
    request.expectedSchemaVersion.length === 0 ||
    !DIGEST_PATTERN.test(request.producerSha256 ?? "")
  ) {
    throw hostError("host.request", "request identity is invalid");
  }

  // require every caller bound below the fixed maximum
  for (const [key, maximum] of Object.entries(MAX_LIMITS)) {
    // reject missing, zero or excessive limits
    if (!isBoundedInteger(request.limits[key], maximum)) {
      throw hostError("host.request", `request limit ${key} is invalid`);
    }
  }
  const inventoryBytes = Buffer.from(request.inventory.json ?? "");
  const runtimeMapBytes = Buffer.from(request.runtimeMap.json ?? "");

  // bind request bytes to their exact byte digests
  if (
    request.inventory.sha256 !== sha256Bytes(inventoryBytes) ||
    request.runtimeMap.sha256 !== sha256Bytes(runtimeMapBytes)
  ) {
    throw hostError("host.request", "request source digest does not match bytes");
  }
  const trustedInventory = await readBoundedFile(
    adapterSet.filesystem,
    TRUSTED_INVENTORY_PATH,
    MAX_SOURCE_INPUT_BYTES,
  );
  const trustedRuntimeMap = await readBoundedFile(
    adapterSet.filesystem,
    TRUSTED_RUNTIME_MAP_PATH,
    MAX_SOURCE_INPUT_BYTES,
  );

  // require exact fixed sibling-tree source inputs
  if (
    !Buffer.from(trustedInventory).equals(inventoryBytes) ||
    !Buffer.from(trustedRuntimeMap).equals(runtimeMapBytes)
  ) {
    throw hostError("host.request", "request source bytes differ from staged reviewed inputs");
  }
  let inventory;
  let runtimeMap;

  // parse the reviewed source inputs without a package dependency
  try {
    inventory = parseStrictJson(inventoryBytes, "host.request");
    runtimeMap = parseStrictJson(runtimeMapBytes, "host.request");
  } catch (error) {
    throw hostError("host.request", `request source JSON is invalid: ${String(error.message)}`);
  }

  // bind the expected schemas before deriving any path
  if (
    inventory?.schemaVersion !== INVENTORY_SCHEMA_VERSION ||
    !Array.isArray(inventory?.entries) ||
    runtimeMap?.contractVersion !== RUNTIME_MAP_SCHEMA_VERSION ||
    !Array.isArray(runtimeMap?.artifacts) ||
    !Array.isArray(runtimeMap?.bindings) ||
    !Array.isArray(runtimeMap?.contexts)
  ) {
    throw hostError("host.request", "staged source schemas are invalid");
  }

  // enforce finite filesystem domains independently of map validation
  for (const artifact of runtimeMap.artifacts) {
    validateArtifactPathDomain(artifact);
  }

  // verify the executed producer bytes on the physical path only
  if (!injected) {
    const producerBytes = await readBoundedFile(
      adapterSet.filesystem,
      new URL(import.meta.url),
      request.limits.maxProbeBytes,
    );

    // reject a substituted producer before host reads
    if (sha256Bytes(producerBytes) !== request.producerSha256) {
      throw hostError("host.request", "producer digest differs from executed bytes");
    }
  }
  return { inventory, runtimeMap };
}

// detect common structured credential forms without returning values
function containsSecretBytes(value) {
  const text = Buffer.from(value ?? "").toString("utf8");
  return /authorization\s*:\s*bearer(?:\s|$)/iu.test(text) ||
    /--(?:password|passwd|secret|token|authorization|private[_-]?key)(?:=|\s+|"\s*,\s*")/iu.test(text) ||
    /["'][^"'\r\n]*(?:password|passwd|secret|token|authorization|private[_-]?key)[^"'\r\n]*["']\s*:/iu.test(text) ||
    /(?:[A-Za-z0-9_-]*(?:password|passwd|secret|token|authorization|private[_-]?key)[A-Za-z0-9_-]*)\s*=/imu.test(text);
}

// encode one bounded raw byte field
function rawByteField(value, maximum) {
  const bytes = Buffer.from(value ?? "");
  const secret = containsSecretBytes(bytes);
  const overLimit = bytes.length > maximum;
  return {
    field: {
      byteLength: bytes.length,
      dataBase64: secret || overLimit ? "" : bytes.toString("base64"),
      encoding: "base64",
      sha256: sha256Bytes(bytes),
      truncated: secret || overLimit,
    },
    secret,
    overLimit,
  };
}

// append one accepted probe or diagnostic error attempt
function appendProbeAttempt(state, probe) {
  const serializedBytes = Buffer.byteLength(canonicalHostJson(probe));

  // leave this fact unresolved when its retained envelope exceeds the global bound
  if (!reserveProbeBytes(state, serializedBytes)) {
    return null;
  }

  // expose only exact successful probes to semantic readers
  if (probe.outcome.status === "ok") {
    state.probes.push(probe);
  } else {
    state.errors.push(probe);
    state.complete = false;
  }
  return probe;
}

// reserve retained output bytes before appending one probe
function reserveProbeBytes(state, byteLength) {
  // stop before retaining output beyond the global evidence bound
  if (state.totalBytes + byteLength > state.request.limits.maxTotalBytes) {
    state.boundsExceeded = true;
    state.complete = false;
    return false;
  }
  state.totalBytes += byteLength;
  return true;
}

// recognize one successfully retained raw probe
function probeSucceeded(probe) {
  return probe?.outcome?.status === "ok";
}

// read one retained probe output during physical dependency planning
function collectedProbeText(probe) {
  // preserve only successful unsanitized stdout
  if (!probeSucceeded(probe) || probe.raw.stdout.truncated) {
    return null;
  }
  return Buffer.from(probe.raw.stdout.dataBase64, "base64").toString("utf8");
}

// execute one selected command before its semantic probe ID is known
async function executeSelectedCommand(state, program, args) {
  // stop before starting a dependent operation after the global deadline
  if (markDeadline(state)) {
    return null;
  }
  const startedAt = state.adapters.clock.now().toISOString();
  let result;
  let forcedStatus = null;

  // capture only bounded command results without a shell
  try {
    result = await state.adapters.executor(program, args, {
      cwd: "/",
      deadlineMs: state.request.limits.deadlineMs,
      env: { ...FIXED_ENVIRONMENT },
      maxBytes: state.request.limits.maxProbeBytes,
      signal: state.abortController.signal,
    });

    // retain only a typed timeout attempt after the global deadline
    if (markDeadline(state)) {
      forcedStatus = "timeout";
      result = { exitCode: null, stderr: Buffer.alloc(0), stdout: Buffer.alloc(0) };
    }
  } catch {
    markDeadline(state);
    forcedStatus = state.abortController.signal.aborted ? "cancelled" : "transport-error";
    result = {
      exitCode: null,
      stderr: Buffer.alloc(0),
      stdout: Buffer.alloc(0),
    };
  }
  const stdout = rawByteField(result.stdout, state.request.limits.maxProbeBytes);
  const stderr = rawByteField(result.stderr, state.request.limits.maxProbeBytes);
  const exitCode = Number.isSafeInteger(result.exitCode) &&
    result.exitCode >= 0 && result.exitCode <= 255
    ? result.exitCode
    : null;
  const status = forcedStatus ?? (stdout.secret || stderr.secret
    ? "secret"
    : stdout.overLimit || stderr.overLimit
      ? "bounds"
      : exitCode === 0
        ? "ok"
        : "error");
  return {
    args,
    completedAt: state.adapters.clock.now().toISOString(),
    outcome: { exitCode, status },
    program,
    raw: { stderr: stderr.field, stdout: stdout.field },
    startedAt,
  };
}

// append one selected command under its validated semantic identity
function appendSelectedCommandProbe(state, id, selected, sourceScope = "host") {
  const probe = {
    completedAt: selected.completedAt,
    contractVersion: HOST_PROBE_VERSION,
    id,
    kind: "command",
    operation: { args: selected.args, program: selected.program },
    outcome: selected.outcome,
    raw: selected.raw,
    sourceScope,
    startedAt: selected.startedAt,
  };
  return appendProbeAttempt(state, probe);
}

// append one typed value derived from retained physical probe inputs
function appendDerivedProbe(
  state,
  id,
  value,
  inputProbeIds,
  sourceScope = "host",
  method = "derived-canonical-json-v1",
) {
  const observedAt = state.adapters.clock.now().toISOString();
  const stdout = rawByteField(
    Buffer.from(canonicalHostJson(value)),
    state.request.limits.maxProbeBytes,
  );
  const stderr = rawByteField(Buffer.alloc(0), state.request.limits.maxProbeBytes);
  const status = stdout.secret ? "secret" : stdout.overLimit ? "bounds" : "ok";

  const probe = {
    completedAt: observedAt,
    contractVersion: HOST_PROBE_VERSION,
    id,
    kind: "derived",
    operation: { inputProbeIds: [...inputProbeIds].sort(), method },
    outcome: { exitCode: null, status },
    raw: { stderr: stderr.field, stdout: stdout.field },
    sourceScope,
    startedAt: observedAt,
  };
  return appendProbeAttempt(state, probe);
}

// parse one exact service-and-container selection snapshot
function parseDockerContainerSelections(value) {
  const text = typeof value === "string" ? value : Buffer.from(value ?? "").toString("utf8");

  // require the exact line-oriented Docker format including its final newline
  if (text.length === 0 || !text.endsWith("\n") || text.includes("\r")) {
    return null;
  }
  const rows = text.slice(0, -1).split("\n");
  const selections = [];
  const services = new Set();
  const containerIds = new Set();

  // bind every selected ID to one fixed Compose service label
  for (const row of rows) {
    const separator = row.indexOf("\t");
    const service = row.slice(0, separator);
    const containerId = row.slice(separator + 1);

    // reject missing extra duplicate or malformed selection identities
    if (separator <= 0 || row.indexOf("\t", separator + 1) !== -1 ||
      !SERVICES.includes(service) || !DIGEST_PATTERN.test(containerId) ||
      services.has(service) || containerIds.has(containerId)) {
      return null;
    }
    services.add(service);
    containerIds.add(containerId);
    selections.push({ containerId, service });
  }
  const expectedLongLived = SERVICES.filter(
    // retain the five persistent service labels
    (service) => service !== "migration",
  ).sort();
  const selectedServices = [...services].sort();
  const withMigration = [...expectedLongLived, "migration"].sort();

  // require exactly the five persistent services and optional completed migration
  if (canonicalHostJson(selectedServices) !== canonicalHostJson(expectedLongLived) &&
    canonicalHostJson(selectedServices) !== canonicalHostJson(withMigration)) {
    return null;
  }
  return selections;
}

// parse one selected Docker container result without reading environment fields
function parseSelectedContainer(selected) {
  const text = collectedProbeText({
    outcome: selected.outcome,
    raw: selected.raw,
  })?.trim();

  // reject absent or malformed selected container output
  if (text === undefined || text === null || text.length === 0) {
    return null;
  }
  try {
    const parsed = JSON.parse(text);
    const value = Array.isArray(parsed) ? parsed[0] : parsed;

    // retain one selected closed container record only
    if (!isRecord(value) || typeof value.service !== "string") {
      return null;
    }
    return value;
  } catch {
    return null;
  }
}

// parse one proc stat record without trusting spaces inside the command name
function parseProcStat(value) {
  const close = value.lastIndexOf(")");

  // reject malformed proc stat records
  if (close < 0) {
    return null;
  }
  const fields = value.slice(close + 2).trim().split(/\s+/u);
  const ppid = Number.parseInt(fields[1] ?? "", 10);
  const startTimeTicks = Number.parseInt(fields[19] ?? "", 10);
  return Number.isSafeInteger(ppid) && Number.isSafeInteger(startTimeTicks)
    ? { ppid, startTimeTicks }
    : null;
}

// collect one container process tree with retained proc source bytes
async function collectContainerProcesses(state, service, selected) {
  const topId = `docker.container.${service}.top`;
  const topProbe = await collectCommandProbe(
    state,
    topId,
    "/usr/bin/docker",
    ["top", selected.raw.id, "-eo", "pid,ppid,args"],
    service,
  );
  const lines = (collectedProbeText(topProbe) ?? "").trimEnd().split(/\r?\n/u);

  // reject a missing or oversized native process table
  if (lines.length < 2 || !/^\s*PID\s+PPID\s+COMMAND\s*$/u.test(lines[0]) ||
    lines.length - 1 > state.request.limits.maxProcesses) {
    state.complete = false;
    return [];
  }
  const records = [];
  const selectedPids = new Set();

  // collect exact proc identities for every selected container PID
  for (const line of lines.slice(1)) {
    const match = line.match(/^\s*([1-9][0-9]*)\s+([0-9]+)\s+\S/u);

    // reject unparseable process rows before selecting proc paths
    if (match === null || selectedPids.has(match[1])) {
      state.complete = false;
      return [];
    }
    selectedPids.add(match[1]);
    const pid = Number.parseInt(match[1], 10);
    const root = `/proc/${String(pid)}`;
    try {
      const cmdline = await readBoundedFile(
        state.adapters.filesystem,
        `${root}/cmdline`,
        state.request.limits.maxProbeBytes,
        state,
      );
      const cgroup = await readBoundedFile(
        state.adapters.filesystem,
        `${root}/cgroup`,
        state.request.limits.maxProbeBytes,
        state,
      );
      const stat = await readBoundedFile(
        state.adapters.filesystem,
        `${root}/stat`,
        state.request.limits.maxProbeBytes,
        state,
      );
      const maps = await readBoundedFile(
        state.adapters.filesystem,
        `${root}/maps`,
        state.request.limits.maxProbeBytes,
        state,
      );
      registerFilePath(state, `${root}/exe`);
      const exePath = await state.adapters.filesystem.readlink(`${root}/exe`);
      enforceDeadline(state);
      const exeLink = Buffer.from(String(exePath));
      const exe = await hashBoundedFile(
        state.adapters.filesystem,
        `${root}/exe`,
        MAX_OPAQUE_FILE_BYTES,
        state,
      );
      const statFields = parseProcStat(stat.toString("utf8"));
      const cgroupPath = cgroup.toString("utf8").match(/^0::(.+)$/mu)?.[1] ?? null;
      const argv = cmdline.toString("utf8").split("\0").filter(Boolean);
      const libcPath = maps.toString("utf8").match(
        /\s(\/[A-Za-z0-9._/+:-]*\/libc\.so(?:\.[0-9]+)*)$/mu,
      )?.[1] ?? null;
      const abiSubjects = [{
        containerId: selected.raw.id,
        expectedGid: 0,
        expectedMode: "0644",
        expectedUid: 0,
        id: `container.${service}.${String(pid)}.os-release`,
        imageId: selected.raw.imageId,
        pid,
        readMode: "bytes",
        relativeComponents: ["etc", "os-release"],
      }];

      // include one mapped libc only when proc maps selected a confined absolute path
      if (libcPath !== null && !libcPath.split("/").includes("..")) {
        abiSubjects.push({
          containerId: selected.raw.id,
          expectedGid: 0,
          expectedMode: "0755",
          expectedUid: 0,
          id: `container.${service}.${String(pid)}.libc`,
          imageId: selected.raw.imageId,
          pid,
          readMode: "sha256",
          relativeComponents: libcPath.split("/").filter(Boolean),
        });
      }
      const abi = await collectSecureReaderBatch(state, {
        contractVersion: HOST_SECURE_READER_REQUEST_VERSION,
        maximumBytes: state.request.limits.maxProbeBytes,
        operation: "container-proc-root",
        profile: state.request.profile,
        subjects: abiSubjects,
      });

      // require one fully validated proc-root ABI batch
      if (abi === null) {
        return [];
      }
      const rootOsRelease = secureSubjectBytes(abi.subjects[0]);
      const libcSha256 = abi.subjects[1]?.contentSha256 ?? null;
      const secureRootProof = {
        contractVersion: "weather-backend-host-secure-root-proof/v1",
        helperSha256: abi.helperSha256,
        subjects: abi.subjects.map(
          // retain every exact source request beside its validated helper proof
          (subject, index) => ({
            contentSha256: subject.contentSha256,
            expected: {
              containerId: abiSubjects[index].containerId,
              expectedGid: abiSubjects[index].expectedGid,
              expectedMode: abiSubjects[index].expectedMode,
              expectedUid: abiSubjects[index].expectedUid,
              id: abiSubjects[index].id,
              imageId: abiSubjects[index].imageId,
              pid: abiSubjects[index].pid,
              readMode: abiSubjects[index].readMode,
            },
            identity: subject.identity,
            procBinding: subject.procBinding,
            relativeComponents: abiSubjects[index].relativeComponents,
          }),
        ),
      };
      const rawFields = [cmdline, cgroup, stat, maps, exeLink].map(
        // scrub every retained proc source independently
        (bytes) => rawByteField(bytes, state.request.limits.maxProbeBytes),
      );

      // reject secret-bearing argv or malformed proc identity
      if (rawFields.some(
        // detect one scrubbed proc source
        (field) => field.secret || field.overLimit,
      ) || statFields === null || cgroupPath === null || argv.length === 0) {
        state.complete = false;
        return [];
      }
      records.push({
        argv,
        cgroupPath,
        exePath,
        exeSha256: exe.sha256,
        mapsSha256: sha256Bytes(maps),
        mappedLibcPath: libcPath,
        mappedLibcSha256: libcSha256,
        pid,
        ppid: statFields.ppid,
        raw: {
          cgroup: rawFields[1].field,
          cmdline: rawFields[0].field,
          exeByteLength: exe.byteLength,
          exeLink: rawFields[4].field,
          maps: rawFields[3].field,
          rootOsRelease: rootOsRelease === null
            ? null
            : rawByteField(rootOsRelease, state.request.limits.maxProbeBytes).field,
          stat: rawFields[2].field,
        },
        rootOsReleaseSha256: rootOsRelease === null ? null : sha256Bytes(rootOsRelease),
        secureRootProof,
        startTimeTicks: statFields.startTimeTicks,
      });
    } catch {
      state.complete = false;
      return [];
    }
  }
  appendDerivedProbe(
    state,
    `docker.container.${service}.processes`,
    {
      containerId: selected.raw.id,
      dockerInitPid: selected.raw.pid,
      processes: records,
    },
    [`docker.container.${service}.inspect`, topId],
    service,
    "docker-top-procfs-v2",
  );
  return records;
}

// collect one direct cgroup identity from an inspected container process tree
async function collectContainerCgroup(state, service, selected, processes) {
  const init = processes.find(
    // select the exact Docker init PID
    (process) => process.pid === Number(selected.raw.pid),
  );

  // leave cgroup evidence unresolved without exact init membership
  if (!isRecord(init) || typeof init.cgroupPath !== "string" ||
    !init.cgroupPath.startsWith("/") || init.cgroupPath.split("/").includes("..")) {
    return;
  }
  const root = `/sys/fs/cgroup${init.cgroupPath}`;
  try {
    const reads = await Promise.allSettled([
      readBoundedFile(state.adapters.filesystem, `${root}/cgroup.controllers`, state.request.limits.maxProbeBytes, state),
      readBoundedFile(state.adapters.filesystem, `${root}/cpu.max`, state.request.limits.maxProbeBytes, state),
      readBoundedFile(state.adapters.filesystem, `${root}/memory.high`, state.request.limits.maxProbeBytes, state),
      readBoundedFile(state.adapters.filesystem, `${root}/memory.max`, state.request.limits.maxProbeBytes, state),
      readBoundedFile(state.adapters.filesystem, `${root}/pids.max`, state.request.limits.maxProbeBytes, state),
    ]);
    const failed = reads.find(
      // identify one failure only after every sibling descriptor settled
      (result) => result.status === "rejected",
    );

    // reject the family after all sibling cleanup completes
    if (failed !== undefined) {
      throw failed.reason;
    }
    const [controllers, cpuMax, memoryHigh, memoryMax, pidsMax] = reads.map(
      // unwrap only the fully settled successful read set
      (result) => result.value,
    );
    appendDerivedProbe(state, `cgroup.blueberry:compose:${service}`, {
      controllers: controllers.toString("utf8").trim().split(/\s+/u).filter(Boolean),
      cpuMax: cpuMax.toString("utf8").trim(),
      memoryHigh: memoryHigh.toString("utf8").trim(),
      memoryMax: memoryMax.toString("utf8").trim(),
      path: init.cgroupPath,
      pid: init.pid,
      pidsMax: pidsMax.toString("utf8").trim(),
      raw: {
        controllers: rawByteField(controllers, state.request.limits.maxProbeBytes).field,
        cpuMax: rawByteField(cpuMax, state.request.limits.maxProbeBytes).field,
        memoryHigh: rawByteField(memoryHigh, state.request.limits.maxProbeBytes).field,
        memoryMax: rawByteField(memoryMax, state.request.limits.maxProbeBytes).field,
        pidsMax: rawByteField(pidsMax, state.request.limits.maxProbeBytes).field,
      },
      scopeId: `blueberry:compose:${service}`,
    }, [
      `docker.container.${service}.inspect`,
      `docker.container.${service}.processes`,
    ], service);
    return true;
  } catch {
    state.complete = false;
    return false;
  }
}

// collect one fixed command probe
async function collectCommandProbe(
  state,
  id,
  program,
  args,
  sourceScope = "host",
  acceptedExitCodes = [0],
) {
  // stop before starting another operation after the global deadline
  if (markDeadline(state)) {
    return null;
  }
  const startedAt = state.adapters.clock.now().toISOString();
  let result;

  // capture executor failures as incomplete evidence
  try {
    result = await state.adapters.executor(program, args, {
      cwd: "/",
      deadlineMs: state.request.limits.deadlineMs,
      env: { ...FIXED_ENVIRONMENT },
      maxBytes: state.request.limits.maxProbeBytes,
      signal: state.abortController.signal,
    });
    const late = markDeadline(state);

    // discard any result that settled after the global deadline
    if (late) {
      return null;
    }
  } catch (error) {
    markDeadline(state);

    // discard abort details instead of retaining late or adapter-controlled text
    if (state.abortController.signal.aborted) {
      return null;
    }
    result = {
      exitCode: 1,
      stderr: Buffer.from("bounded command failed\n"),
      stdout: Buffer.alloc(0),
    };
  }
  const stdout = rawByteField(result.stdout, state.request.limits.maxProbeBytes);
  const stderr = rawByteField(result.stderr, state.request.limits.maxProbeBytes);
  const status = stdout.secret || stderr.secret
    ? "secret"
    : stdout.overLimit || stderr.overLimit
      ? "bounds"
      : acceptedExitCodes.includes(result.exitCode)
        ? "ok"
        : "error";
  const probe = {
    completedAt: state.adapters.clock.now().toISOString(),
    contractVersion: HOST_PROBE_VERSION,
    id,
    kind: "command",
    operation: { args, program },
    outcome: { exitCode: result.exitCode, status },
    raw: { stderr: stderr.field, stdout: stdout.field },
    sourceScope,
    startedAt,
  };
  return appendProbeAttempt(state, probe);
}

// collect one fixed filesystem probe
async function collectFileProbe(state, id, kind, path, sourceScope = "host") {
  // stop before starting another operation after the global deadline
  if (markDeadline(state)) {
    return null;
  }
  const startedAt = state.adapters.clock.now().toISOString();
  let bytes = Buffer.alloc(0);
  let status = "ok";

  // call only the fixed operation selected by the producer
  try {
    if (kind === "readlink") {
      registerFilePath(state, path);
      bytes = Buffer.from(`${await state.adapters.filesystem.readlink(path)}\n`);
      enforceDeadline(state);
    } else if (kind === "statfs") {
      registerFilePath(state, path);
      bytes = Buffer.from(canonicalHostJson(await state.adapters.filesystem.statfs(path)));
      enforceDeadline(state);
    } else if (kind === "directory") {
      bytes = Buffer.from(canonicalHostJson(await collectProcessDirectory(state, path)));
    } else {
      bytes = await readBoundedFile(
        state.adapters.filesystem,
        path,
        state.request.limits.maxProbeBytes,
        state,
      );
    }
  } catch (error) {
    status = error?.code === "ENOENT"
      ? "absent"
      : error?.code === "host.bounds"
        ? "bounds"
        : "error";
    bytes = Buffer.alloc(0);
  }
  const stdout = rawByteField(bytes, state.request.limits.maxProbeBytes);
  const stderr = rawByteField(Buffer.alloc(0), state.request.limits.maxProbeBytes);

  // reject secrets and oversized reads without retaining their bytes
  if (stdout.secret || stdout.overLimit) {
    status = stdout.secret ? "secret" : "bounds";
  }

  const probe = {
    completedAt: state.adapters.clock.now().toISOString(),
    contractVersion: HOST_PROBE_VERSION,
    id,
    kind,
    operation: { path },
    outcome: { exitCode: null, status },
    raw: { stderr: stderr.field, stdout: stdout.field },
    sourceScope,
    startedAt,
  };
  return appendProbeAttempt(state, probe);
}

// collect one fixed regular file after rejecting a linked final path
async function collectRegularFileProbe(state, id, path, sourceScope = "host") {
  try {
    registerFilePath(state, path);
    const details = await state.adapters.filesystem.lstat(path);
    enforceDeadline(state);

    // reject a substituted final node before reading its bytes
    if (!details.isFile() || details.isSymbolicLink()) {
      state.complete = false;
      return null;
    }
  } catch (error) {
    // preserve exact absence through the ordinary typed file probe
    if (error?.code !== "ENOENT") {
      state.complete = false;
      return null;
    }
  }
  return await collectFileProbe(state, id, "file", path, sourceScope);
}

// stream one bounded process directory without materializing all entries
async function collectProcessDirectory(state, path) {
  let directory;
  const processes = [];
  let selected = 0;

  // retain the descriptor until bounded iteration is complete
  try {
    registerFilePath(state, path);
    directory = await state.adapters.filesystem.opendir(path);
    enforceDeadline(state);

    // inspect no more than one entry beyond the configured process bound
    while (selected <= state.request.limits.maxProcesses) {
      const entry = await directory.read();
      enforceDeadline(state);

      // stop at the directory boundary
      if (entry === null) {
        break;
      }

      // ignore non-process directory entries
      if (!entry.isDirectory() || !/^[1-9][0-9]*$/u.test(entry.name)) {
        continue;
      }
      selected += 1;

      // stop before inspecting the first out-of-bound process
      if (selected > state.request.limits.maxProcesses) {
        throw hostError("host.bounds", "process directory exceeds configured cardinality");
      }
      const pid = Number.parseInt(entry.name, 10);
      const process = await collectRunnerProcess(state, pid);

      // retain only exact fixed-account runner processes
      if (process !== null) {
        processes.push(process);
      }
    }
    return processes;
  } finally {
    // close every opened process directory before returning or rejecting
    if (directory !== undefined) {
      await directory.close();
      enforceDeadline(state);
    }
  }
}

// collect one exact fixed-account runner process or discard an unrelated PID
async function collectRunnerProcess(state, pid) {
  // preserve an empty runner set until exact NSS identity is available
  if (state.runnerAccount === undefined) {
    return null;
  }
  const root = `/proc/${String(pid)}`;
  try {
    const status = await readBoundedFile(
      state.adapters.filesystem,
      `${root}/status`,
      state.request.limits.maxProbeBytes,
      state,
    );
    const uid = Number.parseInt(
      status.toString("utf8").match(/^Uid:\s+([0-9]+)/mu)?.[1] ?? "",
      10,
    );

    // discard processes outside the fixed NSS account before reading argv
    if (uid !== state.runnerAccount.uid) {
      return null;
    }
    const cmdline = await readBoundedFile(
      state.adapters.filesystem,
      `${root}/cmdline`,
      state.request.limits.maxProbeBytes,
      state,
    );
    const cgroup = await readBoundedFile(
      state.adapters.filesystem,
      `${root}/cgroup`,
      state.request.limits.maxProbeBytes,
      state,
    );
    const stat = await readBoundedFile(
      state.adapters.filesystem,
      `${root}/stat`,
      state.request.limits.maxProbeBytes,
      state,
    );
    const maps = await readBoundedFile(
      state.adapters.filesystem,
      `${root}/maps`,
      state.request.limits.maxProbeBytes,
      state,
    );
    registerFilePath(state, `${root}/exe`);
    const exePath = await state.adapters.filesystem.readlink(`${root}/exe`);
    enforceDeadline(state);
    const exeLink = Buffer.from(String(exePath));
    const argv = cmdline.toString("utf8").split("\0").filter(Boolean);
    const roots = [
      `${state.runnerAccount.home}/.weather/adjustment-maintenance/runner`,
      `${state.runnerAccount.home}/.local/lib/weather-adjustment-maintenance`,
    ];

    // discard same-account processes outside both fixed runner roots
    if (!argv.some(
      // match only exact runner-root command tokens
      (argument) => roots.some(
        // bind the token under one fixed runner root
        (runnerRoot) => argument === runnerRoot || argument.startsWith(`${runnerRoot}/`),
      ),
    )) {
      return null;
    }
    const rawFields = [cmdline, cgroup, stat, maps, exeLink].map(
      // scrub every retained runner proc source independently
      (bytes) => rawByteField(bytes, state.request.limits.maxProbeBytes),
    );

    // reject secret-bearing runner process metadata
    if (rawFields.some(
      // recognize one scrubbed runner source
      (field) => field.secret || field.overLimit,
    )) {
      state.complete = false;
      return null;
    }
    const statFields = parseProcStat(stat.toString("utf8"));
    const cgroupPath = cgroup.toString("utf8").match(/^0::(.+)$/mu)?.[1] ?? null;

    // reject malformed matching process identities
    if (statFields === null || cgroupPath === null || typeof exePath !== "string") {
      state.complete = false;
      return null;
    }
    const exe = await hashBoundedFile(
      state.adapters.filesystem,
      `${root}/exe`,
      MAX_OPAQUE_FILE_BYTES,
      state,
    );
    return {
      argv,
      cgroupPath,
      exePath,
      exeSha256: exe.sha256,
      mapsSha256: sha256Bytes(maps),
      mappedLibcPath: null,
      mappedLibcSha256: null,
      pid,
      ppid: statFields.ppid,
      raw: {
        cgroup: rawFields[1].field,
        cmdline: rawFields[0].field,
        exeByteLength: exe.byteLength,
        exeLink: rawFields[4].field,
        maps: rawFields[3].field,
        rootOsRelease: null,
        stat: rawFields[2].field,
        status: rawByteField(status, state.request.limits.maxProbeBytes).field,
      },
      rootOsReleaseSha256: null,
      startTimeTicks: statFields.startTimeTicks,
    };
  } catch (error) {
    // disappearing processes are ordinary bounded scan races
    if (error?.code === "ENOENT") {
      return null;
    }
    state.complete = false;
    return null;
  }
}

// collect the common host ABI evidence set
async function collectCommonProbes(state) {
  await collectCommandProbe(state, "host.uname-machine", "/usr/bin/uname", ["-m"]);
  await collectCommandProbe(state, "host.uname-kernel", "/usr/bin/uname", ["-r"]);
  await collectFileProbe(state, "host.cpuinfo", "file", "/proc/cpuinfo");
  await collectFileProbe(state, "host.os-release", "file", "/etc/os-release");
  await collectCommandProbe(state, "host.libc", "/usr/bin/getconf", ["GNU_LIBC_VERSION"]);
  await collectFileProbe(state, "host.mountinfo", "file", "/proc/self/mountinfo");
  await collectFileProbe(state, "host.filesystems", "file", "/proc/filesystems");
  await collectFileProbe(state, "host.self-cgroup", "file", "/proc/self/cgroup");
  await collectFileProbe(state, "host.cgroup-namespace", "readlink", "/proc/self/ns/cgroup");
  await collectFileProbe(state, "host.cgroup-statfs", "statfs", "/sys/fs/cgroup");
}

// collect one fixed unit without reading a source-selected path prematurely
async function collectUnitProbeFamily(state, unit, includeEnabled) {
  const show = await collectCommandProbe(
    state,
    `systemd.${unit}.show`,
    "/usr/bin/systemctl",
    [
      "show",
      unit,
      "--no-pager",
      "--property=Id,LoadState,ActiveState,SubState,UnitFileState,Result,ExecMainStatus,FragmentPath,DropInPaths,ExecStart,ExecReload,ExecStop,TimeoutStartUSec,User,Group",
    ],
  );
  // retain enablement separately for the conditional runner branch
  if (includeEnabled) {
    await collectCommandProbe(
      state,
      `systemd.${unit}.enabled`,
      "/usr/bin/systemctl",
      ["is-enabled", unit],
      "host",
      [0, 1, 3, 4],
    );
  }

  const fields = parseKeyValueLines(collectedProbeText(show) ?? "");

  // retain no nonexistent unit bytes after an exact not-found state
  if (fields.LoadState === "not-found") {
    state.planned.delete(`systemd.${unit}.files`);
    return show;
  }

  // collect effective unit source bytes only after a loaded show result
  if (probeSucceeded(show) && fields.LoadState === "loaded") {
    const paths = [fields.FragmentPath, ...(fields.DropInPaths ?? "").split(/\s+/u)]
      .filter(Boolean);
    const subjects = [];

    // confine each exact systemd-selected fragment in declared order
    for (const path of paths) {
      const safe = path.startsWith("/etc/systemd/system/") ||
        path.startsWith("/usr/lib/systemd/system/") ||
        path.startsWith("/lib/systemd/system/");

      // reject systemd-selected paths outside the fixed unit domains
      if (!safe || path.split("/").includes("..")) {
        state.complete = false;
        return show;
      }
      const subject = secureHostSubject(
        state,
        `systemd.${unit}.file.${String(subjects.length)}`,
        path,
        { mode: "0644", readMode: "bytes" },
      );

      // reject any systemd-selected path outside the secure profile
      if (subject === null) {
        state.complete = false;
        return show;
      }
      subjects.push(subject);
    }
    const secure = await collectSecureReaderBatch(state, {
      contractVersion: HOST_SECURE_READER_REQUEST_VERSION,
      maximumBytes: state.request.limits.maxProbeBytes,
      operation: "confined-files",
      profile: state.request.profile,
      subjects,
    });

    // leave selected unit bytes unresolved when secure collection fails
    if (secure === null) {
      return show;
    }
    const files = secure.subjects.map(
      // retain validated unit bytes and descriptor identity
      (subject, index) => ({
        byteLength: Number.parseInt(subject.identity.pre.size, 10),
        dataBase64: subject.bytes.dataBase64,
        gid: subject.identity.pre.gid,
        mode: subject.identity.pre.mode,
        nlink: Number.parseInt(subject.identity.pre.nlink, 10),
        path: paths[index],
        sha256: subject.contentSha256,
        uid: subject.identity.pre.uid,
      }),
    );
    appendDerivedProbe(
      state,
      `systemd.${unit}.files`,
      files,
      [`systemd.${unit}.show`],
    );
  }

  // leave unit files unresolved until the selected show bytes validate a safe path
  return show;
}

// collect one bounded opaque installed-file identity without retaining its contents
async function collectOpaqueFileIdentity(state, id, path, prerequisiteIds) {
  const mode = path.endsWith(".mjs") ? "0644" : "0755";
  const subjects = await collectSecureHostFiles(state, [{
    id,
    mode,
    path,
    readMode: "sha256",
  }]);

  // leave this identity unresolved when confined collection fails
  if (subjects === null) {
    return null;
  }
  return appendDerivedProbe(
    state,
    id,
    secureSubjectFileValue(subjects[0], path),
    prerequisiteIds,
  );
}

// collect the fixed installed operator mirrors and active bundle identity
async function collectOperatorProbes(state, composeShow) {
  const mirrorEntries = [
    { id: "operator.forced.dispatcher", mode: "0755", path: "/usr/local/bin/weather-ssh-dispatch", readMode: "sha256" },
    { id: "operator.forced.remote-ops", mode: "0755", path: "/usr/local/sbin/weather-remote-ops", readMode: "sha256" },
  ];
  const mirrorSubjects = await collectSecureHostFiles(state, mirrorEntries);

  // retain both fixed operator mirrors from one root-pinned batch
  if (mirrorSubjects !== null) {
    for (let index = 0; index < mirrorEntries.length; index += 1) {
      appendDerivedProbe(
        state,
        mirrorEntries[index].id,
        { file: secureSubjectFileValue(mirrorSubjects[index], mirrorEntries[index].path) },
        [],
        "operator",
        "derived-canonical-json-v1",
      );
    }
  }
  const fields = parseKeyValueLines(collectedProbeText(composeShow) ?? "");
  const bundleSha256 = (fields.ExecStart ?? "").match(
    /\/opt\/weather\/operators\/schema-preserving-release\/([a-f0-9]{64})\//u,
  )?.[1] ?? null;

  // defer active bundle reads until exact effective unit bytes select one digest
  if (bundleSha256 === null) {
    return;
  }
  const stateRoot = "/opt/weather/current/deploy/state/schema-preserving-release-v1";
  const receiptPath = `${stateRoot}/installations/${bundleSha256}.json`;
  const bootstrapPath = `${stateRoot}/installation.json`;
  const immutableSubject = secureHostSubject(state, "operator.installation-receipt.immutable", receiptPath, {
    allowAbsent: true,
    mode: "0600",
    readMode: "bytes",
  });
  const immutableResult = immutableSubject === null
    ? null
    : await collectSecureReaderBatch(state, {
      contractVersion: HOST_SECURE_READER_REQUEST_VERSION,
      maximumBytes: state.request.limits.maxProbeBytes,
      operation: "confined-files",
      profile: state.request.profile,
      subjects: [immutableSubject],
    });
  let receiptSubject = immutableResult?.subjects[0] ?? null;
  let selectedReceiptPath = receiptPath;
  let immutableStatus = receiptSubject?.status === "ok"
    ? "present"
    : receiptSubject?.status ?? null;

  // select the bootstrap receipt only after exact immutable-path ENOENT evidence
  if (receiptSubject?.status === "absent") {
    const bootstrapSubjects = await collectSecureHostFiles(state, [{
      id: "operator.installation-receipt.bootstrap",
      mode: "0600",
      path: bootstrapPath,
      readMode: "bytes",
    }]);
    receiptSubject = bootstrapSubjects?.[0] ?? null;
    selectedReceiptPath = bootstrapPath;
  }
  const receipt = receiptSubject === null || receiptSubject.status !== "ok"
    ? null
    : appendDerivedProbe(state, "operator.installation-receipt", {
      file: secureSubjectFileValue(receiptSubject, selectedReceiptPath),
      raw: { bytes: receiptSubject.bytes },
      selection: {
        bootstrapPath,
        immutablePath: receiptPath,
        immutableStatus,
        selectedPath: selectedReceiptPath,
      },
    }, ["systemd.weather-compose.service.show"], "operator", "derived-canonical-json-v1");

  // collect bundle and drop-in bytes only after the active receipt is readable
  if (!probeSucceeded(receipt)) {
    return;
  }
  const controlSubject = secureHostSubject(
    state,
    "operator.control-plane.files",
    "/opt/weather/current/deploy",
    {
      expectedType: "directory",
      mode: "0755",
      readMode: "tree",
    },
  );
  const controlResult = controlSubject === null
    ? null
    : await collectSecureReaderBatch(state, {
      contractVersion: HOST_SECURE_READER_REQUEST_VERSION,
      maximumBytes: state.request.limits.maxProbeBytes,
      operation: "confined-files",
      profile: state.request.profile,
      subjects: [controlSubject],
    });
  const controlBytes = controlResult === null
    ? null
    : secureSubjectBytes(controlResult.subjects[0]);

  // retain the complete secure v17 tree only from canonical helper bytes
  if (controlBytes !== null) {
    try {
      const controlPlane = parseStrictJson(controlBytes, "host.binding");
      appendDerivedProbe(
        state,
        "operator.control-plane.files",
        controlPlane,
        ["operator.installation-receipt"],
        "operator",
        "secure-control-plane-tree-v1",
      );
    } catch {
      state.complete = false;
    }
  }
  const bundleRoot = `/opt/weather/operators/schema-preserving-release/${bundleSha256}`;
  const bundleEntries = [
    { id: "operator.bundle.schema-preserving-release.mjs", mode: "0644", path: `${bundleRoot}/schema-preserving-release.mjs`, readMode: "sha256" },
    { id: "operator.bundle.schema-preserving-release.sh", mode: "0755", path: `${bundleRoot}/schema-preserving-release.sh`, readMode: "sha256" },
  ];
  const bundleSubjects = await collectSecureHostFiles(state, bundleEntries);

  // retain the immutable active operator bundle as one root-pinned batch
  if (bundleSubjects !== null) {
    for (let index = 0; index < bundleEntries.length; index += 1) {
      appendDerivedProbe(
        state,
        bundleEntries[index].id,
        { file: secureSubjectFileValue(bundleSubjects[index], bundleEntries[index].path) },
        ["operator.installation-receipt"],
        "operator",
        "derived-canonical-json-v1",
      );
    }
  }
  const dropIn = (fields.DropInPaths ?? "").split(/\s+/u).find(
    // select the exact schema-preserving effective drop-in
    (path) => path.endsWith("/10-schema-preserving-release.conf"),
  );

  // retain exact selected drop-in bytes without accepting another path
  if (dropIn === "/etc/systemd/system/weather-compose.service.d/10-schema-preserving-release.conf") {
    const dropInSubjects = await collectSecureHostFiles(state, [{
      id: "operator.weather-compose.dropin",
      mode: "0644",
      path: dropIn,
      readMode: "bytes",
    }]);

    // retain only the securely read selected drop-in
    if (dropInSubjects !== null) {
      appendDerivedProbe(state, "operator.weather-compose.dropin", {
        file: secureSubjectFileValue(dropInSubjects[0], dropIn),
        raw: { bytes: dropInSubjects[0].bytes },
      }, ["systemd.weather-compose.service.show"], "operator", "derived-canonical-json-v1");
    }
  }
}

// collect the fixed Blueberry command and source evidence set
async function collectBlueberryProbes(state) {
  const containers = await collectCommandProbe(
    state,
    "docker.containers.list",
    "/usr/bin/docker",
    [
      "ps",
      "--all",
      "--filter",
      "label=com.docker.compose.project=weather",
      "--no-trunc",
      "--format",
      DOCKER_CONTAINER_LIST_FORMAT,
    ],
  );
  const composeShow = await collectUnitProbeFamily(state, "weather-compose.service", false);
  await collectOperatorProbes(state, composeShow);
  const releaseEntries = [
    { id: "release.current", mode: "0600", path: "/opt/weather/current/deploy/state/current-release", readMode: "bytes" },
    { id: "release.previous", mode: "0600", path: "/opt/weather/current/deploy/state/previous-release", readMode: "bytes" },
    { id: "release.schema", mode: "0600", path: "/opt/weather/current/deploy/state/schema-release", readMode: "bytes" },
  ];
  const releaseSubjects = await collectSecureHostFiles(state, releaseEntries);
  let currentRelease = null;
  let schemaRelease = null;

  // append all three release markers from one pinned secure snapshot
  if (releaseSubjects !== null) {
    currentRelease = appendSecureFileProbe(
      state,
      "release.current",
      releaseEntries[0].path,
      releaseSubjects[0],
    );
    appendSecureFileProbe(
      state,
      "release.previous",
      releaseEntries[1].path,
      releaseSubjects[1],
    );
    schemaRelease = appendSecureFileProbe(
      state,
      "release.schema",
      releaseEntries[2].path,
      releaseSubjects[2],
    );
  }

  // bind the observed release markers to the reviewed collection request
  if (
    collectedProbeText(currentRelease)?.trim() !== state.request.expectedReleaseId ||
    collectedProbeText(schemaRelease)?.trim() !== state.request.expectedSchemaVersion
  ) {
    state.complete = false;
  }

  // defer all container-selected reads until enumeration succeeds
  if (!probeSucceeded(containers)) {
    return;
  }
  const selections = parseDockerContainerSelections(collectedProbeText(containers));

  // reject malformed duplicate or incomplete service selections before inspection
  if (selections === null) {
    state.complete = false;
    return;
  }
  const selectedByService = new Map();

  // inspect every enumerated project container through a selected non-environment format
  for (const selection of selections) {
    const { containerId, service } = selection;
    const selected = await executeSelectedCommand(
      state,
      "/usr/bin/docker",
      ["inspect", "--format", DOCKER_CONTAINER_FORMAT, containerId],
    );

    // stop after a global deadline without appending late bytes
    if (selected === null) {
      return;
    }
    const inspectId = `docker.container.${service}.inspect`;

    // retain every observed failed selected attempt before semantic parsing
    if (!probeSucceeded(selected)) {
      appendSelectedCommandProbe(state, inspectId, selected, service);
      continue;
    }
    const raw = normalizeContainerInspect(parseSelectedContainer(selected));
    const migration = service === "migration";

    // retain malformed or substituted exit-zero bytes as a failed selected attempt
    if (
      !isRecord(raw) ||
      raw.service !== service ||
      raw.project !== "weather" ||
      raw.containerNumber !== 1 ||
      raw.id !== containerId ||
      raw.oneOff !== migration ||
      (migration && (raw.running !== false || raw.status !== "exited" || raw.exitCode !== 0)) ||
      (!migration && raw.running !== true)
    ) {
      state.complete = false;
      appendSelectedCommandProbe(state, inspectId, {
        ...selected,
        outcome: { exitCode: selected.outcome.exitCode, status: "error" },
      }, service);
      continue;
    }
    const inspect = appendSelectedCommandProbe(
      state,
      inspectId,
      selected,
      service,
    );
    selectedByService.set(service, { inspect, raw });
  }
  const selectedServices = [...selectedByService.keys()].sort();
  const listedServices = selections.map(
    // retain the service identities independently of inspect success
    (selection) => selection.service,
  ).sort();
  const expectedLongLived = SERVICES.filter(
    // select the five persistent Compose services
    (service) => service !== "migration",
  ).sort();

  // accept source-proven migration absence without inventing history
  if (canonicalHostJson(listedServices) === canonicalHostJson(expectedLongLived)) {
    state.planned.delete("docker.container.migration.inspect");
    state.planned.delete("docker.container.migration.top");
    state.planned.delete("docker.container.migration.processes");
    state.planned.delete("cgroup.blueberry:compose:migration");
  }
  const expectedWithMigration = [...expectedLongLived, "migration"].sort();

  // reject any missing duplicate extra or substituted Compose service set
  if (
    canonicalHostJson(selectedServices) !== canonicalHostJson(expectedLongLived) &&
    canonicalHostJson(selectedServices) !== canonicalHostJson(expectedWithMigration)
  ) {
    state.complete = false;
  }

  // collect each validated service process and cgroup family
  for (const [service, selected] of selectedByService) {
    // a completed migration has no live process or cgroup fact
    if (service === "migration") {
      state.planned.delete("docker.container.migration.top");
      state.planned.delete("docker.container.migration.processes");
      state.planned.delete("cgroup.blueberry:compose:migration");
      continue;
    }
    const processes = await collectContainerProcesses(state, service, selected);
    const cgroupComplete = await collectContainerCgroup(state, service, selected, processes);

    // stop dependent collection after one unsettled or failed cgroup family
    if (!cgroupComplete) {
      return;
    }
  }

  // collect each image once from a validated representative container
  for (const role of IMAGE_ROLES) {
    const representativeService = role === "server" ? "api" : role;
    const selected = selectedByService.get(representativeService);

    // keep missing representative images unresolved
    if (selected === undefined) {
      continue;
    }
    await collectCommandProbe(
      state,
      `docker.image.${role}.inspect`,
      "/usr/bin/docker",
      ["image", "inspect", "--format", DOCKER_IMAGE_FORMAT, selected.raw.imageId],
      role,
    );
  }
  state.selectedContainers = selectedByService;
  await collectArtifactProbes(state, "blueberry-compose");
}

// format one portable file mode identity
function portableMode(value) {
  return (value & 0o7777).toString(8).padStart(4, "0");
}

// verify an existing artifact stays inside its fixed host or container root
async function requireContainedArtifactPath(state, physicalPath, profile, selected) {
  registerFilePath(state, physicalPath);
  const resolved = await state.adapters.filesystem.realpath(physicalPath);
  enforceDeadline(state);

  // bind container paths inside the selected process root
  if (selected !== null && physicalPath.startsWith(`/proc/${String(selected.raw.pid)}/root/`)) {
    const rootPath = `/proc/${String(selected.raw.pid)}/root`;
    registerFilePath(state, rootPath);
    const resolvedRoot = await state.adapters.filesystem.realpath(rootPath);
    enforceDeadline(state);

    // reject intermediate links that escaped the selected container root
    if (resolved !== resolvedRoot && !resolved.startsWith(`${resolvedRoot}/`)) {
      throw hostError("host.request", "container artifact escaped its selected root");
    }
    return;
  }
  const allowed = profile === "workstation-runner"
    ? [
      `${state.runnerAccount?.home ?? "/unresolved"}/`,
      "/etc/systemd/system/",
      "/opt/weather/",
      "/usr/local/bin/",
      "/usr/local/sbin/",
    ]
    : [
      "/etc/systemd/system/",
      "/opt/weather/current/",
      `/opt/weather/releases/${state.request.expectedReleaseId}/`,
      "/usr/local/bin/",
      "/usr/local/sbin/",
    ];

  // reject host artifacts that resolved outside all fixed profile roots
  if (!allowed.some(
    // match one exact fixed resolved root
    (root) => resolved.startsWith(root),
  )) {
    throw hostError("host.request", "host artifact escaped its fixed root");
  }
}

// collect one exact installed artifact from a validated runtime context
async function collectArtifactObservation(state, context, artifact, profile) {
  const probeId = `artifact.${sha256Bytes(`${context.id}\0${artifact.id}`)}`;
  const declaredPath = artifact.declaredPath ?? artifact.path ?? null;
  let resolvedPath = declaredPath;
  let selected = null;
  let physicalPath = declaredPath;
  let prerequisiteId = "nss.ubuntu.passwd";
  let scope = "host";
  const hostScoped = ["host-file", "runner-file", "systemd-unit"].includes(artifact.kind);

  // resolve the fixed runner home only from retained NSS bytes
  if (profile === "workstation-runner") {
    // stop before any account-home template ambiguity
    if (typeof declaredPath !== "string" || state.runnerAccount === undefined) {
      return;
    }
    resolvedPath = declaredPath.startsWith("${HOME}/")
      ? `${state.runnerAccount.home}${declaredPath.slice("${HOME}".length)}`
      : declaredPath;

    // reject runner-managed files outside the exact NSS home
    if (artifact.kind === "runner-file" &&
      !resolvedPath.startsWith(`${state.runnerAccount.home}/`)) {
      return;
    }
    physicalPath = resolvedPath;
    scope = "host";
  } else if (artifact.kind === "image-config") {
    const role = artifact.imageTarget;
    const representativeService = role === "server" ? "api" : role;
    prerequisiteId = `docker.image.${role}.inspect`;
    selected = state.selectedContainers?.get(representativeService) ?? null;

    // require one inspected representative image before deriving image configuration
    if (selected === null) {
      return;
    }
    const imageProbe = state.probes.find(
      // select the exact image role evidence
      (probe) => probe.id === `docker.image.${role}.inspect`,
    );
    const imageBytes = collectedProbeText(imageProbe);

    // bind image metadata only to retained selected bytes
    if (imageBytes === null) {
      return;
    }
    appendDerivedProbe(state, probeId, {
      artifactId: artifact.id,
      containerId: null,
      declaredPath: null,
      gid: null,
      imageId: selected.raw.imageId,
      mode: null,
      present: true,
      resolvedPath: `image://${role}`,
      scope: "image",
      sha256: sha256Bytes(Buffer.from(imageBytes)),
      symlinkTarget: null,
      type: "image-config",
      uid: null,
    }, [`docker.image.${role}.inspect`], context.id);
    return;
  } else if (artifact.kind === "container-file" || artifact.kind === "package-metadata") {
    const roleService = artifact.imageTarget === "server" ? "api" : artifact.imageTarget;
    const selectedService = context.service === "migration" ? roleService : context.service;
    selected = state.selectedContainers?.get(selectedService) ??
      state.selectedContainers?.get(roleService) ?? null;
    prerequisiteId = `docker.container.${selectedService ?? roleService}.inspect`;

    // require one exact active same-image representative
    if (selected === null || typeof declaredPath !== "string") {
      return;
    }
    physicalPath = `/proc/${String(selected.raw.pid)}/root${declaredPath}`;
    scope = context.service === null || context.service === "migration" ? "image" : "container";
  } else if (context.service !== null) {
    const representativeService = context.service === "migration" ? "api" : context.service;
    selected = state.selectedContainers?.get(representativeService) ?? null;
    prerequisiteId = `docker.container.${representativeService}.inspect`;

    // require the exact active service before binding a mounted host artifact
    if (selected === null) {
      return;
    }
  }

  // reject source-selected paths outside the finite validated domains
  if (typeof physicalPath !== "string" || physicalPath.includes("\0") ||
    physicalPath.split("/").includes("..")) {
    return;
  }
  const cacheKey = `${scope}\0${physicalPath}\0${selected?.raw?.imageId ?? "host"}`;
  const cached = state.artifactPhysicalCache.get(cacheKey);

  // reuse one physical stat and digest across source-owned context backlinks
  if (cached !== undefined) {
    appendDerivedProbe(state, probeId, {
      ...cached,
      artifactId: artifact.id,
      containerId: hostScoped || scope === "image" ? null : selected?.raw?.id ?? null,
      declaredPath,
      imageId: hostScoped ? null : selected?.raw?.imageId ?? null,
      resolvedPath,
      scope,
    }, [prerequisiteId], context.id);
    return;
  }
  let details;

  // retain exact present or absent filesystem state
  try {
    registerFilePath(state, physicalPath);
    details = await state.adapters.filesystem.lstat(physicalPath);
    enforceDeadline(state);
  } catch (error) {
    // retain only an exact missing-path result as absence
    if (error?.code !== "ENOENT") {
      return;
    }
    const missing = {
      gid: null,
      mode: null,
      present: false,
      sha256: null,
      symlinkTarget: null,
      type: "missing",
      uid: null,
    };
    state.artifactPhysicalCache.set(cacheKey, missing);
    appendDerivedProbe(state, probeId, {
      ...missing,
      artifactId: artifact.id,
      containerId: hostScoped || scope === "image" ? null : selected?.raw?.id ?? null,
      declaredPath,
      imageId: hostScoped ? null : selected?.raw?.imageId ?? null,
      resolvedPath,
      scope,
    }, [prerequisiteId], context.id);
    return;
  }

  // reject intermediate symlink escapes before reading any present bytes
  try {
    await requireContainedArtifactPath(state, physicalPath, profile, selected);
  } catch {
    state.complete = false;
    return;
  }
  let bytes = Buffer.alloc(0);
  let symlinkTarget = null;
  let type = "other";

  // hash regular files and retain symlink targets without following them
  if (details.isFile()) {
    type = "regular-file";
    try {
      bytes = await readBoundedFile(
        state.adapters.filesystem,
        physicalPath,
        state.request.limits.maxProbeBytes,
        state,
      );
    } catch {
      return;
    }
  } else if (details.isSymbolicLink()) {
    type = "symlink";
    registerFilePath(state, physicalPath);
    symlinkTarget = await state.adapters.filesystem.readlink(physicalPath);
    enforceDeadline(state);
  }
  const present = {
    gid: details.gid,
    mode: portableMode(details.mode),
    present: true,
    sha256: type === "regular-file" ? sha256Bytes(bytes) : null,
    symlinkTarget,
    type,
    uid: details.uid,
  };
  state.artifactPhysicalCache.set(cacheKey, present);
  appendDerivedProbe(state, probeId, {
    ...present,
    artifactId: artifact.id,
    containerId: hostScoped || scope === "image" ? null : selected?.raw?.id ?? null,
    declaredPath,
    imageId: hostScoped ? null : selected?.raw?.imageId ?? null,
    resolvedPath,
    scope,
  }, [prerequisiteId], context.id);
}

// collect every exact source-owned artifact after activation identity is proven
async function collectArtifactProbes(state, profile) {
  const artifactsById = new Map(state.source.runtimeMap.artifacts.map(
    // index reviewed artifact declarations
    (artifact) => [artifact.id, artifact],
  ));
  let attempted = 0;
  const groups = new Map();

  // traverse only exact contexts owned by this fixed profile
  contexts: for (const context of state.source.runtimeMap.contexts) {
    // skip other hosts and declarative-only alternatives
    if (context.profile !== profile || !contextRequiresPhysicalEvidence(context)) {
      continue;
    }

    // collect every declared installation in this runtime instance
    for (const artifactId of context.artifactIds) {
      attempted += 1;

      // stop artifact traversal at the reviewed cardinality bound
      if (attempted > state.request.limits.maxFiles) {
        state.boundsExceeded = true;
        state.complete = false;
        break contexts;
      }
      const artifact = artifactsById.get(artifactId);

      // leave unknown declarations unresolved rather than selecting a path
      if (!isRecord(artifact)) {
        continue;
      }

      // image configuration is already bound to retained Docker image metadata
      if (artifact.kind === "image-config") {
        await collectArtifactObservation(state, context, artifact, profile);
        continue;
      }
      const probeId = `artifact.${sha256Bytes(`${context.id}\0${artifact.id}`)}`;
      const declaredPath = artifact.declaredPath ?? artifact.path ?? null;

      // reject artifacts without one exact installed filesystem path
      if (typeof declaredPath !== "string") {
        state.complete = false;
        continue;
      }
      let resolvedPath = declaredPath;
      let selected = null;
      let prerequisiteId = "nss.ubuntu.passwd";
      let scope = "host";
      let subject;
      const hostScoped = ["host-file", "runner-file", "systemd-unit"].includes(artifact.kind);

      // resolve workstation paths only through the exact proven NSS account
      if (profile === "workstation-runner") {
        if (state.runnerAccount === undefined) {
          continue;
        }
        resolvedPath = declaredPath.startsWith("${HOME}/")
          ? `${state.runnerAccount.home}${declaredPath.slice("${HOME}".length)}`
          : declaredPath;
        subject = secureHostSubject(state, probeId, resolvedPath, {
          mode: artifact.expectedMode ?? "0644",
          readMode: "sha256",
        });
      } else if (artifact.kind === "container-file" || artifact.kind === "package-metadata") {
        const roleService = artifact.imageTarget === "server" ? "api" : artifact.imageTarget;
        const selectedService = context.service === "migration" ? roleService : context.service;
        selected = state.selectedContainers?.get(selectedService) ??
          state.selectedContainers?.get(roleService) ?? null;
        prerequisiteId = `docker.container.${selectedService ?? roleService}.inspect`;

        // require one active exact-image representative for installed image bytes
        if (selected === null || selected.raw.pid <= 0) {
          continue;
        }
        scope = context.service === null || context.service === "migration" ? "image" : "container";
        subject = {
          containerId: selected.raw.id,
          expectedGid: 10002,
          expectedMode: artifact.expectedMode ?? "0644",
          expectedUid: 10002,
          id: probeId,
          imageId: selected.raw.imageId,
          pid: selected.raw.pid,
          readMode: "sha256",
          relativeComponents: declaredPath.split("/").filter(Boolean),
        };
      } else {
        const representativeService = context.service === "migration" ? "api" : context.service;
        selected = representativeService === null
          ? null
          : state.selectedContainers?.get(representativeService) ?? null;
        prerequisiteId = representativeService === null
          ? "docker.containers.list"
          : `docker.container.${representativeService}.inspect`;
        subject = secureHostSubject(state, probeId, resolvedPath, {
          mode: artifact.expectedMode ?? "0644",
          readMode: "sha256",
        });
      }

      // leave any source path outside fixed secure domains unresolved
      if (subject === null) {
        state.complete = false;
        continue;
      }
      const operation = Object.hasOwn(subject, "pid") ? "container-proc-root" : "confined-files";
      const groupKey = operation === "container-proc-root"
        ? `${operation}\0${String(subject.pid)}\0${subject.containerId}`
        : `${operation}\0${subject.rootSelector}\0${subject.rootPath}`;
      let group = groups.get(groupKey);

      // create one pinned helper batch for each exact root or representative PID
      if (group === undefined) {
        group = { operation, plans: [], subjects: [] };
        groups.set(groupKey, group);
      }
      group.subjects.push(subject);
      group.plans.push({
        artifact,
        context,
        declaredPath,
        hostScoped,
        prerequisiteId,
        probeId,
        resolvedPath,
        scope,
        selected,
      });
    }
  }

  // execute each root-pinned batch once and derive exact per-context backlinks
  for (const group of groups.values()) {
    const result = await collectSecureReaderBatch(state, {
      contractVersion: HOST_SECURE_READER_REQUEST_VERSION,
      maximumBytes: state.request.limits.maxProbeBytes,
      operation: group.operation,
      profile,
      subjects: group.subjects,
    });

    // leave the complete group unresolved after any authority failure
    if (result === null) {
      continue;
    }
    for (let index = 0; index < group.plans.length; index += 1) {
      const plan = group.plans[index];
      const subject = result.subjects[index];
      appendDerivedProbe(state, plan.probeId, {
        artifactId: plan.artifact.id,
        containerId: plan.hostScoped || plan.scope === "image"
          ? null
          : plan.selected?.raw?.id ?? null,
        declaredPath: plan.declaredPath,
        gid: subject.identity.pre.gid,
        imageId: plan.hostScoped ? null : plan.selected?.raw?.imageId ?? null,
        mode: subject.identity.pre.mode,
        present: true,
        resolvedPath: plan.resolvedPath,
        scope: plan.scope,
        sha256: subject.contentSha256,
        symlinkTarget: null,
        type: "regular-file",
        uid: subject.identity.pre.uid,
      }, [plan.prerequisiteId], plan.context.id);
    }
  }
}

// derive the fixed nonroot account from one retained NSS probe
function retainRunnerAccount(state, account) {
  const fields = (collectedProbeText(account) ?? "").trim().split(":");
  const uid = /^[1-9][0-9]*$/u.test(fields[2] ?? "")
    ? Number.parseInt(fields[2], 10)
    : null;
  const gid = /^[1-9][0-9]*$/u.test(fields[3] ?? "")
    ? Number.parseInt(fields[3], 10)
    : null;

  // accept only the exact fixed nonroot account and NSS home
  if (fields.length === 7 && fields[0] === "ubuntu" &&
    Number.isSafeInteger(uid) && Number.isSafeInteger(gid) &&
    fields[5] === "/home/ubuntu") {
    state.runnerAccount = {
      gid,
      home: fields[5],
      uid,
    };
  }
}

// collect the fixed workstation runner evidence set
async function collectRunnerProbes(state) {
  const account = await collectCommandProbe(
    state,
    "nss.ubuntu.passwd",
    "/usr/bin/getent",
    ["passwd", "ubuntu"],
  );

  // collect every exact conditional runner unit
  for (const unit of RUNNER_UNITS) {
    await collectUnitProbeFamily(state, unit, true);
  }

  // do not enumerate home or account processes after failed NSS resolution
  if (probeSucceeded(account)) {
    retainRunnerAccount(state, account);

    // collect dependent runner facts only after exact NSS resolution
    if (state.runnerAccount === undefined) {
      state.complete = false;
    } else {
      await collectRunnerRootProbes(state);
      await collectFileProbe(state, "runner.processes", "directory", "/proc", "runner");
      await collectArtifactProbes(state, "workstation-runner");
    }
  }
}

// close the finite fact catalog with explicit prerequisite failures
function finalizeUnresolved(state) {
  const attempted = new Set(state.probes.map(
    // retain every actually attempted probe identity
    (probe) => probe.id,
  ));
  const terminal = state.deadlineExceeded
    ? ["collection.deadline"]
    : state.boundsExceeded
      ? ["collection.bounds"]
      : null;
  const unresolved = [];

  // explain every planned fact that could not be safely attempted
  for (const [id, prerequisiteIds] of state.planned) {
    // exclude facts with retained physical or typed raw attempts
    if (attempted.has(id)) {
      continue;
    }
    unresolved.push({
      code: "blocked-by-prerequisite",
      id,
      prerequisiteIds: terminal ?? (prerequisiteIds.length > 0
        ? prerequisiteIds
        : ["collection.incomplete"]),
    });
  }
  unresolved.sort(
    // retain deterministic unresolved fact ordering
    (left, right) => left.id.localeCompare(right.id),
  );
  return unresolved;
}

// enforce the shared source-owned descriptor factory on produced facts
function enforceCollectedProbeDescriptors(state) {
  const probes = new Map(state.probes.map(
    // expose only collector-owned complete stdout bytes to dynamic descriptors
    (probe) => [probe.id, {
      ...probe,
      decodedStdout: Buffer.from(probe.raw.stdout.dataBase64, "base64"),
    }],
  ));

  // fail the collection when any produced operation drifts from its validator
  for (const probe of state.probes) {
    const expected = expectedProbeIdentity(state.source, state.request.profile, probe.id, probes);

    // compare the complete operation identity before evidence serialization
    if (expected === null || probe.kind !== expected.kind ||
      probe.sourceScope !== expected.sourceScope ||
      canonicalHostJson(probe.operation) !== canonicalHostJson(expected.operation)) {
      state.complete = false;
    }
  }

  // enforce the same descriptor factory for every retained failed attempt
  for (const error of state.errors) {
    const expected = expectedProbeIdentity(state.source, state.request.profile, error.id, probes);

    // reject collector drift even when semantic bytes were unavailable
    if (expected === null || error.kind !== expected.kind ||
      error.sourceScope !== expected.sourceScope ||
      canonicalHostJson(error.operation) !== canonicalHostJson(expected.operation)) {
      state.complete = false;
    }
  }
}

// collect fixed read-only host evidence
export async function collectBackendHostEvidence(request, adapters) {
  const injected = adapters !== undefined;
  const adapterSet = adapters ?? productionAdapters();
  const source = await validateCollectionRequest(request, adapterSet, injected);
  const abortController = new AbortController();
  const startedAt = adapterSet.clock.now().toISOString();
  const state = {
    abortController,
    adapters: adapterSet,
    artifactPhysicalCache: new Map(),
    boundsExceeded: false,
    complete: true,
    deadlineExceeded: false,
    errors: [],
    filePaths: new Set(),
    injected,
    planned: plannedProbeCatalog(source, request.profile, true),
    probes: [],
    request,
    source,
    startedMonotonicMs: adapterSet.clock.monotonicMs(),
    totalBytes: 0,
  };
  const plannedArtifactCount = [...state.planned.keys()].filter(
    // count only per-context artifact installations
    (id) => id.startsWith("artifact."),
  ).length;

  // fail closed before any artifact traversal beyond the reviewed bound
  if (plannedArtifactCount > request.limits.maxFiles) {
    state.boundsExceeded = true;
    state.complete = false;
  }
  const deadlineTimer = setTimeout(
    // abort the current operation at the global wall-clock deadline
    () => abortController.abort(),
    request.limits.deadlineMs,
  );
  deadlineTimer.unref?.();

  // settle every started operation before finalizing evidence
  try {
    await collectCommonProbes(state);

    // collect only the selected fixed host profile while time remains
    if (!abortController.signal.aborted && request.profile === "blueberry-compose") {
      await collectBlueberryProbes(state);
    } else if (!abortController.signal.aborted) {
      await collectRunnerProbes(state);
    }
  } finally {
    clearTimeout(deadlineTimer);
  }

  // fail closed on total output or deadline exhaustion
  if (
    state.totalBytes > request.limits.maxTotalBytes ||
    markDeadline(state)
  ) {
    state.complete = false;
  }
  enforceCollectedProbeDescriptors(state);
  const unresolved = finalizeUnresolved(state);

  // unresolved required facts cannot become live evidence
  if (unresolved.length > 0) {
    state.complete = false;
  }
  state.probes.sort(
    // retain deterministic probe identity order
    (left, right) => left.id.localeCompare(right.id),
  );
  state.errors.sort(
    // retain deterministic diagnostic attempt order
    (left, right) => left.id.localeCompare(right.id),
  );
  const evidence = {
    authority: {
      canonicalEligible: false,
      inventorySha256: hostEvidenceSha256(source.inventory),
      kind: injected ? "injected-test" : "physical-host",
      liveEligible: !injected && state.complete,
      producerSha256: request.producerSha256,
      runtimeMapSha256: hostEvidenceSha256(source.runtimeMap),
    },
    complete: state.complete,
    completedAt: adapterSet.clock.now().toISOString(),
    contractVersion: HOST_EVIDENCE_CONTRACT_VERSION,
    errors: state.errors,
    hostRole: request.expectedHostRole,
    probes: state.probes,
    producer: HOST_PRODUCER,
    profile: request.profile,
    startedAt,
    unresolved,
  };

  // enforce the final canonical transport bound after base64 expansion
  if (Buffer.byteLength(canonicalHostJson(evidence)) > request.limits.maxTotalBytes) {
    throw hostError("host.bounds", "canonical host evidence exceeds the total byte bound");
  }
  return evidence;
}

// append one stable validation diagnostic
function addDiagnostic(diagnostics, code, path, message) {
  diagnostics.push({ code, path, message });
}

// decode and validate one retained raw field
function decodeRawField(field, path, diagnostics) {
  // require one closed base64 raw-byte record
  if (
    !hasExactKeys(field, ["byteLength", "dataBase64", "encoding", "sha256", "truncated"]) ||
    field.encoding !== "base64" ||
    typeof field.dataBase64 !== "string" ||
    !Number.isSafeInteger(field.byteLength) ||
    field.byteLength < 0 ||
    !DIGEST_PATTERN.test(field.sha256 ?? "") ||
    typeof field.truncated !== "boolean"
  ) {
    addDiagnostic(diagnostics, "host.schema", path, "raw byte field is invalid");
    return null;
  }
  const canonicalBase64 = field.dataBase64.length % 4 === 0 &&
    /^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/u.test(field.dataBase64);
  const bytes = canonicalBase64 ? Buffer.from(field.dataBase64, "base64") : null;

  // reject lenient or noncanonical base64 aliases
  if (bytes === null || bytes.toString("base64") !== field.dataBase64) {
    addDiagnostic(diagnostics, "host.digest", path, "raw bytes are not canonical base64");
    return null;
  }

  // never expose truncated or scrubbed bytes to semantic readers
  if (field.truncated) {
    if (field.dataBase64 !== "") {
      addDiagnostic(diagnostics, "host.digest", path, "truncated raw bytes must not be retained");
    }
    return null;
  }

  // bind every complete field to its exact length and digest
  if (bytes.length !== field.byteLength || sha256Bytes(bytes) !== field.sha256) {
    addDiagnostic(diagnostics, "host.digest", path, "raw bytes differ from their digest or length");
    return null;
  }
  return bytes;
}

// return the fixed identity for one common semantic probe
function commonProbeIdentity(id) {
  const identities = {
    "host.cgroup-namespace": { kind: "readlink", operation: { path: "/proc/self/ns/cgroup" } },
    "host.cgroup-statfs": { kind: "statfs", operation: { path: "/sys/fs/cgroup" } },
    "host.cpuinfo": { kind: "file", operation: { path: "/proc/cpuinfo" } },
    "host.filesystems": { kind: "file", operation: { path: "/proc/filesystems" } },
    "host.libc": { kind: "command", operation: { args: ["GNU_LIBC_VERSION"], program: "/usr/bin/getconf" } },
    "host.mountinfo": { kind: "file", operation: { path: "/proc/self/mountinfo" } },
    "host.os-release": { kind: "file", operation: { path: "/etc/os-release" } },
    "host.self-cgroup": { kind: "file", operation: { path: "/proc/self/cgroup" } },
    "host.uname-kernel": { kind: "command", operation: { args: ["-r"], program: "/usr/bin/uname" } },
    "host.uname-machine": { kind: "command", operation: { args: ["-m"], program: "/usr/bin/uname" } },
  };
  return identities[id] ?? null;
}

// decode one already validated JSON probe without trusting its descriptor
function descriptorProbeJson(probes, id) {
  const bytes = probes.get(id)?.decodedStdout;

  // parse only complete retained bytes
  if (!Buffer.isBuffer(bytes)) {
    return null;
  }
  try {
    return parseStrictJson(bytes, "host.probe");
  } catch {
    return null;
  }
}

// decode one already validated text probe for descriptor planning
function descriptorProbeText(probes, id) {
  const bytes = probes.get(id)?.decodedStdout;
  return Buffer.isBuffer(bytes) ? bytes.toString("utf8") : null;
}

// derive one exact source-owned probe descriptor from validated dependencies
function expectedProbeIdentity(source, profile, id, probes, context = ACTIVE_HOST_SEMANTIC_CONTEXT) {
  const common = commonProbeIdentity(id);

  // reuse the fixed common descriptor table
  if (common !== null) {
    return { ...common, sourceScope: "host" };
  }
  const unitMatch = id.match(/^systemd\.(.+)\.(show|enabled|files)$/u);

  // bind every systemd fact to one exact operation family
  if (unitMatch !== null) {
    const [, unit, fact] = unitMatch;

    // retain one exact effective property query
    if (fact === "show") {
      return {
        kind: "command",
        operation: {
          args: [
            "show",
            unit,
            "--no-pager",
            "--property=Id,LoadState,ActiveState,SubState,UnitFileState,Result,ExecMainStatus,FragmentPath,DropInPaths,ExecStart,ExecReload,ExecStop,TimeoutStartUSec,User,Group",
          ],
          program: "/usr/bin/systemctl",
        },
        sourceScope: "host",
      };
    }

    // retain exact enablement or selected unit-file provenance
    return fact === "enabled"
      ? {
        kind: "command",
        operation: { args: ["is-enabled", unit], program: "/usr/bin/systemctl" },
        sourceScope: "host",
      }
      : {
        kind: "derived",
        operation: { inputProbeIds: [`systemd.${unit}.show`], method: "derived-canonical-json-v1" },
        sourceScope: "host",
      };
  }
  const releasePaths = {
    "release.current": "/opt/weather/current/deploy/state/current-release",
    "release.previous": "/opt/weather/current/deploy/state/previous-release",
    "release.schema": "/opt/weather/current/deploy/state/schema-release",
  };

  // bind all release markers to exact secure source paths
  if (Object.hasOwn(releasePaths, id)) {
    return { kind: "file", operation: { path: releasePaths[id] }, sourceScope: "host" };
  }

  // bind the fixed Compose enumeration vector exactly
  if (id === "docker.containers.list") {
    return {
      kind: "command",
      operation: {
        args: ["ps", "--all", "--filter", "label=com.docker.compose.project=weather", "--no-trunc", "--format", context.dockerContainerListFormat],
        program: "/usr/bin/docker",
      },
      sourceScope: "host",
    };
  }
  const containerMatch = id.match(/^docker\.container\.([^.]+)\.(inspect|top|processes)$/u);

  // derive container-selected descriptors from the list before inspect semantics
  if (containerMatch !== null) {
    const [, service, fact] = containerMatch;
    const inspectId = `docker.container.${service}.inspect`;
    const selections = parseDockerContainerSelections(
      descriptorProbeText(probes, "docker.containers.list"),
    );
    const selection = selections?.find(
      // select the exact service identity from validated list bytes
      (candidate) => candidate.service === service,
    ) ?? null;
    const raw = descriptorProbeJson(probes, inspectId);
    const record = normalizeContainerInspect(Array.isArray(raw) ? raw[0] : raw);
    const containerId = fact === "inspect" ? selection?.containerId : record?.id;

    // refuse a descriptor when its exact validated dependency is unavailable
    if (typeof containerId !== "string") {
      return null;
    }
    if (fact === "inspect") {
      return {
        kind: "command",
        operation: { args: ["inspect", "--format", context.dockerContainerFormat, containerId], program: "/usr/bin/docker" },
        sourceScope: service,
      };
    }
    if (fact === "top") {
      return {
        kind: "command",
        operation: { args: ["top", containerId, "-eo", "pid,ppid,args"], program: "/usr/bin/docker" },
        sourceScope: service,
      };
    }
    return {
      kind: "derived",
      operation: { inputProbeIds: [inspectId, `docker.container.${service}.top`].sort(), method: "docker-top-procfs-v2" },
      sourceScope: service,
    };
  }
  const imageMatch = id.match(/^docker\.image\.([^.]+)\.inspect$/u);

  // derive image inspection only from the fixed representative container
  if (imageMatch !== null) {
    const role = imageMatch[1];
    const representative = role === "server" ? "api" : role;
    const raw = descriptorProbeJson(probes, `docker.container.${representative}.inspect`);
    const record = normalizeContainerInspect(Array.isArray(raw) ? raw[0] : raw);

    // refuse an image descriptor without exact representative bytes
    if (typeof record?.imageId !== "string") {
      return null;
    }
    return {
      kind: "command",
      operation: { args: ["image", "inspect", "--format", context.dockerImageFormat, record.imageId], program: "/usr/bin/docker" },
      sourceScope: role,
    };
  }
  const cgroupMatch = id.match(/^cgroup\.blueberry:compose:(.+)$/u);

  // bind direct cgroup facts to both container and process evidence
  if (cgroupMatch !== null) {
    const service = cgroupMatch[1];
    return {
      kind: "derived",
      operation: {
        inputProbeIds: [`docker.container.${service}.inspect`, `docker.container.${service}.processes`].sort(),
        method: "derived-canonical-json-v1",
      },
      sourceScope: service,
    };
  }
  const fixedDerived = {
    "operator.control-plane.files": { inputProbeIds: ["operator.installation-receipt"], method: "secure-control-plane-tree-v1", sourceScope: "operator" },
    "operator.installation-receipt": { inputProbeIds: ["systemd.weather-compose.service.show"], method: "derived-canonical-json-v1", sourceScope: "operator" },
    "operator.weather-compose.dropin": { inputProbeIds: ["systemd.weather-compose.service.show"], method: "derived-canonical-json-v1", sourceScope: "operator" },
    "runner.root.library": { inputProbeIds: ["nss.ubuntu.passwd"], method: "secure-runner-root-status-v1", sourceScope: "runner" },
    "runner.root.private": { inputProbeIds: ["nss.ubuntu.passwd"], method: "secure-runner-root-status-v1", sourceScope: "runner" },
  };

  // retain fixed derived operator and runner facts
  if (Object.hasOwn(fixedDerived, id)) {
    const selected = fixedDerived[id];
    return {
      kind: "derived",
      operation: { inputProbeIds: selected.inputProbeIds, method: selected.method },
      sourceScope: selected.sourceScope,
    };
  }

  // bind remaining operator file facts to their fixed typed derivation
  if (id.startsWith("operator.")) {
    const inputProbeIds = id.startsWith("operator.bundle.")
      ? ["operator.installation-receipt"]
      : [];
    return {
      kind: "derived",
      operation: { inputProbeIds, method: "derived-canonical-json-v1" },
      sourceScope: "operator",
    };
  }

  // bind the fixed account and process-enumeration facts
  if (id === "nss.ubuntu.passwd") {
    return { kind: "command", operation: { args: ["passwd", "ubuntu"], program: "/usr/bin/getent" }, sourceScope: "host" };
  }
  if (id === "runner.processes") {
    return { kind: "directory", operation: { path: "/proc" }, sourceScope: "runner" };
  }

  // bind each artifact to its exact source-owned context and activation prerequisite
  if (id.startsWith("artifact.")) {
    for (const context of source.runtimeMap.contexts) {
      // inspect only physical contexts on this fixed profile
      if (context.profile !== profile || !contextRequiresPhysicalEvidence(context)) {
        continue;
      }
      for (const artifactId of context.artifactIds) {
        const expectedId = `artifact.${sha256Bytes(`${context.id}\0${artifactId}`)}`;

        // return the exact per-instance artifact descriptor
        if (expectedId === id) {
          const artifact = source.runtimeMap.artifacts.find(
            // select the exact source-owned artifact
            (candidate) => candidate.id === artifactId,
          );
          const roleService = artifact?.imageTarget === "server" ? "api" : artifact?.imageTarget;
          const prerequisiteId = profile === "workstation-runner"
            ? "nss.ubuntu.passwd"
            : artifact?.kind === "image-config"
              ? `docker.image.${artifact.imageTarget}.inspect`
              : ["container-file", "package-metadata"].includes(artifact?.kind)
                ? `docker.container.${context.service === "migration"
                  ? roleService
                  : context.service ?? roleService}.inspect`
              : context.service === "migration"
                ? "docker.container.api.inspect"
                : context.service === null
                  ? "docker.containers.list"
                  : `docker.container.${context.service}.inspect`;
          return {
            kind: "derived",
            operation: { inputProbeIds: [prerequisiteId], method: "derived-canonical-json-v1" },
            sourceScope: context.id,
          };
        }
      }
    }
  }
  return null;
}

// return the exact accepted exit set for one probe family
function acceptedProbeExitCodes(probe) {
  // preserve systemd is-enabled documented state exits only
  if (/^systemd\.weather-adjustment-(?:archive|daily|monthly)\.(?:service|timer)\.enabled$/u.test(probe.id)) {
    return new Set([0, 1, 3, 4]);
  }
  return new Set([probe.kind === "command" ? 0 : null]);
}

// validate one exact accepted or failed probe envelope
function validateProbeEnvelope(probe, path, diagnostics, accepted, context = ACTIVE_HOST_SEMANTIC_CONTEXT) {
  // reject malformed probe identities before decoding bytes
  if (
    !hasExactKeys(probe, [
      "completedAt",
      "contractVersion",
      "id",
      "kind",
      "operation",
      "outcome",
      "raw",
      "sourceScope",
      "startedAt",
    ]) ||
    probe.contractVersion !== context.probeVersion ||
    typeof probe.id !== "string" ||
    !hasExactKeys(probe.raw, ["stderr", "stdout"]) ||
    !hasExactKeys(probe.outcome, ["exitCode", "status"])
  ) {
    addDiagnostic(diagnostics, "host.probe", path, "probe identity is invalid");
    return null;
  }
  const expected = commonProbeIdentity(probe.id);

  // bind common facts to their exact source-owned operations
  if (expected !== null && (probe.sourceScope !== "host" || probe.kind !== expected.kind ||
    canonicalHostJson(probe.operation) !== canonicalHostJson(expected.operation))) {
    addDiagnostic(diagnostics, "host.probe", path, "probe provenance differs from the fixed catalog");
  }
  const acceptedExitCodes = acceptedProbeExitCodes(probe);
  const validExitCode = probe.outcome.exitCode === null ||
    (Number.isSafeInteger(probe.outcome.exitCode) &&
      probe.outcome.exitCode >= 0 && probe.outcome.exitCode <= 255);
  const failureStatuses = new Set([
    "bounds",
    "cancelled",
    "error",
    "secret",
    "timeout",
    "transport-error",
  ]);
  const nullExitStatuses = new Set(["cancelled", "timeout", "transport-error"]);

  // keep semantic probes successful and diagnostic attempts closed and typed
  if (!validExitCode || (accepted
    ? probe.outcome.status !== "ok" || !acceptedExitCodes.has(probe.outcome.exitCode)
    : !failureStatuses.has(probe.outcome.status) ||
      (nullExitStatuses.has(probe.outcome.status)
        ? probe.outcome.exitCode !== null
        : probe.outcome.exitCode === null))) {
    addDiagnostic(diagnostics, "host.probe", path, "probe outcome is invalid for its evidence set");
  }
  const stdout = decodeRawField(probe.raw.stdout, `${path}.raw.stdout`, diagnostics);
  const stderr = decodeRawField(probe.raw.stderr, `${path}.raw.stderr`, diagnostics);

  // explain scrubbed output without retaining its value
  if (probe.outcome.status === "secret") {
    addDiagnostic(diagnostics, "host.secret", path, "probe output contained credential material");
  } else if (probe.outcome.status === "bounds" ||
    probe.raw.stdout.truncated || probe.raw.stderr.truncated) {
    addDiagnostic(diagnostics, "host.bounds", path, "probe output was truncated or exceeded a bound");
  }

  // require truncation to use one exact scrubbed failure status
  if ((probe.raw.stdout.truncated || probe.raw.stderr.truncated) &&
    !["bounds", "secret"].includes(probe.outcome.status)) {
    addDiagnostic(diagnostics, "host.probe", path, "truncated probe outcome is mislabeled");
  }

  // reject retained credential bytes in either stream
  if ((stdout !== null && containsSecretBytes(stdout)) ||
    (stderr !== null && containsSecretBytes(stderr))) {
    addDiagnostic(diagnostics, "host.secret", path, "probe retained secret-like bytes");
    return null;
  }
  return { ...probe, decodedStderr: stderr, decodedStdout: stdout };
}

// validate and index every typed raw probe
function indexEvidenceProbes(evidence, diagnostics, context = ACTIVE_HOST_SEMANTIC_CONTEXT) {
  const probes = new Map();
  const collectionStart = Date.parse(evidence?.startedAt ?? "");
  const collectionEnd = Date.parse(evidence?.completedAt ?? "");

  // require one valid enclosing collection interval
  if (!Number.isFinite(collectionStart) || !Number.isFinite(collectionEnd) || collectionStart > collectionEnd) {
    addDiagnostic(diagnostics, "host.bounds", "$.evidence", "collection interval is invalid");
  }

  // validate every retained probe independently
  for (let index = 0; index < (evidence?.probes?.length ?? 0); index += 1) {
    const probe = evidence.probes[index];
    const path = `$.evidence.probes[${String(index)}]`;
    const validated = validateProbeEnvelope(probe, path, diagnostics, true, context);

    // reject invalid or duplicate probe identities
    if (validated === null || probes.has(probe?.id)) {
      addDiagnostic(diagnostics, "host.probe", path, "probe identity is invalid or duplicated");
      continue;
    }
    probes.set(probe.id, validated);
    const startedAt = Date.parse(probe.startedAt ?? "");
    const completedAt = Date.parse(probe.completedAt ?? "");

    // bind every probe interval inside the collection interval
    if (
      !Number.isFinite(startedAt) ||
      !Number.isFinite(completedAt) ||
      startedAt > completedAt ||
      startedAt < collectionStart ||
      completedAt > collectionEnd
    ) {
      addDiagnostic(diagnostics, "host.bounds", path, "probe interval is outside collection bounds");
    }
  }
  return probes;
}

// validate failed attempts without exposing them to semantic readers
function validateEvidenceErrors(evidence, diagnostics, context = ACTIVE_HOST_SEMANTIC_CONTEXT) {
  const ids = new Set();
  const listedIds = evidence.errors.map(
    // retain the caller-supplied diagnostic ordering
    (item) => item?.id,
  );

  // require one deterministic diagnostic attempt ordering
  if (canonicalHostJson(listedIds) !== canonicalHostJson([...listedIds].sort())) {
    addDiagnostic(diagnostics, "host.probe", "$.evidence.errors", "error attempts are not sorted");
  }

  // validate every failed attempted envelope independently
  for (let index = 0; index < (evidence.errors?.length ?? 0); index += 1) {
    const error = evidence.errors[index];
    const path = `$.evidence.errors[${String(index)}]`;
    const validated = validateProbeEnvelope(error, path, diagnostics, false, context);

    // reject duplicate diagnostic attempt identities
    if (validated === null || ids.has(error?.id)) {
      addDiagnostic(diagnostics, "host.probe", path, "error attempt is invalid or duplicated");
      continue;
    }
    ids.add(error.id);
  }
  return ids;
}

// validate exact attempted and unresolved coverage of the finite probe catalog
function validateProbeCatalog(source, evidence, probes, diagnostics, context = ACTIVE_HOST_SEMANTIC_CONTEXT) {
  const composeShow = probeText(
    probes,
    "systemd.weather-compose.service.show",
    diagnostics,
    false,
  ) ?? "";
  const includeOperator = evidence.profile === "blueberry-compose" ||
    /\/opt\/weather\/operators\/schema-preserving-release\/[a-f0-9]{64}\//u.test(composeShow) || [
    ...probes.keys(),
    ...(evidence.errors ?? []).map(
      // retain failed operator attempt identities during catalog selection
      (item) => item?.id,
    ),
    ...(evidence.unresolved ?? []).map(
    // retain unresolved operator identities during catalog selection
    (item) => item?.id,
    ),
  ].some(
    // recognize the fixed operator probe namespace
    (id) => typeof id === "string" && id.startsWith("operator."),
  );
  const planned = plannedProbeCatalog(source, evidence.profile, includeOperator, context);
  const unresolvedIds = new Set();

  // bind every semantic probe to one exact source-owned descriptor
  for (const [id, probe] of probes) {
    const expected = expectedProbeIdentity(source, evidence.profile, id, probes, context);

    // reject unknown or caller-selected operation provenance
    if (expected === null || probe.kind !== expected.kind ||
      probe.sourceScope !== expected.sourceScope ||
      canonicalHostJson(probe.operation) !== canonicalHostJson(expected.operation)) {
      addDiagnostic(diagnostics, "host.probe", `$.probes.${id}`, "probe provenance differs from the fixed catalog");
    }
  }

  // bind every failed attempt to the same source-owned descriptor factory
  for (const error of evidence.errors) {
    const expected = expectedProbeIdentity(source, evidence.profile, error.id, probes, context);

    // reject every failed attempt without one exact source-owned operation
    if (expected === null || error.kind !== expected.kind ||
      error.sourceScope !== expected.sourceScope ||
      canonicalHostJson(error.operation) !== canonicalHostJson(expected.operation)) {
      addDiagnostic(diagnostics, "host.probe", `$.evidence.errors.${error.id}`, "failed probe provenance differs from the fixed catalog");
    }
  }

  // omit ephemeral migration details when five exact long-lived containers prove absence
  if (evidence.profile === "blueberry-compose") {
    const listed = parseDockerContainerSelections(
      probeText(probes, "docker.containers.list", diagnostics) ?? "",
    );
    const listedServices = listed?.map(
      // retain exact service labels independently of inspect success
      (selection) => selection.service,
    ).sort() ?? [];
    const expectedLongLived = context.services.filter(
      // select the five non-migration services
      (service) => service !== "migration",
    ).sort();

    // treat the selected five-service snapshot as direct migration absence evidence
    if (canonicalHostJson(listedServices) === canonicalHostJson(expectedLongLived) &&
      !probes.has("docker.container.migration.inspect")) {
      planned.delete("docker.container.migration.inspect");
      planned.delete("docker.container.migration.top");
      planned.delete("docker.container.migration.processes");
      planned.delete("cgroup.blueberry:compose:migration");
    }
    const migrationInspect = probeJson(
      probes,
      "docker.container.migration.inspect",
      diagnostics,
      false,
    );
    const migrationRecord = normalizeContainerInspect(
      Array.isArray(migrationInspect) ? migrationInspect[0] : migrationInspect,
    );

    // a completed one-shot has no live cgroup membership to retain
    if (
      isRecord(migrationRecord) &&
      migrationRecord.running === false &&
      migrationRecord.status === "exited"
    ) {
      planned.delete("cgroup.blueberry:compose:migration");
      planned.delete("docker.container.migration.top");

      // accept direct completed state without a nonexistent live process tree
      if (!probes.has("docker.container.migration.processes")) {
        planned.delete("docker.container.migration.processes");
      }
    }
  }

  // omit nonexistent runner unit bytes after exact systemd absence evidence
  if (evidence.profile === "workstation-runner") {
    for (const unit of context.runnerUnits) {
      const show = parseKeyValueLines(
        probeText(probes, `systemd.${unit}.show`, diagnostics) ?? "",
      );

      // a not-found unit has no effective cat or fragment bytes to retain
      if (show.LoadState === "not-found") {
        planned.delete(`systemd.${unit}.files`);
      }
    }
  }

  // validate every closed prerequisite failure record
  for (let index = 0; index < (evidence.unresolved?.length ?? 0); index += 1) {
    const unresolved = evidence.unresolved[index];
    const path = `$.evidence.unresolved[${String(index)}]`;

    // reject unknown duplicate overlapping or open unresolved claims
    if (
      !isRecord(unresolved) ||
      canonicalHostJson(Object.keys(unresolved).sort()) !==
        canonicalHostJson(["code", "id", "prerequisiteIds"]) ||
      unresolved.code !== "blocked-by-prerequisite" ||
      !planned.has(unresolved.id) ||
      unresolvedIds.has(unresolved.id) ||
      probes.has(unresolved.id) ||
      !Array.isArray(unresolved.prerequisiteIds) ||
      unresolved.prerequisiteIds.length === 0 ||
      canonicalHostJson(unresolved.prerequisiteIds) !==
        canonicalHostJson([...new Set(unresolved.prerequisiteIds)].sort())
    ) {
      addDiagnostic(diagnostics, "host.probe", path, "unresolved probe record is invalid");
      continue;
    }
    const allowedPrerequisites = new Set([
      ...planned.keys(),
      "collection.bounds",
      "collection.deadline",
      "collection.incomplete",
    ]);

    // reject caller-authored blocker identities
    if (unresolved.prerequisiteIds.some(
      // identify prerequisites outside this fixed catalog
      (id) => !allowedPrerequisites.has(id),
    )) {
      addDiagnostic(diagnostics, "host.probe", path, "unresolved prerequisite is unknown");
      continue;
    }
    const expectedPrerequisites = planned.get(unresolved.id) ?? [];
    const terminalPrerequisites = ["collection.bounds", "collection.deadline", "collection.incomplete"];
    const terminalRecord = unresolved.prerequisiteIds.length === 1 &&
      terminalPrerequisites.includes(unresolved.prerequisiteIds[0]);

    // require exact source-owned blockers unless one terminal collection bound applies
    if (!terminalRecord && canonicalHostJson(unresolved.prerequisiteIds) !==
      canonicalHostJson(expectedPrerequisites)) {
      addDiagnostic(diagnostics, "host.probe", path, "unresolved prerequisite differs from the fixed catalog");
      continue;
    }
    unresolvedIds.add(unresolved.id);

    // retain resource-terminal blockers as explicit bounds evidence
    if (unresolved.prerequisiteIds.some(
      // recognize one terminal bound sentinel
      (id) => id === "collection.bounds" || id === "collection.deadline",
    )) {
      addDiagnostic(diagnostics, "host.bounds", path, "probe was blocked by a collection bound");
    }
  }

  // require one and only one resolution for every planned fact
  for (const id of planned.keys()) {
    // reject omitted planned facts
    if (!probes.has(id) && !unresolvedIds.has(id)) {
      addDiagnostic(diagnostics, "host.probe", `$.probes.${id}`, "planned probe is neither attempted nor unresolved");
    }
  }

  // reject probes outside the fixed source-owned catalog
  for (const id of probes.keys()) {
    // reject extra producer-selected facts
    if (!planned.has(id)) {
      addDiagnostic(diagnostics, "host.probe", `$.probes.${id}`, "probe is outside the fixed catalog");
    }
  }

  // unresolved facts always prevent a passing live observation
  if (unresolvedIds.size > 0) {
    addDiagnostic(diagnostics, "host.probe", "$.evidence.unresolved", "required host facts remain unresolved");
  }
}

// read one validated probe stdout as UTF-8
function probeText(probes, id, diagnostics, required = true) {
  const probe = probes.get(id);

  // diagnose missing required semantic evidence
  if (probe === undefined) {
    if (required) {
      addDiagnostic(diagnostics, "host.probe", `$.probes.${id}`, "required probe is missing");
    }
    return null;
  }
  // never expose invalid or truncated raw bytes to semantic readers
  if (!Buffer.isBuffer(probe.decodedStdout)) {
    addDiagnostic(diagnostics, "host.probe", `$.probes.${id}`, "probe stdout is unavailable");
    return null;
  }
  try {
    return decodeUtf8Strict(probe.decodedStdout);
  } catch {
    addDiagnostic(diagnostics, "host.probe", `$.probes.${id}`, "probe stdout is not valid UTF-8");
    return null;
  }
}

// parse one canonical JSON probe result
function probeJson(probes, id, diagnostics, required = true) {
  const text = probeText(probes, id, diagnostics, required);

  // preserve optional missing results
  if (text === null) {
    return null;
  }
  try {
    const trimmed = text.trim();
    const value = parseStrictJson(trimmed);

    // require the fixed canonical JSON encoding
    if (canonicalHostJson(value) !== trimmed) {
      addDiagnostic(diagnostics, "host.probe", `$.probes.${id}`, "JSON probe bytes are not canonical");
    }
    return value;
  } catch (error) {
    addDiagnostic(diagnostics, "host.probe", `$.probes.${id}`, `JSON probe is invalid: ${String(error.message)}`);
    return null;
  }
}

// normalize selected Docker template values without reading hidden configuration
function normalizeContainerInspect(value) {
  // preserve malformed records for ordinary identity rejection
  if (!isRecord(value)) {
    return value;
  }
  const oneOff = typeof value.oneOff === "string"
    ? value.oneOff.toLowerCase() === "true"
    : value.oneOff;
  return {
    ...value,
    containerNumber: typeof value.containerNumber === "string"
      ? Number.parseInt(value.containerNumber, 10)
      : value.containerNumber,
    name: typeof value.name === "string" ? value.name.replace(/^\//u, "") : value.name,
    oneOff,
  };
}

// parse fixed KEY=value output without shell evaluation
function parseKeyValueLines(value) {
  const result = {};

  // retain the first exact value for each nonempty key
  for (const line of value.split(/\r?\n/u)) {
    const separator = line.indexOf("=");

    // ignore empty or unrelated lines
    if (separator <= 0) {
      continue;
    }
    const key = line.slice(0, separator);

    // retain the first value to reject ambiguity by comparison later
    if (!(key in result)) {
      result[key] = line.slice(separator + 1);
    }
  }
  return result;
}

// parse one Linux os-release assignment subset
function parseOsRelease(value) {
  const fields = parseKeyValueLines(value);
  // remove one simple double-quoted assignment wrapper
  const unquote = (item) => typeof item === "string" && /^".*"$/u.test(item)
    ? item.slice(1, -1)
    : item ?? "";
  return { id: unquote(fields.ID), version: unquote(fields.VERSION_ID) };
}

// derive the common host ABI record from retained raw probes
function buildHostIdentity(probes, diagnostics) {
  const architecture = probeText(probes, "host.uname-machine", diagnostics)?.trim() ?? "";
  const kernel = probeText(probes, "host.uname-kernel", diagnostics)?.trim() ?? "";
  const cpuinfo = probeText(probes, "host.cpuinfo", diagnostics) ?? "";
  const osRelease = parseOsRelease(probeText(probes, "host.os-release", diagnostics) ?? "");
  const libcAbi = probeText(probes, "host.libc", diagnostics)?.trim() ?? "";
  const mountinfo = probeText(probes, "host.mountinfo", diagnostics) ?? "";
  const filesystems = probeText(probes, "host.filesystems", diagnostics) ?? "";
  const cgroup = probeText(probes, "host.self-cgroup", diagnostics)?.trim() ?? "";
  const cgroupNamespace = probeText(probes, "host.cgroup-namespace", diagnostics)?.trim() ?? "";
  const cgroupStat = probeJson(probes, "host.cgroup-statfs", diagnostics);
  const cpuModel = cpuinfo.match(/^(?:model name|Model)\s*:\s*(.+)$/imu)?.[1]?.trim() ?? "";
  const cgroupMount = mountinfo.split(/\r?\n/u).find(
    // retain the exact cgroup2 mount record
    (line) => / - cgroup2 /u.test(line),
  );

  // require native cgroup2 mount and filesystem authority
  if (
    cgroupMount === undefined ||
    !filesystems.includes("cgroup2") ||
    cgroupStat?.type !== 1667723888 ||
    !/^0::\//u.test(cgroup)
  ) {
    addDiagnostic(diagnostics, "host.identity", "$.identity", "cgroup2 host identity is incomplete");
  }
  const cgroupRoot = cgroupMount?.split(" ")?.[4] ?? "";
  return {
    architecture,
    cgroupNamespace,
    cgroupRoot,
    cgroupVersion: "2",
    cpuModel,
    filesystemType: "cgroup2",
    kernel,
    libcAbi,
    osId: osRelease.id,
    osVersion: osRelease.version,
    probeIds: [
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
    ],
  };
}

// derive one systemd state record from exact raw output
function buildSystemdUnit(probes, unit, diagnostics, includeEnabled) {
  const prefix = `systemd.${unit}`;
  const show = parseKeyValueLines(probeText(probes, `${prefix}.show`, diagnostics) ?? "");
  const files = probeJson(probes, `${prefix}.files`, diagnostics, false) ?? [];
  const enabledText = includeEnabled
    ? probeText(probes, `${prefix}.enabled`, diagnostics)?.trim() ?? ""
    : show.UnitFileState ?? "";
  const probeIds = [`${prefix}.show`];

  // retain optional fragment byte records when installed
  if (probes.has(`${prefix}.files`)) {
    probeIds.push(`${prefix}.files`);
  }
  // retain the separate enabled-state attempt for runner units
  if (includeEnabled) {
    probeIds.push(`${prefix}.enabled`);
  }
  const allowedEnabled = new Set([
    "alias",
    "disabled",
    "enabled",
    "enabled-runtime",
    "generated",
    "indirect",
    "linked",
    "linked-runtime",
    "masked",
    "masked-runtime",
    "not-found",
    "static",
    "transient",
  ]);
  const declaredPaths = new Set([
    show.FragmentPath,
    ...(show.DropInPaths ?? "").split(/\s+/u),
  ].filter(Boolean));

  // require exact selected unit identity and state vocabulary
  if (
    show.Id !== unit ||
    !["active", "activating", "deactivating", "failed", "inactive"].includes(show.ActiveState) ||
    !["loaded", "not-found"].includes(show.LoadState) ||
    !allowedEnabled.has(enabledText)
  ) {
    addDiagnostic(diagnostics, "host.identity", `$.systemd.${unit}`, "systemd unit identity or state is invalid");
  }

  // validate every retained effective unit source byte record
  for (const file of Array.isArray(files) ? files : []) {
    let bytes = Buffer.alloc(0);

    // decode only one closed selected unit file record
    try {
      bytes = Buffer.from(file.dataBase64 ?? "", "base64");
    } catch {
      bytes = Buffer.alloc(0);
    }

    // bind semantic file claims to exact retained bytes and selected paths
    if (
      !hasExactKeys(file, ["byteLength", "dataBase64", "gid", "mode", "nlink", "path", "sha256", "uid"]) ||
      bytes.toString("base64") !== file.dataBase64 ||
      bytes.length !== file.byteLength ||
      !declaredPaths.has(file.path) ||
      sha256Bytes(bytes) !== file.sha256 ||
      containsSecretBytes(bytes) ||
      !/^[0-7]{4}$/u.test(file.mode ?? "") ||
      !Number.isSafeInteger(file.nlink) || file.nlink <= 0 ||
      !Number.isSafeInteger(file.uid) ||
      !Number.isSafeInteger(file.gid)
    ) {
      addDiagnostic(
        diagnostics,
        containsSecretBytes(bytes) ? "host.secret" : "host.binding",
        `$.systemd.${unit}.files`,
        "unit file bytes or path are invalid",
      );
    }
  }

  // loaded units require retained effective cat and source bytes
  if (show.LoadState === "loaded" && files.length === 0) {
    addDiagnostic(diagnostics, "host.probe", `$.systemd.${unit}`, "loaded unit bytes are incomplete");
  }
  return {
    activeState: show.ActiveState ?? "",
    dropInPaths: (show.DropInPaths ?? "").split(/\s+/u).filter(Boolean),
    enabledState: enabledText,
    execMainStatus: Number.parseInt(show.ExecMainStatus ?? "", 10),
    execStart: show.ExecStart ?? "",
    fragmentPath: show.FragmentPath ?? "",
    loadState: show.LoadState ?? "",
    probeIds: probeIds.sort(),
    result: show.Result ?? "",
    subState: show.SubState ?? "",
    unit,
    unitFiles: Array.isArray(files) ? files.map(
      // retain opaque fragment identities without reinterpreting content
      (file) => ({ path: file.path, sha256: file.sha256 }),
    ) : [],
  };
}

// build one typed process record
function buildProcessRecord(raw, probeId, containerId, role) {
  return {
    argv: raw.argv,
    cgroupPath: raw.cgroupPath,
    cmdlineSha256: sha256Bytes(canonicalHostJson(raw.argv)),
    containerId,
    exePath: raw.exePath,
    exeSha256: raw.exeSha256,
    mapsSha256: raw.mapsSha256,
    mappedLibcPath: raw.mappedLibcPath ?? null,
    mappedLibcSha256: raw.mappedLibcSha256 ?? null,
    pid: raw.pid,
    ppid: raw.ppid,
    probeIds: [probeId],
    role,
    rootOsReleaseSha256: raw.rootOsReleaseSha256 ?? null,
    startTimeTicks: raw.startTimeTicks,
  };
}

// recognize one physical typed process record
function isTypedProcessRecord(value) {
  const physical = [
    "argv",
    "cgroupPath",
    "exePath",
    "exeSha256",
    "mapsSha256",
    "mappedLibcPath",
    "mappedLibcSha256",
    "pid",
    "ppid",
    "raw",
    "rootOsReleaseSha256",
    "startTimeTicks",
  ];
  return hasExactKeys(value, physical) || hasExactKeys(value, [...physical, "secureRootProof"]);
}

// recompute physical process semantics from retained proc source bytes
function validatePhysicalProcessRecord(value, path, diagnostics, secureReaderSha256) {
  // require one closed physical process envelope
  const physicalKeys = [
    "argv",
    "cgroupPath",
    "exePath",
    "exeSha256",
    "mappedLibcPath",
    "mappedLibcSha256",
    "mapsSha256",
    "pid",
    "ppid",
    "raw",
    "rootOsReleaseSha256",
    "startTimeTicks",
  ];
  if (!hasExactKeys(value, physicalKeys) &&
    !hasExactKeys(value, [...physicalKeys, "secureRootProof"])) {
    addDiagnostic(diagnostics, "host.probe", path, "physical process envelope is invalid");
    return;
  }

  // require one closed embedded proc-source record
  const rawKeys = [
    "cgroup",
    "cmdline",
    "exeByteLength",
    "exeLink",
    "maps",
    "rootOsRelease",
    "stat",
  ];
  const runnerRawKeys = [...rawKeys, "status"];

  // require one closed container or fixed-account proc source shape
  if (!hasExactKeys(value.raw, rawKeys) && !hasExactKeys(value.raw, runnerRawKeys)) {
    addDiagnostic(diagnostics, "host.schema", path, "physical process raw sources are invalid");
    return;
  }
  const cmdline = decodeRawField(value.raw.cmdline, `${path}.raw.cmdline`, diagnostics);
  const cgroup = decodeRawField(value.raw.cgroup, `${path}.raw.cgroup`, diagnostics);
  const stat = decodeRawField(value.raw.stat, `${path}.raw.stat`, diagnostics);
  const maps = decodeRawField(value.raw.maps, `${path}.raw.maps`, diagnostics);
  const exeLink = decodeRawField(value.raw.exeLink, `${path}.raw.exeLink`, diagnostics);
  const status = value.raw.status === undefined
    ? null
    : decodeRawField(value.raw.status, `${path}.raw.status`, diagnostics);
  const osRelease = value.raw.rootOsRelease === null
    ? null
    : decodeRawField(value.raw.rootOsRelease, `${path}.raw.rootOsRelease`, diagnostics);

  // stop semantic recomputation when any required raw source failed validation
  if ([cmdline, cgroup, stat, maps, exeLink].some(
    // identify one unavailable required raw source
    (bytes) => bytes === null,
  )) {
    addDiagnostic(diagnostics, "host.probe", path, "physical process raw bytes are unavailable");
    return;
  }
  const statFields = parseProcStat(stat.toString("utf8"));
  const cgroupPath = cgroup.toString("utf8").match(/^0::(.+)$/mu)?.[1] ?? null;
  const argv = cmdline.toString("utf8").split("\0").filter(Boolean);
  const mappedLibcPath = maps.toString("utf8").match(
    /\s(\/[A-Za-z0-9._/+:-]*\/libc\.so(?:\.[0-9]+)*)$/mu,
  )?.[1] ?? null;

  // bind every derived process field to the exact retained proc bytes
  if (
    canonicalHostJson(argv) !== canonicalHostJson(value.argv) ||
    cgroupPath !== value.cgroupPath ||
    decodeUtf8Strict(exeLink) !== value.exePath ||
    statFields?.ppid !== value.ppid ||
    statFields?.startTimeTicks !== value.startTimeTicks ||
    sha256Bytes(maps) !== value.mapsSha256 ||
    mappedLibcPath !== value.mappedLibcPath ||
    (osRelease === null ? value.rootOsReleaseSha256 !== null :
      sha256Bytes(osRelease) !== value.rootOsReleaseSha256) ||
    !DIGEST_PATTERN.test(value.exeSha256 ?? "") ||
    !Number.isSafeInteger(value.raw.exeByteLength) ||
    value.raw.exeByteLength < 0 ||
    (value.mappedLibcPath === null ? value.mappedLibcSha256 !== null :
      !DIGEST_PATTERN.test(value.mappedLibcSha256 ?? "")) ||
    (status !== null && !/^Uid:\s+[0-9]+/mu.test(status.toString("utf8")))
  ) {
    addDiagnostic(diagnostics, "host.binding", path, "physical process semantics differ from raw proc bytes");
  }

  // bind every container ABI field to the retained secure proc-root proof
  if (value.secureRootProof !== undefined) {
    const proof = value.secureRootProof;

    // require the reviewed helper and one closed subject set
    if (!hasExactKeys(proof, ["contractVersion", "helperSha256", "subjects"]) ||
      proof.contractVersion !== "weather-backend-host-secure-root-proof/v1" ||
      proof.helperSha256 !== secureReaderSha256 || !Array.isArray(proof.subjects) ||
      ![1, 2].includes(proof.subjects.length)) {
      addDiagnostic(diagnostics, "host.probe", `${path}.secureRootProof`, "secure root proof identity is invalid");
      return;
    }
    const expectedKeys = [
      "containerId",
      "expectedGid",
      "expectedMode",
      "expectedUid",
      "id",
      "imageId",
      "pid",
      "readMode",
    ];

    // validate every retained helper subject independently
    for (let index = 0; index < proof.subjects.length; index += 1) {
      const subject = proof.subjects[index];
      const subjectPath = `${path}.secureRootProof.subjects[${String(index)}]`;

      // require one closed subject and request identity
      if (!hasExactKeys(subject, [
        "contentSha256",
        "expected",
        "identity",
        "procBinding",
        "relativeComponents",
      ]) || !hasExactKeys(subject.expected, expectedKeys) ||
        !DIGEST_PATTERN.test(subject.contentSha256 ?? "") ||
        !validateSecureComponents(subject.relativeComponents)) {
        addDiagnostic(diagnostics, "host.schema", subjectPath, "secure root proof subject is invalid");
        continue;
      }
      const expected = {
        ...subject.expected,
        relativeComponents: subject.relativeComponents,
      };

      // bind descriptor and proc identity through existing strict validators
      try {
        if (!hasExactKeys(subject.identity, ["post", "pre"]) ||
          canonicalHostJson(subject.identity.pre) !== canonicalHostJson(subject.identity.post)) {
          throw hostError("host.identity", "secure root descriptor identity changed");
        }
        validateSecureIdentity(subject.identity.pre, expected, "secure root proof");
        validateSecureProcBinding(subject.procBinding, expected);
        const proofDiagnostics = [];
        const proofCgroup = decodeRawField(
          subject.procBinding.cgroupBefore,
          `${subjectPath}.procBinding.cgroupBefore`,
          proofDiagnostics,
        );

        // bind the helper PID lifetime and cgroup bytes to the retained proc record
        if (subject.procBinding.startTimeBefore !== String(value.startTimeTicks) ||
          proofCgroup === null || proofDiagnostics.length > 0 || !proofCgroup.equals(cgroup)) {
          throw hostError("host.identity", "secure root process sources differ");
        }
      } catch {
        addDiagnostic(diagnostics, "host.identity", subjectPath, "secure root process identity differs");
      }
      const relativePath = `/${subject.relativeComponents.join("/")}`;

      // bind the fixed OS and maps-derived libc subjects to exact content digests
      if (index === 0 && (relativePath !== "/etc/os-release" ||
        subject.contentSha256 !== value.rootOsReleaseSha256)) {
        addDiagnostic(diagnostics, "host.digest", subjectPath, "secure OS content digest differs");
      } else if (index === 1 && subject.contentSha256 !== value.mappedLibcSha256) {
        addDiagnostic(diagnostics, "host.digest", subjectPath, "secure libc content digest differs");
      }
      if (index === 1 && relativePath !== value.mappedLibcPath) {
        addDiagnostic(diagnostics, "host.binding", subjectPath, "secure libc path differs from proc maps");
      }
    }
  }
}

// derive all Blueberry container image and cgroup records
function buildBlueberryRuntime(probes, diagnostics, context = ACTIVE_HOST_SEMANTIC_CONTEXT) {
  const selections = parseDockerContainerSelections(
    probeText(probes, "docker.containers.list", diagnostics) ?? "",
  );
  const selectedByService = new Map((selections ?? []).map(
    // bind each fixed service label to its selected container ID
    (selection) => [selection.service, selection.containerId],
  ));

  // require five long-lived containers plus at most one migration one-shot
  if (selections === null) {
    addDiagnostic(diagnostics, "host.cardinality", "$.docker.containers", "Compose container list is not exact");
  }
  const containers = [];
  const processes = [];
  const cgroups = [];
  const containersByService = new Map();

  // derive each exact Compose service independently
  for (const service of context.services) {
    const inspectId = `docker.container.${service}.inspect`;
    const topId = `docker.container.${service}.top`;
    const processId = `docker.container.${service}.processes`;
    const migration = service === "migration";
    const inspectValue = probeJson(probes, inspectId, diagnostics, !migration);
    const inspect = normalizeContainerInspect(
      Array.isArray(inspectValue) && inspectValue.length === 1
        ? inspectValue[0]
        : isRecord(inspectValue)
          ? inspectValue
          : null,
    );

    // retain no historical state for an absent ephemeral migration one-shot
    if (migration && inspect === null) {
      continue;
    }

    // reject missing, extra or substituted service identities
    if (
      !isRecord(inspect) ||
      !hasExactKeys(inspect, [
        "args",
        "cgroupParent",
        "containerNumber",
        "exitCode",
        "health",
        "id",
        "imageId",
        "imageReference",
        "memory",
        "name",
        "nanoCpus",
        "oneOff",
        "path",
        "pid",
        "pidsLimit",
        "project",
        "readOnlyRootfs",
        "running",
        "service",
        "startedAt",
        "status",
      ]) ||
      inspect.service !== service ||
      inspect.project !== "weather" ||
      inspect.containerNumber !== 1 ||
      inspect.oneOff !== migration ||
      selectedByService.get(service) !== inspect.id
    ) {
      addDiagnostic(diagnostics, "host.identity", `$.docker.containers.${service}`, "container identity differs from fixed Compose service");
      continue;
    }
    const topText = probeText(probes, topId, diagnostics, !migration) ?? "";
    const topLines = topText.trimEnd().split(/\r?\n/u);
    const topProbe = probes.get(topId);
    const processProbe = probes.get(processId);
    const processValue = probeJson(
      probes,
      processId,
      diagnostics,
      !migration || probes.has(processId),
    );
    const rawProcesses = isRecord(processValue) &&
      hasExactKeys(processValue, ["containerId", "dockerInitPid", "processes"])
      ? processValue.processes
      : [];

    // require the exact native top and proc-derived operation identities
    if (!migration && (
      topProbe?.kind !== "command" ||
      topProbe?.sourceScope !== service ||
      canonicalHostJson(topProbe?.operation) !== canonicalHostJson({
        args: ["top", inspect.id, "-eo", "pid,ppid,args"],
        program: "/usr/bin/docker",
      }) ||
      processProbe?.kind !== "derived" ||
      processProbe?.sourceScope !== service ||
      canonicalHostJson(processProbe?.operation) !== canonicalHostJson({
        inputProbeIds: [inspectId, topId].sort(),
        method: "docker-top-procfs-v2",
      })
    )) {
      addDiagnostic(diagnostics, "host.probe", `$.processes.${service}`, "process probe provenance is invalid");
    }

    // bind the native top source and derived process envelope to this container
    if (!migration && (
      topLines.length < 2 ||
      !/^\s*PID\s+PPID\s+COMMAND\s*$/u.test(topLines[0]) ||
      !isRecord(processValue) ||
      processValue.containerId !== inspect.id ||
      processValue.dockerInitPid !== inspect.pid ||
      !Array.isArray(rawProcesses)
    )) {
      addDiagnostic(diagnostics, "host.probe", `$.processes.${service}`, "native top or derived process envelope is invalid");
    }
    const topPids = new Set(topLines.slice(1).map(
      // parse one native top PID without accepting typed JSON
      (line) => Number.parseInt(line.match(/^\s*([1-9][0-9]*)\s+[0-9]+\s+\S/u)?.[1] ?? "", 10),
    ).filter(Number.isSafeInteger));

    // recompute every physical process record before selecting main identity
    for (let index = 0; index < (Array.isArray(rawProcesses) ? rawProcesses.length : 0); index += 1) {
      validatePhysicalProcessRecord(
        rawProcesses[index],
        `$.processes.${service}[${String(index)}]`,
        diagnostics,
        context.secureReaderSha256,
      );

      // require every container ABI record to retain its confined proc-root proof
      if (!isRecord(rawProcesses[index]?.secureRootProof)) {
        addDiagnostic(
          diagnostics,
          "host.probe",
          `$.processes.${service}[${String(index)}].secureRootProof`,
          "container secure root proof is missing",
        );
      } else if (rawProcesses[index].secureRootProof.subjects.some(
        // bind every helper subject to this exact inspected process and image
        (subject) => subject?.expected?.containerId !== inspect.id ||
          subject?.expected?.imageId !== inspect.imageId ||
          subject?.expected?.pid !== rawProcesses[index].pid ||
          subject?.procBinding?.containerId !== inspect.id ||
          subject?.procBinding?.imageId !== inspect.imageId ||
          subject?.procBinding?.pid !== rawProcesses[index].pid,
      )) {
        addDiagnostic(
          diagnostics,
          "host.identity",
          `$.processes.${service}[${String(index)}].secureRootProof`,
          "container secure root proof differs from inspected identity",
        );
      }
    }

    // require the derived process set to equal the retained native top rows
    if (!migration && (topPids.size !== topLines.length - 1 ||
      !Array.isArray(rawProcesses) || rawProcesses.length !== topPids.size ||
      rawProcesses.some(
        // identify a process missing from the native table
        (process) => !topPids.has(process?.pid),
      ))) {
      addDiagnostic(diagnostics, "host.probe", `$.processes.${service}`, "derived process set differs from native top");
    }

    // accept only a completed successful migration one-shot with no live process tree
    if (migration) {
      if (
        inspect.running !== false ||
        inspect.status !== "exited" ||
        inspect.exitCode !== 0 ||
        !Array.isArray(rawProcesses) ||
        rawProcesses.length !== 0
      ) {
        addDiagnostic(diagnostics, "host.identity", `$.docker.containers.${service}`, "migration one-shot state is not a completed success");
        continue;
      }
      const container = {
        actualArgv: null,
        configuredArgv: [inspect.path, ...(inspect.args ?? [])],
        containerId: inspect.id,
        containerNumber: inspect.containerNumber,
        cgroupParent: inspect.cgroupParent,
        exitCode: inspect.exitCode,
        health: inspect.health,
        imageId: inspect.imageId,
        imageReference: inspect.imageReference,
        initPid: null,
        mainPid: null,
        name: inspect.name,
        oneOff: true,
        probeIds: [inspectId, processId].sort(),
        readOnlyRootfs: inspect.readOnlyRootfs,
        running: false,
        service,
        startedAt: inspect.startedAt,
        status: inspect.status,
      };
      containers.push(container);
      containersByService.set(service, container);
      continue;
    }

    // require one bounded process tree containing the Docker init PID
    if (!Array.isArray(rawProcesses) || rawProcesses.length === 0 ||
      !rawProcesses.every(
        // require one closed typed process record shape
        (process) => isTypedProcessRecord(process),
      ) || !rawProcesses.some(
      // retain exact Docker init process identity
      (process) => process?.pid === inspect.pid,
    )) {
      addDiagnostic(diagnostics, "host.identity", `$.processes.${service}`, "container process tree is incomplete");
      continue;
    }
    const main = rawProcesses.find(
      // prefer the direct non-init application process
      (process) => process.pid !== inspect.pid && process.ppid === inspect.pid,
    ) ?? rawProcesses.find(
      // retain any non-init descendant as a bounded fallback
      (process) => process.pid !== inspect.pid,
    ) ?? rawProcesses[0];

    // retain every observed process with an explicit role
    for (const rawProcess of rawProcesses) {
      const role = rawProcess.pid === main.pid
        ? "main"
        : rawProcess.pid === inspect.pid
          ? "init"
          : "descendant";
      processes.push(buildProcessRecord(rawProcess, processId, inspect.id, role));
    }
    const configuredArgv = [inspect.path, ...(inspect.args ?? [])];
    const container = {
      actualArgv: main.argv,
      configuredArgv,
      containerId: inspect.id,
      containerNumber: inspect.containerNumber,
      cgroupParent: inspect.cgroupParent,
      exitCode: inspect.exitCode,
      health: inspect.health,
      imageId: inspect.imageId,
      imageReference: inspect.imageReference,
      initPid: inspect.pid,
      mainPid: main.pid,
      name: inspect.name,
      oneOff: inspect.oneOff,
      probeIds: [inspectId, topId, processId].sort(),
      readOnlyRootfs: inspect.readOnlyRootfs,
      running: inspect.running,
      service,
      startedAt: inspect.startedAt,
      status: inspect.status,
    };
    containers.push(container);
    containersByService.set(service, container);
    const cgroupId = `cgroup.blueberry:compose:${service}`;
    const cgroup = probeJson(probes, cgroupId, diagnostics);

    // bind cgroup membership to the exact init PID and path
    if (
      !isRecord(cgroup) ||
      (!hasExactKeys(cgroup, [
        "controllers",
        "cpuMax",
        "memoryHigh",
        "memoryMax",
        "path",
        "pid",
        "pidsMax",
        "scopeId",
      ]) && !hasExactKeys(cgroup, [
        "controllers",
        "cpuMax",
        "memoryHigh",
        "memoryMax",
        "path",
        "pid",
        "pidsMax",
        "raw",
        "scopeId",
      ])) ||
      cgroup.pid !== inspect.pid ||
      cgroup.path !== main.cgroupPath
    ) {
      addDiagnostic(diagnostics, "host.identity", `$.cgroups.${service}`, "container cgroup identity differs");
    } else {
      // bind the cgroup to its exact source-owned service accounting scope
      if (cgroup.scopeId !== `blueberry:compose:${service}`) {
        addDiagnostic(diagnostics, "host.binding", `$.cgroups.${service}.scopeId`, "container cgroup scope differs");
      }
      // recompute physical cgroup controller and limit semantics from retained bytes
      if (cgroup.raw !== undefined) {
        const rawPath = `$.cgroups.${service}.raw`;

        // reject open or malformed cgroup source records
        if (!hasExactKeys(cgroup.raw, [
          "controllers",
          "cpuMax",
          "memoryHigh",
          "memoryMax",
          "pidsMax",
        ])) {
          addDiagnostic(diagnostics, "host.schema", rawPath, "cgroup raw sources are invalid");
        } else {
          const controllers = decodeRawField(cgroup.raw.controllers, `${rawPath}.controllers`, diagnostics);
          const cpuMax = decodeRawField(cgroup.raw.cpuMax, `${rawPath}.cpuMax`, diagnostics);
          const memoryHigh = decodeRawField(cgroup.raw.memoryHigh, `${rawPath}.memoryHigh`, diagnostics);
          const memoryMax = decodeRawField(cgroup.raw.memoryMax, `${rawPath}.memoryMax`, diagnostics);
          const pidsMax = decodeRawField(cgroup.raw.pidsMax, `${rawPath}.pidsMax`, diagnostics);

          // bind typed controller and limit values to retained cgroupfs bytes
          if (
            canonicalHostJson(controllers.toString("utf8").trim().split(/\s+/u).filter(Boolean)) !==
              canonicalHostJson(cgroup.controllers) ||
            cpuMax.toString("utf8").trim() !== cgroup.cpuMax ||
            memoryHigh.toString("utf8").trim() !== cgroup.memoryHigh ||
            memoryMax.toString("utf8").trim() !== cgroup.memoryMax ||
            pidsMax.toString("utf8").trim() !== cgroup.pidsMax
          ) {
            addDiagnostic(diagnostics, "host.binding", rawPath, "cgroup semantics differ from retained bytes");
          }
        }
      }
      cgroups.push({
        controllers: cgroup.controllers,
        cpuMax: cgroup.cpuMax,
        memoryHigh: cgroup.memoryHigh,
        memoryMax: cgroup.memoryMax,
        path: cgroup.path,
        pidsMax: cgroup.pidsMax,
        probeIds: [cgroupId],
        scopeId: cgroup.scopeId,
      });
    }
  }
  const images = [];

  // derive each fixed image role separately from container configuration
  for (const role of context.imageRoles) {
    const probeId = `docker.image.${role}.inspect`;
    const imageValue = probeJson(probes, probeId, diagnostics);
    const image = Array.isArray(imageValue) && imageValue.length === 1
      ? imageValue[0]
      : isRecord(imageValue)
        ? imageValue
        : null;
    const matchingContainers = containers.filter(
      // retain services assigned to this exact image role
      (container) => context.imageRoleByService[container.service] === role,
    );

    // reject image substitutions across service roles
    if (
      !hasExactKeys(image, [
        "architecture",
        "cmd",
        "entrypoint",
        "imageId",
        "os",
        "repoDigests",
        "revision",
      ]) ||
      matchingContainers.some((container) => container.imageId !== image.imageId)
    ) {
      addDiagnostic(diagnostics, "host.identity", `$.docker.images.${role}`, "image identity differs from selected containers");
      continue;
    }
    images.push({
      architecture: image.architecture,
      declaredCmd: image.cmd,
      declaredEntrypoint: image.entrypoint,
      imageId: image.imageId,
      os: image.os,
      probeIds: [probeId],
      reference: matchingContainers[0]?.imageReference ?? null,
      repoDigests: image.repoDigests,
      revision: image.revision,
      role,
    });
  }
  const serverImage = images.find(
    // select the inspected server image for an absent migration installation
    (image) => image.role === "server",
  );

  // bind the absent migration context to installed server-image bytes only
  if (!containersByService.has("migration") && serverImage !== undefined) {
    containersByService.set("migration", {
      containerId: null,
      imageId: serverImage.imageId,
      imageOnly: true,
    });
  }
  return {
    cgroups,
    docker: {
      composeProject: "weather",
      containers,
      images,
      probeIds: ["docker.containers.list"],
    },
    containersByService,
    processes,
  };
}

// derive installed artifact observations and exact binding backlinks
function buildArtifacts(
  runtimeMap,
  probes,
  containersByService,
  imagesByRole,
  profile,
  diagnostics,
  account,
) {
  const artifactsById = new Map(runtimeMap.artifacts.map(
    // index exact source-owned artifacts
    (artifact) => [artifact.id, artifact],
  ));
  const observations = [];

  // derive one observation per exact runtime context artifact
  for (const context of runtimeMap.contexts.filter(
    // retain only physical contexts on this host profile
    (candidate) => candidate.profile === profile && contextRequiresPhysicalEvidence(candidate),
  )) {
    for (const artifactId of context.artifactIds) {
      const artifact = artifactsById.get(artifactId);
      const probeId = `artifact.${sha256Bytes(`${context.id}\0${artifactId}`)}`;
      const raw = probeJson(probes, probeId, diagnostics);
      const container = context.service === null ? null : containersByService.get(context.service) ?? null;

      // require one exact source-owned artifact record
      if (
        !isRecord(artifact) ||
        !hasExactKeys(raw, [
          "artifactId",
          "containerId",
          "declaredPath",
          "gid",
          "imageId",
          "mode",
          "present",
          "resolvedPath",
          "scope",
          "sha256",
          "symlinkTarget",
          "type",
          "uid",
        ]) ||
        raw.artifactId !== artifactId ||
        typeof raw.present !== "boolean" ||
        (raw.present && raw.type === "regular-file" && !DIGEST_PATTERN.test(raw.sha256 ?? "")) ||
        (raw.present && artifact.expectedMode !== undefined && raw.mode !== artifact.expectedMode) ||
        (raw.present && artifact.expectedType !== undefined && raw.type !== artifact.expectedType) ||
        (raw.present && artifact.ownerPolicy === "root" && (raw.uid !== 0 || raw.gid !== 0)) ||
        (raw.present && artifact.ownerPolicy === "runtime-user" &&
          (raw.uid !== 10002 || raw.gid !== 10002)) ||
        (raw.present && artifact.ownerPolicy === "nss-account" &&
          (raw.uid !== account?.uid || raw.gid !== account?.gid))
      ) {
        addDiagnostic(diagnostics, "host.binding", `$.artifacts.${artifactId}`, "artifact evidence is missing or substituted");
        continue;
      }
      const activeContainer = context.service === null
        ? null
        : containersByService.get(context.service) ?? null;

      // require every source-owned artifact in an active or image-backed context
      if (activeContainer !== null && raw.present !== true) {
        addDiagnostic(diagnostics, "host.binding", `$.artifacts.${artifactId}`, "active runtime artifact is absent");
      }
      const expectedDeclaredPath = artifact.declaredPath ?? artifact.path ?? null;
      const expectedResolvedPath = typeof expectedDeclaredPath === "string" && expectedDeclaredPath.startsWith("${HOME}/")
        ? `${account?.home ?? ""}${expectedDeclaredPath.slice("${HOME}".length)}`
        : artifact.kind === "image-config"
          ? `image://${artifact.imageTarget}`
          : expectedDeclaredPath;
      const imageOnlyMigration = context.service === "migration" && raw.scope === "image";
      const hostScoped = ["host-file", "runner-file", "systemd-unit"].includes(artifact.kind);
      const imageScoped = context.service === null && !hostScoped && raw.scope === "image";
      const expectedImage = imagesByRole.get(artifact.imageTarget);

      // bind path and container or account scope to this exact context
      if (
        raw.declaredPath !== expectedDeclaredPath ||
        raw.resolvedPath !== expectedResolvedPath ||
        (context.service !== null && container === null && !hostScoped) ||
        (hostScoped && raw.containerId !== null) ||
        (imageScoped && (raw.containerId !== null || raw.imageId !== expectedImage?.imageId)) ||
        (container !== null && !hostScoped && imageOnlyMigration &&
          (raw.containerId !== null || raw.imageId !== container.imageId)) ||
        (container !== null && !hostScoped && !imageOnlyMigration &&
          (raw.containerId !== container.containerId || raw.imageId !== container.imageId)) ||
        (context.service === null && raw.containerId !== null)
      ) {
        addDiagnostic(diagnostics, "host.binding", `$.artifacts.${artifactId}`, "artifact scope differs from its source context");
      }
      const bindingIds = runtimeMap.bindings.filter(
        // retain bindings in this exact source-owned instance
        (binding) => binding.contextId === context.id && binding.artifactIds.includes(artifactId),
      ).map(
        // retain stable binding identities only
        (binding) => binding.id,
      ).sort();

      // reject artifacts without an exact source binding
      if (bindingIds.length === 0) {
        addDiagnostic(diagnostics, "host.binding", `$.artifacts.${artifactId}`, "artifact has no exact runtime binding");
      }
      observations.push({
        ...raw,
        bindingIds,
        kind: artifact.kind,
        probeIds: [probeId],
      });
    }
  }
  return observations;
}

// parse one closed operator file observation
function operatorFileEnvelope(probes, id, diagnostics, retainBytes, retainSelection = false) {
  const value = probeJson(probes, id, diagnostics, false);

  // leave an omitted optional family for the caller to diagnose as one closure
  if (value === null) {
    return null;
  }
  const expectedKeys = retainSelection
    ? ["file", "raw", "selection"]
    : retainBytes
      ? ["file", "raw"]
      : ["file"];
  const fileKeys = ["byteLength", "gid", "mode", "nlink", "path", "sha256", "uid"];

  // require one closed file identity envelope
  if (!hasExactKeys(value, expectedKeys) || !hasExactKeys(value.file, fileKeys) ||
    !Number.isSafeInteger(value.file.byteLength) || value.file.byteLength < 0 ||
    !Number.isSafeInteger(value.file.uid) || !Number.isSafeInteger(value.file.gid) ||
    !Number.isSafeInteger(value.file.nlink) || value.file.nlink <= 0 ||
    !/^[0-7]{4}$/u.test(value.file.mode ?? "") ||
    !DIGEST_PATTERN.test(value.file.sha256 ?? "") || typeof value.file.path !== "string") {
    addDiagnostic(diagnostics, "host.schema", `$.release.operator.${id}`, "operator file envelope is invalid");
    return null;
  }
  let bytes = null;

  // require one closed immutable-first receipt selection record
  if (retainSelection && !hasExactKeys(value.selection, [
    "bootstrapPath",
    "immutablePath",
    "immutableStatus",
    "selectedPath",
  ])) {
    addDiagnostic(diagnostics, "host.binding", `$.release.operator.${id}.selection`, "operator receipt selection is invalid");
    return null;
  }

  // validate retained semantic bytes against the exact file identity
  if (retainBytes) {
    if (!hasExactKeys(value.raw, ["bytes"])) {
      addDiagnostic(diagnostics, "host.schema", `$.release.operator.${id}.raw`, "operator raw bytes are invalid");
      return null;
    }
    bytes = decodeRawField(value.raw.bytes, `$.release.operator.${id}.raw.bytes`, diagnostics);
    if (bytes === null || bytes.length !== value.file.byteLength || sha256Bytes(bytes) !== value.file.sha256) {
      addDiagnostic(diagnostics, "host.binding", `$.release.operator.${id}`, "operator file bytes differ from metadata");
      return null;
    }
  }
  return { bytes, file: value.file, selection: value.selection ?? null };
}

// parse one exact systemd execution vector
function parseSystemdExecVector(value) {
  const matches = [...String(value ?? "").matchAll(/\{ path=([^ ;]+) ; argv\[\]=([^;]+?) ;/gu)];

  // accept exactly one effective command vector
  return matches.length === 1
    ? `${matches[0][1]}|${matches[0][2].trim()}`
    : null;
}

// validate one complete predecessor-protected control-plane tree
function validateControlPlaneTree(probes, diagnostics) {
  const tree = probeJson(probes, "operator.control-plane.files", diagnostics, false);
  const path = "$.release.operator.controlPlane";

  // require the closed v17 tree envelope
  if (!hasExactKeys(tree, ["contractVersion", "controlPlaneSha256", "directories", "files"]) ||
    tree.contractVersion !== "weather-schema-preserving-control-plane-tree/v1" ||
    !DIGEST_PATTERN.test(tree.controlPlaneSha256 ?? "") ||
    !Array.isArray(tree.directories) || !Array.isArray(tree.files)) {
    addDiagnostic(diagnostics, "host.schema", path, "operator control-plane tree is invalid");
    return null;
  }
  const directoryPaths = tree.directories.map(
    // retain exact directory proof order
    (directory) => directory?.path,
  );
  const filePaths = tree.files.map(
    // retain exact protected file order
    (file) => file?.path,
  );
  const protectedFilePaths = filePaths.slice(0, -1);

  // require the exact predecessor order and every fixed protected root
  if (canonicalHostJson(directoryPaths) !== canonicalHostJson([...new Set(directoryPaths)].sort()) ||
    filePaths.at(-1) !== "deploy/compose.yaml" ||
    canonicalHostJson(protectedFilePaths) !== canonicalHostJson([...new Set(protectedFilePaths)].sort()) ||
    protectedFilePaths.includes("deploy/compose.yaml") ||
    !["deploy/postgres", "deploy/scripts", "deploy/sudoers", "deploy/systemd"].every(
      // require one complete root directory listing
      (root) => directoryPaths.includes(root),
    ) || !filePaths.includes("deploy/compose.yaml")) {
    addDiagnostic(diagnostics, "host.binding", path, "operator control-plane paths are incomplete");
    return null;
  }
  const declaredFiles = new Set(["deploy/compose.yaml"]);
  const declaredDirectories = new Set(directoryPaths);

  // validate every complete directory listing
  for (const directory of tree.directories) {
    // require one closed sorted entry set
    if (!hasExactKeys(directory, ["entries", "path"]) ||
      typeof directory.path !== "string" || !Array.isArray(directory.entries) ||
      !directory.path.startsWith("deploy/") || directory.path.split("/").includes("..")) {
      addDiagnostic(diagnostics, "host.schema", path, "operator control-plane directory is invalid");
      return null;
    }
    const names = directory.entries.map(
      // retain exact directory entry order
      (entry) => entry?.name,
    );

    // reject duplicate unsorted hidden or unsupported directory entries
    if (canonicalHostJson(names) !== canonicalHostJson([...new Set(names)].sort()) ||
      directory.entries.some(
        // require one literal regular-file or directory entry
        (entry) => !hasExactKeys(entry, ["name", "type"]) ||
          typeof entry.name !== "string" || entry.name.length === 0 ||
          entry.name.includes("/") || [".", ".."].includes(entry.name) ||
          !["directory", "regular-file"].includes(entry.type),
      )) {
      addDiagnostic(diagnostics, "host.binding", path, "operator control-plane entries are invalid");
      return null;
    }

    // retain every entry for exact file and recursive-directory closure
    for (const entry of directory.entries) {
      const entryPath = `${directory.path}/${entry.name}`;

      // classify one declared entry
      if (entry.type === "directory") {
        if (!declaredDirectories.has(entryPath)) {
          addDiagnostic(diagnostics, "host.binding", path, "operator control-plane directory proof is incomplete");
          return null;
        }
      } else {
        declaredFiles.add(entryPath);
      }
    }
  }

  // validate every regular file identity and predecessor digest input
  for (const file of tree.files) {
    // require one closed regular file proof
    if (!hasExactKeys(file, ["identity", "path", "sha256"]) ||
      typeof file.path !== "string" || !DIGEST_PATTERN.test(file.sha256 ?? "") ||
      !hasExactKeys(file.identity, [
        "ctimeNs", "dev", "gid", "ino", "mode", "mtimeNs", "nlink", "size", "type", "uid",
      ]) || file.identity.type !== "regular-file" ||
      !Number.isSafeInteger(file.identity.uid) || !Number.isSafeInteger(file.identity.gid) ||
      file.identity.uid !== 0 || file.identity.gid !== 0 ||
      !/^[0-7]{4}$/u.test(file.identity.mode ?? "")) {
      addDiagnostic(diagnostics, "host.identity", path, "operator control-plane file identity is invalid");
      return null;
    }
  }
  const expectedPaths = [
    ...[...declaredFiles].filter(
      // sort only the recursive protected-root files
      (filePath) => filePath !== "deploy/compose.yaml",
    ).sort(),
    "deploy/compose.yaml",
  ];
  const digest = sha256Bytes(Buffer.from(tree.files.map(
    // preserve the predecessor path-NUL-digest framing
    (file) => `${file.path}\0${file.sha256}\n`,
  ).join("")));

  // bind the complete directory walk to the exact protected file set and digest
  if (canonicalHostJson(filePaths) !== canonicalHostJson(expectedPaths) ||
    digest !== tree.controlPlaneSha256) {
    addDiagnostic(diagnostics, "host.digest", path, "operator control-plane tree digest differs");
    return null;
  }
  return tree;
}

// derive the fixed release identity record
function buildRelease(probes, diagnostics, profile) {
  const currentRelease = probeText(probes, "release.current", diagnostics, false)?.trim() ?? null;
  const previousRelease = probeText(probes, "release.previous", diagnostics, false)?.trim() ?? null;
  const schemaRelease = probeText(probes, "release.schema", diagnostics, false)?.trim() ?? null;
  const probeIds = ["release.current", "release.previous", "release.schema"].filter(
    // retain only collected release identities
    (id) => probes.has(id),
  );
  const operatorIds = [
    "operator.bundle.schema-preserving-release.mjs",
    "operator.bundle.schema-preserving-release.sh",
    "operator.control-plane.files",
    "operator.forced.dispatcher",
    "operator.forced.remote-ops",
    "operator.installation-receipt",
    "operator.weather-compose.dropin",
  ];
  const show = parseKeyValueLines(
    probeText(probes, "systemd.weather-compose.service.show", diagnostics, false) ?? "",
  );
  const operatorSelected = profile === "blueberry-compose" || operatorIds.some(
    // recognize one retained member of the fixed operator family
    (id) => probes.has(id),
  ) || /\/opt\/weather\/operators\/schema-preserving-release\/[a-f0-9]{64}\//u.test(show.ExecStart ?? "");
  const operatorProbeIds = operatorIds.filter(
    // retain only collected operator identities
    (id) => probes.has(id),
  );
  let bundleSha256 = null;
  let dispatcherSha256 = null;
  let dropInSha256 = null;
  let receiptSha256 = null;
  let remoteOpsSha256 = null;

  // validate the complete selected current-v1 operator closure
  if (operatorSelected) {
    const dispatcher = operatorFileEnvelope(probes, "operator.forced.dispatcher", diagnostics, false);
    const remoteOps = operatorFileEnvelope(probes, "operator.forced.remote-ops", diagnostics, false);
    const receiptEnvelope = operatorFileEnvelope(probes, "operator.installation-receipt", diagnostics, true, true);
    const controlPlane = validateControlPlaneTree(probes, diagnostics);
    const bundleModule = operatorFileEnvelope(probes, "operator.bundle.schema-preserving-release.mjs", diagnostics, false);
    const bundleScript = operatorFileEnvelope(probes, "operator.bundle.schema-preserving-release.sh", diagnostics, false);
    const dropIn = operatorFileEnvelope(probes, "operator.weather-compose.dropin", diagnostics, true);
    const files = probeJson(probes, "systemd.weather-compose.service.files", diagnostics, false);

    // require every physical member before deriving operator authority
    if ([dispatcher, remoteOps, receiptEnvelope, controlPlane, bundleModule, bundleScript, dropIn].some(
      // detect one omitted or invalid operator member
      (value) => value === null,
    ) || !Array.isArray(files) || files.length !== 2) {
      addDiagnostic(diagnostics, "host.probe", "$.release.operator", "operator evidence closure is incomplete");
    } else {
      let receipt = null;
      const receiptText = receiptEnvelope.bytes.toString("utf8");

      // parse only canonical compact receipt bytes with one final newline
      try {
        receipt = parseStrictJson(receiptEnvelope.bytes.subarray(0, -1), "host.binding");
      } catch {
        receipt = null;
      }
      const receiptKeys = [
        "baseUnitSha256",
        "bundleSha256",
        "contractVersion",
        "controlPlaneSha256",
        "controlPlaneVersion",
        "dropInSha256",
        "execReload",
        "execStart",
        "execStop",
        "helperDirectory",
        "helperScript",
        "timeoutStartSec",
      ];
      const canonicalReceipt = isRecord(receipt)
        ? `${canonicalHostJson(receipt)}\n`
        : "";
      const basePath = "/etc/systemd/system/weather-compose.service";
      const dropInPath = "/etc/systemd/system/weather-compose.service.d/10-schema-preserving-release.conf";
      const baseFile = files[0];
      const dropInFile = files[1];
      const fileKeys = ["byteLength", "dataBase64", "gid", "mode", "nlink", "path", "sha256", "uid"];
      const baseBytes = hasExactKeys(baseFile, fileKeys)
        ? Buffer.from(baseFile.dataBase64, "base64")
        : Buffer.alloc(0);
      const selectedDropInBytes = hasExactKeys(dropInFile, fileKeys)
        ? Buffer.from(dropInFile.dataBase64, "base64")
        : Buffer.alloc(0);
      const bundleManifest = `${bundleModule.file.sha256}  schema-preserving-release.mjs\n` +
        `${bundleScript.file.sha256}  schema-preserving-release.sh\n`;
      const expectedBundle = sha256Bytes(Buffer.from(bundleManifest));
      const expectedReceiptPaths = isRecord(receipt) ? [
        `/opt/weather/current/deploy/state/schema-preserving-release-v1/installations/${receipt.bundleSha256}.json`,
        "/opt/weather/current/deploy/state/schema-preserving-release-v1/installation.json",
      ] : [];
      const immutableReceiptPath = expectedReceiptPaths[0] ?? null;
      const bootstrapReceiptPath = expectedReceiptPaths[1] ?? null;
      const selection = receiptEnvelope.selection;
      const selectionValid = hasExactKeys(selection, [
        "bootstrapPath",
        "immutablePath",
        "immutableStatus",
        "selectedPath",
      ]) && selection.bootstrapPath === bootstrapReceiptPath &&
        selection.immutablePath === immutableReceiptPath &&
        [["present", immutableReceiptPath], ["absent", bootstrapReceiptPath]].some(
          // accept only immutable-first present or proven-absent fallback selection
          ([status, path]) => selection.immutableStatus === status &&
            selection.selectedPath === path && receiptEnvelope.file.path === path,
        );
      const vectors = {
        execReload: parseSystemdExecVector(show.ExecReload),
        execStart: parseSystemdExecVector(show.ExecStart),
        execStop: parseSystemdExecVector(show.ExecStop),
      };

      // bind receipt semantics to raw effective unit and installed file bytes
      if (!isRecord(receipt) || !hasExactKeys(receipt, receiptKeys) ||
        receipt.contractVersion !== "weather-schema-preserving-release-installation/v1" ||
        receiptText !== canonicalReceipt ||
        receipt.controlPlaneSha256 !== controlPlane.controlPlaneSha256 ||
        receipt.controlPlaneVersion !== 17 ||
        receipt.timeoutStartSec !== 600 || show.TimeoutStartUSec !== "10min" ||
        receipt.execStart !== vectors.execStart || receipt.execReload !== vectors.execReload ||
        receipt.execStop !== vectors.execStop ||
        receipt.helperDirectory !== `/opt/weather/operators/schema-preserving-release/${receipt.bundleSha256}` ||
        receipt.helperScript !== `${receipt.helperDirectory}/schema-preserving-release.sh` ||
        receipt.bundleSha256 !== expectedBundle ||
        !selectionValid || !expectedReceiptPaths.includes(receiptEnvelope.file.path) ||
        receiptEnvelope.file.uid !== 0 || receiptEnvelope.file.gid !== 0 ||
        receiptEnvelope.file.mode !== "0600" || receiptEnvelope.file.nlink !== 1 ||
        bundleModule.file.path !== `${receipt.helperDirectory}/schema-preserving-release.mjs` ||
        bundleModule.file.uid !== 0 || bundleModule.file.gid !== 0 || bundleModule.file.mode !== "0644" || bundleModule.file.nlink !== 1 ||
        bundleScript.file.path !== receipt.helperScript || bundleScript.file.uid !== 0 ||
        bundleScript.file.gid !== 0 || bundleScript.file.mode !== "0755" || bundleScript.file.nlink !== 1 ||
        dispatcher.file.path !== "/usr/local/bin/weather-ssh-dispatch" || dispatcher.file.uid !== 0 ||
        dispatcher.file.gid !== 0 || dispatcher.file.mode !== "0755" || dispatcher.file.nlink !== 1 ||
        remoteOps.file.path !== "/usr/local/sbin/weather-remote-ops" || remoteOps.file.uid !== 0 ||
        remoteOps.file.gid !== 0 || remoteOps.file.mode !== "0755" || remoteOps.file.nlink !== 1 ||
        !hasExactKeys(baseFile, fileKeys) || baseFile.path !== basePath || baseFile.uid !== 0 ||
        baseFile.gid !== 0 || baseFile.mode !== "0644" || baseFile.nlink !== 1 ||
        baseBytes.length !== baseFile.byteLength || sha256Bytes(baseBytes) !== baseFile.sha256 || receipt.baseUnitSha256 !== baseFile.sha256 ||
        !hasExactKeys(dropInFile, fileKeys) || dropInFile.path !== dropInPath || dropInFile.uid !== 0 ||
        dropInFile.gid !== 0 || dropInFile.mode !== "0644" || dropInFile.nlink !== 1 ||
        selectedDropInBytes.length !== dropInFile.byteLength || sha256Bytes(selectedDropInBytes) !== dropInFile.sha256 ||
        !selectedDropInBytes.equals(dropIn.bytes) || receipt.dropInSha256 !== dropIn.file.sha256 ||
        dropIn.file.path !== dropInPath || dropIn.file.uid !== 0 || dropIn.file.gid !== 0 ||
        dropIn.file.mode !== "0644" || dropIn.file.nlink !== 1 ||
        !selectedDropInBytes.toString("utf8").includes("TimeoutStartSec=600")) {
        addDiagnostic(diagnostics, "host.binding", "$.release.operator", "operator receipt or effective unit binding differs");
      } else {
        bundleSha256 = receipt.bundleSha256;
        dispatcherSha256 = dispatcher.file.sha256;
        dropInSha256 = dropIn.file.sha256;
        receiptSha256 = receiptEnvelope.file.sha256;
        remoteOpsSha256 = remoteOps.file.sha256;
      }
    }
  }
  return {
    currentRelease,
    operator: {
      bundleSha256,
      dispatcherSha256,
      dropInSha256,
      forcedCommandSha256: dispatcherSha256,
      probeIds: operatorProbeIds,
      receiptSha256,
      remoteOpsSha256,
      sourceCommit: null,
    },
    previousRelease,
    probeIds,
    schemaRelease,
  };
}

// derive one fixed workstation runner observation
function buildRunner(probes, systemd, processes, diagnostics, context = ACTIVE_HOST_SEMANTIC_CONTEXT) {
  const passwd = probeText(probes, "nss.ubuntu.passwd", diagnostics) ?? "";
  const fields = passwd.trim().split(":");
  const uid = /^[1-9][0-9]*$/u.test(fields[2] ?? "")
    ? Number.parseInt(fields[2], 10)
    : null;
  const gid = /^[1-9][0-9]*$/u.test(fields[3] ?? "")
    ? Number.parseInt(fields[3], 10)
    : null;

  // require exact NSS account identity and fixed nonroot home
  if (
    fields.length !== 7 ||
    fields[0] !== "ubuntu" ||
    !Number.isSafeInteger(uid) ||
    !Number.isSafeInteger(gid) ||
    fields[5] !== "/home/ubuntu"
  ) {
    addDiagnostic(diagnostics, "host.identity", "$.runner.account", "runner NSS account is invalid");
  }
  const account = {
    gid,
    home: fields[5] ?? "",
    name: fields[0] ?? "",
    probeIds: ["nss.ubuntu.passwd"],
    uid,
  };
  const rootIds = ["runner.root.private", "runner.root.library"];
  const expectedRootPaths = [
    "/home/ubuntu/.weather/adjustment-maintenance/runner",
    "/home/ubuntu/.local/lib/weather-adjustment-maintenance",
  ];
  const expectedRootModes = ["0700", "0755"];
  const roots = rootIds.map(
    // derive one exact managed-root state
    (id, index) => {
      const value = probeJson(probes, id, diagnostics);

      // require one closed path-bound root fact
      if (!hasExactKeys(value, ["identity", "path", "present"]) ||
        value.path !== expectedRootPaths[index] || typeof value.present !== "boolean" ||
        (value.present === false && value.identity !== null)) {
        addDiagnostic(diagnostics, "host.binding", `$.runner.roots.${id}`, "runner root fact is invalid");
        return { path: expectedRootPaths[index], present: false };
      }

      // validate one present root descriptor identity
      if (value.present === true) {
        try {
          validateSecureDirectoryIdentity(value.identity, `runner root ${id}`);
          if (value.identity.uid !== uid || value.identity.gid !== gid ||
            value.identity.mode !== expectedRootModes[index]) {
            throw hostError("host.identity", "runner root owner differs");
          }
        } catch {
          addDiagnostic(diagnostics, "host.identity", `$.runner.roots.${id}`, "runner root identity is invalid");
        }
      }
      return { path: value.path, present: value.present };
    },
  );
  const rawProcesses = probeJson(probes, "runner.processes", diagnostics) ?? [];

  // recompute every retained physical runner process identity
  for (let index = 0; index < (Array.isArray(rawProcesses) ? rawProcesses.length : 0); index += 1) {
    validatePhysicalProcessRecord(
      rawProcesses[index],
      `$.runner.processes[${String(index)}]`,
      diagnostics,
      context.secureReaderSha256,
    );
  }
  const runnerProcesses = Array.isArray(rawProcesses) ? rawProcesses.map(
    // retain each bounded runner process identity
    (process) => buildProcessRecord(process, "runner.processes", null, "runner"),
  ) : [];
  const unitInstalled = systemd.some(
    // retain any installed unit as runner installation evidence
    (unit) => unit.loadState !== "not-found",
  );
  const rootInstalled = roots.every(
    // require both managed roots for one installed runner
    (root) => root.present,
  );
  const anyRootInstalled = roots.some(
    // detect one partial root installation
    (root) => root.present,
  );
  const installed = unitInstalled && rootInstalled;
  const active = systemd.some(
    // retain any active unit or matching process as activation evidence
    (unit) => unit.activeState === "active",
  ) || runnerProcesses.length > 0;
  const failed = systemd.some(
    // retain failed unit state independently from active state
    (unit) => unit.activeState === "failed",
  );
  const enabled = systemd.some(
    // retain any persistent enabled unit state
    (unit) => unit.enabledState === "enabled" || unit.enabledState === "enabled-runtime",
  );

  // reject a process that has no installed unit or managed-root evidence
  if ((!installed && runnerProcesses.length > 0) || anyRootInstalled !== unitInstalled ||
    (anyRootInstalled && !rootInstalled)) {
    addDiagnostic(diagnostics, "host.binding", "$.runner.processes", "runner process exists without an installed runner");
  }
  const state = failed
    ? "installed-failed"
    : active
      ? "installed-active"
      : installed && enabled
        ? "installed-enabled-inactive"
        : installed
          ? "installed-disabled-inactive"
          : "not-installed";
  return {
    account,
    probeIds: ["nss.ubuntu.passwd", "runner.processes", ...rootIds, ...systemd.flatMap(
      // retain every unit evidence backlink
      (unit) => unit.probeIds,
    )].sort(),
    processes: runnerProcesses,
    roots: roots.map(
      // expose only the fixed managed-root paths
      (root) => root.path,
    ),
    state,
    units: systemd,
  };
}

// sort stable diagnostics
function sortDiagnostics(diagnostics) {
  return diagnostics.sort(
    // retain deterministic code path and message order
    (left, right) => canonicalHostJson(left).localeCompare(canonicalHostJson(right)),
  );
}

// derive one typed observation from raw host evidence
function buildBackendHostObservationCore(
  { inventory, runtimeMap, evidence, phase },
  context = ACTIVE_HOST_SEMANTIC_CONTEXT,
) {
  const diagnostics = [];

  // require exact source and evidence roots before parsing raw bytes
  if (
    !isRecord(inventory) ||
    inventory.schemaVersion !== context.inventoryVersion ||
    !Array.isArray(inventory.entries) ||
    !isRecord(runtimeMap) ||
    runtimeMap.contractVersion !== context.runtimeMapVersion ||
    !Array.isArray(runtimeMap.artifacts) ||
    !Array.isArray(runtimeMap.bindings) ||
    !Array.isArray(runtimeMap.contexts) ||
    !isRecord(evidence) ||
    evidence.contractVersion !== context.evidenceVersion ||
    evidence.producer !== context.producer ||
    !Array.isArray(evidence.errors) ||
    !Array.isArray(evidence.unresolved) ||
    !context.phases.has(phase)
  ) {
    return {
      diagnostics: [{ code: "host.schema", path: "$", message: "host observation input is invalid" }],
      observation: null,
      ok: false,
    };
  }

  // fail closed when this legacy observer cannot prove a selected native context
  if (runtimeMap.contexts.some(
    // detect one phase-selected declarative context
    (candidate) => phaseSelectsDeclarativeContext(candidate, phase),
  )) {
    addDiagnostic(
      diagnostics,
      "host.binding",
      "$.phase",
      "selected native contexts require a native physical receipt",
    );
  }

  // diagnose hidden evidence assertions without masking authority spoofing
  if (!hasExactKeys(evidence, [
    "authority",
    "complete",
    "completedAt",
    "contractVersion",
    "errors",
    "hostRole",
    "probes",
    "producer",
    "profile",
    "startedAt",
    "unresolved",
  ])) {
    addDiagnostic(diagnostics, "host.schema", "$.evidence", "host evidence root has unsupported fields");
  }

  // require one closed host authority record without transport self-assertion
  if (!hasExactKeys(evidence.authority, [
    "canonicalEligible",
    "inventorySha256",
    "kind",
    "liveEligible",
    "producerSha256",
    "runtimeMapSha256",
  ]) || evidence.authority.canonicalEligible !== false ||
    !["injected-test", "physical-host"].includes(evidence.authority.kind)) {
    addDiagnostic(diagnostics, "host.synthetic", "$.evidence.authority", "host evidence authority is invalid");
  }

  // bind the evidence source inputs to exact canonical bytes
  if (
    evidence.authority?.inventorySha256 !== hostEvidenceSha256(inventory) ||
    evidence.authority?.runtimeMapSha256 !== hostEvidenceSha256(runtimeMap)
  ) {
    addDiagnostic(diagnostics, "host.digest", "$.evidence.authority", "evidence source digest differs");
  }
  const expectedRole = evidence.profile === "blueberry-compose"
    ? "blueberry"
    : evidence.profile === "workstation-runner"
      ? "workstation"
      : null;

  // require one exact profile and role
  if (expectedRole === null || evidence.hostRole !== expectedRole) {
    addDiagnostic(diagnostics, "host.profile", "$.evidence", "host profile or role is invalid");
  }

  // prevent injected evidence from promoting its own authority
  if (
    evidence.authority?.kind === "injected-test" &&
    (evidence.authority.liveEligible !== false || evidence.authority.canonicalEligible !== false)
  ) {
    addDiagnostic(diagnostics, "host.synthetic", "$.evidence.authority", "injected evidence cannot become live authority");
  }
  const probes = indexEvidenceProbes(evidence, diagnostics, context);
  const errorIds = validateEvidenceErrors(evidence, diagnostics, context);
  validateProbeCatalog({ inventory, runtimeMap }, evidence, probes, diagnostics, context);

  // failed attempts must remain unresolved and cannot prove semantic facts
  if (errorIds.size > 0) {
    const unresolvedIds = new Set(evidence.unresolved.map(
      // retain every explicitly unresolved fact identity
      (item) => item?.id,
    ));

    // require one unresolved catalog fact for every failed attempt
    for (const id of errorIds) {
      if (!unresolvedIds.has(id) || probes.has(id)) {
        addDiagnostic(diagnostics, "host.probe", `$.evidence.errors.${id}`, "failed attempt is not unresolved");
      }
    }
  }

  // require a complete collection for a passing observation
  if (evidence.complete !== true) {
    addDiagnostic(diagnostics, "host.probe", "$.evidence.complete", "host evidence collection is incomplete");
  }
  const identity = buildHostIdentity(probes, diagnostics);
  let docker = null;
  let cgroups = [];
  let processes = [];
  let systemd = [];
  let runner = null;
  let containersByService = new Map();
  let account = null;
  let imagesByRole = new Map();

  // derive profile-specific runtime facts only from retained probes
  if (evidence.profile === "blueberry-compose") {
    const runtime = buildBlueberryRuntime(probes, diagnostics, context);
    docker = runtime.docker;
    cgroups = runtime.cgroups;
    processes = runtime.processes;
    containersByService = runtime.containersByService;
    imagesByRole = new Map(runtime.docker.images.map(
      // index exact inspected image roles
      (image) => [image.role, image],
    ));
    systemd = [buildSystemdUnit(probes, "weather-compose.service", diagnostics, false)];
  } else if (evidence.profile === "workstation-runner") {
    systemd = context.runnerUnits.map(
      // derive every exact conditional runner unit state
      (unit) => buildSystemdUnit(probes, unit, diagnostics, true),
    );
    runner = buildRunner(probes, systemd, processes, diagnostics, context);
    account = runner.account;
    processes = runner.processes;
  }
  const artifacts = buildArtifacts(
    runtimeMap,
    probes,
    containersByService,
    imagesByRole,
    evidence.profile,
    diagnostics,
    account,
  );

  // an absent conditional runner cannot hide installed managed runtime bytes
  if (runner?.state === "not-installed" && artifacts.some(
    // detect one installed runner-managed artifact
    (artifact) => artifact.kind === "runner-file" && artifact.present === true,
  )) {
    addDiagnostic(diagnostics, "host.binding", "$.runner.roots", "absent runner has installed managed artifacts");
  }
  const observation = {
    artifacts,
    authority: {
      canonicalEligible: evidence.authority?.canonicalEligible ?? false,
      inventorySha256: evidence.authority?.inventorySha256 ?? null,
      kind: evidence.authority?.kind ?? "unknown",
      liveEligible: evidence.authority?.liveEligible ?? false,
      probeIds: [...probes.keys()].sort(),
      producerSha256: evidence.authority?.producerSha256 ?? null,
      runtimeMapSha256: evidence.authority?.runtimeMapSha256 ?? null,
    },
    cgroups,
    collection: {
      complete: evidence.complete,
      completedAt: evidence.completedAt,
      startedAt: evidence.startedAt,
    },
    contractVersion: context.observationVersion,
    docker,
    evidenceSha256: hostEvidenceSha256(evidence),
    hostRole: evidence.hostRole,
    identity,
    phase,
    processes,
    producer: context.producer,
    profile: evidence.profile,
    release: buildRelease(probes, diagnostics, evidence.profile),
    runner,
    systemd,
  };
  const sorted = sortDiagnostics(diagnostics);
  return { diagnostics: sorted, observation, ok: sorted.length === 0 };
}

// derive one observation with the executing helper identity
export function buildBackendHostObservation(input) {
  return buildBackendHostObservationCore(input, ACTIVE_HOST_SEMANTIC_CONTEXT);
}

// extract the exact retained v1 helper bytes without executing archived source
function retainedV1SecureReaderSource(observerSourceBytes) {
  const source = decodeUtf8Strict(observerSourceBytes, "host.schema");
  const start = "// weather-retained-v1-secure-reader-source:start\nexport const HOST_SECURE_READER_SOURCE = String.raw`";
  const end = "\n`;\n// weather-retained-v1-secure-reader-source:end";
  const first = source.indexOf(start);
  const last = source.lastIndexOf(start);

  // require one exact finite helper source boundary
  if (first < 0 || first !== last) {
    throw hostError("host.schema", "retained observer helper boundary is invalid");
  }
  const contentStart = first + start.length;
  const contentEnd = source.indexOf(end, contentStart);

  // reject missing or ambiguous closing boundaries
  if (contentEnd < 0 || source.indexOf(end, contentEnd + end.length) >= 0) {
    throw hostError("host.schema", "retained observer helper boundary is ambiguous");
  }
  return `${source.slice(contentStart, contentEnd)}\n`;
}

// extract one unique retained exported protocol string without executing archived source
function retainedV1ExportedString(observerSourceBytes, name) {
  const source = decodeUtf8Strict(observerSourceBytes, "host.schema");
  const prefix = `export const ${name} = "`;
  const first = source.indexOf(prefix);
  const last = source.lastIndexOf(prefix);

  // require one plain finite string export from the retained implementation
  if (first < 0 || first !== last) {
    throw hostError("host.schema", `retained observer ${name} export is invalid`);
  }
  const valueStart = first + prefix.length;
  const valueEnd = source.indexOf('";', valueStart);
  const value = valueEnd < 0 ? "" : source.slice(valueStart, valueEnd);

  // reject escapes control bytes and ambiguous suffixes
  if (value.length === 0 || value.length > 256 || /["\\\u0000-\u001f]/u.test(value)) {
    throw hostError("host.schema", `retained observer ${name} value is invalid`);
  }
  return value;
}

// validate one retained full request against its exact retained source inputs
function validateRetainedV1Request(request, inventoryBytes, runtimeMapBytes, observerSourceBytes, context) {
  requireExactKeys(request, REQUEST_KEYS, "host.schema", "request");
  requireExactKeys(request.inventory, new Set(["json", "sha256"]), "host.schema", "request.inventory");
  requireExactKeys(request.runtimeMap, new Set(["json", "sha256"]), "host.schema", "request.runtimeMap");
  requireExactKeys(request.limits, new Set(Object.keys(context.limits)), "host.schema", "request.limits");
  const inventoryJson = decodeUtf8Strict(inventoryBytes, "host.schema");
  const runtimeMapJson = decodeUtf8Strict(runtimeMapBytes, "host.schema");
  const producerSha256 = sha256Bytes(observerSourceBytes);
  const expectedRole = request.profile === "blueberry-compose"
    ? "blueberry"
    : request.profile === "workstation-runner"
      ? "workstation"
      : null;

  // require the complete retained request identity
  if (request.contractVersion !== context.requestVersion || expectedRole === null ||
    request.expectedHostRole !== expectedRole || typeof request.expectedReleaseId !== "string" ||
    request.expectedReleaseId.length === 0 || typeof request.expectedSchemaVersion !== "string" ||
    request.expectedSchemaVersion.length === 0 || request.producerSha256 !== producerSha256 ||
    request.inventory.json !== inventoryJson || request.inventory.sha256 !== sha256Bytes(inventoryBytes) ||
    request.runtimeMap.json !== runtimeMapJson || request.runtimeMap.sha256 !== sha256Bytes(runtimeMapBytes)) {
    throw hostError("host.digest", "retained request differs from retained source bytes");
  }

  // preserve the original fixed collection bounds
  for (const [key, maximum] of Object.entries(context.limits)) {
    // reject missing or widened retained limits
    if (!isBoundedInteger(request.limits[key], maximum)) {
      throw hostError("host.schema", `retained request limit ${key} is invalid`);
    }
  }
  return { expectedRole, producerSha256 };
}

// rebuild one historical v1 observation from full retained raw bytes
export function buildRetainedBackendHostObservationV1(input) {
  const diagnostics = [];

  try {
    requireExactKeys(input, new Set([
      "evidenceBytes", "inventoryBytes", "observerSourceBytes", "phase", "requestBytes", "runtimeMapBytes",
    ]), "host.schema", "retained observation input");
    const inventory = parseStrictJson(input.inventoryBytes, "host.schema");
    const runtimeMap = parseStrictJson(input.runtimeMapBytes, "host.schema");
    const request = parseStrictJson(input.requestBytes, "host.schema");
    const evidence = parseStrictJson(input.evidenceBytes, "host.schema");
    const observerSourceBytes = Buffer.from(input.observerSourceBytes);
    const producer = retainedV1ExportedString(observerSourceBytes, "HOST_PRODUCER");
    const secureReaderSource = retainedV1SecureReaderSource(Buffer.from(input.observerSourceBytes));
    const secureReaderSha256 = sha256Bytes(secureReaderSource);
    const retainedVersions = {
      evidenceVersion: retainedV1ExportedString(observerSourceBytes, "HOST_EVIDENCE_CONTRACT_VERSION"),
      observationVersion: retainedV1ExportedString(observerSourceBytes, "HOST_OBSERVATION_VERSION"),
      probeVersion: retainedV1ExportedString(observerSourceBytes, "HOST_PROBE_VERSION"),
      requestVersion: retainedV1ExportedString(observerSourceBytes, "HOST_EVIDENCE_REQUEST_VERSION"),
    };

    // reject coherent later protocols at the immutable retained-v1 boundary
    if (retainedVersions.evidenceVersion !== RETAINED_HOST_SEMANTIC_CONTEXT_V1.evidenceVersion ||
      retainedVersions.observationVersion !== RETAINED_HOST_SEMANTIC_CONTEXT_V1.observationVersion ||
      retainedVersions.probeVersion !== RETAINED_HOST_SEMANTIC_CONTEXT_V1.probeVersion ||
      retainedVersions.requestVersion !== RETAINED_HOST_SEMANTIC_CONTEXT_V1.requestVersion) {
      throw hostError("host.schema", "retained observer protocol is not v1");
    }
    const context = Object.freeze({
      ...RETAINED_HOST_SEMANTIC_CONTEXT_V1,
      producer,
      secureReaderSha256,
    });
    const source = validateRetainedV1Request(
      request,
      Buffer.from(input.inventoryBytes),
      Buffer.from(input.runtimeMapBytes),
      Buffer.from(input.observerSourceBytes),
      context,
    );

    // require one complete physical capture from the retained producer bytes
    if (evidence.profile !== request.profile || evidence.hostRole !== source.expectedRole ||
      evidence.producer !== context.producer || evidence.authority?.kind !== "physical-host" ||
      evidence.authority?.liveEligible !== true || evidence.authority?.canonicalEligible !== false ||
      evidence.authority?.producerSha256 !== source.producerSha256 || evidence.complete !== true) {
      throw hostError("host.identity", "retained host capture identity is invalid");
    }
    const result = buildBackendHostObservationCore(
      { evidence, inventory, phase: input.phase, runtimeMap },
      context,
    );

    // bind request expectations to the rederived release semantics
    if (result.ok && request.profile === "blueberry-compose" &&
      (result.observation?.release?.currentRelease !== request.expectedReleaseId ||
      result.observation?.release?.schemaRelease !== request.expectedSchemaVersion)) {
      result.diagnostics.push({
        code: "host.identity",
        message: "retained observation release differs from request",
        path: "$.request",
      });
      result.diagnostics = sortDiagnostics(result.diagnostics);
      result.ok = false;
    }
    return result;
  } catch (error) {
    diagnostics.push({
      code: typeof error?.code === "string" ? error.code : "host.schema",
      message: typeof error?.message === "string" ? error.message : "retained observation is invalid",
      path: "$",
    });
    return { diagnostics: sortDiagnostics(diagnostics), observation: null, ok: false };
  }
}

// read one bounded request from standard input
async function readRequestFromStdin() {
  const maximum = 32 * 1024 * 1024;
  const chunks = [];
  let total = 0;

  // retain request chunks only below the fixed transport bound
  for await (const chunk of process.stdin) {
    const bytes = Buffer.from(chunk);
    total += bytes.length;

    // reject the request before retaining bytes beyond the fixed bound
    if (total > maximum) {
      throw hostError("host.request", "request input exceeds fixed bound");
    }
    chunks.push(bytes);
  }
  return Buffer.concat(chunks, total);
}

// run the fixed host-local collection CLI
async function runHostObserverCli() {
  // reject all command or request-path selection outside the documented transport
  if (
    process.argv.length !== 5 ||
    process.argv[2] !== "collect" ||
    process.argv[3] !== "--request" ||
    process.argv[4] !== "/dev/stdin"
  ) {
    throw hostError("host.request", "usage: backend-host-observation.mjs collect --request /dev/stdin");
  }
  const bytes = await readRequestFromStdin();
  let request;

  // parse the bounded request without evaluating source text
  try {
    request = parseStrictJson(bytes, "host.request");
  } catch {
    throw hostError("host.request", "request input is not valid JSON");
  }
  const evidence = await collectBackendHostEvidence(request);
  process.stdout.write(`${canonicalHostJson(evidence)}\n`);
}

// execute only when invoked as the transported host-local program
if (process.argv[1] !== undefined && import.meta.url === pathToFileURL(process.argv[1]).href) {
  runHostObserverCli().catch(
    // emit one controlled error without request or probe bytes
    (error) => {
      process.stderr.write(`${canonicalHostJson({
        code: typeof error?.code === "string" ? error.code : "host.probe",
        message: typeof error?.message === "string" ? error.message : "host observer failed",
      })}\n`);
      process.exitCode = 1;
    },
  );
}
