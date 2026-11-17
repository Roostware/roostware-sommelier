// Lets any part of the app ask for the settings dialog to open (the coach's setup prompt does),
// optionally on a particular tab. The dialog itself belongs to SettingsMenu; this is just the doorbell.

export type SettingsTab = "general" | "coach" | "leetcode";

class SettingsDialog {
  /** Bumped each time something asks for the dialog; SettingsMenu opens it when this changes. */
  requested = $state(0);
  /** The tab asked for with the latest request, or null to leave the dialog on whatever tab it was on. */
  tab: SettingsTab | null = null;

  open(tab: SettingsTab | null = null) {
    this.tab = tab;
    this.requested++;
  }
}

export const settingsDialog = new SettingsDialog();
