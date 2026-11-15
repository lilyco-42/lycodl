#!/usr/bin/env python3
"""Merge desktop Chrome login cookies into the dedicated lycodl CDP profile.

Chrome 136+ refuses --remote-debugging-port on the default user data dir, so
lycodl uses a dedicated profile at /home/lilyco/.cache/lycodl-cdp-profile.
This script is the ExecStartPre of lycodl-cdp.service (Chrome is stopped):
it takes consistent SQLite snapshots of both profiles and merges cookies by
Chrome's unique key, keeping whichever row has the newer last_update_utc.
That preserves CDP-only platform cookies while importing newer desktop
logins. Both profiles share the same Linux keyring key, so v11 encrypted
cookies remain decryptable. Failures never block Chrome startup.
"""
from __future__ import annotations

import hashlib
import json
import os
import shutil
import sqlite3
import sys
import time
from pathlib import Path

MAIN = Path(os.environ.get("LYCODL_MAIN_CHROME_DIR", "/home/lilyco/.config/google-chrome"))
CDP = Path(os.environ.get("LYCODL_CDP_CHROME_DIR", "/home/lilyco/.cache/lycodl-cdp-profile"))
PROFILE = os.environ.get("LYCODL_CHROME_PROFILE", "Default")
SRC = MAIN / PROFILE / "Cookies"
DST = CDP / PROFILE / "Cookies"
MIN_COOKIES = int(os.environ.get("LYCODL_MIN_COOKIES", "100"))
CDP_MARKER = ("--user-data-dir=" + str(CDP)).encode()

COLS = (
    "creation_utc", "host_key", "top_frame_site_key", "name", "value",
    "encrypted_value", "path", "expires_utc", "is_secure", "is_httponly",
    "last_access_utc", "has_expires", "is_persistent", "priority", "samesite",
    "source_scheme", "source_port", "last_update_utc", "source_type",
    "has_cross_site_ancestor",
)
KEY_COLS = (
    "host_key", "top_frame_site_key", "has_cross_site_ancestor",
    "name", "path", "source_scheme", "source_port",
)


def log(msg: str) -> None:
    print("[lycodl-cdp-sync] " + msg, flush=True)


def connect_ro(path: Path) -> sqlite3.Connection:
    return sqlite3.connect("file:" + str(path) + "?mode=ro", uri=True, timeout=5)


def db_count(path: Path) -> int:
    con = connect_ro(path)
    try:
        return int(con.execute("select count(*) from cookies").fetchone()[0])
    finally:
        con.close()


def db_schema(path: Path) -> list[str]:
    con = connect_ro(path)
    try:
        return [r[1] for r in con.execute("pragma table_info(cookies)")]
    finally:
        con.close()


def db_ok(path: Path) -> bool:
    con = connect_ro(path)
    try:
        return con.execute("pragma integrity_check").fetchone()[0] == "ok"
    finally:
        con.close()


def chrome_running() -> bool:
    """Only a Chrome browser main process can hold the Cookies DB open.

    Renderer/zygote children carry --type=; crashpad_handler carries its own
    path. Neither should make us skip the sync after the service is stopped.
    """
    for entry in os.scandir("/proc"):
        if not entry.name.isdigit():
            continue
        try:
            cmd = Path("/proc/" + entry.name + "/cmdline").read_bytes()
        except OSError:
            continue
        low = cmd.lower()
        if (CDP_MARKER in cmd and b"chrome" in low
                and b"--type=" not in cmd
                and b"crashpad_handler" not in low):
            return True
    return False


def wait_for_chrome_stop() -> bool:
    for _ in range(20):
        if not chrome_running():
            return True
        time.sleep(0.4)
    return not chrome_running()


def snapshot(src: Path, out: Path) -> None:
    """Consistent SQLite backup; fall back to a file copy if the live DB is busy."""
    out.unlink(missing_ok=True)
    last: Exception | None = None
    for attempt in range(4):
        try:
            with connect_ro(src) as source, sqlite3.connect(str(out)) as target:
                source.backup(target, pages=64, sleep=0.03)
            return
        except sqlite3.OperationalError as exc:
            last = exc
            out.unlink(missing_ok=True)
            time.sleep(0.5 * (attempt + 1))
    shutil.copy2(src, out)
    for suffix in ("-journal", "-wal", "-shm"):
        side = Path(str(src) + suffix)
        if side.exists():
            shutil.copy2(side, Path(str(out) + suffix))
    log("live backup fell back to file copy: " + str(last))


