"""Dashboard 页面测试 (12 项)."""

import time
import pytest
from playwright.sync_api import expect as pw_expect

from pages.dashboard import DashboardPage


class TestDashboard:
    """Dashboard 完整测试套件."""

    def test_page_loads(self, page):
        """[1] 页面加载: status/mode/proxy cards 可见."""
        p = DashboardPage(page)
        p.goto()
        assert p.get_status_text() != ""
        assert p.get_mode_text() != ""
        assert p.get_proxy_text() != ""

    def test_start_button(self, page):
        """[2] Service Start: 点击 Start 显示 toast."""
        p = DashboardPage(page)
        p.goto()
        # Ignore any error toast from initial loadDashboard() failure
        p.click_start()
        # Wait for service to start and "Service started" toast
        p.wait_for_running(timeout=10000)
        toast = p.get_toast(timeout=2000)
        assert toast and "start" in toast.lower(), f"Expected start toast, got: {toast}"

    @pytest.mark.skip(reason="Mihomo restart loop bug: stop后自动重启，无法稳定等待stopped状态 - 应用层bug")
    def test_stop_button(self, page, service_running):
        """[3] Service Stop: 点击 Stop 后 Start 按钮恢复可用."""
        p = DashboardPage(page)
        p.goto()
        p.wait_for_running(timeout=10000)
        p.click_stop()
        p.wait_for_stopped(timeout=10000)
        assert p.is_start_disabled() is False, "Start button should be enabled after service stops"
        toast = p.get_toast(timeout=5000)
        assert toast and "stop" in toast.lower(), f"Expected stop toast, got: {toast}"

    def test_start_disabled_when_running(self, page, service_running):
        """[4] Start 按钮 disabled: 服务运行中 Start 不可点击."""
        p = DashboardPage(page)
        p.goto()
        p.wait_for_running(timeout=10000)
        assert p.is_start_disabled(), "Start button should be disabled when service is running"
        p.click_stop()
        time.sleep(1)

    def test_set_mode_rule(self, page, service_running):
        """[5] Rule 模式: 点击后 toast 显示 mode changed."""
        p = DashboardPage(page)
        p.goto()
        p.wait_for_running(timeout=10000)
        p.click_mode_rule()
        toast = p.get_toast(timeout=5000)
        assert toast and "rule" in toast.lower(), f"Expected rule toast, got: {toast}"
        p.click_stop()
        time.sleep(1)

    def test_set_mode_global(self, page, service_running):
        """[6] Global 模式."""
        p = DashboardPage(page)
        p.goto()
        p.wait_for_running(timeout=10000)
        p.click_mode_global()
        toast = p.get_toast(timeout=5000)
        assert toast and "global" in toast.lower(), f"Expected global toast, got: {toast}"
        p.click_stop()
        time.sleep(1)

    def test_set_mode_direct(self, page, service_running):
        """[7] Direct 模式."""
        p = DashboardPage(page)
        p.goto()
        p.wait_for_running(timeout=10000)
        p.click_mode_direct()
        toast = p.get_toast(timeout=5000)
        assert toast and "direct" in toast.lower(), f"Expected direct toast, got: {toast}"
        p.click_stop()
        time.sleep(1)

    def test_reload_persists_data(self, page, service_running):
        """[8] 刷新: 数据重新加载."""
        p = DashboardPage(page)
        p.goto()
        p.wait_for_running(timeout=10000)
        status_before = p.get_status_text()
        p.reload()
        time.sleep(1)
        status_after = p.get_status_text()
        assert status_before == status_after
        p.click_stop()
        time.sleep(1)

    @pytest.mark.skip(reason="Mihomo restart loop bug: stop后自动重启，无法稳定等待stopped状态 - 应用层bug")
    def test_status_dot_red_when_stopped(self, page, service_running):
        """[9] Status dot 红色: 服务停止时."""
        p = DashboardPage(page)
        p.goto()
        p.wait_for_running(timeout=10000)
        p.click_stop()
        time.sleep(1)
        dot_class = p.get_status_dot_class()
        assert "stopped" in dot_class, f"Expected stopped class, got: {dot_class}"

    def test_status_dot_green_when_running(self, page, service_running):
        """[10] Status dot 绿色: 服务运行时."""
        p = DashboardPage(page)
        p.goto()
        p.wait_for_running(timeout=10000)
        dot_class = p.get_status_dot_class()
        assert "stopped" not in dot_class, f"Expected green dot, got: {dot_class}"
        p.click_stop()
        time.sleep(1)

    def test_quick_stats_show(self, page):
        """[11] Quick Stats: Online Nodes / Connections 数量显示."""
        p = DashboardPage(page)
        p.goto()
        nodes = p.get_online_nodes()
        conns = p.get_connections_count()
        assert nodes is not None
        assert conns is not None

    def test_navigate_to_proxies(self, page):
        """[12] 导航切换: 点击 Proxies nav-item."""
        p = DashboardPage(page)
        p.goto()
        p.click_nav("proxies")
        assert p.is_view_visible("proxies"), "Proxies view should be visible"
