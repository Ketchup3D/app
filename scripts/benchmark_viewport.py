#!/usr/bin/env python3
"""Profile a real, isolated native viewport without building or editing the project.

Example:
    python3 scripts/benchmark_viewport.py --project ~/Downloads/Example.ketchup \
        --binary target/release/ketchup-app --seconds 30 --output /tmp/ketchup-bench

The native benchmark renders the saved camera, an orbit without Wood Joinery,
the identical orbit with Wood Joinery, and the saved camera again. Each stage
has two seconds of warmup followed by the requested sampling duration. Reports
contain actual frame intervals, CPU timings, and screenshots, not a CPU-only
estimate of FPS. The supplied binary must already be built.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--project", type=Path, required=True)
    parser.add_argument("--binary", type=Path, default=Path("target/release/ketchup-app"))
    parser.add_argument("--profile", action="store_true", help="Collect detailed CPU scopes (adds timing overhead)")
    parser.add_argument("--edit-joinery", nargs="?", const="auto", metavar="CONNECTION_ID",
                        help="Open the Wood Joinery editor for a saved joint (defaults to the first)")
    parser.add_argument("--seconds", type=float, default=30)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if not 4 <= args.seconds <= 120:
        parser.error("--seconds must be between 4 and 120")
    project, binary = args.project.expanduser().resolve(), args.binary.expanduser().resolve()
    if not project.is_file() or not binary.is_file():
        parser.error("--project and --binary must refer to existing files")
    output = args.output.expanduser().resolve()
    output.mkdir(parents=True, exist_ok=True)
    # A distinct data directory isolates the model, account and UI preferences.
    data = output / "data"
    data.mkdir(exist_ok=True)
    original_hash = hashlib.sha256(project.read_bytes()).hexdigest()
    shutil.copyfile(project, data / "model.json")
    environment = dict(os.environ, KETCHUP_DATA_DIR=str(data),
                       KETCHUP_VIEWPORT_BENCHMARK=str(output / "frames.json"),
                       KETCHUP_BENCH_SECONDS=str(args.seconds), KETCHUP_BENCH_PROFILE="1" if args.profile else "0")
    if args.edit_joinery:
        environment["KETCHUP_BENCH_EDIT_JOINERY"] = args.edit_joinery
    else:
        environment.pop("KETCHUP_BENCH_EDIT_JOINERY", None)
    with (output / "application.log").open("w") as log:
        subprocess.run([str(binary)], env=environment, stdout=log,
                       stderr=subprocess.STDOUT, check=True)
    if hashlib.sha256(project.read_bytes()).hexdigest() != original_hash:
        raise RuntimeError("The source project changed during the benchmark")
    reports = json.loads((output / "frames.json").read_text())
    if len(reports) != 4:
        raise RuntimeError("The viewport benchmark did not finish all four stages")
    metadata = {"project": str(project), "project_sha256": original_hash,
                "binary": str(binary),
                "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
                "sample_seconds_per_stage": args.seconds, "edit_joinery": args.edit_joinery}
    (output / "run.json").write_text(json.dumps(metadata, indent=2) + "\n")
    print("| Viewport workload | FPS | Frame p95 (ms) | CPU mean (ms) |")
    print("| --- | ---: | ---: | ---: |")
    for report in reports:
        frame, cpu = report["frame"], report["eframe_cpu"]
        print(f"| {report['stage']} | {frame['fps']:.2f} | {frame['p95_ms']:.2f} | {cpu['mean_ms']:.2f} |")
    print(f"\nReports and screenshots: {output}")


if __name__ == "__main__":
    main()
