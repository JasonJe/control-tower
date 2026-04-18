"""
全页面入口测试: 运行所有测试套件
用法: pytest test_all_pages.py -v
"""

import pytest


class TestAllPages:
    """Smoke test: 确认所有页面可加载."""

    def test_dashboard_smoke(self, page):
        from pages.dashboard import DashboardPage
        p = DashboardPage(page)
        p.goto()
        assert p.get_status_text() != ""

    def test_proxies_smoke(self, page):
        from pages.proxies import ProxiesPage
        p = ProxiesPage(page)
        p.goto()
        p.wait_view_active("proxies")
        assert p.is_view_visible("proxies")

    def test_profiles_smoke(self, page):
        from pages.profiles import ProfilesPage
        p = ProfilesPage(page)
        p.goto()
        p.wait_view_active("profiles")
        assert p.is_view_visible("profiles")

    def test_rules_smoke(self, page):
        from pages.rules import RulesPage
        p = RulesPage(page)
        p.goto()
        p.wait_view_active("rules")
        assert p.is_view_visible("rules")

    def test_connections_smoke(self, page):
        from pages.connections import ConnectionsPage
        p = ConnectionsPage(page)
        p.goto()
        p.wait_view_active("connections")
        assert p.is_view_visible("connections")

    def test_settings_smoke(self, page):
        from pages.settings import SettingsPage
        p = SettingsPage(page)
        p.goto()
        p.wait_view_active("settings")
        assert p.is_view_visible("settings")
