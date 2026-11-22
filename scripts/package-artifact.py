"""Verify embedded UI versions before copying the Windows installer for delivery."""
import hashlib
import json
from pathlib import Path
import re
import shutil

root = Path(__file__).resolve().parents[1]
package = json.loads((root / "package.json").read_text(encoding="utf-8"))
version = package["version"]
assert re.fullmatch(r"\d+\.\d+\.\d+", version), "Expected a numeric release version"
config = json.loads((root / "src-tauri/tauri.conf.json").read_text(encoding="utf-8"))
assert config["version"] == version, "Frontend and desktop versions differ"
binary = root / "src-tauri/target/release/moyu.exe"
binary_bytes = binary.read_bytes()
assets = list((root / "dist/assets").glob("*.js")) + list((root / "dist/assets").glob("*.css"))
assert assets, "No production UI assets found"
embedded = {asset.name: asset.name.encode() in binary_bytes for asset in assets}
assert all(embedded.values()), f"Desktop executable contains outdated UI assets: {embedded}"
for asset in assets:
    content = asset.read_bytes()
    assert b"__moyuFixture" not in content and "本地界面测试".encode() not in content, "Test fixture entered production"
installer = root / "src-tauri/target/release/bundle/nsis" / f"{config['productName']}_{version}_x64-setup.exe"
ui_report = json.loads((root / "artifacts/ui-smoke.json").read_text(encoding="utf-8"))
assert ui_report["all_passed"], "UI regression suite has unresolved failures"
assert ui_report.get("app_version") == version, "UI regression report is from another version"
layout_report = json.loads((root / "artifacts/compact-layout.json").read_text(encoding="utf-8"))
assert layout_report["all_passed"] and layout_report["app_version"] == version, "Compact layout verification is incomplete"
backend_report = json.loads((root / "artifacts/backend-tests.json").read_text(encoding="utf-8"))
assert backend_report["all_passed"] and backend_report["app_version"] == version, "Backend verification is incomplete"
model_report = json.loads((root / "artifacts/model-api-smoke.json").read_text(encoding="utf-8"))
assert model_report["all_passed"] and model_report["app_version"] == version, "Real image translation verification is incomplete"
capture_report = json.loads((root / "artifacts/capture-smoke.json").read_text(encoding="utf-8"))
assert capture_report["all_passed"] and capture_report["app_version"] == version, "Native capture verification is incomplete"
dialogue_report = json.loads((root / "artifacts/dialogue-api-smoke.json").read_text(encoding="utf-8"))
assert dialogue_report["all_passed"] and dialogue_report["app_version"] == version, "Bilingual dialogue verification is incomplete"
dialogue_ui_report = json.loads((root / "artifacts/dialogue-ui-smoke.json").read_text(encoding="utf-8"))
assert dialogue_ui_report["all_passed"] and dialogue_ui_report["app_version"] == version, "Dialogue UI verification is incomplete"
output = root / "artifacts" / f"Moyu-{version}-windows-x64-setup.exe"
shutil.copyfile(installer, output)
metadata = {
    "version": version,
    "installer": str(output),
    "size": output.stat().st_size,
    "sha256": hashlib.sha256(output.read_bytes()).hexdigest(),
    "embedded_assets": embedded,
    "production_contains_test_fixture": False,
    "backend_tests_passed": backend_report["passed"],
    "backend_tests_version": backend_report["app_version"],
    "ui_tests_passed": len(ui_report["tests"]),
    "layout_tests_passed": len(layout_report["tests"]),
    "native_tray_hide_tested": True,
    "native_tray_hide_tested_version": "0.3.0",
    "real_model_api_tested": True,
    "real_model_api_tested_version": model_report["app_version"],
    "real_model_api_scenario": model_report["scenario"],
    "native_capture_tested": True,
    "native_capture_tested_version": capture_report["app_version"],
    "native_capture_scope": "Actual Win32 registration and hidden synthetic selection-window tests; no interactive user desktop run",
    "native_capture_interactive_desktop_tested": False,
    "verification_archive": f"artifacts/releases/{version}",
    "bilingual_dialogue_api_tested": True,
    "bilingual_dialogue_api_tested_version": dialogue_report["app_version"],
}
(root / "artifacts/build-info.json").write_text(json.dumps(metadata, ensure_ascii=False, indent=2), encoding="utf-8")
archive = root / "artifacts/releases" / version
archive.mkdir(parents=True, exist_ok=True)
for report in ("backend-tests.json", "ui-smoke.json", "compact-layout.json", "model-api-smoke.json", "capture-smoke.json", "dialogue-api-smoke.json", "dialogue-ui-smoke.json", "build-info.json"):
    shutil.copyfile(root / "artifacts" / report, archive / report)
print(json.dumps(metadata, ensure_ascii=False, indent=2))
