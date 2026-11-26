# Publishing the extension (Chrome Web Store, Firefox Add-ons)

`make extension-zip` builds `extension/store/`: `qorebit-chrome-<v>.zip` (no pinned `key`, as the Web Store needs), `qorebit-firefox-<v>.zip`
and `qorebit-source-<v>.zip` (the repository at HEAD without the Xous submodule: Mozilla wants the source of a bundled build, and the build
steps are `cd companion && npm ci`, then `extension/build.sh firefox`). Commit first: the source zip is `git archive HEAD`.

## What to know before submitting

* **The wallet needs a local program.** The extension talks to the bridge on `127.0.0.1:7630` and a Dabao board (or the emulator). Say so in the
  listing and give reviewers a way to see it work: `make demo` runs everything on the emulator with no hardware (README, Quick start), and a short
  screen recording helps. Without this, reviewers may reject it as "does not work".
* **Broad host access.** The two small content scripts run on all http(s) pages to offer `window.qorechain` (connect, Sign In With QoreChain).
  They read nothing from the page. Both stores treat "all sites" as an in-depth review; expect it to take longer.
* **The bridge only knows extension ids it was told.** The store gives Chrome its own extension id, different from the development one: after the
  first upload, add the id the dashboard shows as a second line in `extension/EXTENSION_ID` (the bridge reads every line). Firefox gives each install a
  random `moz-extension://<uuid>`: the wallet shows it when the bridge refuses it, and the user starts the bridge with
  `QOREBIT_EXTENSION_ORIGINS=moz-extension://<uuid> make board-panel` (once per install). That is a real rough edge for ordinary users.
* **It is an unaudited research prototype** (README, "Honest status"). Say that in the description; do not present it as a finished product for real funds.

## Chrome Web Store

Developer account (one-time US$5 fee) at <https://chrome.google.com/webstore/devconsole>. Upload `qorebit-chrome-<v>.zip`. Needed for the listing:
a description, at least one 1280x800 (or 640x400) screenshot, the 128px icon (in the package), a category, and a privacy policy URL (text below, host it
on a page you control, for example the repository's GitHub Pages).

## Firefox Add-ons (AMO)

Free, at <https://addons.mozilla.org/developers/>. Upload `qorebit-firefox-<v>.zip` as "On this site" (listed) or "On your own" (unlisted: Mozilla
signs it and you share the `.xpi`; no review of the listing). Answer yes to "does it use minified/bundled code" and attach `qorebit-source-<v>.zip`
with the build steps above. The manifest declares `data_collection_permissions: none` and the id `qorebit-wallet@qorebit`.
To try it without publishing: `about:debugging#/runtime/this-firefox` → Load Temporary Add-on → `extension/dist-firefox/manifest.json`.

## Text for the forms

**Single purpose.** A companion for the Qorebit hardware wallet: it shows your QoreChain accounts, builds transactions and sign-in requests, and
passes them to your own device, which asks you to approve each one on its buttons. It also lets websites you approve ask for your address and a
Sign In With QoreChain signature.

**Permissions.**
* `storage`: keeps the selected account and network, the list of sites you connected, and the hash of your wallet password, in the browser.
* `sidePanel` (Chrome) / `sidebar_action` (Firefox): the wallet's main window.
* Host `http://127.0.0.1:7630/*`: the local bridge that talks to your device and to QoreChain's public endpoints for balances. Nothing else.
* Content scripts on `http://*/*` and `https://*/*`: add `window.qorechain` to pages so a site can ask to connect or sign in. They only forward
  those requests to the extension; every request opens an approval window and the signature is made on the device after a button press.

**Privacy policy.** Qorebit wallet collects no data. It has no servers, analytics, advertising or tracking. The extension stores, only in your
browser, the selected account and network, the sites you connected, and a salted hash of your wallet password (the password itself is never stored).
It talks only to the bridge on your own computer (127.0.0.1), which forwards balance and transaction requests to QoreChain's public nodes using your
account's public address. Private keys never leave your hardware device and are never available to the extension. Nothing is sold or shared.
