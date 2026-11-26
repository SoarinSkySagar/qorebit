// Checks the extension's manifest, and (when built) that extension/dist is self-contained.
// Run by `make test` (manifest only) and extension/build.sh (manifest + dist).
import { readFileSync, existsSync, readdirSync } from "node:fs";
import { createHash } from "node:crypto";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..", "extension");
const fail = (m) => { console.error("extension check FAILED: " + m); process.exit(1); };
const m = JSON.parse(readFileSync(join(root, "manifest.json"), "utf8"));

if (m.manifest_version !== 3) fail("manifest_version must be 3");
// the pinned key gives the fixed id the bridge allows
const der = Buffer.from(m.key, "base64");
const id = [...createHash("sha256").update(der).digest("hex").slice(0, 32)].map((c) => String.fromCharCode(97 + parseInt(c, 16))).join("");
const pinned = readFileSync(join(root, "EXTENSION_ID"), "utf8").trim();
if (id !== pinned) fail(`the manifest key gives id ${id}, EXTENSION_ID says ${pinned}`);
// minimal powers: the side panel, its own storage and the local bridge; two small content scripts that only relay the
// window.qorechain provider (no host permissions for pages, no page data is read)
const perms = JSON.stringify(m.permissions ?? []);
if (perms !== '["sidePanel","storage"]') fail("permissions must be exactly sidePanel and storage, got " + perms);
if (JSON.stringify(m.host_permissions) !== '["http://127.0.0.1:7630/*"]') fail("host_permissions must be the bridge only");
if (m.externally_connectable || m.web_accessible_resources) fail("no web pages may reach into the extension");
const cs = (m.content_scripts ?? []).map((c) => `${c.js.join(",")}|${c.world ?? "ISOLATED"}|${c.matches.join(",")}`).sort();
if (JSON.stringify(cs) !== JSON.stringify(["content.js|ISOLATED|http://*/*,https://*/*", "inject.js|MAIN|http://*/*,https://*/*"])) fail("content scripts must be exactly inject.js (MAIN) and content.js, on http(s) pages: " + JSON.stringify(cs));
for (const f of ["inject.js", "content.js"]) {
  const t = readFileSync(join(root, f), "utf8");
  if (/\b(eval|new Function|XMLHttpRequest|fetch\()/.test(t)) fail(`${f} must not use eval, fetch or XMLHttpRequest`);
}
for (const f of [m.side_panel.default_path, m.background.service_worker, ...Object.values(m.icons)]) {
  if (!existsSync(join(root, "dist", f)) && !existsSync(join(root, f)) && f !== m.side_panel.default_path) fail("missing file " + f);
}

const dist = join(root, "dist");
if (existsSync(dist)) {
  const walk = (d) => readdirSync(d, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? walk(join(d, e.name)) : [join(d, e.name)]));
  const html = readFileSync(join(dist, "index.html"), "utf8");
  if (/<script(?![^>]*\bsrc=)[^>]*>/i.test(html)) fail("index.html has an inline script (blocked by the extension CSP)");
  if (/(src|href)="\/[^/]/.test(html)) fail("index.html uses absolute asset paths; build with --base ./");
  if (/(src|href)="https?:/i.test(html)) fail("index.html loads something from the network");
  for (const f of walk(dist).filter((p) => /\.(js|css|html)$/.test(p))) {
    const t = readFileSync(f, "utf8");
    for (const u of t.match(/https?:\/\/[^\s"'`)\\]+/g) ?? []) {
      const ok = /^(https?:\/\/(127\.0\.0\.1|localhost)|https?:\/\/(dashboard\.qorechain\.io|explore\.qore\.network|www\.w3\.org|reactjs\.org|react\.dev|github\.com\/facebook|developer\.mozilla\.org|fb\.me|stackoverflow\.com|mths\.be|json-schema\.org))/.test(u);
      if (!ok && !f.endsWith(".js")) fail(`${f} mentions ${u}`);
    }
  }
  console.log("extension dist ok (self-contained, no inline scripts)");
}
console.log(`extension manifest ok (id ${id})`);
