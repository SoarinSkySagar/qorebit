// Qorebit wallet: the toolbar button opens the wallet in the browser's side panel.
// All the work is in the panel page (the companion app); nothing runs here.
chrome.runtime.onInstalled.addListener(() => {
  chrome.sidePanel.setPanelBehavior({ openPanelOnActionClick: true }).catch(() => {});
});
