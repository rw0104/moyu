"""Exercise the real workspace through official mockIPC in an isolated browser.

Requires the project's Vite dev server on 127.0.0.1:1420 and installed Playwright.
Run from any directory: python tests/ui/smoke.py
No model, database, API key, production entry, or native desktop is involved.
"""
from __future__ import annotations

import json
import re
from datetime import datetime, timezone
from pathlib import Path
from playwright.sync_api import sync_playwright, expect
from image_history_cases import cases as image_history_cases
from screenshot_cases import cases as screenshot_cases
from dialogue_cases import cases as dialogue_cases

ROOT = Path(__file__).resolve().parents[2]
URL = "http://127.0.0.1:1420/tests/ui/conversation.html"
REPORT = ROOT / "artifacts" / "ui-smoke.json"
SCREENSHOT = ROOT / "artifacts" / "image-history-conversation-long.png"


def run():
    version = json.loads((ROOT / "package.json").read_text(encoding="utf-8"))["version"]
    report = {"app_version": version, "fixture_url": URL, "real_component": "src/TranslationWorkspace.tsx", "styles": "src/styles.css",
              "transport": "official @tauri-apps/api/mocks mockIPC before dynamic import",
              "model_api_tested": False, "native_desktop_used": False, "tests": []}
    errors = []
    remote_requests = []
    with sync_playwright() as pw:
        try:
            browser = pw.chromium.launch(headless=True)
            report["browser"] = "Playwright Chromium, isolated headless"
        except Exception:
            browser = pw.chromium.launch(headless=True, channel="msedge")
            report["browser"] = "Microsoft Edge, isolated headless"
        context = browser.new_context(viewport={"width": 1440, "height": 1050}, device_scale_factor=1,
                                      locale="zh-CN", reduced_motion="reduce")
        page = context.new_page()
        page.on("pageerror", lambda err: errors.append(str(err)))
        page.on("request", lambda req: remote_requests.append(req.url)
                if not req.url.startswith(("http://127.0.0.1:1420/", "ws://127.0.0.1:1420/", "data:")) else None)
        page.route("https://api.deepseek.com/**", lambda route: route.abort())
        page.set_default_timeout(6000)

        def fresh(size=85):
            page.goto(URL, wait_until="domcontentloaded", timeout=30000)
            page.wait_for_function("() => !!window.__moyuFixture", timeout=30000)
            expect(page.get_by_text("本地界面测试 · 不连接模型", exact=True)).to_be_visible()
            if size != 85:
                page.evaluate("size => window.__moyuFixture.reset(size)", size)
            expect(page.locator("article.conversation-turn:not(.pending-turn):not(.failed-turn)")).to_have_count(min(size, 40))

        def check(name, callback):
            try:
                detail = callback()
                report["tests"].append({"name": name, "status": "passed", "detail": detail})
                print(f"PASS {name}", flush=True)
            except Exception as exc:
                report["tests"].append({"name": name, "status": "failed", "detail": str(exc)[:2200]})
                print(f"FAIL {name}: {str(exc)[:400]}", flush=True)

        def loading_and_fixed_composer():
            fresh()
            composer = page.locator(".composer-dock")
            initial = composer.bounding_box()
            page.locator(".conversation-scroll").evaluate("e => e.scrollTop = 0")
            page.get_by_role("button", name="加载更早的 40 段", exact=False).click()
            expect(page.locator("article.conversation-turn")).to_have_count(80)
            page.locator(".conversation-scroll").evaluate("e => e.scrollTop = 0")
            page.get_by_role("button", name="加载更早的 40 段", exact=False).click()
            expect(page.locator("article.conversation-turn")).to_have_count(85)
            final = composer.bounding_box()
            assert abs(initial["y"] - final["y"]) < 2, (initial, final)
            assert final["y"] + final["height"] <= 1051, final
            return {"row_batches": [40, 80, 85], "composer_y": final["y"], "composer_bottom": final["y"] + final["height"]}

        def long_source_and_translation():
            fresh()
            row = page.locator("article.conversation-turn").last
            row.scroll_into_view_if_needed()
            details = row.locator("details")
            expect(details).to_have_count(1)
            details.locator("summary").click()
            expect(details).to_have_attribute("open", "")
            expect(details.locator("p")).to_contain_text("A useful translation")
            # Long translation stays readable and can be expanded when clipped.
            expand = row.get_by_role("button", name="展开完整译文")
            if expand.count():
                expand.click()
                expect(row.get_by_role("button", name="收起译文")).to_have_attribute("aria-expanded", "true")
            return {"long_source_expands": True, "last_source_characters": len(details.locator("p").inner_text())}

        def previously_blocked_text_translates():
            lengths = []
            for length in [6500, 7680]:
                fresh(0)
                text = "x" * length
                page.locator("#source-text").fill(text)
                expect(page.locator("#source-text")).to_have_value(text)
                expect(page.get_by_role("button", name="翻译这一段", exact=True)).to_be_enabled()
                page.get_by_role("button", name="翻译这一段", exact=True).click()
                page.wait_for_function("window.__moyuFixture.pending.translate")
                assert page.evaluate("window.__moyuFixture.lastRequest.text") == text
                page.evaluate("window.__moyuFixture.resolveTranslate()")
                expect(page.locator("article.conversation-turn:not(.pending-turn):not(.failed-turn)")).to_have_count(1)
                expect(page.locator(".translated-prose")).to_have_text(f"测试译文：{text}")
                calls = page.evaluate("window.__moyuFixture.calls")
                assert len([call for call in calls if call["command"] == "plan_translation"]) == 1
                assert len([call for call in calls if call["command"] == "translate"]) == 1
                lengths.append(length)
            return {"characters_preserved_and_translated": lengths, "send_enabled": True,
                    "verified_fixture_turns_per_input": 1, "model_called": False}

        def batch_source():
            return "FIRST|" + ("A" * 29994) + "SECOND|" + ("中" * 29993) + "THIRD|" + ("Z" * 9994)

        def start_batch():
            fresh(0)
            text = batch_source()
            assert len(text) == 70000
            page.locator("#source-text").fill(text)
            page.get_by_role("button", name="翻译这一段", exact=True).click()
            page.wait_for_function("window.__moyuFixture.pending.translate")
            expect(page.get_by_label("长文翻译进度", exact=True)).to_be_visible()
            return text

        def translation_calls():
            return [call["args"]["request"] for call in page.evaluate("window.__moyuFixture.calls")
                    if call["command"] == "translate"]

        def automatic_batch_is_sequential_and_complete():
            text = start_batch()
            next_draft = "下一篇材料的草稿保持原样。"
            page.locator("#source-text").fill(next_draft)
            sent = []
            for index in range(3):
                page.wait_for_function("window.__moyuFixture.pending.translate")
                assert len(translation_calls()) == index + 1
                sent.append(page.evaluate("window.__moyuFixture.lastRequest.text"))
                page.evaluate("window.__moyuFixture.resolveTranslate()")
                expect(page.locator("article.conversation-turn:not(.pending-turn):not(.failed-turn)")).to_have_count(index + 1)
                if index < 2:
                    expect(page.get_by_label("长文翻译进度", exact=True)).to_contain_text(f"{index + 1} / 3")
            expect(page.locator(".pending-turn")).to_have_count(0)
            assert "".join(sent) == text
            assert page.evaluate("window.__moyuFixture.maximumConcurrentTranslations") == 1
            assert len(translation_calls()) == 3
            expect(page.locator("#source-text")).to_have_value(next_draft)
            return {"source_characters": len(text), "fixture_segment_characters": list(map(len, sent)),
                    "all_source_preserved": True, "max_concurrent_requests": 1, "next_draft_preserved": True}

        def batch_pause_resumes_only_remaining_segments():
            text = start_batch()
            first = page.evaluate("window.__moyuFixture.lastRequest.text")
            page.get_by_role("button", name="暂停长文翻译", exact=True).click()
            page.evaluate("window.__moyuFixture.resolveTranslate()")
            expect(page.get_by_label("长文翻译进度", exact=True)).to_contain_text("已暂停")
            expect(page.get_by_role("button", name="继续长文翻译", exact=True)).to_be_visible()
            assert not page.evaluate("window.__moyuFixture.pending.translate")
            assert len(translation_calls()) == 1
            expect(page.locator(".batch-progress")).to_have_attribute("value", "1")
            page.screenshot(path=str(ROOT / "artifacts" / "image-history-longtext-paused.png"), full_page=False)
            page.get_by_role("button", name="继续长文翻译", exact=True).click()
            remaining = []
            for _ in range(2):
                page.wait_for_function("window.__moyuFixture.pending.translate")
                remaining.append(page.evaluate("window.__moyuFixture.lastRequest.text"))
                page.evaluate("window.__moyuFixture.resolveTranslate()")
            expect(page.locator("article.conversation-turn:not(.pending-turn):not(.failed-turn)")).to_have_count(3)
            assert first + "".join(remaining) == text
            assert len(translation_calls()) == 3
            return {"paused_after_completed_segments": 1, "resumed_segments": 2, "completed_segment_repeated": False}

        def batch_failure_retains_remaining_and_new_draft():
            text = start_batch()
            next_draft = "网络等待期间编辑的下一段。"
            page.locator("#source-text").fill(next_draft)
            page.evaluate("window.__moyuFixture.resolveTranslate()")
            page.wait_for_function("window.__moyuFixture.pending.translate")
            failed_source = page.evaluate("window.__moyuFixture.lastRequest.text")
            page.evaluate("window.__moyuFixture.resolveTranslate(true)")
            expect(page.get_by_role("button", name="重试并继续长文翻译", exact=True)).to_be_visible()
            expect(page.locator(".batch-progress")).to_have_attribute("value", "1")
            expect(page.locator("#source-text")).to_have_value(next_draft)
            assert len(translation_calls()) == 2
            assert not page.evaluate("window.__moyuFixture.pending.translate")
            page.get_by_role("button", name="重试并继续长文翻译", exact=True).click()
            page.wait_for_function("window.__moyuFixture.pending.translate")
            assert page.evaluate("window.__moyuFixture.lastRequest.text") == failed_source
            page.evaluate("window.__moyuFixture.resolveTranslate()")
            page.wait_for_function("window.__moyuFixture.pending.translate")
            page.evaluate("window.__moyuFixture.resolveTranslate()")
            expect(page.locator("article.conversation-turn:not(.pending-turn):not(.failed-turn)")).to_have_count(3)
            requests = translation_calls()
            assert len(requests) == 4
            assert requests[1]["text"] == requests[2]["text"]
            assert requests[0]["text"] + requests[2]["text"] + requests[3]["text"] == text
            expect(page.locator("#source-text")).to_have_value(next_draft)
            return {"successful_segments": 3, "attempts": 4, "only_failed_segment_repeated": True,
                    "unprocessed_source_preserved": True, "next_draft_preserved": True}

        def interrupted_batch_can_retrieve_remaining_without_losing_draft():
            text = start_batch()
            next_draft = "另外准备的下一篇材料。"
            first = page.evaluate("window.__moyuFixture.lastRequest.text")
            page.locator("#source-text").fill(next_draft)
            page.get_by_role("button", name="暂停长文翻译", exact=True).click()
            page.evaluate("window.__moyuFixture.resolveTranslate()")
            expect(page.get_by_role("button", name="继续长文翻译", exact=True)).to_be_visible()
            page.evaluate("window.__moyuFixture.resetContextAndRefresh()")
            expect(page.get_by_label("长文翻译进度", exact=True)).to_contain_text("设置或上下文已更新")
            expect(page.get_by_role("button", name="继续长文翻译", exact=True)).to_have_count(0)
            page.get_by_role("button", name="取回未译原文", exact=True).click()
            expect(page.get_by_label("长文翻译进度", exact=True)).to_have_count(0)
            expect(page.locator("#source-text")).to_have_value(f"{text[len(first):]}\n\n{next_draft}")
            assert len(translation_calls()) == 1
            expect(page.locator("article.conversation-turn:not(.pending-turn):not(.failed-turn)")).to_have_count(1)
            return {"outdated_queue_not_resumed": True, "completed_translation_preserved": True,
                    "remaining_characters_retrieved": len(text) - len(first), "new_draft_preserved": True}

        def real_application_tray_button_invokes_native_bridge():
            page.goto(f"{URL}?application=1", wait_until="domcontentloaded", timeout=30000)
            page.wait_for_function("() => !!window.__moyuFixture", timeout=30000)
            expect(page.get_by_text("本地应用测试 · 不连接模型", exact=True)).to_be_visible()
            expect(page.get_by_role("heading", name="连续翻译", exact=False)).to_be_visible()
            tray_button = page.get_by_role("button", name="收起到托盘", exact=True)
            expect(tray_button).to_be_enabled()
            tray_button.click()
            page.wait_for_function("() => window.__moyuFixture.calls.some(c => c.command === 'hide_to_tray')")
            tray_calls = [call for call in page.evaluate("window.__moyuFixture.calls") if call["command"] == "hide_to_tray"]
            assert len(tray_calls) == 1
            return {"real_component": "src/App.tsx", "ipc_command": "hide_to_tray", "invocations": 1,
                    "native_tray_behavior_tested": False}

        def changing_context_during_request_retains_unprocessed_source():
            text = start_batch()
            first = page.evaluate("window.__moyuFixture.lastRequest.text")
            page.evaluate("window.__moyuFixture.resolveTranslate()")
            page.wait_for_function("window.__moyuFixture.pending.translate")
            next_draft = "旧请求等待期间的新草稿。"
            page.locator("#source-text").fill(next_draft)
            page.evaluate("window.__moyuFixture.resetContextAndRefresh()")
            expect(page.get_by_label("长文翻译进度", exact=True)).to_contain_text("设置或上下文已更新")
            retrieve = page.get_by_role("button", name="取回未译原文", exact=True)
            expect(retrieve).to_be_disabled()
            # A real backend rejects the request whose context has become stale.
            # The fixture deliberately rejects it, rather than fabricating a saved
            # successful response from the obsolete context.
            page.evaluate("window.__moyuFixture.resolveTranslate(true)")
            expect(retrieve).to_be_enabled()
            expect(page.locator(".failed-turn")).to_have_count(0)
            assert not page.evaluate("window.__moyuFixture.pending.translate")
            assert len(translation_calls()) == 2
            retrieve.click()
            expect(page.locator("#source-text")).to_have_value(f"{text[len(first):]}\n\n{next_draft}")
            expect(page.locator("article.conversation-turn:not(.pending-turn):not(.failed-turn)")).to_have_count(1)
            return {"context_changed_during_segment": 2, "completed_segments_kept": 1,
                    "unprocessed_source_retained": True, "obsolete_request_failure_not_added": True,
                    "retrieval_disabled_until_pending_request_settles": True}

        def pending_draft_and_direction():
            fresh(0)
            original = "The first request remains intact."
            next_draft = "这是等待期间准备的下一段草稿。"
            page.get_by_label("原文语言", exact=True).select_option("en")
            page.locator("#source-text").fill(original)
            page.get_by_role("button", name="翻译这一段", exact=True).click()
            page.wait_for_function("window.__moyuFixture.pending.translate")
            page.locator("#source-text").fill(next_draft)
            page.get_by_label("原文语言", exact=True).select_option("zh-CN")
            request = page.evaluate("window.__moyuFixture.lastRequest")
            assert request["text"] == original and request["sourceLanguage"] == "en" and request["targetLanguage"] == "zh-CN", request
            page.evaluate("window.__moyuFixture.resolveTranslate()")
            expect(page.locator("article.conversation-turn:not(.pending-turn)")).to_have_count(1)
            expect(page.locator(".translated-prose")).to_have_text(f"测试译文：{original}")
            expect(page.locator("#source-text")).to_have_value(next_draft)
            return {"captured_direction": "en → zh-CN", "next_draft_preserved": True, "completed_turn_appended": True}

        def failure_retry_keeps_next_draft():
            fresh(0)
            original = "Retry exactly this source."
            next_draft = "Continue preparing the next paragraph."
            page.locator("#source-text").fill(original)
            page.get_by_role("button", name="翻译这一段", exact=True).click()
            page.wait_for_function("window.__moyuFixture.pending.translate")
            page.locator("#source-text").fill(next_draft)
            page.evaluate("window.__moyuFixture.resolveTranslate(true)")
            expect(page.locator(".failed-turn")).to_have_count(1)
            expect(page.locator("#source-text")).to_have_value(next_draft)
            page.get_by_role("button", name="重试这一段").click()
            page.wait_for_function("window.__moyuFixture.pending.translate")
            assert page.evaluate("window.__moyuFixture.lastRequest.text") == original
            page.evaluate("window.__moyuFixture.resolveTranslate()")
            expect(page.locator(".failed-turn")).to_have_count(0)
            expect(page.locator("article.conversation-turn")).to_have_count(1)
            expect(page.locator("#source-text")).to_have_value(next_draft)
            return {"failure_and_retry": True, "snapshot_retried": True, "next_draft_preserved": True}

        def edited_translation_requires_locked_review():
            fresh()
            row = page.locator("article.conversation-turn").last
            row.scroll_into_view_if_needed()
            row.get_by_role("button", name="编辑译文", exact=True).click()
            editor = row.locator(".translation-editor")
            candidate = "人工编辑的译文，需要重新校验。"
            editor.fill(candidate)
            expect(row.locator(".edited-badge")).to_have_text("已编辑，尚未校验")
            row.get_by_role("button", name="校验并记住").click()
            page.wait_for_function("window.__moyuFixture.pending.remember")
            assert editor.evaluate("e => e.disabled || e.readOnly"), "The editor must be locked while its text is being reviewed"
            expect(row.locator(".edited-badge")).to_have_text("已编辑，尚未校验")
            assert page.evaluate("window.__moyuFixture.lastMemory.translation") == candidate
            page.evaluate("window.__moyuFixture.resolveRemember()")
            expect(row.locator(".edited-badge")).to_have_text("已编辑，校验通过")
            expect(row.get_by_role("button", name="已存入记忆")).to_be_disabled()
            return {"unreviewed_badge": True, "editor_locked_during_review": True, "verified_only_after_resolve": True}

        def deletion_is_authoritative():
            fresh(0)
            page.locator("#source-text").fill("Delete this acknowledged row.")
            page.get_by_role("button", name="翻译这一段", exact=True).click()
            page.wait_for_function("window.__moyuFixture.pending.translate")
            page.evaluate("window.__moyuFixture.resolveTranslate()")
            expect(page.locator("article.conversation-turn")).to_have_count(1)
            page.wait_for_function("() => window.__moyuFixture.calls.filter(c => c.command === 'get_bootstrap').length > 0")
            page.evaluate("window.__moyuFixture.removeLastAndRefresh()")
            expect(page.locator("article.conversation-turn")).to_have_count(0)
            return {"deleted_acknowledged_row_not_resurrected": True}

        def new_material_keeps_new_typing():
            fresh()
            page.locator("#source-text").fill("Old draft before starting another material.")
            page.get_by_role("button", name="新建上下文").click()
            page.wait_for_function("window.__moyuFixture.pending.session")
            next_draft = "重置请求等待期间，新输入的原文必须保留。"
            page.locator("#source-text").fill(next_draft)
            expect(page.locator(".new-session-button")).to_be_disabled()
            expect(page.get_by_role("button", name="翻译这一段", exact=True)).to_be_disabled()
            page.evaluate("window.__moyuFixture.resolveSession()")
            expect(page.locator("article.conversation-turn")).to_have_count(0)
            expect(page.locator("#source-text")).to_have_value(next_draft)
            expect(page.locator(".context-boundary")).to_have_count(0)
            return {"feed_cleared": True, "new_typing_preserved": True, "no_stale_preserve_banner": True}

        def context_reset_preserves_feed():
            fresh()
            page.evaluate("window.__moyuFixture.resetContextAndRefresh()")
            expect(page.locator("article.conversation-turn")).to_have_count(40)
            expect(page.locator(".context-boundary")).to_contain_text("先前译文保留")
            return {"same_material_preserved": True, "context_notice": True}

        def thousand_rows_are_batched():
            fresh(1000)
            # Counts and model context limits remain available on demand without
            # permanently taking a row of the reading area.
            context_details = page.locator(".context-details")
            expect(context_details).not_to_have_attribute("open", "")
            page.get_by_label("上下文信息", exact=True).click()
            expect(page.locator(".context-count")).to_contain_text(re.compile(r"1,?000"))
            expect(page.locator(".context-popover")).to_contain_text("12")
            expect(page.locator(".context-popover")).to_contain_text("24,000")
            expect(page.locator(".context-popover")).to_contain_text("不自动清理")
            page.get_by_label("上下文信息", exact=True).click()
            page.locator(".conversation-scroll").evaluate("e => e.scrollTop = 0")
            page.get_by_role("button", name="加载更早的 40 段", exact=False).click()
            expect(page.locator("article.conversation-turn")).to_have_count(80)
            assert page.evaluate("document.documentElement.scrollWidth <= window.innerWidth"), "Unexpected horizontal document overflow"
            return {"retained": 1000, "initial_rendered": 40, "after_load_earlier": 80, "horizontal_overflow": False}

        def reading_older_does_not_jump():
            fresh()
            page.locator("#source-text").fill("A new paragraph while reading older translations.")
            page.get_by_role("button", name="翻译这一段", exact=True).click()
            page.wait_for_function("window.__moyuFixture.pending.translate")
            page.locator(".conversation-scroll").evaluate("e => e.scrollTop = 0")
            expect(page.get_by_role("button", name="回到最新", exact=True)).to_be_visible()
            first = page.locator("article.conversation-turn").first.get_attribute("aria-label")
            page.evaluate("window.__moyuFixture.resolveTranslate()")
            page.wait_for_function("!window.__moyuFixture.pending.translate")
            expect(page.locator("article.conversation-turn").first).to_have_attribute("aria-label", first)
            assert page.locator(".conversation-scroll").evaluate("e => e.scrollTop") < 5
            return {"first_visible_group_preserved": first, "scroll_top_preserved": True}

        def compact_windows_keep_composer_visible():
            layouts = []
            for width, height in [(1280, 850), (900, 640)]:
                page.set_viewport_size({"width": width, "height": height})
                fresh()
                dock = page.locator(".composer-dock").bounding_box()
                region = page.locator(".conversation-scroll").bounding_box()
                assert dock["y"] + dock["height"] <= height + 1, dock
                assert region["height"] >= 80, region
                assert page.evaluate("document.documentElement.scrollWidth <= innerWidth")
                layouts.append({"width": width, "height": height, "reading_region_height": region["height"]})
            page.set_viewport_size({"width": 1440, "height": 1050})
            return layouts

        for name, callback in [
            ("85_rows_load_in_40_row_batches_and_composer_stays_fixed", loading_and_fixed_composer),
            ("long_source_and_translation_can_expand", long_source_and_translation),
            ("6500_and_7680_characters_submit_and_translate_without_rejection", previously_blocked_text_translates),
            ("long_batch_automatically_translates_sequentially_without_source_loss", automatic_batch_is_sequential_and_complete),
            ("long_batch_pause_resume_does_not_repeat_completed_segments", batch_pause_resumes_only_remaining_segments),
            ("long_batch_failure_retry_retains_remaining_source_and_next_draft", batch_failure_retains_remaining_and_new_draft),
            ("interrupted_batch_retrieves_remaining_source_without_losing_draft", interrupted_batch_can_retrieve_remaining_without_losing_draft),
            ("context_change_during_pending_segment_retains_remaining_source", changing_context_during_request_retains_unprocessed_source),
            ("real_application_tray_control_invokes_native_bridge", real_application_tray_button_invokes_native_bridge),
            ("next_draft_and_request_snapshot_survive_direction_change", pending_draft_and_direction),
            ("failure_retry_preserves_next_draft", failure_retry_keeps_next_draft),
            ("edited_translation_unverified_until_locked_review_resolves", edited_translation_requires_locked_review),
            ("deleted_acknowledged_row_does_not_resurrect", deletion_is_authoritative),
            ("new_material_preserves_new_typing_without_stale_banner", new_material_keeps_new_typing),
            ("context_only_reset_preserves_transcript", context_reset_preserves_feed),
            ("1000_rows_render_in_bounded_batches", thousand_rows_are_batched),
            ("reading_older_group_is_preserved_when_new_translation_arrives", reading_older_does_not_jump),
            ("compact_windows_keep_composer_visible", compact_windows_keep_composer_visible),
        ]:
            check(name, callback)

        for name, callback in image_history_cases(page, fresh, ROOT, URL):
            check(name, callback)

        for name, callback in screenshot_cases(page, ROOT, URL):
            check(name, callback)

        for name, callback in dialogue_cases(page, ROOT, URL):
            check(name, callback)

        fresh()
        page.locator("article.conversation-turn").last.scroll_into_view_if_needed()
        page.locator("#source-text").fill("继续粘贴下一段原文，前面的译文仍可随时回看。")
        page.screenshot(path=str(SCREENSHOT), full_page=False)
        page.locator('article[aria-label="第 79 段翻译"]').evaluate("""element => {
            const region = document.querySelector('.conversation-scroll');
            region.scrollTop += element.getBoundingClientRect().top - region.getBoundingClientRect().top - 18;
        }""")
        page.locator('article[aria-label="第 79 段翻译"] .translated-prose').scroll_into_view_if_needed()
        page.evaluate("() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))")
        page.screenshot(path=str(ROOT / "artifacts" / "image-history-conversation-thread.png"), full_page=False)
        production = context.new_page()
        production.on("pageerror", lambda err: errors.append(str(err)))
        production.set_viewport_size({"width": 1280, "height": 850})
        production.goto("http://127.0.0.1:1420/", wait_until="domcontentloaded", timeout=30000)
        expect(production.get_by_role("heading", name="连续翻译", exact=False)).to_be_visible()
        expect(production.locator("article.conversation-turn")).to_have_count(0)
        production.screenshot(path=str(ROOT / "artifacts" / "image-history-empty.png"))
        production.get_by_role("button", name="设置", exact=True).click()
        expect(production.locator("#api-key")).to_be_disabled()
        expect(production.locator(".settings-sections").get_by_text("24,000", exact=False).first).to_be_visible()
        production.screenshot(path=str(ROOT / "artifacts" / "image-history-settings.png"))
        report["production_preview"] = {"empty_feed": True, "api_key_disabled": True, "context_limits_disclosed": True}
        report["screenshot"] = str(SCREENSHOT)
        report["page_errors"] = errors
        report["external_network_requests"] = remote_requests
        report["all_passed"] = all(test["status"] == "passed" for test in report["tests"]) and not errors and not remote_requests
        report["recorded_at_utc"] = datetime.now(timezone.utc).isoformat()
        REPORT.parent.mkdir(parents=True, exist_ok=True)
        REPORT.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
        browser.close()
    print(f"Report: {REPORT}", flush=True)
    raise SystemExit(0 if report["all_passed"] else 1)


if __name__ == "__main__":
    run()
