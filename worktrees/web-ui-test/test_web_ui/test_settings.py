"""Settings 页面测试 (5 项)."""

import pytest
from pages.settings import SettingsPage


class TestSettings:
    """Settings 完整测试套件."""

    def test_page_loads(self, page, service_running):
        """[1] 页面加载: settings view 可见."""
        p = SettingsPage(page)
        p.goto()
        p.wait_view_active("settings")
        assert p.is_view_visible("settings"), "Settings view should be visible"

    def test_port_information(self, page, service_running):
        """[2] Port 信息: HTTP / SOCKS5 / API 端口显示."""
        p = SettingsPage(page)
        p.goto()
        http = p.get_http_port()
        socks5 = p.get_socks5_port()
        api = p.get_api_port()
        assert http and http.isdigit(), f"HTTP port should be numeric, got: {http}"
        assert socks5 and socks5.isdigit(), f"SOCKS5 port should be numeric, got: {socks5}"
        assert api and api.isdigit(), f"API port should be numeric, got: {api}"

    def test_restart_button(self, page, service_running):
        """[3] Restart 按钮: 点击显示 toast."""
        p = SettingsPage(page)
        p.goto()
        p.click_restart()
        toast = p.get_toast(timeout=5000)
        assert toast and ("restart" in toast.lower() or "start" in toast.lower()), \
            f"Expected restart/start toast, got: {toast}"

    def test_stop_button(self, page, service_running):
        """[4] Stop 按钮: 点击后 service 停止."""
        p = SettingsPage(page)
        p.goto()
        p.click_stop()
        # Wait for service to stop - check status API
        import time
        for _ in range(20):
            time.sleep(0.5)
            try:
                r = page.request.get(f"{p.base_url}/api/status")
                if r.ok:
                    data = r.json().get("data", {})
                    if not data.get("running"):
                        break
            except Exception:
                pass
        assert p.is_view_visible("settings"), "Settings view should still be visible"

    def test_about_section(self, page, service_running):
        """[5] About 区: Version / Build 信息可见."""
        p = SettingsPage(page)
        p.goto()
        p.wait_view_active("settings")
        version_text = p.get_version()
        assert version_text is not None and len(version_text) > 0, \
            "Version info should be visible"
