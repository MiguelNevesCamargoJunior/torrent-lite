import hashlib
import json
import tempfile
import unittest
import zipfile
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))
from package_preview import package_preview


class PreviewPackageTests(unittest.TestCase):
    def test_package_contains_verified_binary_and_dependency_inventory_deterministically(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binary = root / "torrent-lite.exe"
            binary.write_bytes(b"synthetic native binary")
            readme = root / "README.md"
            readme.write_text("Experimental build", encoding="utf-8")
            license_file = root / "LICENSE"
            license_file.write_text("Apache-2.0", encoding="utf-8")
            metadata = {
                "packages": [
                    {"id": "torrent-lite 0.1.0-dev", "name": "torrent-lite", "version": "0.1.0-dev", "license": "Apache-2.0", "source": None},
                    {"id": "zlib 1.2.0", "name": "zlib", "version": "1.2.0", "license": "Zlib", "source": "registry+https://example.invalid"},
                    {"id": "alpha 2.0.0", "name": "alpha", "version": "2.0.0", "license": None, "source": "registry+https://example.invalid"},
                ],
                "resolve": {"root": "torrent-lite 0.1.0-dev", "nodes": [
                    {"id": "torrent-lite 0.1.0-dev", "deps": [{"pkg": "zlib 1.2.0", "dep_kinds": [{"kind": None}]}, {"pkg": "alpha 2.0.0", "dep_kinds": [{"kind": "dev"}]}]},
                    {"id": "zlib 1.2.0", "deps": []},
                    {"id": "alpha 2.0.0", "deps": []},
                ]},
            }
            first = root / "first"
            second = root / "second"
            first.mkdir()
            second.mkdir()
            commit = "a" * 40
            archive_one, checksum_one = package_preview(binary, metadata, first, "windows-x86_64", commit, readme, license_file)
            archive_two, checksum_two = package_preview(binary, metadata, second, "windows-x86_64", commit, readme, license_file)
            self.assertEqual(archive_one.read_bytes(), archive_two.read_bytes())
            self.assertEqual(checksum_one.read_bytes(), checksum_two.read_bytes())
            self.assertEqual(checksum_one.read_text(), f"{hashlib.sha256(archive_one.read_bytes()).hexdigest()}  {archive_one.name}\n")
            with zipfile.ZipFile(archive_one) as archive:
                self.assertEqual(archive.namelist(), ["LICENSE", "README.md", "dependencies.json", "manifest.json", "torrent-lite.exe"])
                self.assertEqual(archive.read("torrent-lite.exe"), b"synthetic native binary")
                manifest = json.loads(archive.read("manifest.json"))
                self.assertEqual(manifest["commit"], commit)
                self.assertEqual(manifest["binary_sha256"], hashlib.sha256(binary.read_bytes()).hexdigest())
                self.assertEqual(manifest["features"], ["browser-ui", "mcp"])
                dependencies = json.loads(archive.read("dependencies.json"))
                self.assertEqual([item["name"] for item in dependencies["packages"]], ["torrent-lite", "zlib"])

    def test_invalid_input_does_not_create_an_archive(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binary = root / "torrent-lite.exe"
            binary.write_bytes(b"binary")
            readme = root / "README.md"
            readme.write_text("readme")
            license_file = root / "LICENSE"
            license_file.write_text("license")
            output = root / "output"
            output.mkdir()
            with self.assertRaises(ValueError):
                package_preview(binary, {"packages": [], "resolve": {}}, output, "unknown", "bad", readme, license_file)
            self.assertEqual(list(output.iterdir()), [])


if __name__ == "__main__":
    unittest.main()
