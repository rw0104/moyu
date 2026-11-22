"""Screenshot UI contracts using synthetic events and official Tauri mocks.

No native screen, private image, clipboard contents, or model API is accessed.
Windows registration, overlay geometry, and cancellation are tested separately.
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
        page.wait_for_function("window.__moyuFixture.calls.some(c => c.command === 'get_capture_preferences')")
        expect(page.get_by_role("button", name="截屏并翻译", exact=True)).to_be_enabled()

    def settings():
        page.get_by_role("button", name="设置", exact=True).click()
        expect(page.get_by_label("截屏翻译快捷键", exact=True)).to_be_enabled()

    def record_shortcut(keys):
        field = page.get_by_label("截屏翻译快捷键", exact=True)
        field.focus()
        page.wait_for_function("window.__moyuFixture.captureRecording")
        page.keyboard.press(keys)
        return field

    def emit(name):
        page.evaluate("image => window.__moyuFixture.emitCaptureImage(image.dataUrl, image.name)",
                      {"dataUrl": data_url, "name": name})

    def resolve_image(text):
        page.evaluate("text => window.__moyuFixture.resolveImage(false, text)", text)
        page.wait_for_function("window.__moyuFixture.pending.translate")

    def complete_translation(count):
        page.evaluate("window.__moyuFixture.resolveTranslate()")
        expect(page.locator("article.conversation-turn:not(.pending-turn):not(.failed-turn)")).to_have_count(count)

    def preserved_attachment():
        page.get_by_label("选择待翻译图片", exact=True).set_input_files(
            {"name": "next-draft-attachment.png", "mimeType": "image/png", "buffer": image_bytes})
        expect(page.locator(".composer-attachment")).to_contain_text("next-draft-attachment.png")

    def calls(command):
        return page.evaluate("command => window.__moyuFixture.calls.filter(c => c.command === command)", command)

    def custom_shortcut_saves_and_reappears():
        fresh()
        settings()
        field = page.get_by_label("截屏翻译快捷键", exact=True)
        expect(field).to_have_value("Ctrl+Alt+T")
        field = record_shortcut("Control+Alt+K")
        expect(field).to_have_value("Ctrl+Alt+K")
        page.get_by_role("button", name="保存截屏快捷键", exact=True).click()
        page.wait_for_function("window.__moyuFixture.capturePreferences.shortcut === 'Ctrl+Alt+K'")
        page.wait_for_function("!window.__moyuFixture.captureRecording")
        page.get_by_role("button", name="连续翻译", exact=True).click()
        expect(page.get_by_role("button", name="截屏并翻译", exact=True)).to_have_attribute("title", __import__("re").compile("Ctrl.*Alt.*K"))
        settings()
        expect(page.get_by_label("截屏翻译快捷键", exact=True)).to_have_value("Ctrl+Alt+K")
        page.screenshot(path=str(root / "artifacts/screenshot-custom-shortcut.png"))
        return {"custom_shortcut": "Ctrl+Alt+K", "settings_reopen_retains_value": True,
                "recording_temporarily_suspends_registration": True, "native_registration_tested": False}

    def shortcut_conflict_disable_and_restore():
        fresh()
        settings()
        record_shortcut("Control+Alt+Y")
        page.evaluate("window.__moyuFixture.failNextShortcutSave()")
        page.get_by_role("button", name="保存截屏快捷键", exact=True).click()
        expect(page.get_by_text("此快捷键已被其他应用占用，请更换组合。", exact=True)).to_be_visible()
        assert page.evaluate("window.__moyuFixture.capturePreferences.shortcut") == "Ctrl+Alt+T"
        page.get_by_role("button", name="关闭截屏快捷键", exact=True).click()
        page.wait_for_function("window.__moyuFixture.capturePreferences.shortcut === '' && !window.__moyuFixture.capturePreferences.active")
        page.get_by_role("button", name="恢复默认截屏快捷键", exact=True).click()
        page.wait_for_function("window.__moyuFixture.capturePreferences.shortcut === 'Ctrl+Alt+T' && window.__moyuFixture.capturePreferences.active")
        expect(page.get_by_label("截屏翻译快捷键", exact=True)).to_have_value("Ctrl+Alt+T")
        return {"conflict_is_visible": True, "conflict_does_not_replace_saved_shortcut": True,
                "disable_then_restore_default": True}

    def recording_escape_and_navigation_restore_registration():
        fresh()
        settings()
        field = record_shortcut("Escape")
        page.wait_for_function("!window.__moyuFixture.captureRecording")
        expect(field).to_have_value("Ctrl+Alt+T")
        field.focus()
        page.wait_for_function("window.__moyuFixture.captureRecording")
        page.evaluate("window.dispatchEvent(new Event('blur'))")
        page.wait_for_function("!window.__moyuFixture.captureRecording")
        expect(field).not_to_be_focused()
        field.focus()
        page.wait_for_function("window.__moyuFixture.captureRecording")
        page.get_by_role("button", name="历史记录", exact=True).click()
        page.wait_for_function("!window.__moyuFixture.captureRecording")
        assert len(calls("set_capture_shortcut")) == 0
        assert len(calls("start_capture")) == 0
        return {"escape_restores_registration": True, "window_blur_restores_registration_and_blurs_field": True,
                "field_can_record_again_after_window_blur": True, "unmount_restores_registration": True,
                "recording_does_not_change_saved_shortcut": True}

    def capture_button_only_opens_native_selection():
        fresh()
        page.get_by_role("button", name="截屏并翻译", exact=True).click()
        page.wait_for_function("window.__moyuFixture.captureStarts === 1")
        assert not page.evaluate("window.__moyuFixture.pending.extract || window.__moyuFixture.pending.translate")
        assert len(calls("extract_image")) == 0
        assert len(calls("translate")) == 0
        return {"native_start_command_once": True, "no_ready_image_no_model_request": True,
                "native_overlay_tested": False}

    def invalid_shortcut_is_rejected_before_save():
        fresh()
        settings()
        field = record_shortcut("Shift+Z")
        expect(page.locator(".capture-settings [role=alert]")).to_contain_text("请使用 Ctrl 或 Alt")
        expect(field).to_have_value("Ctrl+Alt+T")
        field.evaluate("""element => element.dispatchEvent(new KeyboardEvent('keydown', {
          key:'F12', code:'F12', ctrlKey:true, bubbles:true, cancelable:true
        }))""")
        expect(field).to_have_value("Ctrl+Alt+T")
        expect(page.get_by_role("button", name="保存截屏快捷键", exact=True)).to_be_disabled()
        page.keyboard.press("Control+Shift+F2")
        expect(field).to_have_value("Ctrl+Shift+F2")
        page.keyboard.press("Escape")
        page.wait_for_function("!window.__moyuFixture.captureRecording")
        assert len(calls("set_capture_shortcut")) == 0
        return {"shift_only_and_reserved_f12_rejected": True, "valid_function_key_recordable": True,
                "invalid_input_never_saved": True}

    def capture_translates_independently_and_preserves_draft():
        fresh()
        draft = "Next draft must stay separate from captured text."
        page.locator("#source-text").fill(draft)
        preserved_attachment()
        emit("synthetic-area-a.png")
        page.wait_for_function("window.__moyuFixture.pending.extract")
        assert page.evaluate("window.__moyuFixture.lastImageRequest.name") == "synthetic-area-a.png"
        text = "Only the captured source should be translated."
        resolve_image(text)
        request = page.evaluate("window.__moyuFixture.lastRequest")
        assert request["text"] == text, request
        assert request["sourceImage"]["name"] == "synthetic-area-a.png", request
        complete_translation(1)
        expect(page.locator("#source-text")).to_have_value(draft)
        expect(page.locator(".composer-attachment")).to_contain_text("next-draft-attachment.png")
        expect(page.locator(".turn-source-image")).to_contain_text("synthetic-area-a.png")
        page.screenshot(path=str(root / "artifacts/screenshot-translation-preserved-draft.png"))
        return {"ready_event_auto_ocr_then_translate": True, "draft_and_existing_image_preserved": True,
                "captured_text_not_combined_with_draft": True}

    def busy_translation_queues_two_captures_sequentially():
        fresh()
        page.locator("#source-text").fill("The existing translation finishes first.")
        page.get_by_role("button", name="翻译这一段", exact=True).click()
        page.wait_for_function("window.__moyuFixture.pending.translate")
        page.locator("#source-text").fill("Draft while capture jobs wait.")
        emit("queued-area-one.png")
        emit("queued-area-two.png")
        assert len(calls("extract_image")) == 0
        expect(page.get_by_label("截图翻译队列", exact=True)).to_be_visible()
        complete_translation(1)
        page.wait_for_function("window.__moyuFixture.pending.extract")
        assert page.evaluate("window.__moyuFixture.lastImageRequest.name") == "queued-area-one.png"
        resolve_image("First captured paragraph.")
        assert len(calls("extract_image")) == 1
        complete_translation(2)
        page.wait_for_function("window.__moyuFixture.pending.extract")
        assert page.evaluate("window.__moyuFixture.lastImageRequest.name") == "queued-area-two.png"
        resolve_image("Second captured paragraph.")
        complete_translation(3)
        expect(page.locator("#source-text")).to_have_value("Draft while capture jobs wait.")
        assert page.evaluate("window.__moyuFixture.maximumConcurrentRequests") == 1
        assert len(calls("extract_image")) == 2 and len(calls("translate")) == 3
        return {"two_captures_wait_for_existing_translation": True, "capture_order_preserved": True,
                "maximum_concurrent_requests": 1, "next_draft_preserved": True}

    def capture_ocr_failure_retains_job_and_retries():
        fresh()
        page.locator("#source-text").fill("Unsent draft during failed screenshot OCR.")
        emit("retry-capture-ocr.png")
        page.wait_for_function("window.__moyuFixture.pending.extract")
        page.evaluate("window.__moyuFixture.resolveImage(true)")
        retry = page.get_by_role("button", name="重试截图翻译 retry-capture-ocr.png", exact=True)
        expect(retry).to_be_visible()
        assert len(calls("translate")) == 0
        expect(page.locator("#source-text")).to_have_value("Unsent draft during failed screenshot OCR.")
        retry.click()
        page.wait_for_function("window.__moyuFixture.pending.extract")
        resolve_image("The captured source is recovered after retry.")
        complete_translation(1)
        assert len(calls("extract_image")) == 2
        expect(page.locator("#source-text")).to_have_value("Unsent draft during failed screenshot OCR.")
        return {"failed_ocr_keeps_capture_for_retry": True, "draft_preserved": True,
                "no_translation_before_successful_ocr": True}

    def capture_translation_retry_reuses_recognized_text():
        fresh()
        page.locator("#source-text").fill("Keep my original draft during a translation retry.")
        preserved_attachment()
        emit("retry-capture-translation.png")
        page.wait_for_function("window.__moyuFixture.pending.extract")
        resolve_image("Recognized screenshot text is reused on retry.")
        original = page.evaluate("window.__moyuFixture.lastRequest")
        page.evaluate("window.__moyuFixture.resolveTranslate(true)")
        retry = page.get_by_role("button", name="重试这一段", exact=True)
        expect(retry).to_be_visible()
        retry.click()
        page.wait_for_function("window.__moyuFixture.pending.translate")
        assert page.evaluate("window.__moyuFixture.lastRequest") == original
        complete_translation(1)
        assert len(calls("extract_image")) == 1
        expect(page.locator("#source-text")).to_have_value("Keep my original draft during a translation retry.")
        expect(page.locator(".composer-attachment")).to_contain_text("next-draft-attachment.png")
        return {"translation_retry_reuses_recognized_text_and_thumbnail": True,
                "ocr_requests": 1, "draft_and_existing_attachment_preserved": True}

    def settings_page_capture_returns_to_translation_and_error_is_visible():
        fresh()
        settings()
        page.evaluate("window.__moyuFixture.emitCaptureError()")
        expect(page.get_by_text("测试截屏错误：选区不可用，请重试。", exact=True)).to_be_visible()
        assert not page.evaluate("window.__moyuFixture.pending.extract || window.__moyuFixture.pending.translate")
        emit("capture-from-settings.png")
        expect(page.get_by_role("heading", name="连续翻译", exact=True)).to_be_visible()
        page.wait_for_function("window.__moyuFixture.pending.extract")
        resolve_image("A screenshot can arrive while settings are open.")
        complete_translation(1)
        return {"capture_listener_stays_active_on_settings_page": True,
                "ready_image_returns_to_workspace": True, "capture_error_visible_without_model_call": True}

    return [
        ("custom_screenshot_shortcut_records_saves_and_reappears", custom_shortcut_saves_and_reappears),
        ("shortcut_conflict_is_visible_and_disable_restore_work", shortcut_conflict_disable_and_restore),
        ("shortcut_recording_escape_and_unmount_restore_registration", recording_escape_and_navigation_restore_registration),
        ("invalid_or_reserved_shortcut_is_rejected_before_save", invalid_shortcut_is_rejected_before_save),
        ("capture_button_only_starts_native_selection_until_ready", capture_button_only_opens_native_selection),
        ("capture_ready_autotranslates_without_mixing_or_losing_existing_draft", capture_translates_independently_and_preserves_draft),
        ("two_screenshots_queue_behind_busy_translation_without_concurrency", busy_translation_queues_two_captures_sequentially),
        ("capture_ocr_failure_keeps_image_and_allows_retry", capture_ocr_failure_retains_job_and_retries),
        ("capture_translation_retry_preserves_draft_and_does_not_repeat_ocr", capture_translation_retry_reuses_recognized_text),
        ("capture_listener_survives_settings_and_errors_are_visible", settings_page_capture_returns_to_translation_and_error_is_visible),
    ]
