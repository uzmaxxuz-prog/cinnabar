"""Check exact Bevy pinning, lockfile drift and release-branch reachability."""

import copy
import importlib.util
from pathlib import Path
import sys
import tomllib
import unittest

SCRIPTS = Path(__file__).resolve().parents[1] / "scripts"
sys.path.insert(0, str(SCRIPTS))
SPEC = importlib.util.spec_from_file_location("bevy_fork_pin", SCRIPTS / "bevy_fork_pin.py")
fork_pin = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(fork_pin)


def fixture() -> tuple[dict, dict]:
    """Return a small consistent manifest and lockfile with two version-matched Bevy crates."""
    rev = "a" * 40
    patches = {name: {"git": fork_pin.FORK_URL, "rev": rev} for name in ("bevy", "bevy_render")}
    manifest = {"workspace": {"dependencies": {"bevy": {"version": "=0.20.0"}}}, "patch": {"crates-io": patches}}
    lock = {"package": [{"name": name, "version": "0.20.0", "source": f"git+{fork_pin.FORK_URL}?rev={rev}#{rev}"} for name in patches]}
    return manifest, lock


class BevyForkPinTests(unittest.TestCase):
    def test_exact_pin_derives_its_release_branch(self):
        manifest, lock = fixture()
        self.assertEqual(fork_pin.pin(manifest), ("uzmaxxuz-prog/bevy", "a" * 40, "cinnabar-v0.20"))
        fork_pin.check_lock(manifest, lock, "a" * 40)

    def test_split_revisions_and_short_revisions_are_rejected(self):
        for revision in ("b" * 40, "abc123"):
            manifest, _ = fixture()
            manifest["patch"]["crates-io"]["bevy_render"]["rev"] = revision
            with self.assertRaises(ValueError):
                fork_pin.pin(manifest)

    def test_new_unpatched_or_registry_bevy_crate_fails(self):
        manifest, lock = fixture()
        for source in (None, "registry+https://github.com/rust-lang/crates.io-index"):
            modified = copy.deepcopy(lock)
            modified["package"].append({"name": "bevy_new", "version": "0.20.0", "source": source})
            with self.assertRaises(ValueError):
                fork_pin.check_lock(manifest, modified, "a" * 40)

    def test_dependency_pr_head_cannot_pass_the_release_branch_guard(self):
        self.assertTrue(fork_pin.on_branch("identical"))
        self.assertTrue(fork_pin.on_branch("behind"))
        self.assertFalse(fork_pin.on_branch("ahead"))
        self.assertFalse(fork_pin.on_branch("diverged"))

    def test_wrong_or_mixed_fork_owners_are_rejected(self):
        for names in (("bevy",), ("bevy", "bevy_render")):
            with self.subTest(names=names):
                manifest, _ = fixture()
                for name in names:
                    manifest["patch"]["crates-io"][name]["git"] = "https://github.com/cinnabar-mc/bevy"
                with self.assertRaises(ValueError):
                    fork_pin.pin(manifest)

    def test_shared_short_revision_is_rejected(self):
        manifest, _ = fixture()
        for patch in manifest["patch"]["crates-io"].values():
            patch["rev"] = "abc123"
        with self.assertRaises(ValueError):
            fork_pin.pin(manifest)

    def test_missing_patch_for_a_locked_bevy_crate_is_rejected(self):
        manifest, lock = fixture()
        del manifest["patch"]["crates-io"]["bevy_render"]
        _, rev, _ = fork_pin.pin(manifest)
        with self.assertRaises(ValueError):
            fork_pin.check_lock(manifest, lock, rev)

    def test_stale_or_registry_source_for_a_patched_crate_is_rejected(self):
        for source in (
            None,
            "registry+https://github.com/rust-lang/crates.io-index",
            f"git+https://github.com/uzmaxxuz-prog/bevy?rev={'b' * 40}#{'b' * 40}",
            f"git+https://github.com/uzmaxxuz-prog/bevy?rev={'a' * 40}#{'b' * 40}",
            f"git+https://github.com/cinnabar-mc/bevy?rev={'a' * 40}#{'a' * 40}",
        ):
            with self.subTest(source=source):
                manifest, lock = fixture()
                lock["package"][1]["source"] = source
                with self.assertRaises(ValueError):
                    fork_pin.check_lock(manifest, lock, "a" * 40)

    def test_stale_version_for_a_patched_crate_is_rejected(self):
        manifest, lock = fixture()
        lock["package"][1]["version"] = "0.19.0"
        with self.assertRaises(ValueError):
            fork_pin.check_lock(manifest, lock, "a" * 40)

    def test_independently_versioned_registry_dependency_is_allowed(self):
        manifest, lock = fixture()
        lock["package"].append({
            "name": "bevy_mikktspace", "version": "1.0.0",
            "source": "registry+https://github.com/rust-lang/crates.io-index",
        })
        fork_pin.check_lock(manifest, lock, "a" * 40)

    def test_workspace_graph_uses_the_same_fork_pin(self):
        root = SCRIPTS.parents[1]
        manifest = tomllib.loads((root / "Cargo.toml").read_text())
        repository, rev, _ = fork_pin.pin(manifest)
        self.assertEqual(repository, "uzmaxxuz-prog/bevy")
        version = manifest["workspace"]["dependencies"]["bevy"]["version"].removeprefix("=")
        lock = tomllib.loads((root / "Cargo.lock").read_text())
        fork_pin.check_lock(manifest, lock, rev)
        selected = {
            package["name"] for package in lock["package"]
            if package["version"] == version
            and (package["name"] == "bevy" or package["name"].startswith("bevy_"))
        }
        patches = {
            name for name in manifest["patch"]["crates-io"]
            if name == "bevy" or name.startswith("bevy_")
        }
        self.assertEqual(patches, selected)


if __name__ == "__main__":
    unittest.main()
