"""Installer contract tests against a local release server; no external I/O."""
import functools
import hashlib
import http.server
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import threading
import unittest


SCRIPT = (Path(__file__).resolve().parent.parent / "install.sh").read_bytes()
TARGETS = ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu",
           "x86_64-apple-darwin", "aarch64-apple-darwin"]


class InstallerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="kiroha-install-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.downloads = self.root / "releases"
        self.bin = self.root / "install path with spaces"
        self.scratch = self.root / "scratch"
        self.scratch.mkdir()
        self.requests = []
        requests = self.requests

        class Handler(http.server.SimpleHTTPRequestHandler):
            def do_GET(self):
                requests.append(self.path)
                super().do_GET()

            def log_message(self, *args):
                pass

        self.server = http.server.ThreadingHTTPServer(
            ("127.0.0.1", 0), functools.partial(Handler, directory=str(self.root)))
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.addCleanup(self.stop_server)
        self.base = f"http://127.0.0.1:{self.server.server_port}/releases"
        self.release()

    def stop_server(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=5)

    def release(self, version="0.3.0", checksum_ok=True, symlink=False):
        for location in ["latest/download", "download/v0.3.0"]:
            folder = self.downloads / location
            folder.mkdir(parents=True, exist_ok=True)
            sums = []
            for target in TARGETS:
                asset = f"kiroha-{target}.tar.gz"
                response = json.dumps({"implementation": f"kiroha-rs/{version}"}, separators=(",", ":"))
                payload = f"#!/bin/sh\nprintf '%s\\n' '{response}'\n".encode()
                with tarfile.open(folder / asset, "w:gz") as archive:
                    member = tarfile.TarInfo("kiroha")
                    member.mode = 0o755
                    if symlink:
                        member.type = tarfile.SYMTYPE
                        member.linkname = str(self.root / "victim")
                        archive.addfile(member)
                    else:
                        member.size = len(payload)
                        archive.addfile(member, io.BytesIO(payload))
                digest = hashlib.sha256((folder / asset).read_bytes()).hexdigest() if checksum_ok else "0" * 64
                sums.append(f"{digest}  {asset}\n")
            (folder / "SHA256SUMS").write_text("".join(sums))

    def invoke(self, *args, extra_env=None):
        env = dict(os.environ, KIROHA_RELEASE_BASE_URL=self.base,
                   KIROHA_INSTALL_DIR=str(self.bin), TMPDIR=str(self.scratch))
        env.pop("KIROHA_VERSION", None)
        if extra_env:
            env.update(extra_env)
        result = subprocess.run([os.environ.get("KIROHA_TEST_BASH", "bash"), "-s", "--", *args], input=SCRIPT,
                                capture_output=True, env=env, timeout=25)
        self.assertEqual(list(self.scratch.iterdir()), [], "download temporary files leaked")
        self.assertEqual(list(self.bin.glob(".kiroha-install.*")), [], "installation temporary files leaked")
        return result

    def old_install(self):
        self.bin.mkdir()
        (self.bin / "kiroha").write_text("OLD")

    def test_latest_install_and_paths_with_spaces(self):
        result = self.invoke()
        self.assertEqual(result.returncode, 0, result.stderr)
        output = subprocess.check_output([str(self.bin / "kiroha"), "--version"], timeout=5)
        self.assertEqual(json.loads(output)["implementation"], "kiroha-rs/0.3.0")
        self.assertTrue(any(p.startswith("/releases/latest/download/") for p in self.requests))

    def test_pinned_version_and_flag_override(self):
        result = self.invoke("--version", "0.3.0", extra_env={"KIROHA_VERSION": "9.9.9"})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(all("/download/v0.3.0/" in p for p in self.requests))

    def test_checksum_failure_preserves_existing_install(self):
        self.old_install()
        self.release(checksum_ok=False)
        result = self.invoke()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b"checksum mismatch", result.stderr)
        self.assertEqual((self.bin / "kiroha").read_text(), "OLD")

    def test_missing_asset_preserves_existing_install(self):
        self.old_install()
        for p in (self.downloads / "latest/download").glob("*.tar.gz"):
            p.unlink()
        result = self.invoke()
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual((self.bin / "kiroha").read_text(), "OLD")

    def test_archive_symlink_is_not_executed_or_followed(self):
        victim = self.root / "victim"
        victim.write_text("untouched")
        victim.chmod(0o600)
        self.release(symlink=True)
        result = self.invoke()
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(victim.read_text(), "untouched")
        self.assertEqual(victim.stat().st_mode & 0o777, 0o600)
        self.assertFalse((self.bin / "kiroha").exists())

    def test_wrong_pinned_binary_version_preserves_existing_install(self):
        self.old_install()
        self.release(version="9.9.9")
        result = self.invoke("--version", "v0.3.0")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b"does not match", result.stderr)
        self.assertEqual((self.bin / "kiroha").read_text(), "OLD")

    def test_all_platform_mappings(self):
        for system, machine, target in [
            ("Linux", "x86_64", TARGETS[0]), ("Linux", "aarch64", TARGETS[1]),
            ("Darwin", "x86_64", TARGETS[2]), ("Darwin", "arm64", TARGETS[3]),
        ]:
            mock = self.root / "mock"
            mock.mkdir(exist_ok=True)
            uname = mock / "uname"
            uname.write_text(f'#!/bin/sh\ncase "$1" in -s) echo {system};; -m) echo {machine};; esac\n')
            uname.chmod(0o755)
            self.requests.clear()
            result = self.invoke(extra_env={"PATH": f"{mock}:{os.environ['PATH']}"})
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn(f"/releases/latest/download/kiroha-{target}.tar.gz", self.requests)

    def test_invalid_arguments_and_unsupported_os_do_not_download(self):
        for args in [("--version", "../../bad"), ("--bogus",), ("--version",)]:
            self.assertNotEqual(self.invoke(*args).returncode, 0)
        mock = self.root / "mock"
        mock.mkdir()
        uname = mock / "uname"
        uname.write_text('#!/bin/sh\ncase "$1" in -s) echo UnsupportedOS;; -m) echo x86_64;; esac\n')
        uname.chmod(0o755)
        result = self.invoke(extra_env={"PATH": f"{mock}:{os.environ['PATH']}"})
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b"unsupported OS", result.stderr)
        self.assertEqual(self.requests, [])


if __name__ == "__main__":
    unittest.main()
