#!/usr/bin/env python3
"""Run the specification's measurement script; --check detects stale artifacts.

Install tiktoken==0.14.0 first. Python's recorded version is the only field
ignored by --check, since it does not affect the measured text/token tables.
"""
import argparse
import json
from pathlib import Path
import subprocess
import sys
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    spec = root / "spec.md"
    lines = spec.read_bytes().decode("utf-8").split("\n")
    begin = "<!-- KIROHA_" + "MEASUREMENT_SCRIPT_BEGIN -->"
    end = "<!-- KIROHA_" + "MEASUREMENT_SCRIPT_END -->"
    starts = [i for i, line in enumerate(lines) if line == begin]
    stops = [i for i, line in enumerate(lines) if line == end]
    if len(starts) != 1 or len(stops) != 1:
        raise SystemExit("ambiguous measurement-script markers")
    a, b = starts[0], stops[0]
    if not (a < b and lines[a + 1] == "```python" and lines[b - 1] == "```"):
        raise SystemExit("invalid measurement-script fence")
    with tempfile.TemporaryDirectory(prefix="kiroha-measure-") as tmp:
        script = Path(tmp) / "measure.py"
        script.write_bytes(("\n".join(lines[a + 2:b - 1]) + "\n").encode("utf-8"))
        output = Path(tmp) / "measurement.json" if args.check else root / "measurement.json"
        subprocess.run([sys.executable, str(script), str(spec), str(output)], check=True)
        measured = json.loads(output.read_text())
        if not measured["documents"]["generation_card"]["within_target"]:
            raise SystemExit("generation card exceeds its 4096-token target")
        if args.check:
            recorded = json.loads((root / "measurement.json").read_text())
            for value in (measured, recorded):
                value["toolchain"].pop("python", None)
            if measured != recorded:
                raise SystemExit("measurement.json is stale: run python scripts/measure.py")
            print("measurement.json matches the specification")


if __name__ == "__main__":
    main()
