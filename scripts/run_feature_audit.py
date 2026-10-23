#!/usr/bin/env python3
"""Run native feature checks sequentially, including GPU and CPU workloads.

Results and full command logs are written under target/qa/feature-audit. All
autosave writes use a temporary data directory, never the user's saved model.
"""
import datetime
import json
import os
from pathlib import Path
import platform
import re
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "target/qa/feature-audit"
TOOLS = {
    "Select": "close_snapped_outline_then_select_and_delete_face_or_edge",
    "Line": "click_line_precision_and_axis_lock_through_real_ui",
    "Rectangle": "shape_tools_create_faces_and_undo_as_one_operation",
    "Circle": "shape_tools_create_faces_and_undo_as_one_operation",
    "Polygon": "polygon_minimum_and_maximum_sides_commit_valid_faces_and_undo",
    "Freehand": "freehand_drag_commits_a_stroke_and_escape_cancels_it",
    "PushPull": "push_pull_pointer_preview_numeric_commit_cancel_and_undo",
    "Flip": "flip_uses_keyboard_inspector_and_clickable_planes_without_detaching_objects",
    "OuterShell": "tool_outer_shell_menu", "Union": "tool_union_menu",
    "Intersect": "tool_intersect_menu", "Trim": "tool_trim_menu", "Split": "tool_split_menu",
    "Scale": "move_and_scale_real_pointer_clicks_and_axis_locked_numeric_distance",
    "Offset": "offset_and_paint_through_viewport_preserve_history_and_saved_color",
    "Move": "move_and_scale_real_pointer_clicks_and_axis_locked_numeric_distance",
    "Paint": "all_paint_swatches_apply_the_selected_color_to_a_face",
    "TapeMeasure": "tape_measure_snaps_reads_distance_and_restarts_without_editing_geometry",
    "Protractor": "protractor_uses_three_snapped_points_and_measures_in_three_dimensions",
    "Dimensions": "dimensions_commit_only_after_placement_and_support_selection_delete_and_undo",
    "Erase": "eraser_sweep_is_one_undo_step_and_ui_panels_never_draw",
    "Orbit": "orbit_uses_all_objects_center_after_pan_and_zoom",
}
WORKFLOWS = {
    "Projects: New / Save / Discard / Cancel": "new_project_save_discard_and_cancel_buttons",
    "Open / invalid and cancelled files": "failed_or_cancelled_file_operations",
    "Save / Save As / atomic replacement": "ketchup_files_round_trip_complete_projects",
    "Native autosave / corruption / write failure / recovery": "native_autosave_roundtrip",
    "SketchUp import / hierarchy / holes / materials / units": "skp_import_preserves_nested_geometry",
    "OBJ export / holes / visibility": "obj_export_preserves_hole_area",
    "Groups / Ungroup / right-click menu": "context_menu_preserves_multiselection",
    "Components / Explode": "context_menu_creates_named_component",
    "Nested editing / breadcrumb / Escape": "double_click_enters_nested_objects",
    "Linked component propagation": "nested_component_edits_reach_standalone_instances",
    "Cut / Copy / Paste / placement / undo": "clipboard_events_support_in_place_paste",
    "Malformed clipboard": "malformed_clipboard_payloads",
    "Lock / Unlock / Hide / Unhide All": "context_menu_lock_protects_delete",
    "Tags: Add / Rename / Cancel / Delete / Undo": "tag_ui_add_rename_cancel_delete",
    "Tags: Assign / Untagged / visibility / clipboard": "tagged_clipboard_merges_names",
    "Tags: Unicode case / duplicate names": "unicode_tag_names_cannot_create_invalid_case_duplicates",
    "Component library placement / cancellation": "library_placement_creates_linked_instance",
    "Move Copy / all array syntaxes / revision / undo": "move_copy_arrays_divide_repeat_revise",
    "Move Copy / invalid arrays": "move_copy_invalid_arrays_are_atomic",
    "Snapping / axis locks / editing scope": "cached_snapping_matches_geometry_locks_visibility",
    "Dimensions / X Y Z alignment": "dimensions_axis_lock_updates_preview",
    "Selection: window / crossing": "crossing_and_window_selection_differ",
    "Navigation / Shift pan / middle mouse": "shift_orbit_pans_and_middle_navigation",
    "Zoom / cursor anchor": "pan_translates_view_without_rotating_and_zoom_keeps_anchor",
    "Window controls / minimum width / resize": "toolbar_window_actions_remain_accessible",
    "Keyboard shortcuts / help": "all_keyboard_tool_shortcuts",
    "Invalid typed measurements": "invalid_precision_inputs_preserve_model",
    "Every tool: interrupted gestures / focus loss / resize": "every_tool_survives_interrupted_gestures",
    "Display: Reset all settings": "reset_display_restores_every_setting",
    "Display: independent GPU pixel comparisons": "gpu_renders_individual_display_options",
    "Axes: front / back GPU depth": "gpu_renders_empty_and_modeled_workspace",
}


def run(label, command, env):
    print(f"Running {label}...", flush=True)
    started = time.monotonic()
    log = OUT / f"{label}.log"
    with log.open("w") as output:
        process = subprocess.Popen(command, cwd=ROOT, env=env, stdout=subprocess.PIPE,
                                   stderr=subprocess.STDOUT, text=True, bufsize=1)
        for line in process.stdout:
            output.write(line)
            output.flush()
            if any(marker in line for marker in ["test result:", "FAILED", "Finished `", "has been running"]):
                print(line.rstrip(), flush=True)
        code = process.wait()
    return {"label": label, "command": command, "returncode": code,
            "seconds": round(time.monotonic() - started, 3), "log": str(log)}