def check_os_crypt() -> None:
    """Warn but do not rewrite Local State; CDP-only cookies must stay decryptable."""
    src_state, dst_state = MAIN / "Local State", CDP / "Local State"
    if not src_state.is_file() or not dst_state.is_file():
        return
    try:
        main_crypt = json.loads(src_state.read_text(encoding="utf-8")).get("os_crypt")
        cdp_crypt = json.loads(dst_state.read_text(encoding="utf-8")).get("os_crypt")
    except (OSError, ValueError) as exc:
        log("Local State read failed, skipped: " + str(exc))
        return
    if isinstance(main_crypt, dict) and main_crypt != cdp_crypt:
        log("WARNING: os_crypt differs; keeping CDP Local State unchanged")


def merge(cdp_snap: Path, main_snap: Path, merged: Path) -> tuple[int, int, int]:
    shutil.copy2(cdp_snap, merged)
    con = sqlite3.connect(str(merged), timeout=5)
    changes = 0
    try:
        con.execute("pragma busy_timeout=5000")
        con.execute("attach database ? as main_state", (str(main_snap),))
        con.execute("begin immediate")
        key_cond = " and ".join("c.%s = m.%s" % (k, k) for k in KEY_COLS)
        cols = ", ".join(COLS)
        mcols = ", ".join("m." + c for c in COLS)
        con.execute(
            "insert or replace into cookies (%s) "
            "select %s from main_state.cookies m "
            "left join main.cookies c on %s "
            "where c.host_key is null or m.last_update_utc > c.last_update_utc"
            % (cols, mcols, key_cond)
        )
        changes = con.execute("select changes()").fetchone()[0]
        key_expr = ", ".join(KEY_COLS)
        union_count = con.execute(
            "select count(*) from (select %s from main.cookies union "
            "select %s from main_state.cookies)" % (key_expr, key_expr)
        ).fetchone()[0]
        actual = con.execute("select count(*) from cookies").fetchone()[0]
        if actual != union_count:
            raise sqlite3.DatabaseError("merged count %d != union %d" % (actual, union_count))
        con.commit()
        con.execute("detach database main_state")
    finally:
        con.close()
    return changes, actual, union_count


def main() -> int:
    if not SRC.is_file() or not DST.is_file():
        log("Cookies DB missing, keeping existing CDP login state")
        return 0
    if not wait_for_chrome_stop():
        log("WARNING: CDP Chrome still running, skipping sync to avoid clobbering live DB")
        return 0
    try:
        src_count, dst_count = db_count(SRC), db_count(DST)
        # Only the desktop source is guarded: a nearly empty CDP profile is
        # exactly the state we must heal. The merge below is additive (rows are
        # keyed and the union is verified), so fewer CDP rows are never unsafe.
        if src_count < MIN_COOKIES:
            log("desktop Chrome has too few cookies (%d/%d), keeping existing CDP login state" %
                (src_count, dst_count))
            return 0
        if dst_count == 0:
            log("CDP profile has no cookies yet, importing from desktop Chrome")
        if db_schema(SRC) != list(COLS) or db_schema(DST) != list(COLS):
            log("Cookies schema differs from expected Chrome version, keeping existing CDP login state")
            return 0
    except sqlite3.Error as exc:
        log("Cookies DB unreadable, keeping existing CDP login state: " + str(exc))
        return 0

    base = CDP / PROFILE
    cdp_snap = base / "Cookies.cdp-snap.tmp"
    main_snap = base / "Cookies.main-snap.tmp"
    merged = base / "Cookies.cdp-merge.tmp"
    final_tmp = base / "Cookies.cdp-sync.tmp"
    try:
        snapshot(DST, cdp_snap)
        snapshot(SRC, main_snap)
        if not (db_ok(cdp_snap) and db_ok(main_snap)):
            log("snapshot failed integrity check, keeping existing CDP login state")
            return 0
        changes, final_count, union_count = merge(cdp_snap, main_snap, merged)
        if not db_ok(merged) or final_count != db_count(merged) or final_count < max(src_count, dst_count):
            log("merged DB failed verification, keeping existing CDP login state")
            return 0
        digest = hashlib.sha256(merged.read_bytes()).hexdigest()[:16]
        shutil.copy2(merged, final_tmp)
        os.chmod(final_tmp, 0o600)
        os.replace(final_tmp, DST)
        DST.with_name("Cookies-journal").unlink(missing_ok=True)
        for suffix in ("-wal", "-shm"):
            Path(str(DST) + suffix).unlink(missing_ok=True)
        check_os_crypt()
        log("merged desktop Chrome cookies: main %d, cdp %d, imported/updated %d, union %d (sha256:%s)" %
            (src_count, dst_count, changes, union_count, digest))
    except Exception as exc:  # never block Chrome startup on a sync problem
        log("WARNING: sync failed, keeping existing CDP login state: " + str(exc))
        final_tmp.unlink(missing_ok=True)
        return 0
    finally:
        for p in (cdp_snap, main_snap, merged):
            p.unlink(missing_ok=True)
            for suffix in ("-journal", "-wal", "-shm"):
                Path(str(p) + suffix).unlink(missing_ok=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
