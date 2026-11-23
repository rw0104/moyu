"""Measure production compact chrome and exercise draft resizing with local data.

Run after `npm run dev`: python tests/ui/compact.py
Production screenshots use the real unmocked browser preview. Transcript checks
use the separately marked official mockIPC fixture; no model is contacted.
"""
from __future__ import annotations

import json
from datetime import datetime, timezone
from pathlib import Path

from playwright.sync_api import expect, sync_playwright

ROOT = Path(__file__).resolve().parents[2]
ORIGIN = "http://127.0.0.1:1420/"
FIXTURE = ORIGIN + "tests/ui/conversation.html"
REPORT = ROOT / "artifacts" / "compact-layout.json"

METRICS = """() => {
  const box = selector => {
    const element = document.querySelector(selector);
    if (!element) return null;
    const rect = element.getBoundingClientRect();
    const css = getComputedStyle(element);
    return {x: rect.x, y: rect.y, width: rect.width, height: rect.height,
      background: css.backgroundColor, boxShadow: css.boxShadow,
      fontSize: css.fontSize, lineHeight: css.lineHeight,
      overflowY: css.overflowY, borderTopWidth: css.borderTopWidth};
  };
  return {viewport: {width: innerWidth, height: innerHeight},
    topbar: box('.topbar'), header: box('.conversation-header'),
    context: box('.context-strip'), dock: box('.composer-dock'),
    composer: box('.composer'), input: box('#source-text'),
    reading: box('.conversation-scroll'), timeline: box('.conversation-timeline'),
    horizontalOverflow: document.documentElement.scrollWidth > innerWidth,
    permanentContextStrip: !!document.querySelector('.context-strip'),
    contextDetailsOpen: !!document.querySelector('.context-details[open]'),
    fixture: !!window.__moyuFixture};
}"""


