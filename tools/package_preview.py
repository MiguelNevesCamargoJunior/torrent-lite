"""Create deterministic, explicitly unsigned native preview archives."""

import argparse
import hashlib
import json
import re
import subprocess
import zipfile
from pathlib import Path

PLATFORMS = {"windows-x86_64", "linux-x86_64", "macos-arm64"}
FEATURES = ["browser-ui", "mcp"]


def _json_bytes(value):
    return (json.dumps(value, indent=2, sort_keys=True, ensure_ascii=False) + "\n").encode("utf-8")


def _selected_packages(metadata):
    packages = {package["id"]: package for package in metadata["packages"]}
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    root = metadata["resolve"]["root"]
    if root not in nodes or root not in packages:
        raise ValueError("Cargo metadata has no workspace root")
    selected = set()
    waiting = [root]
    while waiting:
        package_id = waiting.pop()
        if package_id in selected:
            continue
        if package_id not in nodes or package_id not in packages:
            raise ValueError("Cargo dependency graph is incomplete")
        selected.add(package_id)
        for dependency in nodes[package_id]["deps"]:
            if any(kind.get("kind") in (None, "build") for kind in dependency["dep_kinds"]):
                waiting.append(dependency["pkg"])
    return sorted(
        (
            {
                "name": packages[package_id]["name"],
                "version": packages[package_id]["version"],
                "license": packages[package_id].get("license") or "NOASSERTION",
                "source": packages[package_id].get("source"),
            }
            for package_id in selected
        ),
        key=lambda item: (item["name"], item["version"], item["source"] or ""),
    )


def _zip_entry(archive, name, content, executable=False):
    info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
    info.compress_type = zipfile.ZIP_DEFLATED
    info.create_system = 3
    info.external_attr = (0o100755 if executable else 0o100644) << 16
    archive.writestr(info, content)


def package_preview(binary, metadata, output, platform, commit, readme, license_file):
    binary, output, readme, license_file = map(Path, (binary, output, readme, license_file))
    if platform not in PLATFORMS or not re.fullmatch(r"[0-9a-f]{40}", commit):
        raise ValueError("invalid platform or commit")
    expected_binary = "torrent-lite.exe" if platform.startswith("windows-") else "torrent-lite"
    if binary.name != expected_binary or not binary.is_file() or binary.stat().st_size == 0:
        raise ValueError("release binary is missing or incorrectly named")
    if not output.is_dir() or not readme.is_file() or not license_file.is_file():
        raise ValueError("package input is missing")
    packages = _selected_packages(metadata)
    root_id = metadata["resolve"]["root"]
    root_package = next(package for package in metadata["packages"] if package["id"] == root_id)
    version = root_package["version"]
    if root_package["name"] != "torrent-lite" or not re.fullmatch(r"[0-9A-Za-z.+-]+", version):
        raise ValueError("invalid torrent-lite package metadata")
    binary_bytes = binary.read_bytes()
    binary_hash = hashlib.sha256(binary_bytes).hexdigest()
    manifest = {
        "binary": expected_binary,
        "binary_sha256": binary_hash,
        "commit": commit,
        "features": FEATURES,
        "platform": platform,
        "release_status": "experimental unsigned preview",
        "version": version,
    }
    inventory = {
        "scope": "Cargo normal and build dependency graph; may include other target platforms",
        "packages": packages,
    }
    archive_name = f"torrent-lite-{version}-{platform}-{commit[:7]}.zip"
    archive_path = output / archive_name
    files = {
        "LICENSE": license_file.read_bytes(),
        "README.md": readme.read_bytes(),
        "dependencies.json": _json_bytes(inventory),
        "manifest.json": _json_bytes(manifest),
        expected_binary: binary_bytes,
    }
    with zipfile.ZipFile(archive_path, "w") as archive:
        for name in sorted(files):
            _zip_entry(archive, name, files[name], executable=name == expected_binary)
    archive_hash = hashlib.sha256(archive_path.read_bytes()).hexdigest()
    checksum_path = output / "SHA256SUMS"
    checksum_path.write_text(f"{archive_hash}  {archive_name}\n", encoding="ascii", newline="\n")
    return archive_path, checksum_path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--platform", required=True, choices=sorted(PLATFORMS))
    parser.add_argument("--commit", required=True)
    args = parser.parse_args()
    metadata = json.loads(
        subprocess.check_output(
            ["cargo", "+1.91.1", "metadata", "--locked", "--format-version", "1", "--features", ",".join(FEATURES)]
        )
    )
    archive, checksum = package_preview(
        args.binary, metadata, args.output, args.platform, args.commit, Path("README.md"), Path("LICENSE")
    )
    print(archive)
    print(checksum)


if __name__ == "__main__":
    main()
