#!/usr/bin/env python3
"""Self-contained regression test for scripts/sync-chrome-cookies.py."""
from __future__ import annotations

import os
import sqlite3
import subprocess
import sys
import tempfile
from pathlib import Path

COLS = (
    "creation_utc", "host_key", "top_frame_site_key", "name", "value",
    "encrypted_value", "path", "expires_utc", "is_secure", "is_httponly",
    "last_access_utc", "has_expires", "is_persistent", "priority", "samesite",
    "source_scheme", "source_port", "last_update_utc", "source_type",
    "has_cross_site_ancestor",
)


def make_db(path, rows):
    Path(path).unlink(missing_ok=True)
    con = sqlite3.connect(path)
    try:
        con.execute("""create table cookies (
            creation_utc integer not null, host_key text not null,
            top_frame_site_key text not null, name text not null,
            value text not null, encrypted_value blob not null,
            path text not null, expires_utc integer not null,
            is_secure integer not null, is_httponly integer not null,
            last_access_utc integer not null, has_expires integer not null,
            is_persistent integer not null, priority integer not null,
            samesite integer not null, source_scheme integer not null,
            source_port integer not null, last_update_utc integer not null,
            source_type integer not null, has_cross_site_ancestor integer not null,
            unique (host_key, top_frame_site_key, has_cross_site_ancestor,
                    name, path, source_scheme, source_port))""")
        con.executemany(
            "insert into cookies (" + ", ".join(COLS) + ") values (" + ", ".join(["?"] * len(COLS)) + ")",
            rows,
        )
        con.commit()
    finally:
        con.close()


def row(host, name, ts, value):
    return (1, host, "", name, value, b"", "/", 0, 0, 0, 0, 0, 0, 0, 0, 2, 443, ts, 0, 1)


def read_rows(path):
    con = sqlite3.connect(path)
    try:
        cur = con.execute("select host_key, name, value from cookies")
        return {(r[0], r[1]): r[2] for r in cur}
    finally:
        con.close()


def run_sync(script, main_dir, cdp_dir, min_cookies="1"):
    env = os.environ.copy()
    env.update({
        "LYCODL_MAIN_CHROME_DIR": str(main_dir),
        "LYCODL_CDP_CHROME_DIR": str(cdp_dir),
        "LYCODL_CHROME_PROFILE": "Default",
        "LYCODL_MIN_COOKIES": min_cookies,
    })
    return subprocess.run([sys.executable, str(script)], env=env, check=True, capture_output=True, text=True)


def main():
    repo = Path(__file__).resolve().parents[1]
    script = repo / "scripts" / "sync-chrome-cookies.py"
    if not script.is_file():
        raise SystemExit("missing scripts/sync-chrome-cookies.py")
    with tempfile.TemporaryDirectory() as td:
        root = Path(td)
        main_dir, cdp_dir = root / "main", root / "cdp"
        (main_dir / "Default").mkdir(parents=True)
        (cdp_dir / "Default").mkdir(parents=True)
        main_db = main_dir / "Default" / "Cookies"
        cdp_db = cdp_dir / "Default" / "Cookies"
        make_db(main_db, [
            row("a.example", "shared", 200, "main_new"),
            row("a.example", "main_only", 100, "main_only"),
            row("b.example", "shared", 50, "main_old"),
        ])
        make_db(cdp_db, [
            row("a.example", "shared", 100, "cdp_old"),
            row("b.example", "shared", 150, "cdp_new"),
            row("c.example", "cdp_only", 100, "cdp_only"),
        ])
        first = run_sync(script, main_dir, cdp_dir)
        assert "merged desktop Chrome cookies" in first.stdout, first.stdout + first.stderr
        assert "imported/updated 2" in first.stdout, first.stdout
        got = read_rows(cdp_db)
        assert got == {
            ("a.example", "shared"): "main_new",
            ("b.example", "shared"): "cdp_new",
            ("a.example", "main_only"): "main_only",
            ("c.example", "cdp_only"): "cdp_only",
        }, got
        second = run_sync(script, main_dir, cdp_dir)
        assert "imported/updated 0" in second.stdout, second.stdout
        assert len(read_rows(cdp_db)) == 4
        assert not list((cdp_dir / "Default").glob("Cookies.*tmp"))
    with tempfile.TemporaryDirectory() as td:
        # Regression: a wiped CDP profile (Chrome recreates only a few rows)
        # must be healed from the desktop profile, not silently skipped.
        root = Path(td)
        main_dir, cdp_dir = root / "main", root / "cdp"
        (main_dir / "Default").mkdir(parents=True)
        (cdp_dir / "Default").mkdir(parents=True)
        main_db = main_dir / "Default" / "Cookies"
        cdp_db = cdp_dir / "Default" / "Cookies"
        make_db(main_db, [row("a.example", "n%d" % i, 200, "v") for i in range(150)])
        make_db(cdp_db, [row("wipe.example", "only", 200, "v")])
        healed = run_sync(script, main_dir, cdp_dir, min_cookies="100")
        assert "merged desktop Chrome cookies: main 150, cdp 1" in healed.stdout, healed.stdout
        assert len(read_rows(cdp_db)) == 151, len(read_rows(cdp_db))

        # Fully wiped CDP DB (Chrome had not written anything back yet).
        make_db(cdp_db, [])
        wiped = run_sync(script, main_dir, cdp_dir, min_cookies="100")
        assert "importing from desktop Chrome" in wiped.stdout, wiped.stdout
        assert "merged desktop Chrome cookies: main 150, cdp 0" in wiped.stdout, wiped.stdout
        assert len(read_rows(cdp_db)) == 150, len(read_rows(cdp_db))

        # A thin desktop profile is still refused, so a fresh/empty main Chrome
        # cannot wipe an authenticated CDP profile.
        make_db(main_db, [row("a.example", "one", 200, "v")])
        refused = run_sync(script, main_dir, cdp_dir, min_cookies="100")
        assert "desktop Chrome has too few cookies" in refused.stdout, refused.stdout
        assert len(read_rows(cdp_db)) == 150

    print("SYNC_CHROME_COOKIES_TEST_OK")


if __name__ == "__main__":
    main()
