"""Connections page object."""

from .base import BasePage


class ConnectionsPage(BasePage):
    """Connections 页面."""

    TABLE = "#conn-table"
    LIST = "#conn-list"
    LOADING = "#conn-loading"
    EMPTY = "#conn-empty"
    TOTAL = "#conn-total"
    UPLOAD = "#conn-upload"
    DOWNLOAD = "#conn-download"
    COUNTDOWN = "#conn-countdown"

    def goto(self):
        self.page.goto(f"{self.base_url}/")
        self.click_nav("connections")

    def wait_loaded(self, timeout=10000):
        """等待连接列表加载完成."""
        self.page.wait_for_selector(f"{self.TABLE}", state="visible", timeout=timeout)

    def get_total(self) -> int:
        text = self.page.text_content(self.TOTAL)
        try:
            return int(text) if text else 0
        except ValueError:
            return 0

    def get_rows(self):
        """返回连接行列表."""
        rows = self.page.query_selector_all(f"{self.LIST} tr")
        return rows

    def is_loading(self) -> bool:
        return self.page.is_visible(self.LOADING)

    def is_empty(self) -> bool:
        return self.page.is_visible(self.EMPTY)

    def click_close_first(self):
        """点击第一行的 Close 按钮."""
        first_close = self.page.query_selector(f"{self.LIST} tr button")
        if first_close:
            first_close.click()