def run():
    prior = json.loads(REPORT.read_text(encoding="utf-8")) if REPORT.exists() else {}
    version = json.loads((ROOT / "package.json").read_text(encoding="utf-8"))["version"]
    report = {"app_version": version, "recorded_at_utc": datetime.now(timezone.utc).isoformat(),
              "baseline": prior.get("baseline"), "after": [], "tests": [],
              "model_api_tested": False, "native_desktop_used": False,
              "production_url": ORIGIN, "fixture_url": FIXTURE}
    errors, remote_requests = [], []
    with sync_playwright() as pw:
        browser = pw.chromium.launch(headless=True)
        context = browser.new_context(locale="zh-CN", reduced_motion="reduce")
        context.on("page", lambda page: page.on("pageerror", lambda error: errors.append(str(error))))
        context.on("request", lambda request: remote_requests.append(request.url)
                   if not request.url.startswith((ORIGIN, "ws://127.0.0.1:1420/", "data:")) else None)
        context.route("https://api.deepseek.com/**", lambda route: route.abort())

        def check(name, callback):
            try:
                detail = callback()
                report["tests"].append({"name": name, "status": "passed", "detail": detail})
                print(f"PASS {name}", flush=True)
            except Exception as exc:
                report["tests"].append({"name": name, "status": "failed", "detail": str(exc)[:2400]})
                print(f"FAIL {name}: {str(exc)[:500]}", flush=True)

        def compact_geometry():
            layouts = []
            for width, height in [(1280, 850), (1920, 1080), (900, 640)]:
                page = context.new_page()
                page.set_viewport_size({"width": width, "height": height})
                page.goto(ORIGIN, wait_until="networkidle")
                expect(page.get_by_role("heading", name="连续翻译", exact=True)).to_be_visible()
                expect(page.locator("article.conversation-turn")).to_have_count(0)
                metrics = page.evaluate(METRICS)
                metrics["reading_share"] = metrics["reading"]["height"] / height
                report["after"].append(metrics)
                page.screenshot(path=str(ROOT / "artifacts" / f"image-history-compact-{width}x{height}.png"))
                assert not metrics["fixture"], "Production entry must not contain test data or mock IPC"
                assert metrics["topbar"] is None, metrics
                assert metrics["header"]["y"] == 0, metrics["header"]
                assert metrics["header"]["height"] <= 52, metrics["header"]
                assert not metrics["permanentContextStrip"], metrics
                assert not metrics["contextDetailsOpen"], metrics
                assert metrics["dock"]["height"] <= 130, metrics["dock"]
                assert metrics["dock"]["y"] + metrics["dock"]["height"] <= height + 1, metrics["dock"]
                assert metrics["reading"]["height"] >= height - 182, metrics["reading"]
                assert metrics["composer"]["width"] <= 900, metrics["composer"]
                assert not metrics["horizontalOverflow"], metrics
                if width == 1920:
                    assert metrics["reading_share"] >= .78, metrics
                layouts.append({"viewport": metrics["viewport"], "toolbar_height": metrics["header"]["height"],
                                "idle_dock_height": metrics["dock"]["height"],
                                "reading_height": metrics["reading"]["height"], "reading_share": metrics["reading_share"]})
                page.close()
            # The real App also runs through the official desktop IPC mock, so
            # desktop chrome can be measured without the browser-preview notice.
            # This is recorded separately and never described as a native run.
            application = context.new_page()
            application.set_viewport_size({"width": 1920, "height": 1080})
            application.goto(FIXTURE + "?application=1", wait_until="networkidle")
            expect(application.get_by_text("本地应用测试 · 不连接模型", exact=True)).to_be_visible()
            expect(application.get_by_role("heading", name="连续翻译", exact=True)).to_be_visible()
            app_metrics = application.evaluate(METRICS)
            app_metrics["transport"] = "real App with official mockIPC, not a native run"
            report["application_mockipc_idle"] = app_metrics
            assert app_metrics["header"]["height"] <= 52, app_metrics["header"]
            assert app_metrics["dock"]["height"] <= 115, app_metrics["dock"]
            assert app_metrics["reading"]["height"] / 1080 >= .78, app_metrics["reading"]
            assert not app_metrics["horizontalOverflow"], app_metrics
            application.screenshot(path=str(ROOT / "artifacts" / "image-history-compact-application-mock-1920x1080.png"))
            application.close()
            return layouts

        def draft_autogrows_and_shrinks_preserving_reading():
            page = context.new_page()
            page.set_viewport_size({"width": 1280, "height": 850})
            page.goto(FIXTURE, wait_until="networkidle")
            page.wait_for_function("() => !!window.__moyuFixture")
            expect(page.get_by_text("本地界面测试 · 不连接模型", exact=True)).to_be_visible()
            expect(page.locator("article.conversation-turn")).to_have_count(40)
            row = page.locator('article[aria-label="第 79 段翻译"]')
            row.locator(".translated-prose").scroll_into_view_if_needed()
            row.evaluate("""element => {
              const region = document.querySelector('.conversation-scroll');
              region.scrollTop += element.getBoundingClientRect().top - region.getBoundingClientRect().top - 18;
            }""")
            page.wait_for_timeout(100)
            idle = page.evaluate(METRICS)
            before = row.bounding_box()["y"]
            original = row.locator(".translated-prose").inner_text()
            text = "\n".join(f"Line {index + 1}: translation draft for continuous reading." for index in range(8))
            page.locator("#source-text").fill(text)
            expect(page.locator("#source-text")).to_have_value(text)
            grown = page.evaluate(METRICS)
            assert 36 <= idle["input"]["height"] <= 60, idle["input"]
            assert 100 <= grown["input"]["height"] <= 160, grown["input"]
            assert grown["input"]["height"] > idle["input"]["height"] + 50, (idle["input"], grown["input"])
            assert abs(row.bounding_box()["y"] - before) <= 2, "Draft resizing moved the currently read paragraph"
            assert row.locator(".translated-prose").inner_text() == original
            assert not grown["horizontalOverflow"]
            page.screenshot(path=str(ROOT / "artifacts" / "image-history-compact-draft-expanded.png"))
            page.locator("#source-text").fill("")
            expect(page.locator("#source-text")).to_have_value("")
            empty = page.evaluate(METRICS)
            assert abs(empty["input"]["height"] - idle["input"]["height"]) <= 1, (idle["input"], empty["input"])
            assert empty["dock"]["height"] <= 130, empty["dock"]
            assert abs(row.bounding_box()["y"] - before) <= 2, "Clearing the draft moved the currently read paragraph"
            page.screenshot(path=str(ROOT / "artifacts" / "image-history-compact-thread.png"))
            details = {"source_lines": 8, "idle_input_height": idle["input"]["height"],
                       "grown_input_height": grown["input"]["height"], "cleared_input_height": empty["input"]["height"],
                       "visible_translation_preserved": True, "paragraph_y_preserved": True,
                       "fixture_data_only": True, "timeline_width": idle["timeline"]["width"]}
            assert idle["timeline"]["width"] <= 900, idle["timeline"]
            page.close()
            return details

        def context_disclosure_is_transient_and_does_not_move_content():
            page = context.new_page()
            page.set_viewport_size({"width": 900, "height": 640})
            page.goto(FIXTURE, wait_until="networkidle")
            page.wait_for_function("() => !!window.__moyuFixture")
            before = page.locator(".conversation-scroll").bounding_box()
            button = page.get_by_label("上下文信息", exact=True)
            expect(button).to_be_visible()
            expect(page.locator(".context-details")).not_to_have_attribute("open", "")
            button.click()
            expect(page.locator(".context-details")).to_have_attribute("open", "")
            expect(page.locator(".context-popover")).to_be_visible()
            popover = page.locator(".context-popover").bounding_box()
            assert popover["x"] >= 0 and popover["x"] + popover["width"] <= 901, popover
            assert page.locator(".conversation-scroll").bounding_box() == before
            button.click()
            expect(page.locator(".context-details")).not_to_have_attribute("open", "")
            page.close()
            return {"closed_by_default": True, "explicitly_opened_and_closed": True,
                    "content_geometry_preserved": True, "fits_compact_window": True}

        check("real_production_entry_compact_geometry_at_three_window_sizes", compact_geometry)
        check("draft_autogrows_eight_lines_and_shrinks_without_moving_reading", draft_autogrows_and_shrinks_preserving_reading)
        check("context_disclosure_overlays_without_consuming_reading_space", context_disclosure_is_transient_and_does_not_move_content)
        if report["baseline"] and report["after"]:
            after = next((item for item in report["after"] if item["viewport"]["width"] == 1920), None)
            if after:
                before = report["baseline"]
                report["comparison_1920x1080"] = {
                    "top_chrome_before": before["reading"]["y"], "top_chrome_after": after["reading"]["y"],
                    "dock_before": before["dock"]["height"], "dock_after": after["dock"]["height"],
                    "reading_before": before["reading"]["height"], "reading_after": after["reading"]["height"],
                    "additional_reading_height": after["reading"]["height"] - before["reading"]["height"]}
        report["page_errors"] = errors
        report["external_network_requests"] = remote_requests
        report["all_passed"] = all(test["status"] == "passed" for test in report["tests"]) and not errors and not remote_requests
        REPORT.parent.mkdir(parents=True, exist_ok=True)
        REPORT.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
        browser.close()
    print(f"Report: {REPORT}", flush=True)
    raise SystemExit(0 if report["all_passed"] else 1)


if __name__ == "__main__":
    run()
