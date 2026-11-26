// Checks the extension manifests (Chrome and Firefox) and, when built, that each dist is self-contained.
// Run by `make test` (manifests only) and extension/build.sh (manifest + dist).
import { readFileSync, existsSync, readdirSync } from "node:fs";
import { createHash } from "node:crypto";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..", "extension");
const fail = (m) => { console.error("extension check FAILED: " + m); process.exit(1); };
const read = (f) => JSON.parse(readFileSync(join(root, f), "utf8"));
const chrome = read("manifest.json");
const firefox = read("manifest.firefox.json");
const BRIDGE = "http://127.0.0.1:7630/*";
const SCRIPTS = ["content.js|ISOLATED|http://*/*,https://*/*", "inject.js|MAIN|http://*/*,https://*/*"];

for (const [name, m] of [["chrome", chrome], ["firefox", firefox]]) {
  if (m.manifest_version !== 3) fail(`${name}: manifest_version must be 3`);
  if (m.version !== chrome.version) fail(`${name}: the two manifests have different versions`);
  if (m.externally_connectable || m.web_accessible_resources) fail(`${name}: no web pages may reach into the extension`);
  // two tiny content scripts that only relay the window.qorechain provider (nothing is read from pages)
  const cs = (m.content_scripts ?? []).map((c) => `${c.js.join(",")}|${c.world ?? "ISOLATED"}|${c.matches.join(",")}`).sort();
  if (JSON.stringify(cs) !== JSON.stringify(SCRIPTS)) fail(`${name}: content scripts must be exactly inject.js (MAIN) and content.js, on http(s) pages: ` + JSON.stringify(cs));
  if (!/connect-src http:\/\/127\.0\.0\.1:7630'?$/.test(m.content_security_policy.extension_pages)) fail(`${name}: connect-src must be the bridge only`);
  for (const f of Object.values(m.icons)) if (!existsSync(join(root, f))) fail(`${name}: missing icon ${f}`);
}
for (const f of ["inject.js", "content.js"]) {
  const t = readFileSync(join(root, f), "utf8");
  if (/\b(eval|new Function|XMLHttpRequest|fetch\()/.test(t)) fail(`${f} must not use eval, fetch or XMLHttpRequest`);
}

// Chrome: the pinned key gives the fixed id the bridge allows
const der = Buffer.from(chrome.key, "base64");
const id = [...createHash("sha256").update(der).digest("hex").slice(0, 32)].map((c) => String.fromCharCode(97 + parseInt(c, 16))).join("");
const pinned = readFileSync(join(root, "EXTENSION_ID"), "utf8").trim().split(/\s+/)[0];
if (id !== pinned) fail(`the manifest key gives id ${id}, EXTENSION_ID says ${pinned}`);
if (JSON.stringify(chrome.permissions) !== '["sidePanel","storage"]') fail("chrome: permissions must be exactly sidePanel and storage, got " + JSON.stringify(chrome.permissions));
if (JSON.stringify(chrome.host_permissions) !== JSON.stringify([BRIDGE])) fail("chrome: host_permissions must be the bridge only");
if (!chrome.side_panel?.default_path || !chrome.background?.service_worker) fail("chrome: side_panel and a service worker are needed");

// Firefox: no Chrome-only keys; a gecko id; the sidebar; scripts, not a service worker
if (JSON.stringify(firefox.permissions) !== '["storage"]') fail("firefox: permissions must be exactly storage, got " + JSON.stringify(firefox.permissions));
if (JSON.stringify(firefox.host_permissions) !== JSON.stringify([BRIDGE, "http://*/*", "https://*/*"])) fail("firefox: host_permissions must be the bridge and the pages the content scripts run on");
for (const k of ["key", "side_panel", "minimum_chrome_version"]) if (k in firefox) fail(`firefox: "${k}" is Chrome-only`);
if (firefox.background?.service_worker || JSON.stringify(firefox.background?.scripts) !== '["background.js"]') fail("firefox: the background must be scripts: [background.js]");
if (!firefox.browser_specific_settings?.gecko?.id || !firefox.sidebar_action?.default_panel) fail("firefox: a gecko id and a sidebar_action are needed");
if (JSON.stringify(firefox.browser_specific_settings.gecko.data_collection_permissions?.required) !== '["none"]') fail("firefox: data_collection_permissions must say none");

function checkDist(dir, name) {
  const dist = join(root, dir);
  if (!existsSync(dist)) return;
  const walk = (d) => readdirSync(d, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? walk(join(d, e.name)) : [join(d, e.name)]));
  const html = readFileSync(join(dist, "index.html"), "utf8");
  if (/<script(?![^>]*\bsrc=)[^>]*>/i.test(html)) fail(`${name}: index.html has an inline script (blocked by the extension CSP)`);
  if (/(src|href)="\/[^/]/.test(html)) fail(`${name}: index.html uses absolute asset paths; build with --base ./`);
  if (/(src|href)="https?:/i.test(html)) fail(`${name}: index.html loads something from the network`);
  for (const f of walk(dist).filter((p) => /\.(js|css|html)$/.test(p))) {
    const t = readFileSync(f, "utf8");
    for (const u of t.match(/https?:\/\/[^\s"'`)\\]+/g) ?? []) {
      const ok = /^(https?:\/\/(127\.0\.0\.1|localhost)|https?:\/\/(dashboard\.qorechain\.io|explore\.qore\.network|www\.w3\.org|reactjs\.org|react\.dev|github\.com\/facebook|developer\.mozilla\.org|fb\.me|stackoverflow\.com|mths\.be|json-schema\.org))/.test(u);
      if (!ok && !f.endsWith(".js")) fail(`${f} mentions ${u}`);
    }
  }
  const m = JSON.parse(readFileSync(join(dist, "manifest.json"), "utf8"));
  for (const f of [m.background.service_worker ?? m.background.scripts[0], "inject.js", "content.js", "index.html"]) if (!existsSync(join(dist, f))) fail(`${name}: dist is missing ${f}`);
  console.log(`${name} dist ok (self-contained, no inline scripts)`);
}
checkDist("dist", "chrome");
checkDist("dist-firefox", "firefox");
console.log(`extension manifests ok (chrome id ${id})`);
