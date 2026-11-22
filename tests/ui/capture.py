"""Capture the real conversation UI with clearly marked, local test content."""
from pathlib import Path
from playwright.sync_api import sync_playwright, expect

root = Path(__file__).resolve().parents[2]
with sync_playwright() as playwright:
    browser = playwright.chromium.launch(headless=True)
    page = browser.new_page(viewport={"width": 1440, "height": 1050}, locale="zh-CN", reduced_motion="reduce")
    page.goto("http://127.0.0.1:1420/tests/ui/conversation.html", wait_until="domcontentloaded")
    page.wait_for_function("() => !!window.__moyuFixture")
    row = page.locator('article[aria-label="第 79 段翻译"]')
    row.locator('.translated-prose').scroll_into_view_if_needed()
    page.evaluate("() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))")
    row.evaluate("""element => {
        const region = document.querySelector('.conversation-scroll');
        region.scrollTop += element.getBoundingClientRect().top - region.getBoundingClientRect().top - 18;
    }""")
    page.wait_for_timeout(250)
    expect(row.locator('.translated-prose')).to_be_visible()
    page.screenshot(path=str(root / "artifacts" / "image-history-conversation-thread.png"))
    print("Saved conversation thread screenshot")
    browser.close()
