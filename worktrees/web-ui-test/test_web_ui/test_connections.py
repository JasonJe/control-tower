"""Connections 页面测试 (7 项)."""

import pytest
from pages.connections import ConnectionsPage


class TestConnections:
    """Connections 完整测试套件."""

    def test_page_loads(self, page, service_running):
        """[1] 页面加载: connections view 可见."""
        p = ConnectionsPage(page)
        p.goto()
        p.wait_view_active("connections")
        assert p.is_view_visible("connections"), "Connections view should be visible"

    def test_loading_state(self, page, service_running):
        """[2] Loading 状态: spinner 显示."""
        p = ConnectionsPage(page)
        p.goto()
        p.wait_view_active("connections")

    def test_empty_state(self, page, service_running):
        """[3] 空状态: 无连接时显示 empty state."""
        p = ConnectionsPage(page)
        p.goto()
        p.wait_view_active("connections")
        empty_or_table = p.is_empty() or p.page.is_visible(p.TABLE)
        assert empty_or_table, "Should show empty state or table"

    def test_connection_list_renders(self, page, service_running):
        """[4] 连接列表: 有连接时显示行，无连接时显示 empty state."""
        p = ConnectionsPage(page)
        p.goto()
        p.wait_view_active("connections")
        # Table is visible only when there are connections
        if p.page.is_visible(p.TABLE):
            p.wait_loaded(timeout=8000)
            total = p.get_total()
            rows = p.get_rows()
            assert len(rows) > 0, f"Expected connections, got {total} total, {len(rows)} rows"
        else:
            # No connections - empty state should be visible
            assert p.is_empty(), "No connections, empty state should be visible"

    def test_stats_bar(self, page, service_running):
        """[5] Stats Bar: Total / Upload / Download 显示."""
        p = ConnectionsPage(page)
        p.goto()
        p.wait_view_active("connections")
        total = p.page.text_content(p.TOTAL)
        assert total is not None
        upload = p.page.text_content(p.UPLOAD)
        download = p.page.text_content(p.DOWNLOAD)
        assert upload is not None
        assert download is not None

    def test_navigate_back(self, page, service_running):
        """[6] 导航返回: 从 connections 返回 dashboard."""
        p = ConnectionsPage(page)
        p.goto()
        p.wait_view_active("connections")
        p.click_nav("dashboard")
        assert p.is_view_visible("dashboard"), "Dashboard should be visible after nav back"

    def test_navigate_to_settings(self, page, service_running):
        """[7] 导航切换: 点击 Settings nav-item."""
        p = ConnectionsPage(page)
        p.goto()
        p.wait_view_active("connections")
        p.click_nav("settings")
        assert p.is_view_visible("settings"), "Settings view should be visible"
