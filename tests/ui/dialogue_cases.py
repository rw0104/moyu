"""Bidirectional conversation transport and display regression cases.

All detection/translation responses are deterministic local mockIPC values.
These tests do not establish real language-detection accuracy or call a model.
"""
from __future__ import annotations

import base64
from pathlib import Path

from playwright.sync_api import expect


def cases(page, root: Path, url: str):
    image_bytes = (root / "tests/ui/fixtures/translation-source.png").read_bytes()
    data_url = "data:image/png;base64," + base64.b64encode(image_bytes).decode("ascii")

    def fresh():
        page.set_viewport_size({"width": 1440, "height": 1050})
        page.goto(url + "?application=1", wait_until="domcontentloaded", timeout=30000)
        page.wait_for_function("() => !!window.__moyuFixture", timeout=30000)
        expect(page.get_by_role("heading", name="连续翻译", exact=True)).to_be_visible()
        page.get_by_role("button", name="双向对话", exact=True).click()
        expect(page.get_by_role("heading", name="双向对话", exact=True)).to_be_visible()
        expect(page.get_by_label("客户语言", exact=True)).to_have_value("auto")

    def command_calls(command):
        return page.evaluate("command => window.__moyuFixture.calls.filter(c => c.command === command)", command)

    def submit(text, source, target):
        page.locator("#source-text").fill(text)
        page.get_by_role("button", name="翻译这一段", exact=True).click()
        page.wait_for_function("window.__moyuFixture.pending.translate")
        request = page.evaluate("window.__moyuFixture.lastRequest")
        assert request["text"] == text, request
        assert request["sourceLanguage"] == source and request["targetLanguage"] == target, request
        return request

    def complete(count):
        page.evaluate("window.__moyuFixture.resolveTranslate()")
        expect(page.locator("article.conversation-turn:not(.pending-turn):not(.failed-turn)")).to_have_count(count)
        page.wait_for_function("!window.__moyuFixture.pending.translate")

    def bilingual_replies_auto_reverse_direction():
        fresh()
        session = page.evaluate("window.__moyuFixture.state.sessionId")
        outbound = submit("您好，我们明天安排发货。", "zh-CN", "en")
        complete(1)
        inbound = submit("Please confirm the tracking number.", "en", "zh-CN")
        complete(2)
        assert outbound["sessionId"] == inbound["sessionId"] == session
        expect(page.locator("article[data-dialogue-direction=outbound]")).to_have_count(1)
        expect(page.locator("article[data-dialogue-direction=inbound]")).to_have_count(1)
        assert len(command_calls("resolve_dialogue")) == 2
        expect(page.get_by_label("对话翻译方向", exact=True)).to_have_value("auto")
        page.screenshot(path=str(root / "artifacts/dialogue-bilingual-thread.png"))
        return {"chinese_to_english_and_english_to_chinese": True, "same_conversation": True,
                "direction_labels_rendered": True, "real_language_detection_tested": False}

    def auto_peer_follows_german_customer_then_chinese_reply():
        fresh()
        submit("Hallo, wann wird die Bestellung versandt?", "de", "zh-CN")
        complete(1)
        assert page.evaluate("window.__moyuFixture.state.dialoguePreferences.lastPeerLanguage") == "de"
        submit("订单将在明天发出。", "zh-CN", "de")
        complete(2)
        expect(page.get_by_label("客户语言", exact=True)).to_have_value("auto")
        return {"german_incoming_translates_to_chinese": True, "next_chinese_reply_targets_german": True,
                "automatic_peer_preference_retained": True}

    def fixed_peer_is_not_overwritten_by_third_language():
        fresh()
        page.get_by_label("客户语言", exact=True).select_option("fr")
        page.wait_for_function("window.__moyuFixture.state.dialoguePreferences.peerLanguage === 'fr'")
        submit("Hallo, ich habe eine Frage zur Bestellung.", "de", "zh-CN")
        complete(1)
        submit("请告诉我您的订单号。", "zh-CN", "fr")
        complete(2)
        expect(page.get_by_label("客户语言", exact=True)).to_have_value("fr")
        return {"third_language_detected_as_actual_source": True, "fixed_french_reply_target_preserved": True}

    def ambiguous_short_text_exposes_fallback_and_manual_direction():
        fresh()
        submit("12345", "en", "zh-CN")
        complete(1)
        expect(page.get_by_text("短句方向不明确", exact=False)).to_be_visible()
        page.get_by_label("对话翻译方向", exact=True).select_option("outbound")
        submit("SKU-A12", "zh-CN", "en")
        complete(2)
        route_calls = command_calls("resolve_dialogue")
        assert route_calls[-1]["args"]["request"]["direction"] == "outbound"
        return {"neutral_text_uses_known_direction_with_notice": True, "manual_direction_can_correct_next_message": True}

    def failed_translation_retry_does_not_repeat_language_route():
        fresh()
        request = submit("请提供收货地址。", "zh-CN", "en")
        page.evaluate("window.__moyuFixture.resolveTranslate(true)")
        expect(page.get_by_role("button", name="重试这一段", exact=True)).to_be_visible()
        page.locator("#source-text").fill("下一份草稿不能被失败重试清空。")
        page.get_by_role("button", name="重试这一段", exact=True).click()
        page.wait_for_function("window.__moyuFixture.pending.translate")
        assert page.evaluate("window.__moyuFixture.lastRequest") == request
        assert len(command_calls("resolve_dialogue")) == 1
        complete(1)
        expect(page.locator("#source-text")).to_have_value("下一份草稿不能被失败重试清空。")
        return {"resolved_direction_is_immutable_on_retry": True, "route_calls": 1, "next_draft_preserved": True}

    def language_route_failure_keeps_source_for_retry():
        fresh()
        text = "语言识别网络失败时，原文不能丢失。"
        page.evaluate("window.__moyuFixture.failNextDialogueRoute()")
        page.locator("#source-text").fill(text)
        page.get_by_role("button", name="翻译这一段", exact=True).click()
        expect(page.get_by_text("测试语言识别失败：原文已保留，请重试。", exact=True)).to_be_visible()
        expect(page.locator("#source-text")).to_have_value(text)
        assert not page.evaluate("window.__moyuFixture.pending.translate")
        assert len(command_calls("translate")) == 0
        submit(text, "zh-CN", "en")
        complete(1)
        assert len(command_calls("resolve_dialogue")) == 2
        return {"route_failure_keeps_source": True, "no_translation_with_unresolved_direction": True,
                "retry_routes_then_translates": True}

    def screenshot_route_failure_reuses_recognized_image_on_retry():
        fresh()
        page.locator("#source-text").fill("截图识别方向失败时保留下一条草稿。")
        page.evaluate("window.__moyuFixture.failNextDialogueRoute()")
        page.evaluate("image => window.__moyuFixture.emitCaptureImage(image.dataUrl, image.name)", {"dataUrl": data_url, "name": "route-retry-screenshot.png"})
        page.wait_for_function("window.__moyuFixture.pending.extract")
        page.evaluate("window.__moyuFixture.resolveImage(false, 'Hallo, bitte senden Sie das Paket.')")
        retry = page.get_by_role("button", name="重试截图翻译 route-retry-screenshot.png", exact=True)
        expect(retry).to_be_visible()
        assert len(command_calls("translate")) == 0
        retry.click()
        page.wait_for_function("window.__moyuFixture.pending.translate")
        assert len(command_calls("extract_image")) == 1
        assert len(command_calls("resolve_dialogue")) == 2
        request = page.evaluate("window.__moyuFixture.lastRequest")
        assert request["sourceLanguage"] == "de" and request["targetLanguage"] == "zh-CN"
        complete(1)
        expect(page.locator("#source-text")).to_have_value("截图识别方向失败时保留下一条草稿。")
        return {"recognized_screenshot_survives_route_failure": True, "ocr_calls": 1,
                "route_retry_calls": 2, "next_draft_kept": True}

    def uploaded_image_routes_after_ocr():
        fresh()
        page.get_by_label("选择待翻译图片", exact=True).set_input_files({"name": "german-customer.png", "mimeType": "image/png", "buffer": image_bytes})
        page.get_by_role("button", name="翻译这一段", exact=True).click()
        page.wait_for_function("window.__moyuFixture.pending.extract")
        assert len(command_calls("resolve_dialogue")) == 0
        page.evaluate("window.__moyuFixture.resolveImage(false, 'Hallo, bitte senden Sie die Rechnung.')")
        page.wait_for_function("window.__moyuFixture.pending.translate")
        request = page.evaluate("window.__moyuFixture.lastRequest")
        assert request["sourceLanguage"] == "de" and request["targetLanguage"] == "zh-CN", request
        assert request["sourceImage"]["name"] == "german-customer.png"
        complete(1)
        return {"ocr_precedes_language_route": True, "image_uses_detected_german_to_chinese": True}

    def screenshot_routes_without_mixing_original_draft():
        fresh()
        page.locator("#source-text").fill("我正在编辑的下一条中文消息。")
        page.evaluate("image => window.__moyuFixture.emitCaptureImage(image.dataUrl, image.name)", {"dataUrl": data_url, "name": "french-screenshot.png"})
        page.wait_for_function("window.__moyuFixture.pending.extract")
        page.evaluate("window.__moyuFixture.resolveImage(false, 'Bonjour, pouvez-vous confirmer le prix?')")
        page.wait_for_function("window.__moyuFixture.pending.translate")
        request = page.evaluate("window.__moyuFixture.lastRequest")
        assert request["sourceLanguage"] == "fr" and request["targetLanguage"] == "zh-CN", request
        assert "我正在" not in request["text"]
        complete(1)
        expect(page.locator("#source-text")).to_have_value("我正在编辑的下一条中文消息。")
        return {"screenshot_ocr_uses_dialogue_route": True, "unsent_draft_not_mixed_or_erased": True}

    def mode_switches_preserve_distinct_drafts_and_conversations():
        fresh()
        dialogue_session = page.evaluate("window.__moyuFixture.state.sessionId")
        submit("Hello, this is a separate customer conversation.", "en", "zh-CN")
        complete(1)
        page.locator("#source-text").fill("双向对话独立草稿。")
        page.get_by_label("选择待翻译图片", exact=True).set_input_files({"name": "dialogue-draft.png", "mimeType": "image/png", "buffer": image_bytes})
        expect(page.locator(".composer-attachment")).to_contain_text("dialogue-draft.png")
        page.get_by_role("button", name="连续翻译", exact=True).click()
        expect(page.get_by_role("heading", name="连续翻译", exact=True)).to_be_visible()
        standard_session = page.evaluate("window.__moyuFixture.state.sessionId")
        assert standard_session != dialogue_session
        expect(page.locator("article.conversation-turn:not(.pending-turn):not(.failed-turn)")).to_have_count(0)
        expect(page.locator(".composer-attachment")).to_have_count(0)
        page.locator("#source-text").fill("Standard mode keeps its own draft.")
        page.get_by_label("选择待翻译图片", exact=True).set_input_files({"name": "standard-draft.png", "mimeType": "image/png", "buffer": image_bytes})
        expect(page.locator(".composer-attachment")).to_contain_text("standard-draft.png")
        page.get_by_role("button", name="双向对话", exact=True).click()
        expect(page.get_by_role("heading", name="双向对话", exact=True)).to_be_visible()
        expect(page.locator("#source-text")).to_have_value("双向对话独立草稿。")
        expect(page.locator(".composer-attachment")).to_contain_text("dialogue-draft.png")
        expect(page.locator("article.conversation-turn:not(.pending-turn):not(.failed-turn)")).to_have_count(1)
        assert page.evaluate("window.__moyuFixture.state.sessionId") == dialogue_session
        page.get_by_role("button", name="连续翻译", exact=True).click()
        expect(page.get_by_role("heading", name="连续翻译", exact=True)).to_be_visible()
        expect(page.locator("#source-text")).to_have_value("Standard mode keeps its own draft.")
        expect(page.locator(".composer-attachment")).to_contain_text("standard-draft.png")
        return {"workspace_session_ids_isolated": True, "drafts_recover_independently": True, "attachments_recover_independently": True,
                "translation_history_not_crossed": True}

    def history_reopens_correct_dialogue_mode_and_peer():
        fresh()
        page.get_by_label("客户语言", exact=True).select_option("de")
        page.wait_for_function("window.__moyuFixture.state.dialoguePreferences.peerLanguage === 'de'")
        submit("历史客户对话应该按原模式恢复。", "zh-CN", "de")
        complete(1)
        session = page.evaluate("window.__moyuFixture.state.sessionId")
        page.get_by_role("button", name="连续翻译", exact=True).click()
        expect(page.get_by_role("heading", name="连续翻译", exact=True)).to_be_visible()
        page.get_by_role("button", name="历史记录", exact=True).click()
        row = page.locator(".history-conversation").filter(has_text="历史客户对话应该按原模式恢复")
        expect(row).to_have_count(1)
        row.get_by_role("button", name="打开会话", exact=True).click()
        expect(page.get_by_role("heading", name="双向对话", exact=True)).to_be_visible()
        expect(page.get_by_label("客户语言", exact=True)).to_have_value("de")
        assert page.evaluate("window.__moyuFixture.state.sessionId") == session
        expect(page.locator("article.conversation-turn:not(.pending-turn):not(.failed-turn)")).to_have_count(1)
        return {"saved_mode_and_peer_restored_from_history": True, "same_session_reopened": True}

    def rendered_lists_code_and_html_remain_safe_and_readable():
        fresh()
        text = "Hello.\n\n- Keep item one\n- Keep item two\n\n```html\n<img src=https://example.invalid/tracker.png onerror=alert(1)>\n```\n<script>alert(1)</script>"
        dialogs = []
        page.on("dialog", lambda dialog: (dialogs.append(dialog.message), dialog.dismiss()))
        submit(text, "en", "zh-CN")
        complete(1)
        translated = page.locator(".translated-prose").last
        expect(translated).to_contain_text("<img src=https://example.invalid/tracker.png onerror=alert(1)>")
        assert translated.locator("img,script,iframe,a[href]").count() == 0
        assert translated.locator("pre").count() >= 1
        assert translated.locator("li").count() >= 2
        assert not dialogs, dialogs
        for viewport in ({"width": 900, "height": 640}, {"width": 1280, "height": 850}):
            page.set_viewport_size(viewport)
            assert not page.evaluate("document.documentElement.scrollWidth > innerWidth")
            dock = page.locator(".composer-dock").bounding_box()
            assert dock["y"] + dock["height"] <= viewport["height"] + 1
        page.screenshot(path=str(root / "artifacts/dialogue-safe-readable-text.png"))
        return {"safe_structured_lists_and_code": True, "html_not_executed_or_fetched": True,
                "compact_windows_remain_within_viewport": True}

    return [
        ("dialogue_chinese_and_english_reverse_automatically_without_switching", bilingual_replies_auto_reverse_direction),
        ("dialogue_auto_peer_follows_german_customer_for_chinese_reply", auto_peer_follows_german_customer_then_chinese_reply),
        ("dialogue_fixed_peer_survives_third_language_incoming_message", fixed_peer_is_not_overwritten_by_third_language),
        ("dialogue_ambiguous_short_text_discloses_fallback_and_manual_override", ambiguous_short_text_exposes_fallback_and_manual_direction),
        ("dialogue_failed_translation_retry_reuses_resolved_direction", failed_translation_retry_does_not_repeat_language_route),
        ("dialogue_route_failure_preserves_source_and_only_translates_after_retry", language_route_failure_keeps_source_for_retry),
        ("dialogue_screenshot_route_retry_reuses_ocr_and_preserves_next_draft", screenshot_route_failure_reuses_recognized_image_on_retry),
        ("dialogue_uploaded_image_routes_after_recognized_text", uploaded_image_routes_after_ocr),
        ("dialogue_screenshot_autotranslates_without_mixing_next_draft", screenshot_routes_without_mixing_original_draft),
        ("dialogue_and_standard_modes_keep_separate_sessions_and_drafts", mode_switches_preserve_distinct_drafts_and_conversations),
        ("dialogue_history_restores_mode_and_fixed_peer", history_reopens_correct_dialogue_mode_and_peer),
        ("dialogue_lists_code_and_html_are_readable_without_executable_markup", rendered_lists_code_and_html_remain_safe_and_readable),
    ]
