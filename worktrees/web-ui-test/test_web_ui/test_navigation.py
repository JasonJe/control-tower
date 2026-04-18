"""导航与多页面集成测试 (6 项)."""

import pytest
from playwright.sync_api import expect as pw_expect

from pages.dashboard import DashboardPage
from pages.proxies import ProxiesPage
from pages.profiles import ProfilesPage
from pages.rules import RulesPage
from pages.connections import ConnectionsPage
from pages.settings import SettingsPage


class TestNavigation:
    """全页面导航测试套件."""

    def test_all_nav_items_present(self, page):
        """[1] 所有 Nav 项: dashboard/proxies/profiles/rules/connections/settings 均可见."""
        p = DashboardPage(page)
        p.goto()
        for view in ["dashboard", "proxies", "profiles", "rules", "connections", "settings"]:
            selector = f'[data-view="{view}"]'
            assert page.is_visible(selector), f"Nav item '{view}' should be visible"

    def test_nav_to_proxies(self, page):
        """[2] 导航到 Proxies."""
        p = DashboardPage(page)
        p.goto()
        p.click_nav("proxies")
        assert p.is_view_visible("proxies")

    def test_nav_to_profiles(self, page):
        """[3] 导航到 Profiles."""
        p = DashboardPage(page)
        p.goto()
        p.click_nav("profiles")
        assert p.is_view_visible("profiles")

    def test_nav_to_rules(self, page):
        """[4] 导航到 Rules."""
        p = DashboardPage(page)
        p.goto()
        p.click_nav("rules")
        assert p.is_view_visible("rules")

    def test_nav_to_connections(self, page):
        """[5] 导航到 Connections."""
        p = DashboardPage(page)
        p.goto()
        p.click_nav("connections")
        assert p.is_view_visible("connections")

    def test_nav_to_settings(self, page):
        """[6] 导航到 Settings."""
        p = DashboardPage(page)
        p.goto()
        p.click_nav("settings")
        assert p.is_view_visible("settings")
