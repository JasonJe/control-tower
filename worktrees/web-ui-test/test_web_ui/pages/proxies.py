"""Proxies page object."""

from .base import BasePage


class ProxiesPage(BasePage):
    """Proxies 页面."""

    SEARCH_INPUT = "#proxy-search"
    TABLE = "#proxy-table"
    LOADING = "#proxy-loading"
    EMPTY = "#proxy-empty"
    LIST = "#proxy-list"

    def goto(self):
        self.page.goto(f"{self.base_url}/")
        self.click_nav("proxies")

    def wait_loaded(self, timeout=15000):
        """等待加载完成（spinner 消失）."""
        self.page.wait_for_selector(f"{self.TABLE}", state="visible", timeout=timeout)

    def is_loading(self) -> bool:
        return self.page.is_visible(self.LOADING)

    def is_table_visible(self) -> bool:
        return self.page.is_visible(self.TABLE)

    def is_empty_visible(self) -> bool:
        return self.page.is_visible(self.EMPTY)

    def search(self, text: str):
        self.page.fill(self.SEARCH_INPUT, text)

    def clear_search(self):
        self.page.fill(self.SEARCH_INPUT, "")

    def get_proxy_rows(self):
        return self.page.query_selector_all(f"{self.LIST} tr")

    def get_first_proxy_name(self) -> str:
        rows = self.get_proxy_rows()
        if not rows:
            return ""
        return rows[0].query_selector("td:first-child").inner_text()

    def click_proxy_row(self, name: str):
        """点击指定名称的代理行（支持模糊匹配）."""
        self.page.click(f"{self.LIST} tr:has-text('{name}')")

    def get_latency_class(self, row) -> str:
        td = row.query_selector("td:nth-child(3)")
        cls = td.get_attribute("class") or ""
        return cls

    def has_selected_badge(self, row) -> bool:
        return row.query_selector(".badge.info") is not None