def test_results(log):
    results, pending = {}, None
    for line in log.splitlines():
        match = re.match(r"test (\S+) \.\.\.\s*(.*)", line)
        if match:
            pending, tail = match.groups()
            status = re.match(r"(ok|FAILED|ignored)\b", tail)
            if status:
                results[pending] = status.group(1)
                pending = None
        elif pending and line.strip() in ("ok", "FAILED", "ignored"):
            results[pending], pending = line.strip(), None
    return results


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="ketchup-feature-audit-") as directory:
        env = dict(os.environ, KETCHUP_DATA_DIR=directory)
        commands = [
            ("rust", ["cargo", "test", "--workspace", "--locked", "--config",
                      "profile.test.package.ketchup-app.opt-level=2", "--config",
                      "profile.test.package.ketchup-core.opt-level=2", "--",
                      "--include-ignored", "--test-threads=1", "--show-output"]),
            ("workers", ["node", "--test", "scripts/test_skp_import.cjs"]),
            ("format", ["cargo", "fmt", "--all", "--", "--check"]),
            ("whitespace", ["git", "diff", "--check"]),
        ]
        runs = [run(label, command, env) for label, command in commands]
    rust_log = (OUT / "rust.log").read_text()
    results = test_results(rust_log)
    tool_times = {name: {"mean_ms": float(mean), "max_ms": float(maximum)} for name, mean, maximum
                  in re.findall(r"TOOL_CPU (\w+) mean=([\d.]+) max=([\d.]+) ms", rust_log)}
    comparisons = {name: int(pixels) for name, pixels in
                   re.findall(r"DISPLAY_RENDER audit-(\S+) changed=(\d+)", rust_log)}
    display_times = {name: float(ms) for name, ms in
                     re.findall(r"DISPLAY_CPU audit-(\S+) ([\d.]+) ms", rust_log)}

    def evidence(name, pattern):
        tests = {test: state for test, state in results.items() if pattern in test}
        return {"feature": name, "status": "passed" if tests and all(s == "ok" for s in tests.values()) else "unverified-or-failed", "tests": tests}

    features = [evidence(name, pattern) for name, pattern in {**TOOLS, **WORKFLOWS}.items()]
    fields = re.findall(r"pub (\w+):", (ROOT / "crates/ketchup-core/src/display.rs").read_text().split("impl Default")[0])
    field_results = []
    for field in fields:
        pairs = {key: value for key, value in comparisons.items()
                 if key == field or field == "face_style" and key.startswith("style-")}
        field_results.append({"field": field, "status": "passed" if pairs else "unverified-or-failed",
                              "gpu_changed_pixels": pairs})
    workers_log = (OUT / "workers.log").read_text()
    worker_results = re.findall(r"^ok \d+ - (.+)$", workers_log, re.M)
    limitations = [
        "Native Linux tests use synthetic egui pointer/keyboard events and an offscreen Vulkan GPU.",
        "Browser importer/worker logic runs in Node; full browser startup, localStorage, downloads and WebGPU/WebGL fallback were not exercised.",
        "Native OS file-picker and system-clipboard integration, Windows/macOS execution, and installed desktop launcher require live platform checks.",
        "Numeric checks cover representative values and documented boundaries; this is not exhaustive enumeration of all inputs or option combinations.",
        "CPU frame timings exclude GPU presentation; per-tool workloads measure pointer interaction on 4096 imported faces.",
    ]
    passed = all(r["returncode"] == 0 for r in runs) and all(f["status"] == "passed" for f in features + field_results) and set(tool_times) == set(TOOLS)
    report = {"timestamp_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
              "platform": platform.platform(), "automated_audit_passed": passed,
              "runs": runs, "rust_tests": results, "worker_tests": worker_results,
              "features": features, "display_fields": field_results,
              "tool_cpu": tool_times, "display_cpu_ms": display_times,
              "gpu_comparisons": comparisons, "limitations": limitations}
    (OUT / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    lines = ["KetchUp feature audit", report["timestamp_utc"], report["platform"],
             f"Automated checks: {'PASS' if passed else 'FAIL / INCOMPLETE'}",
             f"Rust tests: {sum(s == 'ok' for s in results.values())} passed / {len(results)} recorded",
             f"Worker tests: {len(worker_results)} passed", "", "Feature evidence:"]
    for feature in features:
        lines.append(f"[{feature['status']}] {feature['feature']}")
        lines.extend(f"  {name}: {status}" for name, status in feature["tests"].items())
    lines += ["", "Independent display GPU comparisons:"]
    lines.extend(f"[passed] {name}: {pixels} viewport pixels changed" for name, pixels in comparisons.items())
    lines += ["", "4096-face pointer workloads (CPU milliseconds):"]
    lines.extend(f"{name}: mean {t['mean_ms']}, max {t['max_ms']}" for name, t in tool_times.items())
    lines += ["", "Scope and remaining platform checks:"] + limitations
    (OUT / "report.txt").write_text("\n".join(lines) + "\n")
    print(f"Report: {OUT / 'report.txt'}", flush=True)
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())
