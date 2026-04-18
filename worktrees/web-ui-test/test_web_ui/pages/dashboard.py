"""Dashboard page object."""

from .base import BasePage


class DashboardPage(BasePage):
    """Dashboard 页面."""

    def __init__(self, page):
        super().__init__(page)
        self.status_card = "#dash-status"
        self.mode_card = "#dash-mode"
        self.proxy_card = "#dash-proxy"
        self.btn_start = "#btn-start"
        self.btn_stop = "#btn-stop"

    def goto(self):
        self.page.goto(f"{self.base_url}/")
        self.wait_view_active("dashboard")

    def get_status_text(self) -> str:
        return self.page.text_content(self.status_card)

    def get_mode_text(self) -> str:
        return self.page.text_content(self.mode_card)

    def get_proxy_text(self) -> str:
        return self.page.text_content(self.proxy_card)

    def click_start(self):
        self.page.click(self.btn_start)

    def click_stop(self):
        self.page.click(self.btn_stop)

    def click_mode_rule(self):
        self.page.click('button:text("Rule")')

    def click_mode_global(self):
        self.page.click('button:text("Global")')

    def click_mode_direct(self):
        self.page.click('button:text("Direct")')

    def is_start_disabled(self) -> bool:
        return self.page.is_disabled(self.btn_start)

    def is_stop_disabled(self) -> bool:
        return self.page.is_disabled(self.btn_stop)

    def get_online_nodes(self) -> str:
        return self.page.text_content("#dash-nodes")

    def get_connections_count(self) -> str:
        return self.page.text_content("#dash-connections")

    def wait_for_running(self, timeout=10000):
        """等待服务启动完成 (btn-start 变为 disabled)."""
        self.page.wait_for_selector(f"{self.btn_start}[disabled]", timeout=timeout)

    def wait_for_stopped(self, timeout=10000):
        """等待服务停止完成 (btn-stop 变为 disabled)."""
        self.page.wait_for_selector(f"{self.btn_stop}[disabled]", timeout=timeout)
