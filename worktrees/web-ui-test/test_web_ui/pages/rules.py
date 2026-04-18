"""Rules page object."""

import time
from .base import BasePage


class RulesPage(BasePage):
    """Rules 页面."""

    TYPE_SELECT = "#rule-type"
    VALUE_INPUT = "#rule-value"
    PROXY_INPUT = "#rule-proxy"
    SUBMIT_BTN = "#view-rules button:has-text('Add')"
    TABLE = "#rules-table"
    LOADING = "#rules-loading"
    EMPTY = "#rules-empty"
    LIST = "#rules-list"
    PAGINATION = "#rules-pagination"

    def goto(self):
        self.page.goto(f"{self.base_url}/")
        self.click_nav("rules")

    def wait_loaded(self, timeout=10000):
        """等待 rules 列表加载完成（table 或 empty 可见）."""
        self.page.wait_for_function(
            "() => document.querySelector('#rules-table') !== null || document.querySelector('#rules-empty') !== null",
            timeout=timeout,
        )
        # Then wait for one to actually be visible (not display:none)
        self.page.wait_for_function(
            "() => { const t = document.querySelector('#rules-table'); const e = document.querySelector('#rules-empty'); return (t && t.offsetParent !== null) || (e && e.offsetParent !== null); }",
            timeout=timeout,
        )

    def add_rule(self, rule_type: str, value: str, proxy: str):
        self.page.select_option(self.TYPE_SELECT, rule_type)
        self.page.fill(self.VALUE_INPUT, value)
        self.page.fill(self.PROXY_INPUT, proxy)
        self.page.click(self.SUBMIT_BTN)

    def get_rule_rows(self):
        return self.page.query_selector_all(f"{self.LIST} tr")

    def delete_rule(self, row_index: int):
        """row_index 是 1-based 显示索引."""
        btns = self.page.query_selector_all(f"{self.LIST} button.danger")
        if row_index - 1 < len(btns):
            btns[row_index - 1].click()

    def is_pagination_visible(self) -> bool:
        return self.page.is_visible(self.PAGINATION + "[style*='flex']")

    def get_pagination_text(self) -> str:
        return self.page.text_content(self.PAGINATION) or ""

    def click_next_page(self):
        self.page.click(f"{self.PAGINATION} button:has-text('Next')")

    def get_rule_type_options(self):
        return self.page.query_selector_all(f"{self.TYPE_SELECT} option")