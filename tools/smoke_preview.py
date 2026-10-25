"""Exercise the packaged native CLI without making network connections."""

import argparse
import json
import subprocess
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--binary", type=Path, required=True)
args = parser.parse_args()

for command in ("--help", "serve", "ui", "mcp"):
    arguments = [str(args.binary), command]
    if command != "--help":
        arguments.append("--help")
    subprocess.run(arguments, check=True, stdout=subprocess.DEVNULL)

result = subprocess.check_output(
    [str(args.binary), "inspect-magnet", "magnet:?xt=urn:btih:000102030405060708090a0b0c0d0e0f10111213", "--json"],
    text=True,
)
assert json.loads(result)["info_hash"] == "000102030405060708090a0b0c0d0e0f10111213"
