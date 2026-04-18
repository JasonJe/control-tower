"""Profiles page object."""

import time
from .base import BasePage


class ProfilesPage(BasePage):
    """Profiles 页面."""

    URL_INPUT = "#profile-url"
    NAME_INPUT = "#profile-name"
    SUBMIT_BTN = "button[type='submit']:has-text('Add')"
    TABLE = "#profiles-table"
    LOADING = "#profiles-loading"
    EMPTY = "#profiles-empty"
    LIST = "#profiles-list"

    def goto(self):
        self.page.goto(f"{self.base_url}/")
        self.click_nav("profiles")

    def wait_loaded(self, timeout=10000):
        self.page.wait_for_selector(f"{self.TABLE}", state="visible", timeout=timeout)

    def add_profile(self, url: str, name: str = ""):
        self.page.fill(self.URL_INPUT, url)
        if name:
            self.page.fill(self.NAME_INPUT, name)
        self.page.click(self.SUBMIT_BTN)

    def get_profile_rows(self):
        return self.page.query_selector_all(f"{self.LIST} tr")

    def is_table_visible(self) -> bool:
        return self.page.is_visible(self.TABLE)

    def delete_profile(self):
        """点击第一个可用的 Delete 按钮."""
        delete_btns = self.page.query_selector_all(f"{self.LIST} button.danger")
        if delete_btns:
            delete_btns[0].click()

    def confirm_delete(self):
        self.page.click("#deleteModalBtn")

    def cancel_delete(self):
        self.page.click('button:has-text("Cancel")')

    def is_modal_visible(self) -> bool:
        return self.page.is_visible("#deleteModal.show")

    def get_active_profile_row(self):
        return self.page.query_selector(f"{self.LIST} tr.profile-active")